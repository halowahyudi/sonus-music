use std::collections::HashSet;

use sonus_core::Track;

use crate::db::{Database, Result};
use crate::tracks::{track_from_row, TRACK_COLUMNS};

/// Local, offline-friendly recommendations drawn from play history.
///
/// This is the last-resort radio: it works with no network at all. The mix
/// prefers tracks by the same artist, then the user's most-played songs, and
/// finishes with stuff they haven't heard in a while for variety.
pub struct Recommendations;

impl Recommendations {
    /// Discover tracks similar to `seed`, excluding anything the caller
    /// already has queued.
    ///
    /// Artists are matched case-insensitively because YouTube metadata is
    /// inconsistent about casing and "artist feat." suffixes.
    pub fn similar(
        db: &Database,
        seed: &Track,
        exclude: &[String],
        limit: usize,
    ) -> Result<Vec<Track>> {
        let mut picks: Vec<Track> = Vec::new();
        let mut seen: std::collections::HashSet<String> =
            std::iter::once(seed.youtube_id.clone()).collect();
        seen.extend(exclude.iter().cloned());

        // 1. Same artist, most played first. The exact-artist match is the
        //    strongest signal we have locally.
        if let Some(artist) = seed.artist.as_deref().filter(|a| !a.is_empty()) {
            picks.extend(same_artist(db, artist, &seen, limit)?);
            seen.extend(picks.iter().map(|t| t.youtube_id.clone()));
        }

        // 2. Top of the most-played chart, skipping the same-artist picks.
        if picks.len() < limit {
            picks.extend(top_picks(db, &seen, limit - picks.len())?);
            seen.extend(picks.iter().map(|t| t.youtube_id.clone()));
        }

        // 3. Rarely-heard tracks — play history's oldest entries. Keeps the
        //    mix from going stale when the same ten songs dominate history.
        if picks.len() < limit {
            picks.extend(rarely_played(db, &seen, limit - picks.len())?);
        }

        Ok(picks)
    }
}

/// Tracks by this artist that are already in the library, most-played first.
///
/// Anonymous `?` bindings keep the parameter numbers right regardless of the
/// exclude set size — named `?N` would collide with the offsets here.
fn same_artist(
    db: &Database,
    artist: &str,
    exclude: &HashSet<String>,
    limit: usize,
) -> Result<Vec<Track>> {
    if limit == 0 {
        return Ok(vec![]);
    }
    let placeholders = placeholders(exclude.len());
    let sql = format!(
        "SELECT {TRACK_COLUMNS}
         FROM tracks t
         JOIN (
            SELECT youtube_id,
                   COUNT(*) AS plays,
                   MAX(played_at) AS last_played,
                   MAX(id) AS last_id
            FROM history
            GROUP BY youtube_id
         ) h ON t.youtube_id = h.youtube_id
         WHERE LOWER(t.artist) = LOWER(?)
           AND t.youtube_id NOT IN ({placeholders})
         ORDER BY h.plays DESC, h.last_played DESC, h.last_id DESC
         LIMIT ?"
    );
    let limit_i64 = limit as i64;
    let mut params: Vec<&dyn rusqlite::ToSql> = vec![&artist];
    params.extend(exclude.iter().map(|id| id as &dyn rusqlite::ToSql));
    params.push(&limit_i64);

    let mut stmt = db.conn().prepare(&sql)?;
    let tracks = stmt
        .query_map(rusqlite::params_from_iter(params), track_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tracks)
}

/// Known tracks the user plays most, newest-wins on ties.
fn top_picks(db: &Database, exclude: &HashSet<String>, limit: usize) -> Result<Vec<Track>> {
    if limit == 0 {
        return Ok(vec![]);
    }
    let placeholders = placeholders(exclude.len());
    let sql = format!(
        "SELECT {TRACK_COLUMNS}
         FROM tracks t
         JOIN (
            SELECT youtube_id,
                   COUNT(*) AS plays,
                   MAX(played_at) AS last_played,
                   MAX(id) AS last_id
            FROM history
            GROUP BY youtube_id
         ) h ON t.youtube_id = h.youtube_id
         WHERE t.youtube_id NOT IN ({placeholders})
         ORDER BY h.plays DESC, h.last_played DESC, h.last_id DESC
         LIMIT ?"
    );
    let limit_i64 = limit as i64;
    let mut params: Vec<&dyn rusqlite::ToSql> = exclude
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    params.push(&limit_i64);

    let mut stmt = db.conn().prepare(&sql)?;
    let tracks = stmt
        .query_map(rusqlite::params_from_iter(params), track_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tracks)
}

