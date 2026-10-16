//! Manifest of installed binaries.
//!
//! `deps.json` in the bin directory records which tool, at which version, we
//! downloaded. It lets us detect staleness (yt-dlp changes often — YouTube
//! keeps breaking it) and skip a reinstall when the on-disk version already
//! matches what the remote advertises.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A three-part version like `7.0.2` or `2026.8.19`.
///
/// yt-dlp dates use `2026.8.19` (single-digit months stay single-digit), so
/// comparisons must be numeric, not lexicographic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        // Tags carry noise around the numbers: `b6.1.1`, `v7.0.2`, `release-2026.08.19`…
        // Skip leading non-digit characters, then read three dot-separated numbers.
        let s = s.trim();
        let digits_start = s.find(|c: char| c.is_ascii_digit())?;
        let mut parts = s[digits_start..].split('.');
        Some(Self {
            major: parts.next()?.parse().ok()?,
            minor: parts.next()?.parse().ok()?,
            patch: parts.next()?.parse().ok()?,
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// What the manifest knows about one installed tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledEntry {
    pub version: Version,
}

/// The whole manifest: one entry per tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub tools: std::collections::BTreeMap<String, InstalledEntry>,
}

impl Manifest {
    pub fn installed_version(&self, tool: &str) -> Option<Version> {
        self.tools.get(tool).map(|e| e.version)
    }
}

/// Path of the manifest inside the bin directory.
fn manifest_path(bin: &Path) -> PathBuf {
    bin.join("deps.json")
}

/// Read the manifest, tolerating a missing or corrupt file.
///
/// A corrupt manifest is treated as empty — the worst that happens is one
/// redundant reinstall, which is cheaper than refusing to start.
pub fn load_manifest(bin: &Path) -> Manifest {
    let path = manifest_path(bin);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Write the manifest, ignoring failures — it's a convenience, not critical.
pub fn save_manifest(bin: &Path, manifest: &Manifest) {
    let _ = std::fs::create_dir_all(bin);
    if let Ok(raw) = serde_json::to_string_pretty(manifest) {
        let _ = std::fs::write(manifest_path(bin), raw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parses_three_parts() {
        let v = Version::parse("2026.8.19").unwrap();
        assert_eq!(v.major, 2026);
        assert_eq!(v.minor, 8);
        assert_eq!(v.patch, 19);
    }

    #[test]
    fn version_rejects_garbage() {
        assert!(Version::parse("").is_none());
        assert!(Version::parse("7").is_none());
        assert!(Version::parse("a.b.c").is_none());
    }

    #[test]
    fn version_skips_leading_tag_noise() {
        // eugeneware/ffmpeg-static tags its releases `b6.1.1`.
        assert_eq!(Version::parse("b6.1.1").unwrap().to_string(), "6.1.1");
        assert_eq!(Version::parse("v7.0.2").unwrap().to_string(), "7.0.2");
        assert_eq!(Version::parse("release-2026.08.19").unwrap().to_string(), "2026.8.19");
    }

    #[test]
    fn version_ordering_is_numeric() {
        // Lexicographic would say 2026.9.1 < 2026.10.1; numeric must not.
        let a = Version::parse("2026.9.1").unwrap();
        let b = Version::parse("2026.10.1").unwrap();
        assert!(a < b);
    }

    #[test]
    fn version_round_trips() {
        let v = Version::parse("7.0.2").unwrap();
        assert_eq!(v.to_string(), "7.0.2");
    }

    #[test]
    fn manifest_round_trips_through_disk() {
        let dir = std::env::temp_dir().join("sonus_deps_manifest_test");
        let _ = std::fs::remove_dir_all(&dir);

        let mut m = Manifest::default();
        m.tools.insert(
            "yt-dlp".into(),
            InstalledEntry { version: Version::parse("2026.8.19").unwrap() },
        );
        save_manifest(&dir, &m);

        let loaded = load_manifest(&dir);
        assert_eq!(
            loaded.installed_version("yt-dlp").unwrap().to_string(),
            "2026.8.19"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_manifest_loads_empty() {
        let dir = std::env::temp_dir().join("sonus_deps_manifest_none");
        let _ = std::fs::remove_dir_all(&dir);
        let m = load_manifest(&dir);
        assert!(m.tools.is_empty());
    }

    #[test]
    fn corrupt_manifest_loads_empty() {
        let dir = std::env::temp_dir().join("sonus_deps_manifest_corrupt");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("deps.json"), "not json at all").unwrap();
        let m = load_manifest(&dir);
        assert!(m.tools.is_empty());
    }
}
