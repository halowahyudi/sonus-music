use serde::{Deserialize, Serialize};

/// A track is identified by its YouTube video ID.
///
/// Using the video ID as the primary key (rather than a generated UUID) means
/// the same song always maps to the same row: playlists, history and favourites
/// can't fragment into duplicates when a track is searched twice.
pub type TrackId = String;

/// Everything needed to display a track and stream it.
///
/// Deliberately small — thumbnails are lazy-loaded and extra metadata is only
/// fetched when a track is actually played.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    /// YouTube video ID. Also the track's identity.
    pub youtube_id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_secs: Option<u64>,
    pub thumbnail_url: Option<String>,
}

impl Track {
    pub fn new(youtube_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            youtube_id: youtube_id.into(),
            title: title.into(),
            artist: None,
            album: None,
            duration_secs: None,
            thumbnail_url: None,
        }
    }

    pub fn id(&self) -> &TrackId {
        &self.youtube_id
    }

    pub fn with_artist(mut self, artist: impl Into<String>) -> Self {
        self.artist = Some(artist.into());
        self
    }

    pub fn with_album(mut self, album: impl Into<String>) -> Self {
        self.album = Some(album.into());
        self
    }

    pub fn with_duration(mut self, secs: u64) -> Self {
        self.duration_secs = Some(secs);
        self
    }

    pub fn with_thumbnail(mut self, url: impl Into<String>) -> Self {
        self.thumbnail_url = Some(url.into());
        self
    }

    /// Artist for display, falling back to a placeholder when unknown.
    pub fn artist_or_unknown(&self) -> &str {
        self.artist
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("Unknown Artist")
    }

    /// "Artist - Title", the canonical single-line label.
    pub fn label(&self) -> String {
        match self.artist.as_deref().filter(|s| !s.is_empty()) {
            Some(artist) => format!("{artist} - {}", self.title),
            None => self.title.clone(),
        }
    }

    /// Duration as `M:SS`, or `--:--` when unknown.
    pub fn duration_label(&self) -> String {
        match self.duration_secs {
            Some(secs) => format!("{}:{:02}", secs / 60, secs % 60),
            None => "--:--".to_string(),
        }
    }

    /// URL used for streaming. Regular YouTube — yt-dlp handles the redirect
    /// and it avoids the extra Music extractor round-trip on playback.
    pub fn stream_url(&self) -> String {
        format!("https://www.youtube.com/watch?v={}", self.youtube_id)
    }

    /// YouTube Music URL — used for metadata, where the Music extractor
    /// returns structured artist/album fields.
    pub fn music_url(&self) -> String {
        format!("https://music.youtube.com/watch?v={}", self.youtube_id)
    }

    /// YouTube's auto-generated thumbnail for this video.
    pub fn default_thumbnail_url(&self) -> String {
        format!("https://i.ytimg.com/vi/{}/mqdefault.jpg", self.youtube_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_leaves_optionals_empty() {
        let t = Track::new("abc12345678", "Song");
        assert_eq!(t.youtube_id, "abc12345678");
        assert_eq!(t.title, "Song");
        assert!(t.artist.is_none());
        assert!(t.duration_secs.is_none());
        assert_eq!(t.id(), "abc12345678");
    }

    #[test]
    fn label_uses_artist_when_present() {
        let t = Track::new("abc12345678", "Rosewood").with_artist("Bonobo");
        assert_eq!(t.label(), "Bonobo - Rosewood");
    }

    #[test]
    fn label_falls_back_to_title() {
        assert_eq!(Track::new("abc12345678", "Rosewood").label(), "Rosewood");
    }

    #[test]
    fn label_ignores_blank_artist() {
        let t = Track::new("abc12345678", "Rosewood").with_artist("");
        assert_eq!(t.label(), "Rosewood");
        assert_eq!(t.artist_or_unknown(), "Unknown Artist");
    }

    #[test]
    fn duration_label_formats_minutes_and_seconds() {
        assert_eq!(
            Track::new("a", "x").with_duration(0).duration_label(),
            "0:00"
        );
        assert_eq!(
            Track::new("a", "x").with_duration(9).duration_label(),
            "0:09"
        );
        assert_eq!(
            Track::new("a", "x").with_duration(214).duration_label(),
            "3:34"
        );
        assert_eq!(Track::new("a", "x").duration_label(), "--:--");
    }

    #[test]
    fn builder_chains() {
        let t = Track::new("abc12345678", "Rosewood")
            .with_artist("Bonobo")
            .with_album("Black Sands")
            .with_duration(300)
            .with_thumbnail("http://x/y.jpg");
        assert_eq!(t.album.as_deref(), Some("Black Sands"));
        assert_eq!(t.thumbnail_url.as_deref(), Some("http://x/y.jpg"));
    }

    #[test]
    fn urls_use_the_video_id() {
        let t = Track::new("abc12345678", "Song");
        assert!(t.stream_url().contains("youtube.com/watch?v=abc12345678"));
        assert!(t.music_url().contains("music.youtube.com"));
        assert!(t.default_thumbnail_url().contains("abc12345678"));
    }

    #[test]
    fn equality_is_by_content() {
        let a = Track::new("abc12345678", "Song").with_artist("X");
        let b = Track::new("abc12345678", "Song").with_artist("X");
        assert_eq!(a, b);
    }
}
