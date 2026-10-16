//! Downloading, verifying, and installing the tools.
//!
//! The happy path for a fresh machine: `install_if_missing` resolves what's
//! already on disk, and only when something's missing does it touch the
//! network. Each download streams to a temp file while reporting progress, is
//! verified against its SHA-256 digest, and only then atomically renamed into
//! place — a half-written binary never becomes the "installed" one.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::manifest::{load_manifest, save_manifest, InstalledEntry, Manifest, Version};
use crate::release::Release;
use crate::resolve::{release_for, resolve_ffmpeg, resolve_ytdlp, ResolvedBinary};
use crate::{bin_dir, ensure_executable, ffmpeg_exe_name, ytdlp_exe_name};

/// The tools Sonus can install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dependency {
    YtDlp,
    Ffmpeg,
}

impl Dependency {
    pub fn name(&self) -> &'static str {
        match self {
            Dependency::YtDlp => "yt-dlp",
            Dependency::Ffmpeg => "ffmpeg",
        }
    }

    /// The local file the installed binary lives in.
    fn exe_name(&self) -> &'static str {
        match self {
            Dependency::YtDlp => ytdlp_exe_name(),
            Dependency::Ffmpeg => ffmpeg_exe_name(),
        }
    }
}

/// Where the binary for a tool will be installed, before checking existence.
pub fn managed_path(dep: Dependency) -> PathBuf {
    bin_dir().join(dep.exe_name())
}

/// Result of a successful install.
#[derive(Debug)]
pub struct InstallOutcome {
    pub dependency: Dependency,
    pub version: Version,
    pub path: PathBuf,
    /// True when the binary lives in Sonus' own bin dir (not on PATH).
    pub managed: bool,
}

/// Errors during download/install.
#[derive(Debug, Error)]
pub enum InstallError {
    #[error("already installed at {0}")]
    AlreadyInstalled(PathBuf),

    #[error("{0}")]
    Resolution(#[from] crate::resolve::ResolutionError),

    #[error("network error downloading {tool}: {detail}")]
    Network { tool: &'static str, detail: String },

    #[error("digest mismatch for {tool}: expected {expected}, got {actual}")]
    DigestMismatch { tool: &'static str, expected: String, actual: String },

    #[error("io error installing {tool}: {detail}")]
    Io { tool: &'static str, detail: String },

    #[error("malformed download for {tool}: {detail}")]
    Malformed { tool: &'static str, detail: String },
}

/// True when a tool isn't usable anywhere yet — no network access needed.
pub fn needs_install(dep: Dependency) -> bool {
    matches!(resolve(dep), ResolvedBinary::Missing)
}

/// Resolve a tool the same way the player does (env → managed → PATH).
pub fn resolve(dep: Dependency) -> ResolvedBinary {
    match dep {
        Dependency::YtDlp => resolve_ytdlp(),
        Dependency::Ffmpeg => resolve_ffmpeg(),
    }
}

/// Install a tool unless one is already available.
pub fn install_if_missing(dep: Dependency) -> Result<InstallOutcome, InstallError> {
    match resolve(dep) {
        ResolvedBinary::Found(path) => Err(InstallError::AlreadyInstalled(path)),
        ResolvedBinary::Managed(path, version) => Ok(InstallOutcome {
            dependency: dep,
            version,
            path,
            managed: true,
        }),
        ResolvedBinary::Missing => install(dep, &mut |_| {}),
    }
}

/// Install a tool, reporting download progress through `on_progress`.
///
/// `on_progress` receives the fraction downloaded (0.0–1.0). Used by the GUI
/// to draw a progress bar instead of freezing the first launch.
pub fn install<F>(dep: Dependency, on_progress: &mut F) -> Result<InstallOutcome, InstallError>
where
    F: FnMut(f32),
{
    download_and_record(dep, on_progress)
}

/// Re-download a tool even if a version already exists.
pub fn update<F>(dep: Dependency, on_progress: &mut F) -> Result<InstallOutcome, InstallError>
where
    F: FnMut(f32),
{
    download_and_record(dep, on_progress)
}

/// Shared body of `install` and `update`: fetch, verify, place, record.
fn download_and_record<F>(dep: Dependency, on_progress: &mut F) -> Result<InstallOutcome, InstallError>
where
    F: FnMut(f32),
{
    let bin = bin_dir();
    std::fs::create_dir_all(&bin).map_err(|e| InstallError::Io { tool: dep.name(), detail: e.to_string() })?;

    let release = release_for(dep.name())?;
    let outcome = download_and_install(&bin, dep, &release, on_progress)?;

    // Record what we installed so staleness checks don't redownload blindly.
    let mut manifest: Manifest = load_manifest(&bin);
    manifest.tools.insert(dep.name().to_string(), InstalledEntry { version: outcome.version });
    save_manifest(&bin, &manifest);

    Ok(outcome)
}

/// Stream a release's artifact to disk, verify its digest, and move it into
/// place under `bin`.
fn download_and_install<F>(
    bin: &Path,
    dep: Dependency,
    release: &Release,
    on_progress: &mut F,
) -> Result<InstallOutcome, InstallError>
where
    F: FnMut(f32),
{
    let target = bin.join(dep.exe_name());

    // Download to a temp path first; an interrupted download must never
    // overwrite a working binary we already had.
    let tmp = release.temp_download.clone();
    download(release, &tmp, on_progress)?;

    // The digest describes the *downloaded* file — the gzip stream, for
    // ffmpeg — so verify before any decompression.
    verify_sha256(&tmp, &release.artifact.sha256, dep)?;

    if let Some(extract_to) = &release.artifact.extract_to {
        let out = bin.join(extract_to);
        gunzip(&tmp, &out).map_err(|e| InstallError::Io { tool: dep.name(), detail: e.to_string() })?;
        let _ = std::fs::remove_file(&tmp);
        std::fs::rename(&out, &target).map_err(|e| InstallError::Io { tool: dep.name(), detail: e.to_string() })?;
    } else {
        std::fs::rename(&tmp, &target).map_err(|e| InstallError::Io { tool: dep.name(), detail: e.to_string() })?;
    }

    ensure_executable(&target).map_err(|e| InstallError::Io { tool: dep.name(), detail: e.to_string() })?;

    Ok(InstallOutcome {
        dependency: dep,
        version: release.version,
        path: target,
        managed: true,
    })
}

/// Stream the artifact to `dest`, calling `on_progress` as bytes land.
fn download<F>(release: &Release, dest: &Path, on_progress: &mut F) -> Result<(), InstallError>
where
    F: FnMut(f32),
{
    let tool: &'static str = match release.tool.as_str() {
        "yt-dlp" => "yt-dlp",
        _ => "ffmpeg",
    };
    let response = ureq::get(&release.artifact.url)
        .set("User-Agent", "sonus-music")
        .call()
        .map_err(|e| InstallError::Network { tool, detail: e.to_string() })?;

    // Content-Length isn't always present on these CDN redirects, so fall
    // back to the size from the release metadata.
    let total = release.artifact.size.max(1);
    let mut file = std::fs::File::create(dest)
        .map_err(|e| InstallError::Io { tool, detail: e.to_string() })?;
    let mut reader = response.into_reader();
    let mut buf = [0u8; 64 * 1024];
    let mut written = 0u64;

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| InstallError::Io { tool, detail: e.to_string() })?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| InstallError::Io { tool, detail: e.to_string() })?;
        written += n as u64;
        on_progress((written as f32 / total as f32).min(1.0));
    }

    Ok(())
}

/// Verify a file matches its expected SHA-256.
fn verify_sha256(path: &Path, expected: &str, dep: Dependency) -> Result<(), InstallError> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)
        .map_err(|e| InstallError::Io { tool: dep.name(), detail: e.to_string() })?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| InstallError::Io { tool: dep.name(), detail: e.to_string() })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected.to_ascii_lowercase() {
        return Err(InstallError::DigestMismatch {
            tool: dep.name(),
            expected: expected.to_string(),
            actual,
        });
    }
    Ok(())
}

