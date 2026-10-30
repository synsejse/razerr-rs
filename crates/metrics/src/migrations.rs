use diesel::{
    connection::SimpleConnection,
    migration::MigrationSource,
    prelude::*,
    sql_types::{BigInt, Text},
    sqlite::Sqlite,
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

use crate::Error;

pub(crate) const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

#[derive(QueryableByName)]
struct TableCount {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

#[derive(QueryableByName)]
struct Version {
    #[diesel(sql_type = Text)]
    version: String,
}

fn versions(connection: &mut SqliteConnection) -> Result<Vec<String>, Error> {
    let table: TableCount = diesel::sql_query("SELECT count(*) AS count FROM sqlite_master WHERE type='table' AND name='__diesel_schema_migrations'").get_result(connection)?;
    if table.count == 0 {
        return Ok(Vec::new());
    }
    Ok(
        diesel::sql_query("SELECT version FROM __diesel_schema_migrations ORDER BY version")
            .load::<Version>(connection)?
            .into_iter()
            .map(|row| row.version)
            .collect(),
    )
}

fn expected<S: MigrationSource<Sqlite>>(source: &S) -> Result<Vec<String>, Error> {
    let mut expected: Vec<_> = source
        .migrations()
        .map_err(Error::Migration)?
        .into_iter()
        .map(|migration| migration.name().version().to_string())
        .collect();
    expected.sort();
    Ok(expected)
}

fn reject_unknown(applied: &[String], expected: &[String]) -> Result<(), Error> {
    for version in applied {
        if !expected.contains(version) {
            return Err(Error::TooNew(version.clone()));
        }
    }
    Ok(())
}

pub(crate) fn check_reader(connection: &mut SqliteConnection) -> Result<(), Error> {
    let expected = expected(&MIGRATIONS)?;
    let applied = versions(connection)?;
    reject_unknown(&applied, &expected)?;
    if applied != expected {
        return Err(Error::NeedsMigration);
    }
    Ok(())
}

pub(crate) fn migrate<S: MigrationSource<Sqlite>>(
    connection: &mut SqliteConnection,
    source: S,
) -> Result<(), Error> {
    let expected = expected(&source)?;
    connection.immediate_transaction(|connection| {
        // Check only after acquiring the write lock; a competing opener may have upgraded.
        reject_unknown(&versions(connection)?, &expected)?;
        connection
            .run_pending_migrations(source)
            .map_err(Error::Migration)?;
        Ok(())
    })
}

pub(crate) fn configure(connection: &mut SqliteConnection, writable: bool) -> Result<(), Error> {
    connection.batch_execute("PRAGMA busy_timeout = 2000")?;
    connection.batch_execute("PRAGMA foreign_keys = ON")?;
    if writable {
        // Journal mode is established outside the migration transaction.
        connection.batch_execute("PRAGMA journal_mode = WAL")?;
        connection.batch_execute("PRAGMA synchronous = NORMAL")?;
    } else {
        // The URI already opens read-only. This also documents/enforces the intent.
        connection.batch_execute("PRAGMA query_only = ON")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_names_are_consecutive_and_descriptive() {
        let migrations =
            MigrationSource::<Sqlite>::migrations(&MIGRATIONS).expect("embedded migrations");
        let mut names: Vec<_> = migrations
            .iter()
            .map(|migration| migration.name().to_string())
            .collect();
        names.sort();
        for (index, name) in names.iter().enumerate() {
            let (number, suffix) = name.split_once('_').expect("number_name");
            assert_eq!(number, format!("{:04}", index + 1));
            assert_ne!(suffix, "");
            assert!(
                suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            );
        }
    }

    #[test]
    fn failed_batch_rolls_back_earlier_migrations() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let first = directory.path().join("0001_first");
        let second = directory.path().join("0002_second");
        std::fs::create_dir(&first)?;
        std::fs::create_dir(&second)?;
        std::fs::write(
            first.join("up.sql"),
            "CREATE TABLE entries (id INTEGER); INSERT INTO entries VALUES (7);",
        )?;
        std::fs::write(first.join("down.sql"), "")?;
        std::fs::write(
            second.join("up.sql"),
            "CREATE TABLE unfinished (id INTEGER); INSERT INTO missing VALUES (1);",
        )?;
        std::fs::write(second.join("down.sql"), "")?;
        let path = directory.path().join("history.sqlite3");
        let mut connection = SqliteConnection::establish(path.to_str().ok_or("path")?)?;
        let source = diesel_migrations::FileBasedMigrations::from_path(directory.path())?;
        assert!(migrate(&mut connection, source).is_err());
        let tables: TableCount =
            diesel::sql_query("SELECT count(*) AS count FROM sqlite_master WHERE type='table'")
                .get_result(&mut connection)?;
        assert_eq!(tables.count, 0);
        std::fs::write(second.join("up.sql"), "INSERT INTO entries VALUES (9);")?;
        let source = diesel_migrations::FileBasedMigrations::from_path(directory.path())?;
        migrate(&mut connection, source)?;
        let entries: TableCount = diesel::sql_query("SELECT count(*) AS count FROM entries")
            .get_result(&mut connection)?;
        assert_eq!(entries.count, 2);
        assert_eq!(versions(&mut connection)?, ["0001", "0002"]);
        Ok(())
    }
}
