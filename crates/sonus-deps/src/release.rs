//! Descriptions of downloadable releases.
//!
//! The two sources Sonus downloads from expose their assets in different
//! shapes, but both report a SHA-256 digest per asset. We fold them into a
//! single `Release` struct with an `Artifact` per file we care about.

use serde::{Deserialize, Serialize};

/// One downloadable file: where it is, what it is, and a digest to verify it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// Display name, e.g. `yt-dlp_linux` or `ffmpeg-linux-x64.gz`.
    pub name: String,
    /// Full download URL.
    pub url: String,
    /// File size in bytes, for progress bars.
    pub size: u64,
    /// SHA-256 digest, hex-encoded, of the *downloaded* file.
    pub sha256: String,
    /// Local filename to save the download as.
    pub filename: String,
    /// If the file is gzip-compressed, the name it decompresses to.
    pub extract_to: Option<String>,
}

/// A set of artifacts for one tool release.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub tool: String,
    pub version: super::Version,
    /// The artifact matching the current platform.
    pub artifact: Artifact,
    /// Temp path the download lands in before verification.
    pub temp_download: std::path::PathBuf,
    /// Temp path for a decompressed artifact, if gzipped.
    pub temp_extracted: Option<std::path::PathBuf>,
}
