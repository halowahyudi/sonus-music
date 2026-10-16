use serde::{Deserialize, Serialize};

use crate::track::Track;

pub type PlaylistId = String;

/// A named, ordered collection of tracks.
///
/// Stored in the library database. `id` is a UUID string so renaming a playlist
/// doesn't change its identity, unlike tracks which key on the YouTube ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: PlaylistId,
    pub name: String,
    pub tracks: Vec<Track>,
}

impl Playlist {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            tracks: Vec::new(),
        }
    }

    pub fn with_id(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            tracks: Vec::new(),
        }
    }

    /// Append a track. No-op if it's already in the playlist — a playlist
    /// shouldn't contain the same song twice.
    pub fn add(&mut self, track: Track) -> bool {
        if self.contains(&track.youtube_id) {
            return false;
        }
        self.tracks.push(track);
        true
    }

    /// Insert at a specific index, clamped to the end.
    pub fn insert(&mut self, index: usize, track: Track) -> bool {
        if self.contains(&track.youtube_id) {
            return false;
        }
        let index = index.min(self.tracks.len());
        self.tracks.insert(index, track);
        true
    }

    pub fn remove(&mut self, youtube_id: &str) -> bool {
        let before = self.tracks.len();
        self.tracks.retain(|t| t.youtube_id != youtube_id);
        self.tracks.len() != before
    }

    /// Move a track from one position to another. Returns false if the source
    /// index is out of range.
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tracks.len() {
            return false;
        }
        let track = self.tracks.remove(from);
        let to = to.min(self.tracks.len());
        self.tracks.insert(to, track);
        true
    }

    pub fn contains(&self, youtube_id: &str) -> bool {
        self.tracks.iter().any(|t| t.youtube_id == youtube_id)
    }

    pub fn get(&self, index: usize) -> Option<&Track> {
        self.tracks.get(index)
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// Total runtime, ignoring tracks with unknown duration.
    pub fn total_duration_secs(&self) -> u64 {
        self.tracks.iter().filter_map(|t| t.duration_secs).sum()
    }

    /// Total runtime as `H:MM:SS` or `M:SS`.
    pub fn total_duration_label(&self) -> String {
        let total = self.total_duration_secs();
        let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
        if h > 0 {
            format!("{h}:{m:02}:{s:02}")
        } else {
            format!("{m}:{s:02}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track::new(id, format!("Song {id}"))
    }

    #[test]
    fn new_playlist_is_empty() {
        let p = Playlist::new("Chill");
        assert_eq!(p.name, "Chill");
        assert!(p.is_empty());
        assert!(!p.id.is_empty());
    }

    #[test]
    fn ids_are_unique() {
        assert_ne!(Playlist::new("A").id, Playlist::new("A").id);
    }

    #[test]
    fn add_appends_in_order() {
        let mut p = Playlist::new("Chill");
        assert!(p.add(track("a")));
        assert!(p.add(track("b")));
        assert_eq!(p.len(), 2);
        assert_eq!(p.get(0).unwrap().youtube_id, "a");
        assert_eq!(p.get(1).unwrap().youtube_id, "b");
    }

    #[test]
    fn add_rejects_duplicates() {
        let mut p = Playlist::new("Chill");
        assert!(p.add(track("a")));
        assert!(!p.add(track("a")));
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn insert_clamps_to_end() {
        let mut p = Playlist::new("Chill");
        p.add(track("a"));
        assert!(p.insert(99, track("b")));
        assert_eq!(p.get(1).unwrap().youtube_id, "b");
    }

    #[test]
    fn insert_rejects_duplicates() {
        let mut p = Playlist::new("Chill");
        p.add(track("a"));
        assert!(!p.insert(0, track("a")));
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn remove_reports_whether_it_did_anything() {
        let mut p = Playlist::new("Chill");
        p.add(track("a"));
        assert!(p.remove("a"));
        assert!(!p.remove("a"));
        assert!(p.is_empty());
    }

    #[test]
    fn reorder_moves_track() {
        let mut p = Playlist::new("Chill");
        for id in ["a", "b", "c"] {
            p.add(track(id));
        }
        assert!(p.reorder(0, 2));
        let order: Vec<&str> = p.tracks.iter().map(|t| t.youtube_id.as_str()).collect();
        assert_eq!(order, ["b", "c", "a"]);
    }

    #[test]
    fn reorder_out_of_range_is_a_noop() {
        let mut p = Playlist::new("Chill");
        p.add(track("a"));
        assert!(!p.reorder(5, 0));
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn contains_checks_membership() {
        let mut p = Playlist::new("Chill");
        p.add(track("a"));
        assert!(p.contains("a"));
        assert!(!p.contains("b"));
    }

    #[test]
    fn total_duration_sums_known_tracks() {
        let mut p = Playlist::new("Chill");
        p.add(track("a").with_duration(60));
        p.add(track("b").with_duration(120));
        p.add(track("c")); // unknown duration — skipped
        assert_eq!(p.total_duration_secs(), 180);
        assert_eq!(p.total_duration_label(), "3:00");
    }

    #[test]
    fn total_duration_label_switches_to_hours() {
        let mut p = Playlist::new("Long");
        p.add(track("a").with_duration(3725));
        assert_eq!(p.total_duration_label(), "1:02:05");
    }

    #[test]
    fn empty_playlist_has_zero_duration() {
        assert_eq!(Playlist::new("Empty").total_duration_label(), "0:00");
    }
}
