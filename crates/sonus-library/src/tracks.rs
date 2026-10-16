use rusqlite::Row;
use sonus_core::Track;

use crate::db::{Database, Result};

/// Read a `Track` from a row selected in the canonical column order.
///
/// Columns are qualified with the `t` alias — every query that uses this
/// constant joins `tracks` against another table, and an unqualified
/// `youtube_id` is ambiguous in those joins.
pub(crate) const TRACK_COLUMNS: &str =
    "t.youtube_id, t.title, t.artist, t.album, t.duration_secs, t.thumbnail_url";

pub(crate) fn track_from_row(row: &Row<'_>) -> rusqlite::Result<Track> {
    Ok(Track {
        youtube_id: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        album: row.get(3)?,
        duration_secs: row.get(4)?,
        thumbnail_url: row.get(5)?,
    })
}

/// CRUD for the track table.
pub struct Tracks;

impl Tracks {
    /// Insert a track, or update it if the YouTube ID is already known.
    ///
    /// Re-searching a song must not create a second row — playlists and
    /// history reference tracks by YouTube ID, so a duplicate would silently
    /// split a song's history in two.
    pub fn upsert(db: &Database, track: &Track) -> Result<()> {
        db.conn().execute(
            "INSERT INTO tracks
                (youtube_id, title, artist, album, duration_secs, thumbnail_url)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(youtube_id) DO UPDATE SET
                title         = excluded.title,
                artist        = COALESCE(excluded.artist, tracks.artist),
                album         = COALESCE(excluded.album, tracks.album),
                duration_secs = COALESCE(excluded.duration_secs, tracks.duration_secs),
                thumbnail_url = COALESCE(excluded.thumbnail_url, tracks.thumbnail_url),
                updated_at    = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
            rusqlite::params![
                track.youtube_id,
                track.title,
                track.artist,
                track.album,
                track.duration_secs,
                track.thumbnail_url,
            ],
        )?;
        Ok(())
    }

