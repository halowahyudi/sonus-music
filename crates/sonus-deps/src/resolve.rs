//! Resolving which binary to run, and which download matches this machine.
//!
//! Resolution is offline-friendly: the player can start without touching the
//! network. Only when a tool is genuinely missing (or stale) do we fetch the
//! release metadata to pick a download.

use std::path::PathBuf;

use thiserror::Error;

use crate::manifest::Version;
use crate::release::{Artifact, Release};
use crate::{bin_dir, ffmpeg_exe_name, find_on_path, ytdlp_exe_name};

/// The two GitHub repos Sonus pulls from.
const YTDLP_OWNER: &str = "yt-dlp/yt-dlp";
const FFMPEG_OWNER: &str = "eugeneware/ffmpeg-static";
const YTDLP_RELEASE: &str = "latest";
const FFMPEG_RELEASE: &str = "latest";

#[derive(Debug, Error)]
pub enum ResolutionError {
    #[error("network error: {0}")]
    Network(String),

    #[error("could not read release info: {0}")]
    Json(#[from] serde_json::Error),

    #[error("no {tool} download available for this platform (os={os}, arch={arch})")]
    UnsupportedPlatform {
        tool: String,
        os: String,
        arch: String,
    },
}

/// How Sonus finds an installed binary, in priority order:
///
/// 1. An explicit `SONUS_YTDLP_PATH` / `SONUS_FFMPEG_PATH` override.
/// 2. A copy Sonus installed itself in the data dir.
/// 3. Whatever is on PATH (a distro package, a manual install, ...).
///
/// The PATH fallback matters: users who already manage yt-dlp or ffmpeg with
/// their package manager shouldn't be forced onto our bundled copy.
#[derive(Debug)]
pub enum ResolvedBinary {
    /// Found outside the managed bin dir (PATH or env override).
    Found(PathBuf),
    /// Sonus installed this one; `Version` is what the manifest records.
    Managed(PathBuf, Version),
    /// Not found anywhere — the caller should offer to install it.
    Missing,
}

impl ResolvedBinary {
    pub fn path(&self) -> Option<&std::path::Path> {
        match self {
            ResolvedBinary::Found(p) | ResolvedBinary::Managed(p, _) => Some(p),
            ResolvedBinary::Missing => None,
        }
    }

    pub fn is_missing(&self) -> bool {
        matches!(self, ResolvedBinary::Missing)
    }
}

/// Resolve yt-dlp without any network access.
pub fn resolve_ytdlp() -> ResolvedBinary {
    resolve_tool("SONUS_YTDLP_PATH", ytdlp_exe_name(), "yt-dlp")
}

/// Resolve ffmpeg without any network access.
pub fn resolve_ffmpeg() -> ResolvedBinary {
    resolve_tool("SONUS_FFMPEG_PATH", ffmpeg_exe_name(), "ffmpeg")
}

fn resolve_tool(env_var: &str, exe_name: &'static str, tool: &str) -> ResolvedBinary {
    if let Some(path) = env_override(env_var) {
        return ResolvedBinary::Found(path);
    }

    let managed = bin_dir().join(exe_name);
    if managed.is_file() {
        let version = crate::load_manifest(&bin_dir()).installed_version(tool);
        if let Some(version) = version {
            return ResolvedBinary::Managed(managed, version);
        }
        // A binary on disk with no manifest entry — treat as managed anyway.
        return ResolvedBinary::Found(managed);
    }

    find_on_path(exe_name)
        .map(ResolvedBinary::Found)
        .unwrap_or(ResolvedBinary::Missing)
}

/// A non-empty env var override; blank values are treated as unset.
fn env_override(name: &str) -> Option<PathBuf> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
}

/// Which asset name matches this machine, given the release JSON.
fn pick_asset<'a>(
    release_json: &'a serde_json::Value,
    candidates: &[&'static str],
    tool: &str,
) -> Result<&'a str, ResolutionError> {
    let assets = release_json
        .get("assets")
        .and_then(|a| a.as_array())
        .ok_or_else(|| ResolutionError::Network("release has no assets".into()))?;

    for candidate in candidates {
        if assets
            .iter()
            .any(|a| a.get("name").and_then(|n| n.as_str()) == Some(*candidate))
        {
            return Ok(candidate);
        }
    }
    Err(ResolutionError::UnsupportedPlatform {
        tool: tool.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    })
}

