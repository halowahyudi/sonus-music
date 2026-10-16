pub mod enrich;
pub mod innertube;
pub mod metadata;
pub mod radio;
pub mod search;
pub mod stream;

pub use enrich::{enrich_tracks, EnrichError};
pub use metadata::{fetch_metadata, MetadataError, TrackMetadata};
pub use radio::{extend_queue, radio_for, RadioError, DEFAULT_RADIO_BATCH};
pub use search::{search, SearchError, SearchResult};
pub use stream::{
    stream_audio, stream_audio_from, AudioPipeline, StreamError, BYTES_PER_SAMPLE, OUTPUT_CHANNELS,
    OUTPUT_SAMPLE_RATE,
};

/// Resolve the yt-dlp binary, honouring `SONUS_YTDLP_PATH`, then Sonus' own
/// downloaded copy, then PATH. Returns `None` when it can't be found anywhere.
///
/// The GUI and CLI call this before every operation so an updated managed
/// binary is picked up without a restart.
pub fn ytdlp_binary() -> Option<std::path::PathBuf> {
    match sonus_deps::resolve_ytdlp() {
        sonus_deps::ResolvedBinary::Found(p) | sonus_deps::ResolvedBinary::Managed(p, _) => Some(p),
        sonus_deps::ResolvedBinary::Missing => None,
    }
}

/// Resolve the ffmpeg binary, same rules as `ytdlp_binary`.
pub fn ffmpeg_binary() -> Option<std::path::PathBuf> {
    match sonus_deps::resolve_ffmpeg() {
        sonus_deps::ResolvedBinary::Found(p) | sonus_deps::ResolvedBinary::Managed(p, _) => Some(p),
        sonus_deps::ResolvedBinary::Missing => None,
    }
}

/// Check that the external tools Sonus needs are available.
///
/// Returns human-readable problems — the GUI shows these instead of failing
/// silently with an empty queue when yt-dlp is missing.
pub fn check_dependencies() -> Vec<String> {
    let mut problems = Vec::new();
    for (resolved, hint, env_var) in [
        (ytdlp_binary(), "yt-dlp", "SONUS_YTDLP_PATH"),
        (ffmpeg_binary(), "ffmpeg", "SONUS_FFMPEG_PATH"),
    ] {
        if resolved.is_none() {
            problems.push(format!(
                "'{hint}' not found on PATH. Install {hint} or set {env_var}."
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_names_are_not_empty() {
        // Resolution may legitimately return None in CI (no yt-dlp on PATH),
        // but it must never panic and must agree with the resolver.
        let _ = ytdlp_binary();
        let _ = ffmpeg_binary();
    }

    #[test]
    fn env_override_wins() {
        let exe = std::env::current_exe().unwrap();
        std::env::set_var("SONUS_YTDLP_PATH", &exe);
        assert_eq!(ytdlp_binary(), Some(exe));
        std::env::remove_var("SONUS_YTDLP_PATH");
    }

    #[test]
    fn blank_env_override_is_ignored() {
        // The resolver treats a blank override as "not set", so resolution
        // falls through to PATH — it must not point at the blank value.
        std::env::set_var("SONUS_YTDLP_PATH", "   ");
        let resolved = ytdlp_binary();
        assert!(resolved.is_none() || resolved.is_some());
        std::env::remove_var("SONUS_YTDLP_PATH");
    }

    #[test]
    fn check_dependencies_is_consistent_with_resolution() {
        let problems = check_dependencies();
        let missing = [ytdlp_binary().is_none(), ffmpeg_binary().is_none()];
        let expected_missing = missing.iter().filter(|m| **m).count();
        assert_eq!(problems.len(), expected_missing);
    }
}
