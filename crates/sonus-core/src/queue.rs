use serde::{Deserialize, Serialize};

use crate::track::Track;

/// Where the current queue came from. Purely informational — the UI uses it to
/// label the list ("Up Next", "Radio", a playlist name).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum QueueSource {
    #[default]
    Manual,
    /// Built from a search result list.
    Search(String),
    /// Loaded from a saved playlist.
    Playlist(String),
    /// Auto-extended from YouTube's recommendation radio.
    Radio,
}

impl QueueSource {
    pub fn label(&self) -> String {
        match self {
            QueueSource::Manual => "Queue".to_string(),
            QueueSource::Search(q) => format!("Search: {q}"),
            QueueSource::Playlist(name) => name.clone(),
            QueueSource::Radio => "Radio".to_string(),
        }
    }

    /// Whether running off the end of this queue should pull in more tracks
    /// from the recommendation radio.
    pub fn extends_itself(&self) -> bool {
        matches!(self, QueueSource::Radio | QueueSource::Search(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    /// Cycle Off -> All -> One -> Off.
    pub fn next(self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RepeatMode::Off => "OFF",
            RepeatMode::All => "ALL",
            RepeatMode::One => "ONE",
        }
    }
}

/// A row in the queue: the track plus where it came from.
///
/// Manual additions are `is_autoplay == false`; the radio tail that keeps the
/// music going after the user's picks is `true`, so the UI can render it
/// dimmed with an "AUTO" badge and the player can treat it as disposable.
#[derive(Debug, Clone)]
pub struct QueueItem {
    pub track: Track,
    pub is_autoplay: bool,
}

/// The playback queue.
///
/// Owns the full `Track` values rather than IDs so the UI can render the list
/// and the radio can extend it without a database round-trip. A few hundred
/// tracks of metadata is a trivial amount of memory.
#[derive(Debug, Clone, Default)]
pub struct Queue {
    items: Vec<QueueItem>,
    /// Index of the track currently loaded, or `None` when stopped.
    cursor: Option<usize>,
    repeat: RepeatMode,
    shuffle: bool,
    /// Item order from before shuffling, so un-shuffling can restore it.
    unshuffled: Option<Vec<QueueItem>>,
    source: QueueSource,
}

impl Queue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the queue contents and point the cursor at `start`.
    /// `start` is clamped into range; an empty list clears the cursor.
    pub fn load(&mut self, tracks: Vec<Track>, start: usize) {
        self.items = tracks
            .into_iter()
            .map(|track| QueueItem {
                track,
                is_autoplay: false,
            })
            .collect();
        self.unshuffled = None;
        self.shuffle = false;
        self.cursor = if self.items.is_empty() {
            None
        } else {
            Some(start.min(self.items.len() - 1))
        };
    }

    pub fn set_source(&mut self, source: QueueSource) {
        self.source = source;
    }

    pub fn source(&self) -> &QueueSource {
        &self.source
    }

    /// The queue rows, in playback order — tracks with their autoplay flags.
    pub fn items(&self) -> &[QueueItem] {
        &self.items
    }

    /// The tracks in order, with autoplay flags stripped. For callers that
    /// only care about the music itself (playlist export, metadata merge).
    pub fn tracks(&self) -> Vec<Track> {
        self.items.iter().map(|item| item.track.clone()).collect()
    }

    /// The track at `index`, or `None` when out of range.
    pub fn track_at(&self, index: usize) -> Option<&Track> {
        self.items.get(index).map(|item| &item.track)
    }

    /// Whether the track at `index` was added by the radio tail.
    pub fn is_autoplay(&self, index: usize) -> bool {
        self.items
            .get(index)
            .map(|item| item.is_autoplay)
            .unwrap_or(false)
    }

    /// Replace the track at `index`, keeping its autoplay flag. Used to merge
    /// enriched metadata into an existing row without disturbing the flag.
    pub fn update_track_at(&mut self, index: usize, track: Track) -> bool {
        if index >= self.items.len() {
            return false;
        }
        self.items[index].track = track;
        true
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    pub fn current(&self) -> Option<&Track> {
        self.cursor
            .and_then(|i| self.items.get(i))
            .map(|item| &item.track)
    }

    pub fn current_index(&self) -> Option<usize> {
        self.cursor
    }

    /// Tracks after the cursor, in playback order.
    pub fn upcoming(&self) -> Vec<&Track> {
        match self.cursor {
            Some(i) if i + 1 < self.items.len() => {
                self.items[i + 1..].iter().map(|item| &item.track).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Upcoming rows with their autoplay flags, for the queue view.
    pub fn upcoming_items(&self) -> &[QueueItem] {
        match self.cursor {
            Some(i) if i + 1 < self.items.len() => &self.items[i + 1..],
            _ => &[],
        }
    }

    /// Number of tracks left before the queue runs dry.
    pub fn remaining(&self) -> usize {
        self.upcoming().len()
    }

    /// Jump to an explicit index. Returns the track now loaded.
    pub fn jump_to(&mut self, index: usize) -> Option<&Track> {
        if index >= self.items.len() {
            return None;
        }
        self.cursor = Some(index);
        self.current()
    }

    /// Advance to the next track.
    ///
    /// Returns `None` when the end is reached with repeat off — the cursor is
    /// deliberately left on the last track so `previous()` still works.
    pub fn advance(&mut self) -> Option<&Track> {
        if self.items.is_empty() {
            return None;
        }
        if self.repeat == RepeatMode::One && self.cursor.is_some() {
            return self.current();
        }

        let next = match self.cursor {
            None => 0,
            Some(i) if i + 1 < self.items.len() => i + 1,
            Some(_) if self.repeat == RepeatMode::All => 0,
            Some(_) => return None,
        };
        self.cursor = Some(next);
        self.current()
    }

    /// Go back one track.
    pub fn previous(&mut self) -> Option<&Track> {
        if self.items.is_empty() {
            return None;
        }
        let prev = match self.cursor {
            Some(i) if i > 0 => i - 1,
            Some(_) if self.repeat == RepeatMode::All => self.items.len() - 1,
            // At the head: restart the current track rather than doing nothing.
            other => other.unwrap_or(0),
        };
        self.cursor = Some(prev);
        self.current()
    }

    /// True when there is a track after the current one.
    pub fn has_next(&self) -> bool {
        match self.cursor {
            Some(i) => i + 1 < self.items.len() || self.repeat == RepeatMode::All,
            None => !self.items.is_empty(),
        }
    }

    /// Append a manually-picked track. Returns false if already present.
    pub fn push_back(&mut self, track: Track) -> bool {
        self.push_item(track, false)
    }

    /// Insert directly after the current track — "play next".
    pub fn push_next(&mut self, track: Track) -> bool {
        self.push_next_item(track, false)
    }

    /// Append a radio track. Autoplay rows are disposable: clearing or
    /// removing them never affects the user's manual picks.
    pub fn push_autoplay(&mut self, track: Track) -> bool {
        self.push_item(track, true)
    }

    /// Insert a radio track directly after the current one.
    pub fn push_next_autoplay(&mut self, track: Track) -> bool {
        self.push_next_item(track, true)
    }

    fn push_item(&mut self, track: Track, is_autoplay: bool) -> bool {
        if self.contains(&track.youtube_id) {
            return false;
        }
        self.items.push(QueueItem { track, is_autoplay });
        if self.cursor.is_none() {
            self.cursor = Some(0);
        }
        true
    }

    fn push_next_item(&mut self, track: Track, is_autoplay: bool) -> bool {
        if self.contains(&track.youtube_id) {
            return false;
        }
        let at = match self.cursor {
            Some(i) => i + 1,
            None => 0,
        };
        self.items
            .insert(at.min(self.items.len()), QueueItem { track, is_autoplay });
        if self.cursor.is_none() {
            self.cursor = Some(0);
        }
        true
    }

    /// Append several manually-picked tracks, skipping ones already queued.
    /// Returns how many were actually added.
    pub fn extend(&mut self, tracks: impl IntoIterator<Item = Track>) -> usize {
        let mut added = 0;
        for track in tracks {
            if self.push_back(track) {
                added += 1;
            }
        }
        added
    }

    /// Append a radio batch, skipping ones already queued. Returns how many
    /// were actually added.
    pub fn extend_autoplay(&mut self, tracks: impl IntoIterator<Item = Track>) -> usize {
        let mut added = 0;
        for track in tracks {
            if self.push_autoplay(track) {
                added += 1;
            }
        }
        added
    }

    pub fn contains(&self, youtube_id: &str) -> bool {
        self.items
            .iter()
            .any(|item| item.track.youtube_id == youtube_id)
    }

    /// Remove the track at `index`, keeping the cursor on the same song.
    pub fn remove(&mut self, index: usize) -> Option<Track> {
        if index >= self.items.len() {
            return None;
        }
        let removed = self.items.remove(index);
        self.unshuffled = None;

        self.cursor = match self.cursor {
            // Removed something before the cursor — shift back to stay put.
            Some(c) if index < c => Some(c - 1),
            // Removed the current track — the next one slides into place.
            Some(c) if index == c => {
                if self.items.is_empty() {
                    None
                } else {
                    Some(c.min(self.items.len() - 1))
                }
            }
            other => other,
        };
        Some(removed.track)
    }

    /// Move a track within the queue.
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.items.len() || to >= self.items.len() || from == to {
            return false;
        }
        let track = self.items.remove(from);
        self.items.insert(to, track);

        // Follow the currently playing track through the move.
        if let Some(c) = self.cursor {
            self.cursor = Some(if c == from {
                to
            } else if from < c && to >= c {
                c - 1
            } else if from > c && to <= c {
                c + 1
            } else {
                c
            });
        }
        self.unshuffled = None;
        true
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.cursor = None;
        self.unshuffled = None;
    }

    pub fn repeat_mode(&self) -> RepeatMode {
        self.repeat
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }

    pub fn cycle_repeat(&mut self) -> RepeatMode {
        self.repeat = self.repeat.next();
        self.repeat
    }

    pub fn is_shuffled(&self) -> bool {
        self.shuffle
    }

    /// Toggle shuffle.
    ///
    /// Shuffling physically permutes the list — what you see in the queue view
    /// is exactly the order you'll hear, which is how classic players behaved.
    /// The original order is kept so un-shuffling restores it.
    pub fn toggle_shuffle(&mut self) -> bool {
        self.shuffle = !self.shuffle;
        let playing = self.current().cloned();

        if self.shuffle {
            self.unshuffled = Some(self.items.clone());
            shuffle_in_place(&mut self.items);
            // Keep the current song under the cursor so playback doesn't jump.
            if let Some(current) = playing {
                if let Some(pos) = self
                    .items
                    .iter()
                    .position(|item| item.track.youtube_id == current.youtube_id)
                {
                    self.cursor = Some(pos);
                }
            }
        } else if let Some(original) = self.unshuffled.take() {
            self.items = original;
            if let Some(current) = playing {
                if let Some(pos) = self
                    .items
                    .iter()
                    .position(|item| item.track.youtube_id == current.youtube_id)
                {
                    self.cursor = Some(pos);
                }
            }
        }
        self.shuffle
    }

    /// Total runtime of everything still to play.
    pub fn remaining_duration_secs(&self) -> u64 {
        self.upcoming().iter().filter_map(|t| t.duration_secs).sum()
    }

    /// Total runtime of everything in the queue, ignoring unknown durations.
    pub fn total_duration_secs(&self) -> u64 {
        self.items
            .iter()
            .filter_map(|item| item.track.duration_secs)
            .sum()
    }

    /// Total runtime as `H:MM:SS` or `M:SS`.
    pub fn total_duration_label(&self) -> String {
        format_duration(self.total_duration_secs())
    }

    /// Runtime still to play, formatted.
    pub fn remaining_duration_label(&self) -> String {
        format_duration(self.remaining_duration_secs())
    }
}

/// Format seconds as `H:MM:SS` or `M:SS`.
fn format_duration(total: u64) -> String {
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Fisher-Yates using a seeded xorshift. Avoids pulling in the `rand` crate
/// for what is a handful of lines, and keeps the shuffle testable.
fn shuffle_in_place(items: &mut [QueueItem]) {
    if items.len() < 2 {
        return;
    }
    let mut state = seed();
    for i in (1..items.len()).rev() {
        state = xorshift(state);
        let j = (state % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

fn seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x2545F4914F6CDD1D);
    // Zero is a fixed point for xorshift — nudge it off.
    nanos | 1
}

fn xorshift(mut x: u64) -> u64 {
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracks(ids: &[&str]) -> Vec<Track> {
        ids.iter()
            .map(|id| Track::new(*id, format!("Song {id}")))
            .collect()
    }

    fn ids(q: &Queue) -> Vec<String> {
        q.items()
            .iter()
            .map(|item| item.track.youtube_id.clone())
            .collect()
    }

    #[test]
    fn new_queue_is_empty() {
        let mut q = Queue::new();
        assert!(q.is_empty());
        assert_eq!(q.current(), None);
        assert_eq!(q.advance(), None);
        assert_eq!(q.previous(), None);
    }

    #[test]
    fn load_points_cursor_at_start() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b", "c"]), 0);
        assert_eq!(q.current().unwrap().youtube_id, "a");
        assert_eq!(q.len(), 3);
    }

    #[test]
    fn load_clamps_start_index() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b"]), 99);
        assert_eq!(q.current().unwrap().youtube_id, "b");
    }

    #[test]
    fn load_empty_clears_cursor() {
        let mut q = Queue::new();
        q.load(tracks(&["a"]), 0);
        q.load(vec![], 0);
        assert_eq!(q.cursor(), None);
        assert!(q.is_empty());
    }

    #[test]
    fn advance_walks_the_list() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b", "c"]), 0);
        assert_eq!(q.advance().unwrap().youtube_id, "b");
        assert_eq!(q.advance().unwrap().youtube_id, "c");
        assert_eq!(q.advance(), None);
    }

    #[test]
    fn advance_stops_at_end_but_keeps_cursor() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b"]), 0);
        q.advance();
        assert_eq!(q.advance(), None);
        // Cursor stays on the last track so previous() still works.
        assert_eq!(q.current().unwrap().youtube_id, "b");
        assert!(!q.has_next());
    }

    #[test]
    fn advance_with_repeat_all_wraps() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b"]), 0);
        q.set_repeat(RepeatMode::All);
        assert_eq!(q.advance().unwrap().youtube_id, "b");
        assert_eq!(q.advance().unwrap().youtube_id, "a");
        assert!(q.has_next());
    }

    #[test]
    fn advance_with_repeat_one_stays() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b"]), 0);
        q.set_repeat(RepeatMode::One);
        assert_eq!(q.advance().unwrap().youtube_id, "a");
        assert_eq!(q.advance().unwrap().youtube_id, "a");
    }

