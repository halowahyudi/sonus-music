use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ytdlp_binary;

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("yt-dlp not found. Install it or set SONUS_YTDLP_PATH")]
    BinaryNotFound,

    #[error("yt-dlp exited with code {0}: {1}")]
    ExitError(i32, String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Full metadata for a YouTube Music track.
///
/// yt-dlp's Music extractor only populates `artist`/`album` when YouTube Music
/// has structured data for the track, so we fall back to `uploader`/`channel`
/// and finally to parsing the title.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackMetadata {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<u64>,
    pub thumbnail: Option<String>,
}

impl From<TrackMetadata> for sonus_core::Track {
    fn from(m: TrackMetadata) -> Self {
        let mut track = sonus_core::Track::new(m.id, m.title);
        track.artist = m.artist;
        track.album = m.album;
        track.duration_secs = m.duration;
        track.thumbnail_url = m.thumbnail;
        track
    }
}

/// Fetch full metadata for a single track.
///
/// Uses the `music.youtube.com` URL so yt-dlp picks the YouTube Music
/// extractor, which returns clean `artist`/`track`/`album` fields instead of
/// the uploader-oriented fields the regular extractor gives.
///
/// Call lazily — only when a track is actually about to play.
pub async fn fetch_metadata(youtube_id: &str) -> Result<TrackMetadata, MetadataError> {
    let url = format!("https://music.youtube.com/watch?v={youtube_id}");

    let binary = ytdlp_binary().ok_or(MetadataError::BinaryNotFound)?;
    let output = tokio::process::Command::new(binary)
        .arg("--dump-json")
        .arg("--no-warnings")
        .arg("--no-playlist")
        .arg(&url)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                MetadataError::BinaryNotFound
            } else {
                MetadataError::Io(e)
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MetadataError::ExitError(
            output.status.code().unwrap_or(-1),
            stderr.trim().to_string(),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_metadata(&stdout, youtube_id)
}

/// Pull the fields we care about out of yt-dlp's JSON.
///
/// Split from the subprocess call so it can be tested against a recorded
/// payload instead of hitting the network.
pub fn parse_metadata(stdout: &str, youtube_id: &str) -> Result<TrackMetadata, MetadataError> {
    let raw = parse_first_json_object(stdout)?;

    // `track` is the clean song title on YouTube Music; `title` may carry an
    // "Artist - " prefix that we've already split out into `artist`.
    let title = raw["track"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| raw["title"].as_str())
        .unwrap_or("Unknown Title")
        .to_string();

    let artist = raw["artist"]
        .as_str()
        .or_else(|| raw["uploader"].as_str())
        .or_else(|| raw["channel"].as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    Ok(TrackMetadata {
        id: raw["id"].as_str().unwrap_or(youtube_id).to_string(),
        title,
        artist,
        album: raw["album"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        duration: raw["duration"].as_u64(),
        thumbnail: raw["thumbnail"].as_str().map(str::to_string),
    })
}

/// Extract the first JSON object from yt-dlp output.
///
/// yt-dlp emits one object per line when not pretty-printing, but `--dump-json`
/// output can also be indented across many lines. Parsing the whole payload
/// first handles the pretty case; the line-by-line pass handles playlists and
/// any leading log noise.
fn parse_first_json_object(stdout: &str) -> Result<serde_json::Value, MetadataError> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Ok(value);
    }

    // Multi-line / multi-object output: the first line that parses on its own
    // wins. Blank lines are skipped rather than treated as an error.
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            return Ok(value);
        }
    }

    // Nothing parsed — surface the real error from the whole payload so the
    // message points at the actual syntax problem.
    serde_json::from_str::<serde_json::Value>(trimmed).map_err(MetadataError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_music_payload() {
        let json = r#"{
            "id": "abc12345678",
            "track": "Rosewood",
            "artist": "Bonobo",
            "album": "Black Sands",
            "duration": 300,
            "thumbnail": "https://i.ytimg.com/vi/abc12345678/maxres.jpg"
        }"#;
        let m = parse_metadata(json, "abc12345678").unwrap();
        assert_eq!(m.id, "abc12345678");
        assert_eq!(m.title, "Rosewood");
        assert_eq!(m.artist.as_deref(), Some("Bonobo"));
        assert_eq!(m.album.as_deref(), Some("Black Sands"));
        assert_eq!(m.duration, Some(300));
    }

    #[test]
    fn falls_back_to_uploader_for_artist() {
        let json = r#"{"id":"x","title":"Song","uploader":"Some Channel"}"#;
        let m = parse_metadata(json, "abc12345678").unwrap();
        assert_eq!(m.artist.as_deref(), Some("Some Channel"));
    }

    #[test]
    fn falls_back_to_channel_when_uploader_missing() {
        let json = r#"{"id":"x","title":"Song","channel":"A Channel"}"#;
        let m = parse_metadata(json, "abc12345678").unwrap();
        assert_eq!(m.artist.as_deref(), Some("A Channel"));
    }

    #[test]
    fn prefers_track_over_title() {
        let json = r#"{"id":"x","track":"Clean","title":"Artist - Clean"}"#;
        let m = parse_metadata(json, "abc12345678").unwrap();
        assert_eq!(m.title, "Clean");
    }

    #[test]
    fn ignores_blank_track_field() {
        let json = r#"{"id":"x","track":"","title":"Fallback"}"#;
        assert_eq!(
            parse_metadata(json, "abc12345678").unwrap().title,
            "Fallback"
        );
    }

    #[test]
    fn uses_the_requested_id_when_missing_from_payload() {
        let m = parse_metadata(r#"{"title":"Song"}"#, "abc12345678").unwrap();
        assert_eq!(m.id, "abc12345678");
    }

    #[test]
    fn missing_fields_degrade_gracefully() {
        let m = parse_metadata(r#"{"id":"x"}"#, "abc12345678").unwrap();
        assert_eq!(m.title, "Unknown Title");
        assert_eq!(m.artist, None);
        assert_eq!(m.album, None);
        assert_eq!(m.duration, None);
    }

    #[test]
    fn blank_album_is_none() {
        let m = parse_metadata(r#"{"id":"x","album":""}"#, "abc12345678").unwrap();
        assert_eq!(m.album, None);
    }

    #[test]
    fn empty_input_is_not_an_error() {
        assert!(parse_metadata("", "abc12345678").is_ok());
    }

    #[test]
    fn skips_leading_blank_lines() {
        let json = "\n\n{\"id\":\"x\",\"track\":\"Song\"}";
        assert_eq!(parse_metadata(json, "abc12345678").unwrap().title, "Song");
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse_metadata("not json", "abc12345678").is_err());
    }

    #[test]
    fn metadata_converts_to_track() {
        let m = TrackMetadata {
            id: "abc12345678".into(),
            title: "Rosewood".into(),
            artist: Some("Bonobo".into()),
            album: Some("Black Sands".into()),
            duration: Some(300),
            thumbnail: None,
        };
        let track: sonus_core::Track = m.into();
        assert_eq!(track.youtube_id, "abc12345678");
        assert_eq!(track.album.as_deref(), Some("Black Sands"));
    }
}
