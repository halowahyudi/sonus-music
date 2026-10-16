use sonus_core::playlist::{Playlist, PlaylistId};
use sonus_core::Track;

use crate::db::{Database, LibraryError, Result};
use crate::tracks::{track_from_row, TRACK_COLUMNS};

/// Persistence for named playlists.
pub struct Playlists;

impl Playlists {
    /// Create an empty playlist and return its new ID.
    pub fn create(db: &Database, name: &str) -> Result<PlaylistId> {
        let playlist = Playlist::new(name);
        db.conn().execute(
            "INSERT INTO playlists (id, name) VALUES (?1, ?2)",
            rusqlite::params![playlist.id, playlist.name],
        )?;
        Ok(playlist.id)
    }

    /// Create a playlist with tracks already in it, in one transaction.
    pub fn create_with(db: &Database, name: &str, tracks: &[Track]) -> Result<PlaylistId> {
        let playlist = Playlist::new(name);
        let tx = db.conn().unchecked_transaction()?;
        {
            tx.execute(
                "INSERT INTO playlists (id, name) VALUES (?1, ?2)",
                rusqlite::params![playlist.id, playlist.name],
            )?;

            let mut insert_track = tx.prepare(
                "INSERT INTO tracks
                    (youtube_id, title, artist, album, duration_secs, thumbnail_url)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(youtube_id) DO UPDATE SET
                    title = excluded.title, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
            )?;
            let mut insert_link = tx.prepare(
                "INSERT OR IGNORE INTO playlist_tracks (playlist_id, youtube_id, position)
                 VALUES (?1, ?2, ?3)",
            )?;

            for (position, track) in tracks.iter().enumerate() {
                insert_track.execute(rusqlite::params![
                    track.youtube_id,
                    track.title,
                    track.artist,
                    track.album,
                    track.duration_secs,
                    track.thumbnail_url,
                ])?;
                insert_link.execute(rusqlite::params![
                    playlist.id,
                    track.youtube_id,
                    position as i64
                ])?;
            }
        }
        tx.commit()?;
        Ok(playlist.id)
    }

    pub fn rename(db: &Database, id: &str, name: &str) -> Result<()> {
        let changed = db.conn().execute(
            "UPDATE playlists SET name = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
            rusqlite::params![id, name],
        )?;
        if changed == 0 {
            return Err(LibraryError::NotFound(format!("playlist {id}")));
        }
        Ok(())
    }

    /// Append a track. No-op if it's already in the playlist.
    pub fn add_track(db: &Database, playlist_id: &str, track: &Track) -> Result<bool> {
        let tx = db.conn().unchecked_transaction()?;

        tx.execute(
            "INSERT INTO tracks
                (youtube_id, title, artist, album, duration_secs, thumbnail_url)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(youtube_id) DO UPDATE SET
                title = excluded.title, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
            rusqlite::params![
                track.youtube_id,
                track.title,
                track.artist,
                track.album,
                track.duration_secs,
                track.thumbnail_url,
            ],
        )?;

        // Append past the current maximum so ordering survives removals.
        let next: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_tracks WHERE playlist_id = ?1",
            rusqlite::params![playlist_id],
            |row| row.get(0),
        )?;

        let inserted = tx.execute(
            "INSERT OR IGNORE INTO playlist_tracks (playlist_id, youtube_id, position)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![playlist_id, track.youtube_id, next],
        )?;

        tx.execute(
            "UPDATE playlists SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
            rusqlite::params![playlist_id],
        )?;