/// The least recently played songs — history's oldest survivors.
fn rarely_played(db: &Database, exclude: &HashSet<String>, limit: usize) -> Result<Vec<Track>> {
    if limit == 0 {
        return Ok(vec![]);
    }
    let placeholders = placeholders(exclude.len());
    let sql = format!(
        "SELECT {TRACK_COLUMNS}
         FROM tracks t
         JOIN (
            SELECT youtube_id,
                   MIN(played_at) AS first_played,
                   MAX(id) AS last_id
            FROM history
            GROUP BY youtube_id
         ) h ON t.youtube_id = h.youtube_id
         WHERE t.youtube_id NOT IN ({placeholders})
         ORDER BY h.first_played ASC, h.last_id ASC
         LIMIT ?"
    );
    let limit_i64 = limit as i64;
    let mut params: Vec<&dyn rusqlite::ToSql> = exclude
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    params.push(&limit_i64);

    let mut stmt = db.conn().prepare(&sql)?;
    let tracks = stmt
        .query_map(rusqlite::params_from_iter(params), track_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tracks)
}

/// `?, ?, ?` — or an always-true clause when the set is empty, since
/// `NOT IN ()` is invalid SQL.
fn placeholders(len: usize) -> String {
    if len == 0 {
        return "''".to_string();
    }
    vec!["?"; len].join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        Database::in_memory().unwrap()
    }

    fn track(id: &str, artist: &str) -> Track {
        Track::new(id, format!("Song {id}")).with_artist(artist)
    }

    fn record(db: &Database, t: &Track, times: usize) {
        for _ in 0..times {
            crate::history::History::record(db, t).unwrap();
        }
    }

    #[test]
    fn same_artist_tracks_come_first() {
        let db = db();
        let seed = track("aaaaaaaaaaa", "Bonobo");
        record(&db, &seed, 2);
        let same = track("bbbbbbbbbbb", "Bonobo");
        record(&db, &same, 1);
        let other = track("ccccccccccc", "Other");
        record(&db, &other, 5);

        let picks = Recommendations::similar(&db, &seed, &[], 10).unwrap();
        assert_eq!(picks[0].youtube_id, "bbbbbbbbbbb", "same artist wins");
        assert!(
            picks.iter().any(|t| t.youtube_id == "ccccccccccc"),
            "top picks round out the list"
        );
    }

    #[test]
    fn seed_and_queued_tracks_are_excluded() {
        let db = db();
        let seed = track("aaaaaaaaaaa", "Bonobo");
        record(&db, &seed, 2);
        let queued = track("bbbbbbbbbbb", "Bonobo");
        record(&db, &queued, 1);
        let free = track("ccccccccccc", "Bonobo");
        record(&db, &free, 1);

        let picks = Recommendations::similar(&db, &seed, &["bbbbbbbbbbb".to_string()], 10).unwrap();
        assert!(!picks.iter().any(|t| t.youtube_id == "aaaaaaaaaaa"));
        assert!(!picks.iter().any(|t| t.youtube_id == "bbbbbbbbbbb"));
        assert!(picks.iter().any(|t| t.youtube_id == "ccccccccccc"));
    }

    #[test]
    fn artist_match_is_case_insensitive() {
        let db = db();
        let seed = track("aaaaaaaaaaa", "bonobo");
        record(&db, &seed, 1);
        let same = track("bbbbbbbbbbb", "BONOBO");
        record(&db, &same, 1);

        let picks = Recommendations::similar(&db, &seed, &[], 10).unwrap();
        assert!(picks.iter().any(|t| t.youtube_id == "bbbbbbbbbbb"));
    }

    #[test]
    fn empty_history_returns_nothing() {
        let db = db();
        let seed = track("aaaaaaaaaaa", "Bonobo");
        let picks = Recommendations::similar(&db, &seed, &[], 10).unwrap();
        assert!(picks.is_empty());
    }

    #[test]
    fn limit_is_respected() {
        let db = db();
        for i in 0..5 {
            let t = track(&format!("id{i:09}"), "Bonobo");
            record(&db, &t, 1);
        }
        let seed = track("aaaaaaaaaaa", "Other");
        let picks = Recommendations::similar(&db, &seed, &[], 3).unwrap();
        assert_eq!(picks.len(), 3);
    }

    #[test]
    fn no_artist_seed_falls_through_to_top_picks() {
        let db = db();
        let seed = Track::new("aaaaaaaaaaa", "No Artist");
        record(&db, &seed, 4);
        let a = track("bbbbbbbbbbb", "Who");
        record(&db, &a, 1);

        let picks = Recommendations::similar(&db, &seed, &[], 10).unwrap();
        assert!(!picks.is_empty());
        // The seed itself is always excluded.
        assert!(!picks.iter().any(|t| t.youtube_id == "aaaaaaaaaaa"));
        assert!(picks.iter().any(|t| t.youtube_id == "bbbbbbbbbbb"));
    }
}
