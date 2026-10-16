//! Checking whether an installed tool is out of date.
//!
//! yt-dlp is notorious for breaking as YouTube changes, so a version that was
//! current last week can silently stop resolving tracks today. We check the
//! latest version against what's installed, but never more than once a day —
//! every launch hammering GitHub's API is both rude and slow.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::install::{resolve, Dependency};
use crate::manifest::{load_manifest, Version};
use crate::resolve::{latest_version, ResolvedBinary, ResolutionError};
use crate::bin_dir;

/// How long between staleness checks. yt-dlp releases every few days, so daily
/// is plenty.
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Result of a staleness check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleStatus {
    /// Up to date (or version unknown).
    Fresh,
    /// A newer version exists; `current` and `latest` for display.
    Stale { current: Version, latest: Version },
    /// Not installed in Sonus' bin dir (system-provided) — nothing to update.
    NotManaged,
    /// We couldn't reach the network to ask.
    Unreachable,
}

/// Check whether `dep` has a newer version available, but only once a day.
///
/// The throttle is tracked by the mtime of a marker file in the bin dir, so it
/// survives restarts and is naturally per-user.
pub fn check_stale(dep: Dependency) -> StaleStatus {
    let marker = marker_path(dep);

    // Respect the throttle: unless the marker is older than a day, skip the
    // network call entirely and report whatever we knew.
    if marker_young(&marker) {
        return match resolved_version(dep) {
            Some(_) => StaleStatus::Fresh,
            None => StaleStatus::NotManaged,
        };
    }
    let _ = touch(&marker);

    let current = match resolved_version(dep) {
        Some(v) => v,
        None => return StaleStatus::NotManaged,
    };

    let latest = match latest_version(dep.name()) {
        Ok(v) => v,
        Err(_) => return StaleStatus::Unreachable,
    };

    if latest > current {
        StaleStatus::Stale { current, latest }
    } else {
        StaleStatus::Fresh
    }
}

/// The version of a tool that is actually running (managed or system).
fn resolved_version(dep: Dependency) -> Option<Version> {
    match resolve(dep) {
        ResolvedBinary::Managed(_, v) => Some(v),
        // System-provided: version lives in the manifest only if it was us who
        // installed it. For a PATH binary there's no reliable manifest entry.
        ResolvedBinary::Found(_) => {
            let manifest = load_manifest(&bin_dir());
            manifest.installed_version(dep.name())
        }
        ResolvedBinary::Missing => None,
    }
}

fn marker_path(dep: Dependency) -> PathBuf {
    bin_dir().join(format!(".{}.check", dep.name()))
}

fn marker_young(marker: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(marker) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    modified + CHECK_INTERVAL > SystemTime::now()
}

fn touch(marker: &Path) -> std::io::Result<()> {
    std::fs::write(marker, b"")
}

/// Check if there's any newer version available, ignoring the throttle.
/// Used by `sonus deps update` to decide whether to bother downloading.
pub fn update_available(dep: Dependency) -> Result<Option<Version>, ResolutionError> {
    let current = resolved_version(dep);
    let latest = latest_version(dep.name())?;
    Ok(match current {
        Some(c) if latest > c => Some(latest),
        Some(_) => None,
        None => Some(latest),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique marker path per test, so parallel runs can't collide.
    fn marker(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sonus_marker_{}_{}", name, std::process::id()))
    }

    #[test]
    fn marker_young_detects_recent_check() {
        let path = marker("young");
        let _ = std::fs::remove_file(&path);
        touch(&path).unwrap();
        assert!(marker_young(&path));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn marker_absent_is_not_young() {
        let path = marker("absent");
        let _ = std::fs::remove_file(&path);
        assert!(!marker_young(&path));
    }

    #[test]
    fn marker_expires_after_interval() {
        let path = marker("old");
        let _ = std::fs::remove_file(&path);
        touch(&path).unwrap();
        // Rewind the mtime to well before the interval.
        let old = SystemTime::now() - Duration::from_secs(48 * 60 * 60);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(old))
            .unwrap();
        assert!(!marker_young(&path));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn stale_status_derives_partial_eq() {
        // Guards that the status type can be compared in GUI code.
        assert_eq!(
            StaleStatus::Stale {
                current: Version::parse("2026.1.1").unwrap(),
                latest: Version::parse("2026.8.19").unwrap(),
            },
            StaleStatus::Stale {
                current: Version::parse("2026.1.1").unwrap(),
                latest: Version::parse("2026.8.19").unwrap(),
            }
        );
        assert_ne!(StaleStatus::Fresh, StaleStatus::NotManaged);
    }
}
