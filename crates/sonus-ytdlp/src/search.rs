use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ytdlp_binary;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("yt-dlp not found. Install it or set SONUS_YTDLP_PATH")]
    BinaryNotFound,

    #[error("yt-dlp exited with code {0}: {1}")]
    ExitError(i32, String),

    #[error("failed to parse search results: {0}")]
    Parse(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// One row from a YouTube Music search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub youtube_id: String,
    pub title: String,
    pub artist: Option<String>,
    pub duration_secs: Option<u64>,
}

impl From<SearchResult> for sonus_core::Track {
    fn from(r: SearchResult) -> Self {
        let mut track = sonus_core::Track::new(r.youtube_id, r.title);
        track.artist = r.artist;
        track.duration_secs = r.duration_secs;
        track
    }
}

impl From<&SearchResult> for sonus_core::Track {
    fn from(r: &SearchResult) -> Self {
        let mut track = sonus_core::Track::new(r.youtube_id.clone(), r.title.clone());
        track.artist = r.artist.clone();
        track.duration_secs = r.duration_secs;
        track
    }
}

/// Search YouTube Music for songs. Returns up to `limit` results.
///
/// Uses `music.youtube.com/search?q=QUERY` rather than yt-dlp's `ytmsearch`
/// prefix: the prefix isn't supported by every yt-dlp build, while the URL
/// always routes through the YouTube Music extractor. That's what keeps
/// non-music content (vlogs, interviews, gameplay) out of the results.
///
/// `--flat-playlist` keeps it fast — no per-result metadata fetch.
pub async fn search(query: &str, limit: usize) -> Result<Vec<SearchResult>, SearchError> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let url = format!(
        "https://music.youtube.com/search?q={}",
        url_encode_query(query)
    );

    // The Music listing leads with albums (MPREb_*), channels and playlists
    // before the actual songs, and those get filtered out during parsing.
    // Asking for exactly `limit` would therefore come back short — or empty,
    // since the first few rows are usually albums.
    let fetch = fetch_count(limit);

    let stdout = run_ytdlp(&[
        "--flat-playlist",
        "--print",
        "%(id)s\t%(title)s\t%(duration)s",
        "--playlist-end",
        &fetch.to_string(),
        &url,
    ])
    .await?;

    let mut results = parse_search_output(&stdout)?;
    results.truncate(limit);
    Ok(results)
}

/// How many rows to ask yt-dlp for, given how many we actually want.
///
/// Over-fetching is necessary because the listing is padded with albums and
/// channels that get filtered out. Three times the request covers the usual
/// mix; the flat listing is cheap, so the extra rows cost little.
fn fetch_count(limit: usize) -> usize {
    // saturating_add, not `+`: a caller passing usize::MAX would otherwise
    // panic in debug builds.
    limit.saturating_mul(3).max(limit.saturating_add(10))
}