/// yt-dlp publishes raw per-platform executables.
fn ytdlp_asset(release_json: &serde_json::Value) -> Result<&str, ResolutionError> {
    let candidates: Vec<&'static str> = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => vec!["yt-dlp.exe"],
        ("windows", "aarch64") => vec!["yt-dlp_arm64.exe"],
        ("windows", "x86") => vec!["yt-dlp_x86.exe"],
        ("linux", "x86_64") => vec!["yt-dlp_linux"],
        ("linux", "aarch64") => vec!["yt-dlp_linux_aarch64"],
        ("macos", "x86_64") | ("macos", "aarch64") => vec!["yt-dlp_macos"],
        _ => vec![],
    };
    pick_asset(release_json, &candidates, "yt-dlp")
}

/// ffmpeg-static publishes gzipped per-platform binaries.
fn ffmpeg_asset(release_json: &serde_json::Value) -> Result<&str, ResolutionError> {
    let candidates: Vec<&'static str> = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => vec!["ffmpeg-win32-x64.gz"],
        ("linux", "x86_64") => vec!["ffmpeg-linux-x64.gz"],
        ("linux", "aarch64") => vec!["ffmpeg-linux-arm64.gz"],
        ("macos", "x86_64") => vec!["ffmpeg-darwin-x64.gz"],
        ("macos", "aarch64") => vec!["ffmpeg-darwin-arm64.gz"],
        _ => vec![],
    };
    pick_asset(release_json, &candidates, "ffmpeg")
}

fn asset_node<'a>(
    release_json: &'a serde_json::Value,
    name: &str,
) -> Option<&'a serde_json::Value> {
    release_json
        .get("assets")?
        .as_array()?
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(name))
}

fn url_for(owner: &str, release: &str, asset: &str) -> String {
    // "latest" is a special GitHub alias — the download URL differs from a
    // pinned tag's URL.
    if release == "latest" {
        format!("https://github.com/{owner}/releases/latest/download/{asset}")
    } else {
        format!("https://github.com/{owner}/releases/download/{release}/{asset}")
    }
}

fn sha_for(release_json: &serde_json::Value, asset: &str) -> String {
    asset_node(release_json, asset)
        .and_then(|a| a.get("digest").and_then(|d| d.as_str()))
        // GitHub reports digests as `sha256:<hex>`.
        .map(|d| d.strip_prefix("sha256:").unwrap_or(d).to_string())
        .unwrap_or_default()
}

fn size_for(release_json: &serde_json::Value, asset: &str) -> u64 {
    asset_node(release_json, asset)
        .and_then(|a| a.get("size").and_then(|s| s.as_u64()))
        .unwrap_or(0)
}

/// The latest version advertised for a tool, without downloading anything.
pub fn latest_version(tool: &str) -> Result<Version, ResolutionError> {
    Ok(release_for(tool)?.version)
}

