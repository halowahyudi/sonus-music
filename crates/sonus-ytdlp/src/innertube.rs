use serde_json::Value;

fn parse_duration(text: &str) -> Option<u64> {
    let mut total = 0u64;
    for part in text.split(':') {
        total = total * 60 + part.trim().parse::<u64>().ok()?;
    }
    Some(total)
}

/// Extract a `runs[].text` or a plain string from a YouTube "text" object.
fn text_of(value: &Value) -> Option<String> {
    if let Some(runs) = value.get("runs").and_then(|v| v.as_array()) {
        let text: String = runs
            .iter()
            .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
            .collect();
        return if text.is_empty() { None } else { Some(text) };
    }
    value
        .get("simpleText")
        .and_then(|t| t.as_str())
        .map(String::from)
}

fn track_from_renderer(renderer: &Value) -> Option<sonus_core::Track> {
    let video_id = renderer.get("videoId")?.as_str()?;
    if !crate::search::is_video_id(video_id) {
        return None;
    }
    let title = text_of(renderer.get("title")?)?;
    let artist = renderer
        .get("longBylineText")
        .and_then(|v| text_of(v).map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty());
    let duration_secs = renderer
        .get("lengthText")
        .and_then(text_of)
        .and_then(|s| parse_duration(&s));
    let thumbnail_url = renderer
        .get("thumbnail")
        .and_then(|v| v.get("thumbnails"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.last())
        .and_then(|t| t.get("url"))
        .and_then(|u| u.as_str())
        .map(String::from);

    let mut track = sonus_core::Track::new(video_id, title);
    track.artist = artist;
    track.duration_secs = duration_secs;
    track.thumbnail_url = thumbnail_url;
    Some(track)
}

/// Walk the JSON for every `playlistPanelVideoRenderer` object, wherever it
/// is. Nested paths on YouTube's responses shift between layouts, so a deep
/// scan is more durable than hardcoding `contents.foo.bar`.
#[derive(Debug, Default)]
struct Scan {
    tracks: Vec<sonus_core::Track>,
}

fn scan_value(value: &Value, scan: &mut Scan) {
    match value {
        Value::Object(map) => {
            if let Some(renderer) = map.get("playlistPanelVideoRenderer") {
                if let Some(track) = track_from_renderer(renderer) {
                    scan.tracks.push(track);
                }
            }
            for v in map.values() {
                scan_value(v, scan);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                scan_value(v, scan);
            }
        }
        _ => {}
    }
}

/// Fetch a radio mix directly from YouTube Music's web player data.
///
/// `ytInitialData` is the JSON blob the Music web UI boots from; it costs one
/// GET instead of a yt-dlp subprocess, and it carries artist, duration and
/// thumbnail alongside every radio entry — no enrichment round-trip needed.
/// Returns tracks *after* the seed, ordered the way YouTube would play them.
pub async fn radio_tracks(
    client: &reqwest::Client,
    youtube_id: &str,
    limit: usize,
) -> Result<Vec<sonus_core::Track>, InnertubeError> {
    if !crate::search::is_video_id(youtube_id) {
        return Err(InnertubeError::BadId(youtube_id.to_string()));
    }

    let url = format!(
        "https://music.youtube.com/watch?v={youtube_id}&list=RDAMVM{youtube_id}&start_radio=1&has_verified=1&bpctr=9999999999&hl=en"
    );

    let body = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let json = extract_initial_data(&body).ok_or_else(|| InnertubeError::NoData(url))?;

    let mut scan = Scan::default();
    scan_value(&json, &mut scan);

    let mut tracks: Vec<_> = scan
        .tracks
        .into_iter()
        .filter(|t| t.youtube_id != youtube_id)
        .collect();
    tracks.dedup_by(|a, b| a.youtube_id == b.youtube_id);
    tracks.truncate(limit);

    Ok(tracks)
}

/// Pull the `var ytInitialData = {...};` blob out of the page HTML.
fn extract_initial_data(body: &str) -> Option<Value> {
    let marker = "var ytInitialData = ";
    let start = body.find(marker)? + marker.len();
    // The blob ends at a `;</script>`; a lone `;` would terminate early inside
    // strings, so anchor on the script close instead.
    let rest = &body[start..];
    let end = rest.find(";</script>")?;
    serde_json::from_str(&rest[..end]).ok()
}

#[derive(Debug, thiserror::Error)]
pub enum InnertubeError {
    #[error("not a YouTube video id: {0}")]
    BadId(String),
    #[error("could not reach YouTube Music: {0}")]
    Http(#[from] reqwest::Error),
    #[error("no radio data found in the page (loader changed?): {0}")]
    NoData(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colon_durations() {
        assert_eq!(parse_duration("0:09"), Some(9));
        assert_eq!(parse_duration("3:34"), Some(214));
        assert_eq!(parse_duration("1:02:05"), Some(3725));
        assert_eq!(parse_duration("garbage"), None);
    }

    #[test]
    fn extracts_data_from_a_sample_page() {
        let html = r#"<!doctype html><script>var ytInitialData = {"contents":{"key":[{"playlistPanelVideoRenderer":{"videoId":"abc12345678","title":{"runs":[{"text":"Song"} ]},"longBylineText":{"runs":[{"text":"Artist"} ]},"lengthText":{"runs":[{"text":"3:30"} ]},"thumbnail":{"thumbnails":[{"url":"http://x/y.jpg"}]}}}]}};</script></html>"#;
        let data = extract_initial_data(html).expect("blob should parse");
        assert!(data.get("contents").is_some());
    }

    #[test]
    fn rejects_page_without_the_marker() {
        assert!(extract_initial_data("<html>nothing here</html>").is_none());
    }

    #[test]
    fn scans_playlist_panel_renderers_into_tracks() {
        let json: Value = serde_json::from_str(
            r#"{
                "contents": [
                    {"playlistPanelVideoRenderer": {
                        "videoId": "abc12345678",
                        "title": {"runs": [{"text": "Song One"}]},
                        "longBylineText": {"runs": [{"text": "Artist"}]},
                        "lengthText": {"runs": [{"text": "3:30"}]},
                        "thumbnail": {"thumbnails": [{"url": "http://x/1.jpg"}]}
                    }},
                    {"playlistPanelVideoRenderer": {
                        "videoId": "def23456789",
                        "title": {"simpleText": "Song Two"}
                    }}
                ]
            }"#,
        )
        .unwrap();
        let mut scan = Scan::default();
        scan_value(&json, &mut scan);
        assert_eq!(scan.tracks.len(), 2);
        let first = &scan.tracks[0];
        assert_eq!(first.youtube_id, "abc12345678");
        assert_eq!(first.title, "Song One");
        assert_eq!(first.artist.as_deref(), Some("Artist"));
        assert_eq!(first.duration_secs, Some(210));
        assert_eq!(first.thumbnail_url.as_deref(), Some("http://x/1.jpg"));
        assert_eq!(scan.tracks[1].title, "Song Two");
    }

    #[tokio::test]
    async fn rejects_bad_ids_without_network() {
        let client = reqwest::Client::new();
        let err = radio_tracks(&client, "too-short", 5).await.unwrap_err();
        assert!(matches!(err, InnertubeError::BadId(_)));
    }
}