/// Run yt-dlp with the given args and return stdout.
///
/// Shared by search and radio — both scrape a listing with `--flat-playlist`,
/// so the spawn/error handling lives in one place.
pub(crate) async fn run_ytdlp(args: &[&str]) -> Result<String, SearchError> {
    let binary = ytdlp_binary().ok_or(SearchError::BinaryNotFound)?;
    let output = tokio::process::Command::new(binary)
        .args(args)
        .arg("--no-warnings")
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                SearchError::BinaryNotFound
            } else {
                SearchError::Io(e)
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SearchError::ExitError(
            output.status.code().unwrap_or(-1),
            stderr.trim().to_string(),
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parse the tab-separated `id\ttitle\tduration` listing that yt-dlp prints.
pub(crate) fn parse_search_output(stdout: &str) -> Result<Vec<SearchResult>, SearchError> {
    let results = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(parse_search_line)
        .collect();
    Ok(results)
}

fn parse_search_line(line: &str) -> Option<SearchResult> {
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() < 2 {
        return None;
    }

    let youtube_id = parts[0].trim().to_string();

    // YouTube Music interleaves albums (MPREb_*), channels (UC*), playlists
    // (PL*/VLPL*) and radio (RDAMVM*) with real tracks. Only video IDs are
    // playable, so anything else is dropped here.
    if !is_video_id(&youtube_id) {
        return None;
    }

    // The duration is always the last column when yt-dlp emits all three, but
    // a title containing a literal tab would otherwise be truncated. Treat the
    // final column as the duration only when it actually looks like one, and
    // rejoin everything in between as the title.
    let (title_parts, duration_secs) = match parts.split_last() {
        Some((last, rest)) if rest.len() >= 2 => match parse_duration(last) {
            Some(d) => (&rest[1..], d),
            None => (&rest[1..], None),
        },
        _ => (&parts[1..], None),
    };

    let raw_title = title_parts.join("\t");
    let raw_title = raw_title.trim();
    if raw_title.is_empty() || raw_title.eq_ignore_ascii_case("na") {
        return None;
    }

    let (artist, title) = parse_flat_title(raw_title);

    Some(SearchResult {
        youtube_id,
        title,
        artist,
        duration_secs,
    })
}

/// Parse a duration column. `NA` and junk both mean "unknown".
fn parse_duration(s: &str) -> Option<Option<u64>> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("na") {
        return Some(None);
    }
    s.parse::<u64>().ok().map(Some)
}

/// YouTube video IDs are exactly 11 characters of URL-safe base64.
///
/// Length does the real work here: albums are 22 chars (`MPREb_` + 16),
/// channels are 24 (`UC` + 22), and playlists/radio are longer still. The
/// charset check guards against anything shorter that isn't a video ID.
pub fn is_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Split "Artist - Title" into separate fields.
///
/// YouTube Music titles from the flat listing are usually "Artist - Title",
/// but the artist may itself contain a dash ("Jay-Z - Song" is common).
/// Splitting on the *first* separator gives the better result: the artist
/// keeps any internal dashes.
fn split_artist_title(raw: &str) -> (Option<String>, String) {
    if let Some((artist, title)) = raw.split_once(" - ") {
        let artist = artist.trim();
        let title = title.trim();
        if !artist.is_empty() && !title.is_empty() {
            return (Some(artist.to_string()), title.to_string());
        }
    }
    (None, raw.trim().to_string())
}

/// Noise suffixes that YouTube uploaders append to titles.
///
/// These are meaningless in a music player — "Song (Official Audio)" is just
/// "Song" — and they push the useful part of a title off the edge of a row.
///
/// Every entry must be ASCII: the suffix match compares against the original
/// bytes case-insensitively, which only works while the noise is 1 byte per
/// character.
const TITLE_NOISE: &[&str] = &[
    "(official audio)",
    "[official audio]",
    "(official video)",
    "[official video]",
    "(official music video)",
    "[official music video]",
    "(official lyric video)",
    "(lyric video)",
    "[lyric video]",
    "(lyrics)",
    "[lyrics]",
    "(audio)",
    "[audio]",
    "(visualizer)",
    "[visualizer]",
    "(hd)",
    "[hd]",
    "(hq)",
    "[hq]",
    "(explicit)",
    "[explicit]",
    "(full album)",
    "(music video)",
];

/// Find where a known noise suffix starts, if the title ends with one.
///
/// Returns a byte offset into `title` that is guaranteed to sit on a character
/// boundary, so the caller can slice safely.
fn noise_suffix_start(title: &str) -> Option<usize> {
    let trimmed = title.trim_end();

    for noise in TITLE_NOISE {
        if trimmed.len() < noise.len() {
            continue;
        }
        let cut = trimmed.len() - noise.len();
        // A multi-byte character straddling the cut would panic on slicing.
        // Skipping it is correct: it means the tail can't equal an ASCII noise
        // phrase anyway.
        if trimmed.is_char_boundary(cut) && trimmed[cut..].eq_ignore_ascii_case(noise) {
            return Some(cut);
        }
    }

    // "(prod. Somebody)" is a *prefix* match — the bracketed group starts with
    // the marker but continues with the producer's name, so there's no fixed
    // suffix to compare against. Find the last bracketed group and cut from
    // there, but only when it actually ends the title: in "Song (prod. X)
    // feat. Y" the credit is part of the name and must be left alone.
    if let Some(open) = trimmed.rfind('(') {
        let group = &trimmed[open..];
        if group.ends_with(')') && is_producer_group(&group[1..group.len() - 1]) {
            return Some(open);
        }
    }

    None
}

/// True when the inside of a bracketed group is a producer credit.
fn is_producer_group(inner: &str) -> bool {
    const MARKER: &str = "prod.";
    inner.len() >= MARKER.len() && inner[..MARKER.len()].eq_ignore_ascii_case(MARKER)
}

/// Strip uploader noise and tidy whitespace in a YouTube title.
///
/// Only removes bracketed suffixes that match a known noise phrase, so a title
/// that legitimately contains parentheses survives untouched.
pub fn clean_title(raw: &str) -> String {
    let mut title = raw.trim().to_string();

    // Repeat: a title can carry more than one suffix ("Song (Audio) [HD]").
    while let Some(cut) = noise_suffix_start(&title) {
        let stripped = title[..cut].trim_end().to_string();
        if stripped.is_empty() {
            // Everything was noise — keep the original rather than showing an
            // empty row. Returning here also guarantees the loop terminates:
            // the cut would otherwise be a no-op and spin forever.
            return raw.trim().to_string();
        }
        title = stripped;
    }

    // Collapse runs of whitespace left behind by a removal.
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parse a flat yt-dlp listing row into a display-ready search result.
///
/// Shared by search and radio so both paths clean titles the same way.
pub(crate) fn parse_flat_title(raw: &str) -> (Option<String>, String) {
    let cleaned = clean_title(raw);
    split_artist_title(&cleaned)
}

/// Minimal URL encoding — runs of whitespace become a single `+`, which
/// YouTube handles fine. Everything else passes through; yt-dlp escapes what
/// it needs to.
fn url_encode_query(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_output() {
        assert!(parse_search_output("").unwrap().is_empty());
        assert!(parse_search_output("\n\n").unwrap().is_empty());
    }

    #[test]
    fn parse_single_result_with_artist() {
        let stdout = "dQw4w9WgXcQ\tRick Astley - Never Gonna Give You Up\t214";
        let results = parse_search_output(stdout).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].youtube_id, "dQw4w9WgXcQ");
        assert_eq!(results[0].artist.as_deref(), Some("Rick Astley"));
        assert_eq!(results[0].title, "Never Gonna Give You Up");
        assert_eq!(results[0].duration_secs, Some(214));
    }

    #[test]
    fn parse_filters_out_albums() {
        let stdout =
            "MPREb_bFKC3lKP9qh\tNA\tNA\ndQw4w9WgXcQ\tRick Astley - Never Gonna Give You Up\t214";
        let results = parse_search_output(stdout).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].youtube_id, "dQw4w9WgXcQ");
    }

    #[test]
    fn parse_survives_a_leading_run_of_albums() {
        // This is the real shape YouTube Music returns: albums first, songs
        // after. Filtering must leave the songs intact.
        let stdout = "MPREb_ALeXezIU8OA\tNA\tNA\n\
                      MPREb_GvPMXBmBE6G\tNA\tNA\n\
                      MPREb_r1Y8zSeXWqh\tNA\tNA\n\
                      rHbz7toyD1U\tBreak Apart (feat. Rhye)\t275\n\
                      bI_90Z942ZI\tEmkay\t326";
        let results = parse_search_output(stdout).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].youtube_id, "rHbz7toyD1U");
    }

    #[test]
    fn fetch_count_asks_for_more_than_requested() {
        // Albums and channels eat into the listing, so we always ask for a
        // cushion — otherwise a small limit comes back empty.
        assert!(fetch_count(10) > 10);
        assert!(fetch_count(1) > 1);
        assert_eq!(fetch_count(30), 90);
        // A tiny limit still gets a sane floor.
        assert_eq!(fetch_count(0), 10);
    }

    #[test]
    fn fetch_count_does_not_overflow() {
        assert_eq!(fetch_count(usize::MAX), usize::MAX);
    }

    #[test]
    fn parse_filters_out_channels() {
        let stdout = "UCRr1xG_2WIDs18a6cIiCxeA\tNA\tNA\ndQw4w9WgXcQ\tRick Astley - Never Gonna Give You Up\t214";
        assert_eq!(parse_search_output(stdout).unwrap().len(), 1);
    }

    #[test]
    fn parse_filters_out_radio_and_playlists() {
        let stdout = "RDAMVM52fNscjkST4\tNA\tNA\nVLPLkT4Xs2qpKu\tNA\tNA\ndQw4w9WgXcQ\tRick Astley - Never Gonna Give You Up\t214";
        assert_eq!(parse_search_output(stdout).unwrap().len(), 1);
    }

    #[test]
    fn parse_filters_out_na_titles() {
        let stdout = "abc12345678\tNA\tNA\ndQw4w9WgXcQ\tRick Astley - Never Gonna Give You Up\t214";
        assert_eq!(parse_search_output(stdout).unwrap().len(), 1);
    }

    #[test]
    fn parse_result_without_duration() {
        let results = parse_search_output("abc12345678\tSome Artist - Some Song\tNA").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].duration_secs, None);
    }

    #[test]
    fn parse_result_without_third_column() {
        let results = parse_search_output("abc12345678\tSome Artist - Some Song").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].duration_secs, None);
    }

    #[test]
    fn parse_skips_lines_without_a_tab() {
        assert!(parse_search_output("garbage line").unwrap().is_empty());
    }

    #[test]
    fn parse_keeps_extra_tabs_in_title() {
        // A title containing tabs would otherwise be truncated at the first
        // one. Runs of whitespace are collapsed into single spaces.
        let results = parse_search_output("abc12345678\tArtist - Title\twith\ttabs\t180").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Title with tabs");
        assert_eq!(results[0].duration_secs, Some(180));
    }

    #[test]
    fn split_strips_whitespace() {
        let (artist, title) = split_artist_title("  Daft Punk  -  Around the World  ");
        assert_eq!(artist.as_deref(), Some("Daft Punk"));
        assert_eq!(title, "Around the World");
    }

    #[test]
    fn split_keeps_dashes_inside_the_artist() {
        let (artist, title) = split_artist_title("Jay-Z - 99 Problems");
        assert_eq!(artist.as_deref(), Some("Jay-Z"));
        assert_eq!(title, "99 Problems");
    }

    #[test]
    fn split_without_separator_is_all_title() {
        let (artist, title) = split_artist_title("Just A Song");
        assert_eq!(artist, None);
        assert_eq!(title, "Just A Song");
    }

    #[test]
    fn split_ignores_empty_halves() {
        // A leading separator means the artist half is empty — treat the whole
        // string as the title rather than inventing an artist.
        let (artist, title) = split_artist_title(" - Orphan");
        assert_eq!(artist, None);
        assert_eq!(title, "- Orphan");
    }

    #[test]
    fn split_ignores_empty_title_half() {
        let (artist, title) = split_artist_title("Artist - ");
        assert_eq!(artist, None);
        assert_eq!(title, "Artist -");
    }

    #[test]
    fn search_result_converts_to_track() {
        let result = SearchResult {
            youtube_id: "abc12345678".into(),
            title: "Rosewood".into(),
            artist: Some("Bonobo".into()),
            duration_secs: Some(180),
        };
        let track: sonus_core::Track = result.into();
        assert_eq!(track.youtube_id, "abc12345678");
        assert_eq!(track.artist.as_deref(), Some("Bonobo"));
        assert_eq!(track.duration_secs, Some(180));
    }

    #[test]
    fn clean_title_strips_uploader_noise() {
        assert_eq!(clean_title("GLY (Official Audio)"), "GLY");
        assert_eq!(clean_title("Song (Official Video)"), "Song");
        assert_eq!(clean_title("Song [Official Music Video]"), "Song");
        assert_eq!(clean_title("Song (Lyrics)"), "Song");
        assert_eq!(clean_title("Song (Audio)"), "Song");
        assert_eq!(clean_title("Song [HD]"), "Song");
        assert_eq!(clean_title("Song (Visualizer)"), "Song");
    }

    #[test]
    fn clean_title_strips_multiple_suffixes() {
        assert_eq!(clean_title("Song (Official Audio) [HD]"), "Song");
        assert_eq!(clean_title("Song (Official Video) (Explicit)"), "Song");
    }

    #[test]
    fn clean_title_strips_producer_credit() {
        assert_eq!(clean_title("Song (prod. Somebody)"), "Song");
    }

    #[test]
    fn clean_title_strips_a_producer_credit_at_the_end() {
        // The marker is a *prefix* of the bracketed group, not a suffix of the
        // whole title — a regression once made this branch unreachable.
        assert_eq!(clean_title("Song (prod. Metro Boomin)"), "Song");
        assert_eq!(clean_title("Song (PROD. Someone)"), "Song");
        assert_eq!(clean_title("Song (prod.)"), "Song");
    }

    #[test]
    fn clean_title_keeps_a_mid_title_producer_credit() {
        // Only a trailing group is noise; one in the middle is part of the name.
        assert_eq!(
            clean_title("Song (prod. X) feat. Y"),
            "Song (prod. X) feat. Y"
        );
    }

    #[test]
    fn clean_title_is_safe_with_multibyte_titles() {
        // Lowercasing can change a string's byte length (İ is 2 bytes, its
        // lowercase form is 3), so offsets derived from a lowercased copy must
        // never be used to slice the original — that panics mid-character.
        for raw in [
            "İstanbul (Official Audio)",
            "日本語のタイトル (Official Audio)",
            "Café (Audio)",
            "Ωmega [HD]",
            "Привет (Official Video)",
        ] {
            let cleaned = clean_title(raw);
            assert!(!cleaned.is_empty(), "cleaned to nothing: {raw:?}");
            assert!(
                !cleaned.contains("Official") && !cleaned.contains("Audio"),
                "noise survived in {cleaned:?}"
            );
        }
    }

    #[test]
    fn clean_title_keeps_a_multibyte_title_intact() {
        assert_eq!(clean_title("日本語 (Official Audio)"), "日本語");
        assert_eq!(clean_title("Café (Audio)"), "Café");
    }

    #[test]
    fn clean_title_terminates_on_all_noise_input() {
        // The cut used to be a no-op here, spinning the loop forever. This test
        // hanging is the failure mode, so the timeout is the assertion.
        for raw in ["(Official Audio)", "[HD]", "(Audio) [HD]", "(prod. X)"] {
            let cleaned = clean_title(raw);
            assert_eq!(cleaned, raw, "all-noise input should be kept verbatim");
        }
    }

    #[test]
    fn clean_title_is_idempotent() {
        for raw in [
            "Song (Official Audio) [HD]",
            "GLY (Official Audio)",
            "Song",
            "Song (Live at Wembley)",
            "(Official Audio)",
        ] {
            let once = clean_title(raw);
            assert_eq!(clean_title(&once), once, "not idempotent for {raw:?}");
        }
    }

    #[test]
    fn clean_title_is_case_insensitive() {
        assert_eq!(clean_title("Song (OFFICIAL AUDIO)"), "Song");
        assert_eq!(clean_title("Song (official audio)"), "Song");
    }

    #[test]
    fn clean_title_leaves_meaningful_parentheses_alone() {
        // These aren't noise — removing them would lose real information.
        assert_eq!(
            clean_title("Song (Live at Wembley)"),
            "Song (Live at Wembley)"
        );
        assert_eq!(clean_title("Song (feat. Someone)"), "Song (feat. Someone)");
        assert_eq!(clean_title("Song (Remix)"), "Song (Remix)");
        assert_eq!(clean_title("Song (Acoustic)"), "Song (Acoustic)");
        assert_eq!(clean_title("Untitled"), "Untitled");
    }

    #[test]
    fn clean_title_keeps_the_original_when_it_is_all_noise() {
        // Better to show something odd than an empty row.
        assert_eq!(clean_title("(Official Audio)"), "(Official Audio)");
    }

    #[test]
    fn clean_title_collapses_whitespace() {
        assert_eq!(clean_title("  Song   With   Spaces  "), "Song With Spaces");
        assert_eq!(clean_title("Song  (Official Audio)"), "Song");
    }

    #[test]
    fn clean_title_handles_empty_input() {
        assert_eq!(clean_title(""), "");
        assert_eq!(clean_title("   "), "");
    }

    #[test]
    fn parse_flat_title_splits_and_cleans() {
        let (artist, title) = parse_flat_title("Catching Flies - GLY (Official Audio)");
        assert_eq!(artist.as_deref(), Some("Catching Flies"));
        assert_eq!(title, "GLY");
    }

    #[test]
    fn parse_flat_title_handles_a_bare_title() {
        let (artist, title) = parse_flat_title("Rosewood (Official Audio)");
        assert_eq!(artist, None);
        assert_eq!(title, "Rosewood");
    }

    #[test]
    fn url_encoding_replaces_spaces() {
        assert_eq!(url_encode_query("bonobo rosewood"), "bonobo+rosewood");
        assert_eq!(url_encode_query("  spaced  out  "), "spaced+out");
        assert_eq!(url_encode_query("tabs\tand\nnewlines"), "tabs+and+newlines");
    }

    #[test]
    fn video_id_accepts_real_ids() {
        assert!(is_video_id("dQw4w9WgXcQ"));
        assert!(is_video_id("lpxz8oxJFcs"));
        assert!(is_video_id("a-b_c1234XY"));
    }

    #[test]
    fn video_id_rejects_wrong_length() {
        assert!(!is_video_id("short"));
        assert!(!is_video_id("way-too-long-to-be-a-video-id"));
        assert!(!is_video_id(""));
    }

    #[test]
    fn video_id_rejects_non_video_entities() {
        // Real album, channel and playlist IDs, at their real lengths.
        assert!(!is_video_id("MPREb_bFKC3lKP9qhYLx")); // 22 — album
        assert!(!is_video_id("UCRr1xG_2WIDs18a6cIiCxeA")); // 24 — channel
        assert!(!is_video_id("PLkT4Xs2qpKuZ2N8cC5wZVQeYbJd")); // playlist
        assert!(!is_video_id("RDAMVM52fNscjkST4")); // radio
    }

    #[test]
    fn video_id_rejects_illegal_characters() {
        assert!(!is_video_id("abc def ghi"));
        assert!(!is_video_id("abc/def+ghi"));
    }

    #[tokio::test]
    async fn empty_query_short_circuits() {
        assert!(search("   ", 10).await.unwrap().is_empty());
    }
}