    /// Insert several tracks in one transaction — much faster than N upserts.
    pub fn upsert_many(db: &Database, tracks: &[Track]) -> Result<()> {
        if tracks.is_empty() {
            return Ok(());
        }
        // A transaction keeps this atomic: either the whole batch lands or none
        // of it does, so a crash can't leave a half-written playlist.
        let tx = db.conn().unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO tracks
                    (youtube_id, title, artist, album, duration_secs, thumbnail_url)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(youtube_id) DO UPDATE SET
                    title         = excluded.title,
                    artist        = COALESCE(excluded.artist, tracks.artist),
                    album         = COALESCE(excluded.album, tracks.album),
                    duration_secs = COALESCE(excluded.duration_secs, tracks.duration_secs),
                    thumbnail_url = COALESCE(excluded.thumbnail_url, tracks.thumbnail_url),
                    updated_at    = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
            )?;
            for track in tracks {
                stmt.execute(rusqlite::params![
                    track.youtube_id,
                    track.title,
                    track.artist,
                    track.album,
                    track.duration_secs,
                    track.thumbnail_url,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get(db: &Database, youtube_id: &str) -> Result<Option<Track>> {
        let sql = format!("SELECT {TRACK_COLUMNS} FROM tracks t WHERE t.youtube_id = ?1");
        let result = db
            .conn()
            .query_row(&sql, rusqlite::params![youtube_id], track_from_row);

        match result {
            Ok(track) => Ok(Some(track)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Every track, most recently touched first.
    pub fn all(db: &Database) -> Result<Vec<Track>> {
        // `rowid` breaks ties: timestamps are millisecond-granular, so several
        // inserts can share one and the order would otherwise be arbitrary.
        let sql = format!(
            "SELECT {TRACK_COLUMNS} FROM tracks t ORDER BY t.updated_at DESC, t.rowid DESC"
        );
        let mut stmt = db.conn().prepare(&sql)?;
        let tracks = stmt
            .query_map([], track_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(tracks)
    }

    /// Search titles and artists with a case-insensitive substring match.
    pub fn search(db: &Database, query: &str, limit: usize) -> Result<Vec<Track>> {
        let pattern = format!("%{}%", query.trim());
        let sql = format!(
            "SELECT {TRACK_COLUMNS} FROM tracks t
             WHERE t.title  LIKE ?1 COLLATE NOCASE
                OR t.artist LIKE ?1 COLLATE NOCASE
                OR t.album  LIKE ?1 COLLATE NOCASE
             ORDER BY t.updated_at DESC, t.rowid DESC
             LIMIT ?2"
        );
        let mut stmt = db.conn().prepare(&sql)?;
        let tracks = stmt
            .query_map(rusqlite::params![pattern, limit as i64], track_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(tracks)
    }

    pub fn delete(db: &Database, youtube_id: &str) -> Result<()> {
        db.conn().execute(
            "DELETE FROM tracks WHERE youtube_id = ?1",
            rusqlite::params![youtube_id],
        )?;
        Ok(())
    }

    pub fn count(db: &Database) -> Result<usize> {
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?;
        Ok(count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        Database::in_memory().unwrap()
    }

    fn track(id: &str, title: &str) -> Track {
        Track::new(id, title)
    }

    #[test]
    fn upsert_then_get_round_trips() {
        let db = db();
        let t = track("abc12345678", "Rosewood").with_artist("Bonobo");
        Tracks::upsert(&db, &t).unwrap();

        let loaded = Tracks::get(&db, "abc12345678").unwrap().unwrap();
        assert_eq!(loaded.title, "Rosewood");
        assert_eq!(loaded.artist.as_deref(), Some("Bonobo"));
    }

    #[test]
    fn get_missing_returns_none() {
        assert!(Tracks::get(&db(), "nope").unwrap().is_none());
    }

    #[test]
    fn upsert_updates_instead_of_duplicating() {
        let db = db();
        Tracks::upsert(&db, &track("abc12345678", "Old Title")).unwrap();
        Tracks::upsert(&db, &track("abc12345678", "New Title")).unwrap();

        assert_eq!(Tracks::count(&db).unwrap(), 1);
        assert_eq!(
            Tracks::get(&db, "abc12345678").unwrap().unwrap().title,
            "New Title"
        );
    }

    #[test]
    fn upsert_does_not_erase_known_metadata_with_nulls() {
        let db = db();
        let full = track("abc12345678", "Rosewood")
            .with_artist("Bonobo")
            .with_album("Black Sands")
            .with_duration(300);
        Tracks::upsert(&db, &full).unwrap();

        // A search result only knows the title — it must not wipe the rest.
        Tracks::upsert(&db, &track("abc12345678", "Rosewood")).unwrap();

        let loaded = Tracks::get(&db, "abc12345678").unwrap().unwrap();
        assert_eq!(loaded.artist.as_deref(), Some("Bonobo"));
        assert_eq!(loaded.album.as_deref(), Some("Black Sands"));
        assert_eq!(loaded.duration_secs, Some(300));
    }

    #[test]
    fn upsert_many_writes_every_track() {
        let db = db();
        let tracks = vec![
            track("aaaaaaaaaaa", "One"),
            track("bbbbbbbbbbb", "Two"),
            track("ccccccccccc", "Three"),
        ];
        Tracks::upsert_many(&db, &tracks).unwrap();
        assert_eq!(Tracks::count(&db).unwrap(), 3);
    }

    #[test]
    fn upsert_many_with_empty_slice_is_a_noop() {
        let db = db();
        Tracks::upsert_many(&db, &[]).unwrap();
        assert_eq!(Tracks::count(&db).unwrap(), 0);
    }

    #[test]
    fn all_returns_newest_first() {
        let db = db();
        Tracks::upsert(&db, &track("aaaaaaaaaaa", "First")).unwrap();
        Tracks::upsert(&db, &track("bbbbbbbbbbb", "Second")).unwrap();

        let all = Tracks::all(&db).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].title, "Second");
    }

    #[test]
    fn search_matches_title_and_artist_case_insensitively() {
        let db = db();
        Tracks::upsert(&db, &track("aaaaaaaaaaa", "Rosewood").with_artist("Bonobo")).unwrap();
        Tracks::upsert(&db, &track("bbbbbbbbbbb", "Cirrus").with_artist("Bonobo")).unwrap();
        Tracks::upsert(&db, &track("ccccccccccc", "Kong").with_artist("Other")).unwrap();

        assert_eq!(Tracks::search(&db, "rosewood", 10).unwrap().len(), 1);
        assert_eq!(Tracks::search(&db, "BONOBO", 10).unwrap().len(), 2);
        assert_eq!(Tracks::search(&db, "zzz", 10).unwrap().len(), 0);
    }

    #[test]
    fn search_respects_the_limit() {
        let db = db();
        for i in 0..5 {
            Tracks::upsert(&db, &track(&format!("id{i:09}"), "Same Title")).unwrap();
        }
        assert_eq!(Tracks::search(&db, "Same", 2).unwrap().len(), 2);
    }

    #[test]
    fn search_matches_album() {
        let db = db();
        Tracks::upsert(&db, &track("aaaaaaaaaaa", "Song").with_album("Black Sands")).unwrap();
        assert_eq!(Tracks::search(&db, "black", 10).unwrap().len(), 1);
    }

    #[test]
    fn delete_removes_the_track() {
        let db = db();
        Tracks::upsert(&db, &track("aaaaaaaaaaa", "One")).unwrap();
        Tracks::delete(&db, "aaaaaaaaaaa").unwrap();
        assert_eq!(Tracks::count(&db).unwrap(), 0);
    }

    #[test]
    fn delete_missing_is_not_an_error() {
        assert!(Tracks::delete(&db(), "nope").is_ok());
    }
}