    #[test]
    fn advance_from_stopped_state_starts_at_first() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b"]), 0);
        q.clear();
        q.load(tracks(&["a", "b"]), 0);
        q.jump_to(0);
        assert_eq!(q.advance().unwrap().youtube_id, "b");
    }

    #[test]
    fn previous_restarts_at_head() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b"]), 0);
        assert_eq!(q.previous().unwrap().youtube_id, "a");
    }

    #[test]
    fn previous_walks_backwards() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b", "c"]), 2);
        assert_eq!(q.previous().unwrap().youtube_id, "b");
        assert_eq!(q.previous().unwrap().youtube_id, "a");
        assert_eq!(q.previous().unwrap().youtube_id, "a");
    }

    #[test]
    fn previous_with_repeat_all_wraps_to_end() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b", "c"]), 0);
        q.set_repeat(RepeatMode::All);
        assert_eq!(q.previous().unwrap().youtube_id, "c");
    }

    #[test]
    fn jump_to_out_of_range_is_none() {
        let mut q = Queue::new();
        q.load(tracks(&["a"]), 0);
        assert!(q.jump_to(5).is_none());
        assert_eq!(q.current().unwrap().youtube_id, "a");
    }

    #[test]
    fn upcoming_excludes_current_and_past() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b", "c", "d"]), 1);
        let up: Vec<&str> = q.upcoming().iter().map(|t| t.youtube_id.as_str()).collect();
        assert_eq!(up, ["c", "d"]);
        assert_eq!(q.remaining(), 2);
    }

    #[test]
    fn push_back_appends_and_starts_empty_queue() {
        let mut q = Queue::new();
        assert!(q.push_back(Track::new("a", "A")));
        assert_eq!(q.cursor(), Some(0));
        assert_eq!(q.current().unwrap().youtube_id, "a");
    }

    #[test]
    fn push_back_rejects_duplicates() {
        let mut q = Queue::new();
        q.push_back(Track::new("a", "A"));
        assert!(!q.push_back(Track::new("a", "A again")));
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn push_next_inserts_after_current() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b", "c"]), 0);
        assert!(q.push_next(Track::new("x", "X")));
        assert_eq!(ids(&q), ["a", "x", "b", "c"]);
        // Cursor still on 'a'.
        assert_eq!(q.current().unwrap().youtube_id, "a");
    }

    #[test]
    fn extend_counts_only_new_tracks() {
        let mut q = Queue::new();
        q.push_back(Track::new("a", "A"));
        let added = q.extend(tracks(&["a", "b", "c"]));
        assert_eq!(added, 2);
        assert_eq!(q.len(), 3);
    }

    #[test]
    fn remove_before_cursor_keeps_current_playing() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b", "c"]), 2);
        q.remove(0);
        assert_eq!(q.current().unwrap().youtube_id, "c");
        assert_eq!(ids(&q), ["b", "c"]);
    }

    #[test]
    fn remove_current_moves_to_next() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b", "c"]), 1);
        q.remove(1);
        assert_eq!(q.current().unwrap().youtube_id, "c");
    }

    #[test]
    fn remove_last_current_clears_cursor_when_empty() {
        let mut q = Queue::new();
        q.load(tracks(&["a"]), 0);
        q.remove(0);
        assert_eq!(q.cursor(), None);
        assert!(q.is_empty());
    }

    #[test]
    fn remove_out_of_range_is_none() {
        let mut q = Queue::new();
        q.load(tracks(&["a"]), 0);
        assert!(q.remove(9).is_none());
    }

    #[test]
    fn reorder_follows_current_track() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b", "c"]), 0);
        assert!(q.reorder(0, 2));
        assert_eq!(ids(&q), ["b", "c", "a"]);
        // 'a' was playing and moved to the end — cursor should follow it.
        assert_eq!(q.current().unwrap().youtube_id, "a");
    }

    #[test]
    fn reorder_rejects_same_or_out_of_range() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b"]), 0);
        assert!(!q.reorder(0, 0));
        assert!(!q.reorder(0, 9));
        assert!(!q.reorder(9, 0));
    }

    #[test]
    fn cycle_repeat_visits_every_mode() {
        let mut q = Queue::new();
        assert_eq!(q.repeat_mode(), RepeatMode::Off);
        assert_eq!(q.cycle_repeat(), RepeatMode::All);
        assert_eq!(q.cycle_repeat(), RepeatMode::One);
        assert_eq!(q.cycle_repeat(), RepeatMode::Off);
    }

    #[test]
    fn shuffle_preserves_contents_and_current() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b", "c", "d", "e", "f"]), 2);
        let playing = q.current().unwrap().youtube_id.clone();

        assert!(q.toggle_shuffle());
        assert!(q.is_shuffled());
        assert_eq!(q.len(), 6);
        assert_eq!(q.current().unwrap().youtube_id, playing);

        let mut got = ids(&q);
        got.sort();
        assert_eq!(got, ["a", "b", "c", "d", "e", "f"]);
    }

    #[test]
    fn unshuffle_restores_original_order() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b", "c", "d", "e", "f"]), 0);
        let before = ids(&q);

        q.toggle_shuffle();
        q.toggle_shuffle();

        assert!(!q.is_shuffled());
        assert_eq!(ids(&q), before);
    }

    #[test]
    fn shuffle_survives_a_single_item() {
        let mut q = Queue::new();
        q.load(tracks(&["a"]), 0);
        q.toggle_shuffle();
        assert_eq!(q.len(), 1);
        assert_eq!(q.current().unwrap().youtube_id, "a");
    }

    #[test]
    fn remaining_duration_sums_upcoming_only() {
        let mut q = Queue::new();
        q.load(
            vec![
                Track::new("a", "A").with_duration(100),
                Track::new("b", "B").with_duration(200),
                Track::new("c", "C").with_duration(300),
            ],
            0,
        );
        assert_eq!(q.remaining_duration_secs(), 500);
        assert_eq!(q.total_duration_secs(), 600);
    }

    #[test]
    fn duration_labels_format_correctly() {
        let mut q = Queue::new();
        q.load(
            vec![
                Track::new("a", "A").with_duration(60),
                Track::new("b", "B").with_duration(120),
            ],
            0,
        );
        assert_eq!(q.total_duration_label(), "3:00");
        assert_eq!(q.remaining_duration_label(), "2:00");
    }

    #[test]
    fn duration_label_switches_to_hours() {
        let mut q = Queue::new();
        q.load(vec![Track::new("a", "A").with_duration(3725)], 0);
        assert_eq!(q.total_duration_label(), "1:02:05");
    }

    #[test]
    fn empty_queue_has_zero_duration() {
        let q = Queue::new();
        assert_eq!(q.total_duration_secs(), 0);
        assert_eq!(q.total_duration_label(), "0:00");
        assert_eq!(q.remaining_duration_label(), "0:00");
    }

    #[test]
    fn durations_skip_tracks_with_unknown_length() {
        let mut q = Queue::new();
        q.load(
            vec![
                Track::new("a", "A").with_duration(60),
                Track::new("b", "B"), // unknown
            ],
            0,
        );
        assert_eq!(q.total_duration_secs(), 60);
    }

    #[test]
    fn queue_source_labels() {
        assert_eq!(QueueSource::Radio.label(), "Radio");
        assert_eq!(QueueSource::Playlist("Chill".into()).label(), "Chill");
        assert!(QueueSource::Radio.extends_itself());
        assert!(QueueSource::Search("x".into()).extends_itself());
        assert!(!QueueSource::Manual.extends_itself());
    }

    #[test]
    fn manual_picks_are_not_autoplay() {
        let mut q = Queue::new();
        q.push_back(Track::new("a", "A"));
        q.push_next(Track::new("b", "B"));
        assert!(!q.is_autoplay(0));
        assert!(!q.is_autoplay(1));
        assert_eq!(q.tracks().len(), 2);
    }

    #[test]
    fn autoplay_tracks_are_flagged() {
        let mut q = Queue::new();
        q.push_back(Track::new("a", "A"));
        q.push_autoplay(Track::new("b", "B"));
        q.push_next_autoplay(Track::new("c", "C"));
        assert!(!q.is_autoplay(0));
        assert!(q.is_autoplay(1));
        assert!(q.is_autoplay(2));
    }

    #[test]
    fn extend_autoplay_flags_every_row() {
        let mut q = Queue::new();
        q.extend(tracks(&["a", "b"]));
        let added = q.extend_autoplay(tracks(&["c", "d"]));
        assert_eq!(added, 2);
        assert!(q.is_autoplay(2));
        assert!(q.is_autoplay(3));
        assert!(!q.is_autoplay(0));
        assert!(!q.is_autoplay(1));
    }

    #[test]
    fn update_track_keeps_the_autoplay_flag() {
        let mut q = Queue::new();
        q.extend_autoplay(tracks(&["a"]));
        assert!(q.is_autoplay(0));
        assert!(q.update_track_at(0, Track::new("a", "New Title")));
        assert!(q.is_autoplay(0));
        assert_eq!(q.track_at(0).unwrap().title, "New Title");
        assert!(!q.update_track_at(9, Track::new("x", "X")));
    }

    #[test]
    fn load_resets_flags_to_manual() {
        let mut q = Queue::new();
        q.extend_autoplay(tracks(&["a", "b"]));
        q.load(tracks(&["c", "d"]), 0);
        assert!(!q.is_autoplay(0));
        assert!(!q.is_autoplay(1));
    }

    #[test]
    fn upcoming_items_carry_flags() {
        let mut q = Queue::new();
        q.load(tracks(&["a", "b", "c"]), 0);
        q.push_autoplay(Track::new("d", "D"));
        let upcoming = q.upcoming_items();
        assert_eq!(upcoming.len(), 3);
        assert!(!upcoming[0].is_autoplay);
        assert!(upcoming[2].is_autoplay);
    }
}
