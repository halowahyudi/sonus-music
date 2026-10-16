//! Where the external tools Sonus shells out to come from.
//!
//! Sonus needs yt-dlp and ffmpeg, but neither should be assumed present on a
//! fresh machine. This crate downloads them on demand into a per-user bin
//! directory (`.local/share/sonus/bin` or `%LOCALAPPDATA%\sonus\bin`), verifies
//! each download with its SHA-256 digest, and records what it installed in a
//! small JSON manifest so updates can be checked later.

use std::path::{Path, PathBuf};

pub use install::{
    install, install_if_missing, managed_path, needs_install, update, Dependency, InstallError,
    InstallOutcome,
};
pub use manifest::{load_manifest, save_manifest, Manifest, Version};
pub use release::{Artifact, Release};
pub use resolve::{
    latest_version, release_for, resolve_ffmpeg, resolve_ytdlp, ResolutionError, ResolvedBinary,
};
pub use stale::{check_stale, update_available, StaleStatus};

mod install;
mod manifest;
mod release;
mod resolve;
mod stale;

/// Where Sonus keeps the binaries it downloads.
///
/// Uses the platform data directory so the files survive across runs and can
/// be updated in place — the GUI and CLI resolve to the same location.
pub fn bin_dir() -> PathBuf {
    let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("sonus");
    path.push("bin");
    path
}

/// The `yt-dlp` binary we install is a self-contained Python zipapp; on Unix
/// it needs its exec bit set, on Windows it carries the `.exe` suffix.
pub(crate) fn ytdlp_exe_name() -> &'static str {
    if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    }
}

/// The `ffmpeg` binary name for the current platform.
pub(crate) fn ffmpeg_exe_name() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

/// Resolve a binary against PATH, honouring Windows' PATHEXT extension list.
pub(crate) fn find_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.BAT;.CMD".into())
            .split(';')
            .map(|s| s.to_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };

    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        // On Windows the name is usually spelled without its extension.
        for ext in &extensions {
            let with_ext = dir.join(format!("{name}{ext}"));
            if with_ext.is_file() {
                return Some(with_ext);
            }
        }
    }
    None
}

/// Make sure an installed binary is executable on Unix.
pub(crate) fn ensure_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(perms.mode() | 0o755);
        std::fs::set_permissions(path, perms)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