/// Decompress a `.gz` file into `dest`.
fn gunzip(src: &Path, dest: &Path) -> std::io::Result<()> {
    use flate2::read::GzDecoder;

    let input = std::fs::File::open(src)?;
    let mut decoder = GzDecoder::new(input);
    let mut out = std::fs::File::create(dest)?;
    std::io::copy(&mut decoder, &mut out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_names_are_stable() {
        assert_eq!(Dependency::YtDlp.name(), "yt-dlp");
        assert_eq!(Dependency::Ffmpeg.name(), "ffmpeg");
    }

    #[test]
    fn exe_names_are_platform_appropriate() {
        let name = Dependency::YtDlp.exe_name();
        if cfg!(windows) {
            assert!(name.ends_with(".exe"));
        } else {
            assert!(!name.ends_with(".exe"));
        }
    }

    #[test]
    fn managed_path_lives_under_bin_dir() {
        let p = managed_path(Dependency::YtDlp);
        assert!(p.starts_with(bin_dir()));
    }

    #[test]
    fn digest_mismatch_is_detected() {
        let dir = std::env::temp_dir().join("sonus_deps_digest_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("file");
        std::fs::write(&f, b"hello").unwrap();

        // Wrong digest must fail.
        let err = verify_sha256(&f, "0000000000000000000000000000000000000000000000000000000000000000", Dependency::YtDlp);
        assert!(matches!(err, Err(InstallError::DigestMismatch { .. })));

        // Right digest must pass.
        // sha256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        verify_sha256(&f, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824", Dependency::YtDlp).unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gunzip_round_trips() {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let dir = std::env::temp_dir().join("sonus_deps_gunzip_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = dir.join("in.gz");
        let dst = dir.join("out.bin");
        let raw = b"some binary payload\x00\x01\x02";
        let f = std::fs::File::create(&src).unwrap();
        let mut enc = GzEncoder::new(f, Compression::default());
        enc.write_all(raw).unwrap();
        enc.finish().unwrap();

        gunzip(&src, &dst).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), raw);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn needs_install_is_false_when_env_override_points_somewhere() {
        // Point the override at our own test executable — it exists, so the
        // resolver should consider the tool installed without any network.
        let exe = std::env::current_exe().unwrap();
        std::env::set_var("SONUS_YTDLP_PATH", &exe);
        assert!(!needs_install(Dependency::YtDlp));
        std::env::remove_var("SONUS_YTDLP_PATH");
    }
}
