use sonus_core::Track;

use crate::db::{Database, Result};
use crate::tracks::{track_from_row, TRACK_COLUMNS};

/// Play history.
pub struct History;

impl History {
    /// Record a play. Called each time playback starts on a track.
    ///
    /// The same track can appear many times — this is a log, not a set. Use
    /// `recent` to get a de-duplicated view.
    pub fn record(db: &Database, track: &Track) -> Result<()> {
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
            "INSERT INTO history (youtube_id) VALUES (?1)",
            rusqlite::params![track.youtube_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Most recently played tracks, newest first, without repeats.
    ///
    /// The GROUP BY collapses repeat plays — "recently played" should show a
    /// song once, at the time you last heard it.
    pub fn recent(db: &Database, limit: usize) -> Result<Vec<Track>> {
        let sql = format!(
            "SELECT {TRACK_COLUMNS} FROM tracks t
             JOIN (
                SELECT youtube_id,
                       MAX(played_at) AS last_played,
                       MAX(id) AS last_id
                FROM history
                GROUP BY youtube_id
             ) h ON t.youtube_id = h.youtube_id
             ORDER BY h.last_played DESC, h.last_id DESC
             LIMIT ?1"
        );
        let mut stmt = db.conn().prepare(&sql)?;
        let tracks = stmt
            .query_map(rusqlite::params![limit as i64], track_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(tracks)
    }

    /// How many times a track has been played.
    pub fn play_count(db: &Database, youtube_id: &str) -> Result<usize> {
        let count: i64 = db.conn().query_row(
            "SELECT COUNT(*) FROM history WHERE youtube_id = ?1",
            rusqlite::params![youtube_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// The user's most-played tracks.
    ///
    /// Ties are broken by recency so the order is stable and the newest
    /// favourite wins when two tracks have the same play count.
    pub fn most_played(db: &Database, limit: usize) -> Result<Vec<Track>> {
        let sql = format!(
            "SELECT {TRACK_COLUMNS} FROM tracks t
             JOIN (
                SELECT youtube_id,
                       COUNT(*) AS plays,
                       MAX(played_at) AS last_played,
                       MAX(id) AS last_id
                FROM history
                GROUP BY youtube_id
             ) h ON t.youtube_id = h.youtube_id
             ORDER BY h.plays DESC, h.last_played DESC, h.last_id DESC
             LIMIT ?1"
        );
        let mut stmt = db.conn().prepare(&sql)?;
        let tracks = stmt
            .query_map(rusqlite::params![limit as i64], track_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(tracks)
    }

    pub fn total_plays(db: &Database) -> Result<usize> {
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))?;
        Ok(count as usize)
    }

    pub fn clear(db: &Database) -> Result<()> {
        db.conn().execute("DELETE FROM history", [])?;
        Ok(())
    }

    /// Drop all but the newest `keep` entries.
    ///
    /// History grows without bound otherwise, and it's the kind of table that
    /// quietly turns into tens of megabytes over a couple of years.
    pub fn prune(db: &Database, keep: usize) -> Result<usize> {
        let removed = db.conn().execute(
            "DELETE FROM history WHERE id NOT IN (
                SELECT id FROM history ORDER BY played_at DESC, id DESC LIMIT ?1
             )",
            rusqlite::params![keep as i64],
        )?;
        Ok(removed)
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
    fn record_then_recent() {
        let db = db();
        History::record(&db, &track("aaaaaaaaaaa")).unwrap();
        let recent = History::recent(&db, 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].youtube_id, "aaaaaaaaaaa");
    }

    #[test]
    fn recent_is_newest_first() {
        let db = db();
        History::record(&db, &track("aaaaaaaaaaa")).unwrap();
        History::record(&db, &track("bbbbbbbbbbb")).unwrap();

        let recent = History::recent(&db, 10).unwrap();
        assert_eq!(recent[0].youtube_id, "bbbbbbbbbbb");
    }

    #[test]
    fn recent_collapses_repeat_plays() {
        let db = db();
        History::record(&db, &track("aaaaaaaaaaa")).unwrap();
        History::record(&db, &track("aaaaaaaaaaa")).unwrap();
        History::record(&db, &track("aaaaaaaaaaa")).unwrap();

        let recent = History::recent(&db, 10).unwrap();
        assert_eq!(recent.len(), 1, "a song should appear once in recent");
    }

    #[test]
    fn recent_respects_the_limit() {
        let db = db();
        for i in 0..5 {
            History::record(&db, &track(&format!("id{i:09}"))).unwrap();
        }
        assert_eq!(History::recent(&db, 2).unwrap().len(), 2);
    }

    #[test]
    fn play_count_tracks_repeats() {
        let db = db();
        let t = track("aaaaaaaaaaa");
        assert_eq!(History::play_count(&db, "aaaaaaaaaaa").unwrap(), 0);
        History::record(&db, &t).unwrap();
        History::record(&db, &t).unwrap();
        assert_eq!(History::play_count(&db, "aaaaaaaaaaa").unwrap(), 2);
    }

    #[test]
    fn most_played_orders_by_count() {
        let db = db();
        let a = track("aaaaaaaaaaa");
        let b = track("bbbbbbbbbbb");

        History::record(&db, &a).unwrap();
        History::record(&db, &b).unwrap();
        History::record(&db, &b).unwrap();

        let top = History::most_played(&db, 10).unwrap();
        assert_eq!(top[0].youtube_id, "bbbbbbbbbbb");
        assert_eq!(top[1].youtube_id, "aaaaaaaaaaa");
    }

    #[test]
    fn total_plays_counts_every_row() {
        let db = db();
        let t = track("aaaaaaaaaaa");
        History::record(&db, &t).unwrap();
        History::record(&db, &t).unwrap();
        assert_eq!(History::total_plays(&db).unwrap(), 2);
    }

    #[test]
    fn clear_empties_the_log() {
        let db = db();
        History::record(&db, &track("aaaaaaaaaaa")).unwrap();
        History::clear(&db).unwrap();
        assert_eq!(History::total_plays(&db).unwrap(), 0);
        assert!(History::recent(&db, 10).unwrap().is_empty());
    }

    #[test]
    fn recording_creates_the_track_row() {
        let db = db();
        History::record(&db, &track("aaaaaaaaaaa")).unwrap();
        assert!(crate::tracks::Tracks::get(&db, "aaaaaaaaaaa")
            .unwrap()
            .is_some());
    }

    #[test]
    fn prune_keeps_the_newest_entries() {
        let db = db();
        for i in 0..10 {
            History::record(&db, &track(&format!("id{i:09}"))).unwrap();
        }
        let removed = History::prune(&db, 3).unwrap();
        assert_eq!(removed, 7);
        assert_eq!(History::total_plays(&db).unwrap(), 3);
    }

    #[test]
    fn prune_under_the_limit_removes_nothing() {
        let db = db();
        History::record(&db, &track("aaaaaaaaaaa")).unwrap();
        assert_eq!(History::prune(&db, 100).unwrap(), 0);
    }

    #[test]
    fn empty_history_reads_back_empty() {
        let db = db();
        assert!(History::recent(&db, 10).unwrap().is_empty());
        assert!(History::most_played(&db, 10).unwrap().is_empty());
        assert_eq!(History::total_plays(&db).unwrap(), 0);
    }
}
