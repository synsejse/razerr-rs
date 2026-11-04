use std::{
    ffi::OsStr,
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

use crate::Error;

/// Resolve a sibling executable without consulting PATH or invoking a shell.
fn sibling_path(program: &str) -> Result<PathBuf, Error> {
    let executable = std::env::current_exe().map_err(|source| Error::Launch {
        path: PathBuf::from(program),
        source,
    })?;
    resolve_sibling(&executable, program)
}

fn resolve_sibling(executable: &Path, program: &str) -> Result<PathBuf, Error> {
    let mut components = Path::new(program).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(Error::InvalidProgram);
    }
    let path = executable.with_file_name(program);
    let metadata = fs::metadata(&path).map_err(|source| Error::Launch {
        path: path.clone(),
        source,
    })?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(Error::Launch {
            path,
            source: std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "sibling is not an executable regular file",
            ),
        });
    }
    Ok(path)
}

/// Launch a detached sibling, and reap it without delaying the service loop.
/// Standard input is closed; diagnostics retain the caller's stderr.
fn launch_sibling(program: &str, arguments: &[&OsStr]) -> Result<(), Error> {
    let path = sibling_path(program)?;
    let mut command = Command::new(&path);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .process_group(0);
    let mut child = command.spawn().map_err(|source| Error::Launch {
        path: path.clone(),
        source,
    })?;
    std::thread::Builder::new()
        .name("razerr-child-reaper".into())
        .spawn(move || {
            if let Err(error) = child.wait() {
                tracing::warn!(%error, "could not reap launched application");
            }
        })
        .map_err(|source| Error::Launch { path, source })?;
    Ok(())
}

pub fn launch_gui(settings: bool) -> Result<(), Error> {
    if settings {
        launch_sibling("razerr-rs", &[OsStr::new("--settings")])
    } else {
        launch_sibling("razerr-rs", &[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_executable_siblings_are_accepted() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("service");
        for name in ["", "/bin/sh", "../outside", ".", "some/path"] {
            assert!(resolve_sibling(&executable, name).is_err());
        }
        let path = directory.path().join("razerr-rs");
        fs::write(&path, "binary contents are not executed by this test")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        assert!(resolve_sibling(&executable, "razerr-rs").is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        assert_eq!(resolve_sibling(&executable, "razerr-rs")?, path);
        fs::create_dir(directory.path().join("directory"))?;
        assert!(resolve_sibling(&executable, "directory").is_err());
        Ok(())
    }
}
