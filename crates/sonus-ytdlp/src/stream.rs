use std::process::{Child, ChildStdout, Stdio};
use thiserror::Error;

use crate::{ffmpeg_binary, ytdlp_binary};

/// Raw PCM format the whole pipeline agrees on.
pub const OUTPUT_SAMPLE_RATE: u32 = 48000;
pub const OUTPUT_CHANNELS: u16 = 2;
pub const BYTES_PER_SAMPLE: usize = 2;

#[derive(Debug, Error)]
pub enum StreamError {
    #[error("yt-dlp not found. Install it or set SONUS_YTDLP_PATH")]
    BinaryNotFound,

    #[error("ffmpeg not found. Install it or set SONUS_FFMPEG_PATH")]
    FfmpegNotFound,

    #[error("failed to start stream: {0}")]
    Spawn(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// The full pipeline: yt-dlp → ffmpeg → raw PCM stdout.
///
/// Both children must stay alive while stdout is being read. If either is
/// dropped the pipe closes and playback gets a truncated stream, so the owner
/// holds them for the lifetime of the track.
pub struct AudioPipeline {
    /// Raw PCM (s16le, 48kHz, stereo) comes out here. Taken once by
    /// [`AudioPipeline::take_stdout`].
    stdout: Option<ChildStdout>,
    /// Keep alive — feeds ffmpeg via a pipe.
    pub ytdlp: Child,
    /// Keep alive — decodes and resamples to PCM.
    pub ffmpeg: Child,
}

impl AudioPipeline {
    /// Take the PCM stream, leaving the pipeline holding only its children.
    ///
    /// The pipeline implements `Drop`, so its fields can't be moved out
    /// directly — the consumer takes the reader once and keeps the pipeline
    /// alive to reap the processes.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.stdout.take()
    }

    /// Kill both children and reap them. Safe to call more than once.
    pub fn kill(&mut self) {
        let _ = self.ytdlp.kill();
        let _ = self.ffmpeg.kill();
        let _ = self.ytdlp.wait();
        let _ = self.ffmpeg.wait();
    }
}

impl Drop for AudioPipeline {
    fn drop(&mut self) {
        // Orphaned yt-dlp/ffmpeg processes would keep downloading in the
        // background. Reap them whenever the pipeline goes away.
        self.kill();
    }
}

/// Start streaming a track from the beginning.
pub fn stream_audio(youtube_id: &str) -> Result<AudioPipeline, StreamError> {
    stream_audio_from(youtube_id, 0)
}

/// Start streaming a track from `start_secs` into it.
///
/// Seeking uses yt-dlp's `--download-sections`, which asks the server for a
/// byte range covering the requested window. That means a seek costs one new
/// pipeline rather than a byte-offset into an already-decoded stream — which
/// is the only reliable option for the streaming formats YouTube serves.
pub fn stream_audio_from(youtube_id: &str, start_secs: u64) -> Result<AudioPipeline, StreamError> {
    let url = format!("https://www.youtube.com/watch?v={youtube_id}");

    let ytdlp_path = ytdlp_binary().ok_or(StreamError::BinaryNotFound)?;
    let ffmpeg_path = ffmpeg_binary().ok_or(StreamError::FfmpegNotFound)?;

    let mut ytdlp = std::process::Command::new(ytdlp_path);
    ytdlp
        .arg("-f")
        .arg("bestaudio/best")
        .arg("-o")
        .arg("-")
        .arg("--no-warnings")
        .arg("--no-playlist")
        // --quiet keeps ffmpeg's progress bars off our stderr pipe; without it
        // the pipe can fill up and deadlock the download.
        .arg("--quiet");

    if start_secs > 0 {
        // "*START-" means "from START to the end of the track".
        ytdlp
            .arg("--download-sections")
            .arg(format!("*{start_secs}-"));
        // Without this the cut lands on the nearest keyframe, which can be
        // several seconds off and produces a jarring jump.
        ytdlp.arg("--force-keyframes-at-cuts");
    }

    ytdlp.arg(&url).stdout(Stdio::piped()).stderr(Stdio::null());

    let mut ytdlp_child = ytdlp.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            StreamError::BinaryNotFound
        } else {
            StreamError::Spawn(e.to_string())
        }
    })?;

    // Take the stdout handle before handing the child to ffmpeg as stdin.
    let ytdlp_stdout = ytdlp_child
        .stdout
        .take()
        .ok_or_else(|| StreamError::Spawn("yt-dlp stdout not captured".into()))?;

    let mut ffmpeg = std::process::Command::new(ffmpeg_path);
    ffmpeg
        .arg("-i")
        .arg("pipe:0")
        // Drop video and subtitles if the container has any — we only want audio.
        .arg("-vn")
        .arg("-f")
        .arg("s16le")
        .arg("-acodec")
        .arg("pcm_s16le")
        .arg("-ac")
        .arg(OUTPUT_CHANNELS.to_string())
        .arg("-ar")
        .arg(OUTPUT_SAMPLE_RATE.to_string())
        .arg("pipe:1")
        .stdin(Stdio::from(ytdlp_stdout))
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut ffmpeg_child = ffmpeg.spawn().map_err(|e| {
        // yt-dlp is already running — don't leak it if ffmpeg is missing.
        let _ = ytdlp_child.kill();
        let _ = ytdlp_child.wait();
        if e.kind() == std::io::ErrorKind::NotFound {
            StreamError::FfmpegNotFound
        } else {
            StreamError::Spawn(e.to_string())
        }
    })?;

    let stdout = ffmpeg_child
        .stdout
        .take()
        .ok_or_else(|| StreamError::Spawn("ffmpeg stdout not captured".into()))?;

    Ok(AudioPipeline {
        stdout: Some(stdout),
        ytdlp: ytdlp_child,
        ffmpeg: ffmpeg_child,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_format_matches_engine_expectations() {
        // The engine hard-codes these when building its rodio source; if they
        // drift apart playback turns into noise.
        assert_eq!(OUTPUT_SAMPLE_RATE, 48000);
        assert_eq!(OUTPUT_CHANNELS, 2);
        assert_eq!(BYTES_PER_SAMPLE, 2);
    }

    #[test]
    fn binaries_resolve_or_report_missing() {
        // Resolution may be None in CI, but must never panic.
        let _ = ytdlp_binary();
        let _ = ffmpeg_binary();
    }
}
