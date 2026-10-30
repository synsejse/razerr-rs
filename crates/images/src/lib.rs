//! Bounded image loading with a persistent, content-validated URL cache.
//!
//! Drop the future returned by [`ImageLoader::load`] to cancel a download. Small
//! bounded file/decode operations run on the blocking pool and may finish after
//! cancellation; no detached network task continues downloading.

use std::{
    fs::{self, DirBuilder, File},
    io::{Cursor, Read, Write},
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

use futures_util::StreamExt;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

pub const MAX_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PIXELS: u64 = 4 * 1024 * 1024;
pub const MAX_DIMENSION: u32 = 4096;
const MAX_ALLOCATION: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("image source must be an absolute file path or an HTTP(S) URL")]
    UnsupportedSource,
    #[error("image cache directory must be absolute")]
    RelativeCacheDirectory,
    #[error("encoded image exceeds the 8 MiB limit")]
    TooLarge,
    #[error("image dimensions {width} × {height} exceed the 4096-axis / 4 MP limit")]
    Dimensions { width: u32, height: u32 },
    #[error("only PNG and JPEG images are supported")]
    UnsupportedFormat,
    #[error("cannot access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot decode image: {0}")]
    Decode(#[from] image::ImageError),
    #[error("cannot download image: {0}")]
    Http(#[from] reqwest::Error),
    #[error("image worker failed: {0}")]
    Worker(#[from] tokio::task::JoinError),
}

impl Error {
    fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

#[derive(Clone)]
pub struct ImageLoader {
    cache_dir: PathBuf,
    client: reqwest::Client,
}

impl ImageLoader {
    /// Construct a loader without creating any files or making HTTP requests.
    pub fn new(cache_dir: impl Into<PathBuf>) -> Result<Self, Error> {
        let cache_dir = cache_dir.into();
        if !cache_dir.is_absolute() {
            return Err(Error::RelativeCacheDirectory);
        }
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(4))
            .user_agent("razerr-rs/0.1")
            .build()?;
        Ok(Self { cache_dir, client })
    }

    pub fn cache_path(&self, source: &str) -> PathBuf {
        let key = hex::encode(Sha256::digest(source.as_bytes()));
        self.cache_dir.join(format!("{key}.image"))
    }

    pub async fn load(&self, source: &str) -> Result<RgbaImage, Error> {
        if Path::new(source).is_absolute() {
            let path = PathBuf::from(source);
            return tokio::task::spawn_blocking(move || decode(&read_file(&path)?)).await?;
        }
        let url = reqwest::Url::parse(source).map_err(|_| Error::UnsupportedSource)?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(Error::UnsupportedSource);
        }

        let cache_path = self.cache_path(source);
        let path = cache_path.clone();
        if let Ok(image) = tokio::task::spawn_blocking(move || decode(&read_file(&path)?)).await? {
            return Ok(image);
        }

        let response = self.client.get(url).send().await?.error_for_status()?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BYTES as u64)
        {
            return Err(Error::TooLarge);
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if chunk.len() > MAX_BYTES - bytes.len() {
                return Err(Error::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }

        let (image, bytes) =
            tokio::task::spawn_blocking(move || decode(&bytes).map(|image| (image, bytes)))
                .await??;
        let directory = self.cache_dir.clone();
        let save =
            tokio::task::spawn_blocking(move || write_cache(&directory, &cache_path, &bytes))
                .await?;
        if let Err(error) = save {
            // Pictures remain usable even with a full or read-only cache volume.
            tracing::warn!(%error, "could not cache device image");
        }
        Ok(image)
    }
}

fn read_file(path: &Path) -> Result<Vec<u8>, Error> {
    let metadata = fs::metadata(path).map_err(|source| Error::io(path, source))?;
    if !metadata.is_file() {
        return Err(Error::io(
            path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "image is not a regular file",
            ),
        ));
    }
    if metadata.len() > MAX_BYTES as u64 {
        return Err(Error::TooLarge);
    }
    let file = File::open(path).map_err(|source| Error::io(path, source))?;
    if !file
        .metadata()
        .map_err(|source| Error::io(path, source))?
        .is_file()
    {
        return Err(Error::io(
            path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "image is not a regular file",
            ),
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| Error::io(path, source))?;
    if bytes.len() > MAX_BYTES {
        return Err(Error::TooLarge);
    }
    Ok(bytes)
}

fn dimensions(width: u32, height: u32) -> Result<(), Error> {
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(Error::Dimensions { width, height });
    }
    Ok(())
}

/// Decode only after checking both codec limits and the total pixel count.
pub fn decode(bytes: &[u8]) -> Result<RgbaImage, Error> {
    if bytes.len() > MAX_BYTES {
        return Err(Error::TooLarge);
    }
    let format = image::guess_format(bytes)?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return Err(Error::UnsupportedFormat);
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_ALLOCATION);
    reader.limits(limits);
    let decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    dimensions(width, height)?;
    let pixels = DynamicImage::from_decoder(decoder)?.into_rgba8().into_raw();
    Ok(RgbaImage {
        width,
        height,
        pixels,
    })
}

fn write_cache(directory: &Path, path: &Path, bytes: &[u8]) -> Result<(), Error> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
        .map_err(|source| Error::io(directory, source))?;
    if fs::symlink_metadata(directory)
        .map_err(|source| Error::io(directory, source))?
        .file_type()
        .is_symlink()
    {
        return Err(Error::io(
            directory,
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cache directory is a symbolic link",
            ),
        ));
    }
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
        .map_err(|source| Error::io(directory, source))?;
    let mut file =
        NamedTempFile::new_in(directory).map_err(|source| Error::io(directory, source))?;
    file.write_all(bytes)
        .map_err(|source| Error::io(path, source))?;
    file.as_file()
        .sync_all()
        .map_err(|source| Error::io(path, source))?;
    file.persist(path)
        .map_err(|error| Error::io(path, error.error))?;
    File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|source| Error::io(directory, source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_are_bounded_before_allocation() {
        assert!(dimensions(2048, 2048).is_ok());
        for (width, height) in [
            (0, 1),
            (1, 0),
            (4097, 1),
            (1, 4097),
            (4096, 4096),
            (u32::MAX, u32::MAX),
        ] {
            assert!(dimensions(width, height).is_err());
        }
    }
}