        tx.commit()?;
        Ok(inserted > 0)
    }

    /// Remove a track and close the gap in the ordering.
    pub fn remove_track(db: &Database, playlist_id: &str, youtube_id: &str) -> Result<bool> {
        let tx = db.conn().unchecked_transaction()?;

        let removed = tx.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND youtube_id = ?2",
            rusqlite::params![playlist_id, youtube_id],
        )?;

        // Positions are only used for ordering, so compacting them keeps the
        // numbers meaningful when the playlist is inspected directly.
        tx.execute(
            "UPDATE playlist_tracks SET position = (
                SELECT COUNT(*) FROM playlist_tracks AS other
                WHERE other.playlist_id = playlist_tracks.playlist_id
                  AND other.position < playlist_tracks.position
             )
             WHERE playlist_id = ?1",
            rusqlite::params![playlist_id],
        )?;

        tx.execute(
            "UPDATE playlists SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
            rusqlite::params![playlist_id],
        )?;

        tx.commit()?;
        Ok(removed > 0)
    }

    /// Move a track within a playlist.
    pub fn reorder(db: &Database, playlist_id: &str, from: usize, to: usize) -> Result<bool> {
        let mut playlist = match Self::get(db, playlist_id)? {
            Some(playlist) => playlist,
            None => return Ok(false),
        };
        if !playlist.reorder(from, to) {
            return Ok(false);
        }

        let tx = db.conn().unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "UPDATE playlist_tracks SET position = ?3
                 WHERE playlist_id = ?1 AND youtube_id = ?2",
            )?;
            for (position, track) in playlist.tracks.iter().enumerate() {
                stmt.execute(rusqlite::params![
                    playlist_id,
                    track.youtube_id,
                    position as i64
                ])?;
            }
        }
        tx.commit()?;
        Ok(true)
    }

    /// Load a playlist with its tracks, in order.
    pub fn get(db: &Database, id: &str) -> Result<Option<Playlist>> {
        let name: Option<String> = db
            .conn()
            .query_row(
                "SELECT name FROM playlists WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .ok();

        let Some(name) = name else {
            return Ok(None);
        };

        let sql = format!(
            "SELECT {TRACK_COLUMNS} FROM tracks t
             JOIN playlist_tracks pt ON t.youtube_id = pt.youtube_id
             WHERE pt.playlist_id = ?1
             ORDER BY pt.position"
        );
        let mut stmt = db.conn().prepare(&sql)?;
        let tracks = stmt
            .query_map(rusqlite::params![id], track_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(Some(Playlist {
            id: id.to_string(),
            name,
            tracks,
        }))
    }

    /// Every playlist as `(id, name, track count)`, newest first.
    pub fn all(db: &Database) -> Result<Vec<(PlaylistId, String, usize)>> {
        let mut stmt = db.conn().prepare(
            "SELECT p.id, p.name, COUNT(pt.youtube_id)
             FROM playlists p
             LEFT JOIN playlist_tracks pt ON p.id = pt.playlist_id
             GROUP BY p.id, p.name
             ORDER BY p.updated_at DESC",
        )?;

        let playlists = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? as usize))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(playlists)
    }

    pub fn exists(db: &Database, id: &str) -> Result<bool> {
        let exists: bool = db.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM playlists WHERE id = ?1)",
            rusqlite::params![id],
            |row| row.get(0),
        )?;
        Ok(exists)
    }

    pub fn delete(db: &Database, id: &str) -> Result<()> {
        db.conn()
            .execute("DELETE FROM playlists WHERE id = ?1", rusqlite::params![id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracks::Tracks;

    fn db() -> Database {
        Database::in_memory().unwrap()
    }

    fn track(id: &str) -> Track {
        Track::new(id, format!("Song {id}")).with_duration(180)
    }

    #[test]
    fn create_then_get_is_empty() {
        let db = db();
        let id = Playlists::create(&db, "Chill").unwrap();
        let playlist = Playlists::get(&db, &id).unwrap().unwrap();
        assert_eq!(playlist.name, "Chill");
        assert!(playlist.is_empty());
    }

    #[test]
    fn get_missing_returns_none() {
        assert!(Playlists::get(&db(), "nope").unwrap().is_none());
    }

    #[test]
    fn add_track_persists_and_orders() {
        let db = db();
        let id = Playlists::create(&db, "Chill").unwrap();
        assert!(Playlists::add_track(&db, &id, &track("aaaaaaaaaaa")).unwrap());
        assert!(Playlists::add_track(&db, &id, &track("bbbbbbbbbbb")).unwrap());

        let playlist = Playlists::get(&db, &id).unwrap().unwrap();
        assert_eq!(playlist.len(), 2);
        assert_eq!(playlist.get(0).unwrap().youtube_id, "aaaaaaaaaaa");
        assert_eq!(playlist.get(1).unwrap().youtube_id, "bbbbbbbbbbb");
    }

    #[test]
    fn add_track_rejects_duplicates() {
        let db = db();
        let id = Playlists::create(&db, "Chill").unwrap();
        assert!(Playlists::add_track(&db, &id, &track("aaaaaaaaaaa")).unwrap());
        assert!(!Playlists::add_track(&db, &id, &track("aaaaaaaaaaa")).unwrap());
        assert_eq!(Playlists::get(&db, &id).unwrap().unwrap().len(), 1);
    }

    #[test]
    fn adding_a_track_creates_the_track_row() {
        let db = db();
        let id = Playlists::create(&db, "Chill").unwrap();
        Playlists::add_track(&db, &id, &track("aaaaaaaaaaa")).unwrap();
        assert!(Tracks::get(&db, "aaaaaaaaaaa").unwrap().is_some());
    }

    #[test]
    fn positions_stay_contiguous_after_removal() {
        let db = db();
        let id = Playlists::create(&db, "Chill").unwrap();
        for tid in ["aaaaaaaaaaa", "bbbbbbbbbbb", "ccccccccccc"] {
            Playlists::add_track(&db, &id, &track(tid)).unwrap();
        }
        Playlists::remove_track(&db, &id, "bbbbbbbbbbb").unwrap();

        let positions: Vec<i64> = {
            let mut stmt = db
                .conn()
                .prepare(
                    "SELECT position FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
                )
                .unwrap();
            stmt.query_map(rusqlite::params![id], |r| r.get(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };
        assert_eq!(positions, vec![0, 1]);
    }

    #[test]
    fn remove_track_reports_whether_it_did_anything() {
        let db = db();
        let id = Playlists::create(&db, "Chill").unwrap();
        Playlists::add_track(&db, &id, &track("aaaaaaaaaaa")).unwrap();
        assert!(Playlists::remove_track(&db, &id, "aaaaaaaaaaa").unwrap());
        assert!(!Playlists::remove_track(&db, &id, "aaaaaaaaaaa").unwrap());
    }

    #[test]
    fn reorder_changes_playback_order() {
        let db = db();
        let id = Playlists::create(&db, "Chill").unwrap();
        for tid in ["aaaaaaaaaaa", "bbbbbbbbbbb", "ccccccccccc"] {
            Playlists::add_track(&db, &id, &track(tid)).unwrap();
        }

        assert!(Playlists::reorder(&db, &id, 0, 2).unwrap());

        let playlist = Playlists::get(&db, &id).unwrap().unwrap();
        let order: Vec<&str> = playlist
            .tracks
            .iter()
            .map(|t| t.youtube_id.as_str())
            .collect();
        assert_eq!(order, ["bbbbbbbbbbb", "ccccccccccc", "aaaaaaaaaaa"]);
    }

    #[test]
    fn reorder_on_a_missing_playlist_is_false() {
        assert!(!Playlists::reorder(&db(), "nope", 0, 1).unwrap());
    }

    #[test]
    fn create_with_populates_in_order() {
        let db = db();
        let tracks = vec![track("aaaaaaaaaaa"), track("bbbbbbbbbbb")];
        let id = Playlists::create_with(&db, "Chill", &tracks).unwrap();

        let playlist = Playlists::get(&db, &id).unwrap().unwrap();
        assert_eq!(playlist.len(), 2);
        assert_eq!(playlist.get(0).unwrap().youtube_id, "aaaaaaaaaaa");
    }

    #[test]
    fn rename_updates_the_name() {
        let db = db();
        let id = Playlists::create(&db, "Old").unwrap();
        Playlists::rename(&db, &id, "New").unwrap();
        assert_eq!(Playlists::get(&db, &id).unwrap().unwrap().name, "New");
    }

    #[test]
    fn rename_missing_is_an_error() {
        assert!(Playlists::rename(&db(), "nope", "New").is_err());
    }

    #[test]
    fn all_lists_playlists_with_counts() {
        let db = db();
        let a = Playlists::create(&db, "First").unwrap();
        Playlists::create(&db, "Second").unwrap();
        Playlists::add_track(&db, &a, &track("aaaaaaaaaaa")).unwrap();

        let all = Playlists::all(&db).unwrap();
        assert_eq!(all.len(), 2);

        let first = all.iter().find(|(_, name, _)| name == "First").unwrap();
        assert_eq!(first.2, 1);
    }

    #[test]
    fn deleting_a_playlist_cascades_to_its_tracks() {
        let db = db();
        let id = Playlists::create(&db, "Chill").unwrap();
        Playlists::add_track(&db, &id, &track("aaaaaaaaaaa")).unwrap();

        Playlists::delete(&db, &id).unwrap();

        assert!(!Playlists::exists(&db, &id).unwrap());
        let links: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM playlist_tracks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(links, 0, "join rows should cascade away");
        // The track itself survives — it may be in other playlists.
        assert!(Tracks::get(&db, "aaaaaaaaaaa").unwrap().is_some());
    }

    #[test]
    fn exists_reports_membership() {
        let db = db();
        let id = Playlists::create(&db, "Chill").unwrap();
        assert!(Playlists::exists(&db, &id).unwrap());
        assert!(!Playlists::exists(&db, "nope").unwrap());
    }

    #[test]
    fn same_track_can_live_in_two_playlists() {
        let db = db();
        let a = Playlists::create(&db, "A").unwrap();
        let b = Playlists::create(&db, "B").unwrap();
        Playlists::add_track(&db, &a, &track("aaaaaaaaaaa")).unwrap();
        Playlists::add_track(&db, &b, &track("aaaaaaaaaaa")).unwrap();

        assert_eq!(Playlists::get(&db, &a).unwrap().unwrap().len(), 1);
        assert_eq!(Playlists::get(&db, &b).unwrap().unwrap().len(), 1);
    }
}
