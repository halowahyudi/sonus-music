use sonus_core::Track;

use crate::db::{Database, Result};
use crate::tracks::{track_from_row, TRACK_COLUMNS};

/// Persistence for the user's liked tracks.
pub struct Favorites;

impl Favorites {
    /// Like a track. Also inserts the track itself if it's new.
    pub fn add(db: &Database, track: &Track) -> Result<()> {
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
        tx.execute(
            "INSERT OR IGNORE INTO favorites (youtube_id) VALUES (?1)",
            rusqlite::params![track.youtube_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn remove(db: &Database, youtube_id: &str) -> Result<()> {
        db.conn().execute(
            "DELETE FROM favorites WHERE youtube_id = ?1",
            rusqlite::params![youtube_id],
        )?;
        Ok(())
    }

    /// Like if unliked, unlike if liked. Returns the new state.
    pub fn toggle(db: &Database, track: &Track) -> Result<bool> {
        if Self::contains(db, &track.youtube_id)? {
            Self::remove(db, &track.youtube_id)?;
            Ok(false)
        } else {
            Self::add(db, track)?;
            Ok(true)
        }
    }

    pub fn contains(db: &Database, youtube_id: &str) -> Result<bool> {
        let exists: bool = db.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM favorites WHERE youtube_id = ?1)",
            rusqlite::params![youtube_id],
            |row| row.get(0),
        )?;
        Ok(exists)
    }

    /// Every liked track, most recently added first.
    pub fn all(db: &Database) -> Result<Vec<Track>> {
        let sql = format!(
            "SELECT {TRACK_COLUMNS} FROM tracks t
             JOIN favorites f ON t.youtube_id = f.youtube_id
             ORDER BY f.added_at DESC, f.rowid DESC"
        );
        let mut stmt = db.conn().prepare(&sql)?;
        let tracks = stmt
            .query_map([], track_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(tracks)
    }

    /// The set of liked YouTube IDs — used to mark rows in the UI without a
    /// query per row.
    pub fn ids(db: &Database) -> Result<Vec<String>> {
        let mut stmt = db.conn().prepare("SELECT youtube_id FROM favorites")?;
        let ids = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(ids)
    }

    pub fn count(db: &Database) -> Result<usize> {
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM favorites", [], |r| r.get(0))?;
        Ok(count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        Database::in_memory().unwrap()
    }

    fn track(id: &str) -> Track {
        Track::new(id, format!("Song {id}"))
    }

    #[test]
    fn add_then_contains() {
        let db = db();
        Favorites::add(&db, &track("aaaaaaaaaaa")).unwrap();
        assert!(Favorites::contains(&db, "aaaaaaaaaaa").unwrap());
        assert!(!Favorites::contains(&db, "bbbbbbbbbbb").unwrap());
    }

    #[test]
    fn adding_twice_is_idempotent() {
        let db = db();
        Favorites::add(&db, &track("aaaaaaaaaaa")).unwrap();
        Favorites::add(&db, &track("aaaaaaaaaaa")).unwrap();
        assert_eq!(Favorites::count(&db).unwrap(), 1);
    }

    #[test]
    fn remove_unlikes() {
        let db = db();
        Favorites::add(&db, &track("aaaaaaaaaaa")).unwrap();
        Favorites::remove(&db, "aaaaaaaaaaa").unwrap();
        assert!(!Favorites::contains(&db, "aaaaaaaaaaa").unwrap());
    }

    #[test]
    fn remove_missing_is_not_an_error() {
        assert!(Favorites::remove(&db(), "nope").is_ok());
    }

    #[test]
    fn toggle_flips_the_state() {
        let db = db();
        let t = track("aaaaaaaaaaa");
        assert!(Favorites::toggle(&db, &t).unwrap());
        assert!(Favorites::contains(&db, "aaaaaaaaaaa").unwrap());
        assert!(!Favorites::toggle(&db, &t).unwrap());
        assert!(!Favorites::contains(&db, "aaaaaaaaaaa").unwrap());
    }

    #[test]
    fn all_returns_newest_first() {
        let db = db();
        Favorites::add(&db, &track("aaaaaaaaaaa")).unwrap();
        Favorites::add(&db, &track("bbbbbbbbbbb")).unwrap();

        let all = Favorites::all(&db).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].youtube_id, "bbbbbbbbbbb");
    }

    #[test]
    fn ids_lists_every_liked_track() {
        let db = db();
        Favorites::add(&db, &track("aaaaaaaaaaa")).unwrap();
        Favorites::add(&db, &track("bbbbbbbbbbb")).unwrap();

        let mut ids = Favorites::ids(&db).unwrap();
        ids.sort();
        assert_eq!(ids, ["aaaaaaaaaaa", "bbbbbbbbbbb"]);
    }

    #[test]
    fn liking_creates_the_track_row() {
        let db = db();
        Favorites::add(&db, &track("aaaaaaaaaaa")).unwrap();
        assert!(crate::tracks::Tracks::get(&db, "aaaaaaaaaaa")
            .unwrap()
            .is_some());
    }

    #[test]
    fn empty_favorites_is_empty() {
        let db = db();
        assert_eq!(Favorites::count(&db).unwrap(), 0);
        assert!(Favorites::all(&db).unwrap().is_empty());
        assert!(Favorites::ids(&db).unwrap().is_empty());
    }
}
