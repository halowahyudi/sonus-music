//! Batch metadata enrichment.
//!
//! YouTube Music's flat search listing is fast (~2s for 30 results) but carries
//! no artist or duration. Fetching full metadata per track is accurate but slow
//! (~2s each), so the search path does both: show the flat results immediately,
//! then fill in the details in one batched call.
//!
//! Passing several URLs to a single yt-dlp invocation is far cheaper than one
//! process per track — yt-dlp amortises the client setup across them.

use thiserror::Error;

use crate::ytdlp_binary;

#[derive(Debug, Error)]
pub enum EnrichError {
    #[error("yt-dlp not found. Install it or set SONUS_YTDLP_PATH")]
    BinaryNotFound,

    #[error("yt-dlp exited with code {0}: {1}")]
    ExitError(i32, String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Fetch artist, album and duration for several tracks at once.
///
/// Returns only the tracks that resolved; a single unavailable video doesn't
/// fail the batch, since yt-dlp reports per-URL errors on stderr and still
/// prints the ones that worked.
pub async fn enrich_tracks(youtube_ids: &[String]) -> Result<Vec<sonus_core::Track>, EnrichError> {
    if youtube_ids.is_empty() {
        return Ok(Vec::new());
    }

    let urls: Vec<String> = youtube_ids
        .iter()
        .map(|id| format!("https://music.youtube.com/watch?v={id}"))
        .collect();

    let binary = ytdlp_binary().ok_or(EnrichError::BinaryNotFound)?;
    let mut command = tokio::process::Command::new(binary);
    command
        .arg("--skip-download")
        .arg("--no-warnings")
        .arg("--ignore-errors")
        .arg("--no-playlist")
        .arg("--print")
        // `track` is the clean title; `artist` is populated for music tracks.
        .arg("%(id)s\t%(track)s\t%(title)s\t%(duration)s\t%(artist)s\t%(album)s")
        .args(&urls);

    let output = command.output().await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            EnrichError::BinaryNotFound
        } else {
            EnrichError::Io(e)
        }
    })?;

    // With `--ignore-errors` a partial success still exits non-zero, so only
    // treat it as fatal when there's no usable output at all.
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() && stdout.trim().is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(EnrichError::ExitError(
            output.status.code().unwrap_or(-1),
            stderr.trim().to_string(),
        ));
    }

    Ok(parse_enrich_output(&stdout))
}

/// Parse the tab-separated enrichment output.
fn parse_enrich_output(stdout: &str) -> Vec<sonus_core::Track> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(parse_enrich_line)
        .collect()
}

fn parse_enrich_line(line: &str) -> Option<sonus_core::Track> {
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() < 3 {
        return None;
    }

    let youtube_id = parts[0].trim().to_string();
    if !crate::search::is_video_id(&youtube_id) {
        return None;
    }

    let field = |index: usize| -> Option<String> {
        parts
            .get(index)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("na"))
            .map(str::to_string)
    };

    // Prefer `track` (the clean song name) over `title`, which may still carry
    // an "Artist - " prefix and uploader noise like "(Official Audio)".
    let title = crate::search::clean_title(&field(1).or_else(|| field(2))?);

    let duration_secs = field(3).and_then(|s| s.parse::<u64>().ok());

    let mut track = sonus_core::Track::new(youtube_id, title);
    track.artist = field(4);
    track.album = field(5);
    track.duration_secs = duration_secs;
    Some(track)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_row() {
        let stdout = "lpxz8oxJFcs\tRosewood\tRosewood\t243\tBonobo\tBlack Sands";
        let tracks = parse_enrich_output(stdout);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].youtube_id, "lpxz8oxJFcs");
        assert_eq!(tracks[0].title, "Rosewood");
        assert_eq!(tracks[0].artist.as_deref(), Some("Bonobo"));
        assert_eq!(tracks[0].album.as_deref(), Some("Black Sands"));
        assert_eq!(tracks[0].duration_secs, Some(243));
    }

    #[test]
    fn parses_several_rows() {
        let stdout = "aaaaaaaaaaa\tOne\tOne\t100\tArtist A\tAlbum\n\
                      bbbbbbbbbbb\tTwo\tTwo\t200\tArtist B\tNA";
        let tracks = parse_enrich_output(stdout);
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[1].artist.as_deref(), Some("Artist B"));
        assert_eq!(tracks[1].album, None);
    }

    #[test]
    fn prefers_track_over_title() {
        let stdout = "aaaaaaaaaaa\tClean\tArtist - Clean\t100\tArtist\tNA";
        assert_eq!(parse_enrich_output(stdout)[0].title, "Clean");
    }

    #[test]
    fn falls_back_to_title_when_track_is_missing() {
        let stdout = "aaaaaaaaaaa\tNA\tArtist - Song\t100\tArtist\tNA";
        assert_eq!(parse_enrich_output(stdout)[0].title, "Artist - Song");
    }

    #[test]
    fn treats_na_fields_as_absent() {
        let stdout = "aaaaaaaaaaa\tSong\tSong\tNA\tNA\tNA";
        let tracks = parse_enrich_output(stdout);
        assert_eq!(tracks[0].artist, None);
        assert_eq!(tracks[0].album, None);
        assert_eq!(tracks[0].duration_secs, None);
    }

    #[test]
    fn skips_rows_for_non_videos() {
        let stdout = "MPREb_bFKC3lKP9qhYLx\tNA\tNA\tNA\tNA\tNA\n\
                      aaaaaaaaaaa\tSong\tSong\t100\tA\tNA";
        assert_eq!(parse_enrich_output(stdout).len(), 1);
    }

    #[test]
    fn skips_malformed_rows() {
        assert!(parse_enrich_output("garbage").is_empty());
        assert!(parse_enrich_output("").is_empty());
        assert!(parse_enrich_output("\n\n").is_empty());
    }

    #[test]
    fn skips_rows_with_no_usable_title() {
        let stdout = "aaaaaaaaaaa\tNA\tNA\t100\tArtist\tNA";
        assert!(parse_enrich_output(stdout).is_empty());
    }

    #[test]
    fn skips_non_numeric_durations() {
        let stdout = "aaaaaaaaaaa\tSong\tSong\tnot-a-number\tA\tNA";
        assert_eq!(parse_enrich_output(stdout)[0].duration_secs, None);
    }

    #[test]
    fn handles_artists_with_commas() {
        let stdout = "aaaaaaaaaaa\tSong\tSong\t200\tBonobo, Ichiko Aoba\tNA";
        assert_eq!(
            parse_enrich_output(stdout)[0].artist.as_deref(),
            Some("Bonobo, Ichiko Aoba")
        );
    }

    #[tokio::test]
    async fn empty_input_returns_empty_without_spawning_ytdlp() {
        assert!(enrich_tracks(&[]).await.unwrap().is_empty());
    }
}
