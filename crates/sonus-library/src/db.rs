use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("migration error: {0}")]
    Migration(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, LibraryError>;

/// Current schema version. Bump this and add a step to `migrate` when the
/// schema changes.
const SCHEMA_VERSION: i64 = 2;

/// A SQLite connection wrapped with the Sonus schema.
///
/// One connection is enough — Sonus is single-user and writes are rare.
/// `rusqlite::Connection` is `Send` but not `Sync`, so the GUI keeps it on one
/// thread rather than sharing it.
pub struct Database {
    conn: rusqlite::Connection,
}

impl Database {
    /// Open (or create) the database at `path` and bring the schema up to date.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = rusqlite::Connection::open(path)?;
        Self::prepare(&conn)?;
        Ok(Self { conn })
    }

    /// Open an in-memory database. Used by tests.
    pub fn in_memory() -> Result<Self> {
        let conn = rusqlite::Connection::open_in_memory()?;
        Self::prepare(&conn)?;
        Ok(Self { conn })
    }

    /// Platform-appropriate location for the library database.
    pub fn default_path() -> PathBuf {
        let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("sonus");
        path.push("library.db");
        path
    }

    fn prepare(conn: &rusqlite::Connection) -> Result<()> {
        // WAL keeps reads from blocking on writes; foreign keys must be turned
        // on per-connection or the ON DELETE CASCADE clauses do nothing.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL;",
        )?;
        migrate(conn)
    }

    pub fn conn(&self) -> &rusqlite::Connection {
        &self.conn
    }
}

fn migrate(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;

    let version = schema_version(conn)?;
    if version > SCHEMA_VERSION {
        return Err(LibraryError::Migration(format!(
            "database is at schema v{version}, but this build only understands v{SCHEMA_VERSION}. \
             Upgrade Sonus to open it."
        )));
    }

    // Pre-release databases keyed tracks on a generated UUID. Nothing ever
    // shipped with that schema, so the cleanest migration is to drop the
    // library tables and rebuild — no user data is worth salvaging from a
    // development build.
    if has_legacy_track_schema(conn) {
        conn.execute_batch(LEGACY_DROP)?;
    }

    conn.execute_batch(SCHEMA)?;
    set_schema_version(conn, SCHEMA_VERSION)?;
    Ok(())
}

/// True when `tracks` exists but is missing the `youtube_id` primary key.
fn has_legacy_track_schema(conn: &rusqlite::Connection) -> bool {
    let table_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='tracks')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !table_exists {
        return false;
    }

    let mut stmt = match conn.prepare("PRAGMA table_info(tracks)") {
        Ok(stmt) => stmt,
        Err(_) => return false,
    };
    let columns: Vec<String> = match stmt.query_map([], |row| row.get::<_, String>(1)) {
        Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
        Err(_) => return false,
    };

    // New schema has `youtube_id` as the primary key and no `id` column.
    columns.iter().any(|c| c == "id")
}

fn schema_version(conn: &rusqlite::Connection) -> Result<i64> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .ok();
    Ok(value.and_then(|v| v.parse().ok()).unwrap_or(0))
}

fn set_schema_version(conn: &rusqlite::Connection, version: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![version.to_string()],
    )?;
    Ok(())
}

/// Tables from the UUID-keyed schema. Child tables first — the foreign keys
/// point at `tracks`, so dropping it first would trip the constraint.
const LEGACY_DROP: &str = "
    DROP TABLE IF EXISTS history;
    DROP TABLE IF EXISTS favorites;
    DROP TABLE IF EXISTS playlist_tracks;
    DROP TABLE IF EXISTS playlists;
    DROP TABLE IF EXISTS tracks;
