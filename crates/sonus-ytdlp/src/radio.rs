use crate::search::{is_video_id, parse_search_output, run_ytdlp, SearchError};

pub type RadioError = SearchError;

/// How many tracks to pull when extending the queue.
pub const DEFAULT_RADIO_BATCH: usize = 15;

/// Tracks similar to a seed track, pulled from YouTube's recommendation radio.
///
/// YouTube exposes a per-video "radio" mix at `list=RDAMVM<video_id>`. The
/// Innertube path reuses the page data the Music web UI boots from — one GET,
/// with artist/duration/thumbnail attached, so no enrichment round-trip. When
/// that path breaks (YouTube shuffling the loader), we fall back to asking
/// yt-dlp for the same list as a flat playlist.
///
/// The seed itself is always the first entry; it's dropped here because the
/// caller already has that track queued.
pub async fn radio_for(
    youtube_id: &str,
    limit: usize,
) -> Result<Vec<sonus_core::Track>, RadioError> {
    if !is_video_id(youtube_id) {
        return Err(SearchError::Parse(format!(
            "not a YouTube video id: {youtube_id}"
        )));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| SearchError::Parse(format!("http client: {e}")))?;

    match crate::innertube::radio_tracks(&client, youtube_id, limit).await {
        Ok(tracks) if !tracks.is_empty() => return Ok(tracks),
        // Fall through to the subprocess path on an empty or failed result.
        Ok(_) => {}
        Err(_) => {}
    }

    let url = format!("https://music.youtube.com/watch?v={youtube_id}&list=RDAMVM{youtube_id}");

    let stdout = run_ytdlp(&[
        "--flat-playlist",
        "--print",
        "%(id)s\t%(title)s\t%(duration)s",
        "--playlist-end",
        // Ask for one extra to absorb the seed, which we drop below.
        &(limit + 1).to_string(),
        &url,
    ])
    .await?;

    let mut results = parse_search_output(&stdout)?;
    results.retain(|r| r.youtube_id != youtube_id);

    Ok(results
        .into_iter()
        .take(limit)
        .map(sonus_core::Track::from)
        .collect())
}

/// Extend a queue from a seed track, skipping anything already queued.
///
/// This is the "keep the music going" path: when playback reaches the end of
/// what the user explicitly chose, we top the queue up from the radio rather
/// than stopping.
///
/// The Innertube radio listing arrives with artist/duration/thumbnail already
/// attached, so enrichment is skipped when every row has them. Fallback rows
/// from yt-dlp are flat (fast, ~2s) and then enriched in the same batch,
/// because a queue full of "Unknown Artist" rows looks broken. The enrichment
/// runs while music is already playing, so the extra call is invisible.
pub async fn extend_queue(
    seed: &sonus_core::Track,
    already_queued: &[String],
    limit: usize,
) -> Result<Vec<sonus_core::Track>, RadioError> {
    let candidates = radio_for(&seed.youtube_id, limit).await?;
    let fresh: Vec<sonus_core::Track> = candidates
        .into_iter()
        .filter(|t| !already_queued.contains(&t.youtube_id))
        .collect();

    if fresh.is_empty() {
        return Ok(fresh);
    }

    // Innertube rows are already complete; only the flat fallback needs
    // enriching.
    let needs_enrichment = fresh.iter().any(|t| t.artist.is_none());
    if !needs_enrichment {
        return Ok(fresh);
    }

    let ids: Vec<String> = fresh.iter().map(|t| t.youtube_id.clone()).collect();
    let enriched = crate::enrich::enrich_tracks(&ids).await.unwrap_or_default();

    if enriched.is_empty() {
        // Enrichment is best-effort; fall back to the flat titles.
        return Ok(fresh);
    }

    // Keep the radio's ordering and use the flat title as a fallback for
    // anything enrichment didn't return.
    let by_id: std::collections::HashMap<String, sonus_core::Track> = enriched
        .into_iter()
        .map(|t| (t.youtube_id.clone(), t))
        .collect();

    Ok(fresh
        .into_iter()
        .map(|flat| match by_id.get(&flat.youtube_id) {
            Some(rich) => {
                let mut merged = rich.clone();
                if merged.title.trim().is_empty() {
                    merged.title = flat.title;
                }
                merged
            }
            None => flat,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_short_id_without_spawning_ytdlp() {
        let err = radio_for("too-short", 5).await.unwrap_err();
        assert!(matches!(err, SearchError::Parse(_)));
    }

    #[tokio::test]
    async fn rejects_album_id_without_spawning_ytdlp() {
        let err = radio_for("MPREb_bFKC3lKP9qhYLx", 5).await.unwrap_err();
        assert!(matches!(err, SearchError::Parse(_)));
    }

    #[tokio::test]
    async fn rejects_empty_id() {
        assert!(radio_for("", 5).await.is_err());
    }

    #[tokio::test]
    async fn extend_queue_rejects_bad_seed() {
        let seed = sonus_core::Track::new("bad", "Nope");
        assert!(extend_queue(&seed, &[], 5).await.is_err());
    }
}