/// Build the full `Release` (which artifact + where) for a tool.
pub fn release_for(tool: &str) -> Result<Release, ResolutionError> {
    let (owner, release_tag, url) = match tool {
        "yt-dlp" => (
            YTDLP_OWNER,
            YTDLP_RELEASE,
            format!("https://api.github.com/repos/{YTDLP_OWNER}/releases/{YTDLP_RELEASE}"),
        ),
        "ffmpeg" => (
            FFMPEG_OWNER,
            FFMPEG_RELEASE,
            format!("https://api.github.com/repos/{FFMPEG_OWNER}/releases/{FFMPEG_RELEASE}"),
        ),
        other => {
            return Err(ResolutionError::UnsupportedPlatform {
                tool: other.to_string(),
                os: std::env::consts::OS.to_string(),
                arch: std::env::consts::ARCH.to_string(),
            })
        }
    };

    let response = ureq::get(&url)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "sonus-music")
        .call()
        .map_err(|e| ResolutionError::Network(e.to_string()))?;

    let release_json: serde_json::Value = response
        .into_string()
        .map_err(|e| ResolutionError::Network(e.to_string()))?
        .parse()?;

    let tag = release_json
        .get("tag_name")
        .and_then(|t| t.as_str())
        .unwrap_or("0.0.0");
    let version = Version::parse(tag).unwrap_or(Version {
        major: 0,
        minor: 0,
        patch: 0,
    });

    let (asset_name, extract_to) = match tool {
        "yt-dlp" => (ytdlp_asset(&release_json)?, None),
        "ffmpeg" => (
            ffmpeg_asset(&release_json)?,
            // The gzip decompresses to the bare `ffmpeg`/`ffmpeg.exe` name.
            Some(ffmpeg_exe_name().to_string()),
        ),
        _ => unreachable!("tool validated above"),
    };

    let artifact = Artifact {
        name: asset_name.to_string(),
        url: url_for(owner, release_tag, asset_name),
        size: size_for(&release_json, asset_name),
        sha256: sha_for(&release_json, asset_name),
        filename: asset_name.to_string(),
        extract_to,
    };

    let temp_download = bin_dir().join(format!(".{tool}.download"));
    let temp_extracted =
        (artifact.extract_to.is_some()).then(|| bin_dir().join(format!(".{tool}.extracted")));

    Ok(Release {
        tool: tool.to_string(),
        version,
        artifact,
        temp_download,
        temp_extracted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release_json(assets: &[(&str, &str, u64)]) -> serde_json::Value {
        let assets: Vec<serde_json::Value> = assets
            .iter()
            .map(|(name, digest, size)| {
                serde_json::json!({
                    "name": name,
                    "digest": format!("sha256:{digest}"),
                    "size": size,
                })
            })
            .collect();
        serde_json::json!({ "assets": assets })
    }

    #[test]
    fn env_override_takes_blank_as_unset() {
        std::env::set_var("SONUS_YTDLP_PATH", "   ");
        assert!(env_override("SONUS_YTDLP_PATH").is_none());
        std::env::remove_var("SONUS_YTDLP_PATH");
    }

    #[test]
    fn env_override_reads_value() {
        std::env::set_var("SONUS_YTDLP_PATH", "/opt/bin/yt-dlp");
        assert_eq!(
            env_override("SONUS_YTDLP_PATH").unwrap(),
            PathBuf::from("/opt/bin/yt-dlp")
        );
        std::env::remove_var("SONUS_YTDLP_PATH");
    }

    #[test]
    fn picks_ytdlp_asset_for_current_platform() {
        let json = release_json(&[
            ("yt-dlp_linux", "aa", 1),
            ("yt-dlp.exe", "bb", 1),
            ("yt-dlp_macos", "cc", 1),
        ]);
        let asset = ytdlp_asset(&json).unwrap();
        assert!(["yt-dlp_linux", "yt-dlp.exe", "yt-dlp_macos"].contains(&asset));
    }

    #[test]
    fn picks_ffmpeg_asset_for_current_platform() {
        // Cover every asset the resolver knows about so the test holds no
        // matter which runner architecture CI lands on.
        let json = release_json(&[
            ("ffmpeg-linux-x64.gz", "aa", 1),
            ("ffmpeg-linux-arm64.gz", "aa", 1),
            ("ffmpeg-win32-x64.gz", "bb", 1),
            ("ffmpeg-darwin-x64.gz", "cc", 1),
            ("ffmpeg-darwin-arm64.gz", "cc", 1),
        ]);
        let asset = ffmpeg_asset(&json).unwrap();
        assert!(
            asset.starts_with("ffmpeg-"),
            "asset {asset} must be an ffmpeg asset"
        );
    }

    #[test]
    fn digest_is_stripped_of_its_prefix() {
        let json = release_json(&[("yt-dlp_linux", "abc123", 42)]);
        assert_eq!(sha_for(&json, "yt-dlp_linux"), "abc123");
        assert_eq!(size_for(&json, "yt-dlp_linux"), 42);
    }

    #[test]
    fn missing_asset_is_an_error() {
        let json = release_json(&[("something-else", "aa", 1)]);
        assert!(ytdlp_asset(&json).is_err());
    }

    #[test]
    fn resolve_tool_prefers_env_override() {
        let exe = std::env::current_exe().unwrap();
        std::env::set_var("SONUS_FFMPEG_PATH", &exe);
        let resolved = resolve_ffmpeg();
        assert_eq!(resolved.path().map(|p| p.to_path_buf()), Some(exe));
        std::env::remove_var("SONUS_FFMPEG_PATH");
    }

    #[test]
    fn resolve_tool_missing_when_nothing_found() {
        // A name that cannot exist anywhere on PATH.
        let weird = "definitely-not-a-real-binary-zzz";
        std::env::set_var("PATH", "/nonexistent-dir");
        let exe = crate::resolve::find_on_path(weird);
        std::env::remove_var("PATH");
        assert!(exe.is_none());
    }
}