";

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS tracks (
        youtube_id    TEXT PRIMARY KEY,
        title         TEXT NOT NULL,
        artist        TEXT,
        album         TEXT,
        duration_secs INTEGER,
        thumbnail_url TEXT,
        created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    );

    CREATE TABLE IF NOT EXISTS playlists (
        id         TEXT PRIMARY KEY,
        name       TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    );

    CREATE TABLE IF NOT EXISTS playlist_tracks (
        playlist_id TEXT    NOT NULL,
        youtube_id  TEXT    NOT NULL,
        position    INTEGER NOT NULL,
        PRIMARY KEY (playlist_id, youtube_id),
        FOREIGN KEY (playlist_id) REFERENCES playlists(id) ON DELETE CASCADE,
        FOREIGN KEY (youtube_id)  REFERENCES tracks(youtube_id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS favorites (
        youtube_id TEXT PRIMARY KEY,
        added_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        FOREIGN KEY (youtube_id) REFERENCES tracks(youtube_id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS history (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        youtube_id TEXT NOT NULL,
        played_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        FOREIGN KEY (youtube_id) REFERENCES tracks(youtube_id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_history_played_at ON history(played_at DESC);
    CREATE INDEX IF NOT EXISTS idx_playlist_tracks_position
        ON playlist_tracks(playlist_id, position);
    CREATE INDEX IF NOT EXISTS idx_tracks_updated_at ON tracks(updated_at DESC);
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_in_memory() {
        let db = Database::in_memory().unwrap();
        assert_eq!(schema_version(db.conn()).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn creates_every_table() {
        let db = Database::in_memory().unwrap();
        for table in [
            "tracks",
            "playlists",
            "playlist_tracks",
            "favorites",
            "history",
            "settings",
            "meta",
        ] {
            let exists: bool = db
                .conn()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    rusqlite::params![table],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "missing table: {table}");
        }
    }

    #[test]
    fn reopening_keeps_the_schema_version() {
        let dir = std::env::temp_dir().join("sonus_db_reopen_test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("library.db");

        {
            let _db = Database::open(&path).unwrap();
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(schema_version(db.conn()).unwrap(), SCHEMA_VERSION);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn creates_missing_parent_directories() {
        let dir = std::env::temp_dir().join("sonus_db_nested/a/b");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("sonus_db_nested"));
        let path = dir.join("library.db");

        assert!(Database::open(&path).is_ok());
        assert!(path.exists());

        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("sonus_db_nested"));
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let db = Database::in_memory().unwrap();
        let enabled: bool = db
            .conn()
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert!(enabled, "cascades silently do nothing without this pragma");
    }

    #[test]
    fn refuses_a_future_schema() {
        let db = Database::in_memory().unwrap();
        set_schema_version(db.conn(), SCHEMA_VERSION + 1).unwrap();
        let err = migrate(db.conn()).unwrap_err();
        assert!(matches!(err, LibraryError::Migration(_)));
    }

    #[test]
    fn detects_the_legacy_uuid_schema() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE tracks (id TEXT PRIMARY KEY, youtube_id TEXT NOT NULL);")
            .unwrap();
        assert!(has_legacy_track_schema(&conn));
    }

    #[test]
    fn current_schema_is_not_flagged_as_legacy() {
        let db = Database::in_memory().unwrap();
        assert!(!has_legacy_track_schema(db.conn()));
    }

    #[test]
    fn missing_tracks_table_is_not_legacy() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        assert!(!has_legacy_track_schema(&conn));
    }

    #[test]
    fn migrates_a_legacy_database_in_place() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tracks (id TEXT PRIMARY KEY, youtube_id TEXT NOT NULL);
             CREATE TABLE playlists (id TEXT PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE playlist_tracks (
                 playlist_id TEXT NOT NULL, track_id TEXT NOT NULL, position INTEGER NOT NULL,
                 PRIMARY KEY (playlist_id, track_id)
             );",
        )
        .unwrap();

        migrate(&conn).unwrap();

        // The old columns are gone and the new ones are in place.
        let mut stmt = conn.prepare("PRAGMA table_info(tracks)").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(columns.contains(&"youtube_id".to_string()));
        assert!(!columns.contains(&"id".to_string()));
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn default_path_ends_with_library_db() {
        let path = Database::default_path();
        assert_eq!(path.file_name().unwrap(), "library.db");
        assert!(path.to_string_lossy().contains("sonus"));
    }
}
