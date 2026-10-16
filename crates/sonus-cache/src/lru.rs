use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("cache path error: {0}")]
    Path(String),
}

/// A size-limited LRU file cache for audio data.
///
/// Files are stored flat on disk, keyed by YouTube video ID. When the total
/// size exceeds `max_bytes`, the oldest files (by mtime) are evicted until
/// we're back under the limit.
///
/// Default max is 500MB — enough for ~50-100 cached songs without
/// eating too much disk. Configurable via `AudioCache::with_max_size`.
pub struct AudioCache {
    dir: PathBuf,
    max_bytes: u64,
}

impl AudioCache {
    pub fn new() -> Result<Self, CacheError> {
        Self::with_max_size(500 * 1024 * 1024) // 500 MB default
    }

    pub fn with_max_size(max_bytes: u64) -> Result<Self, CacheError> {
        let dir = Self::default_dir();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir, max_bytes })
    }

    /// Use a custom cache directory. Useful for testing.
    pub fn at_dir(dir: impl AsRef<Path>, max_bytes: u64) -> Result<Self, CacheError> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir, max_bytes })
    }

    /// Platform-appropriate cache directory for Sonus.
    pub fn default_dir() -> PathBuf {
        let mut path = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("sonus");
        path.push("audio");
        path
    }

    /// Returns the path where a given YouTube ID would be cached.
    /// The file may or may not exist.
    pub fn path_for(&self, youtube_id: &str) -> PathBuf {
        self.dir.join(format!("{youtube_id}.bin"))
    }

    pub fn has(&self, youtube_id: &str) -> bool {
        self.path_for(youtube_id).exists()
    }

    /// Store raw audio data for a YouTube ID. Triggers eviction if
    /// the cache would exceed the size limit.
    pub fn store(&self, youtube_id: &str, data: &[u8]) -> Result<(), CacheError> {
        let path = self.path_for(youtube_id);
        fs::write(&path, data)?;
        self.evict_if_needed()?;
        Ok(())
    }

    /// Load cached audio data for a YouTube ID, if it exists.
    /// Updates the mtime so the entry is treated as recently used.
    pub fn load(&self, youtube_id: &str) -> Result<Option<Vec<u8>>, CacheError> {
        let path = self.path_for(youtube_id);
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read(&path)?;
        // Touch the file so it's not the next eviction candidate
        let now = std::time::SystemTime::now();
        if let Ok(file) = std::fs::File::open(&path) {
            let _ = file.set_times(std::fs::FileTimes::new().set_modified(now));
        }
        Ok(Some(data))
    }

    /// Remove a specific entry.
    pub fn remove(&self, youtube_id: &str) -> Result<(), CacheError> {
        let path = self.path_for(youtube_id);
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Clear the entire cache.
    pub fn clear(&self) -> Result<(), CacheError> {
        if self.dir.exists() {
            for entry in fs::read_dir(&self.dir)? {
                let entry = entry?;
                if entry.path().is_file() {
                    fs::remove_file(entry.path())?;
                }
            }
        }
        Ok(())
    }

    /// Total bytes currently used by the cache.
    pub fn used_bytes(&self) -> Result<u64, CacheError> {
        let mut total = 0u64;
        if self.dir.exists() {
            for entry in fs::read_dir(&self.dir)? {
                let entry = entry?;
                if entry.path().is_file() {
                    total += entry.metadata()?.len();
                }
            }
        }
        Ok(total)
    }

    /// Walk the cache directory, sorted oldest-first, and delete files
    /// until we're under the max size.
    fn evict_if_needed(&self) -> Result<(), CacheError> {
        let mut entries: Vec<(PathBuf, std::time::SystemTime, u64)> = Vec::new();

        if self.dir.exists() {
            for entry in fs::read_dir(&self.dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    let meta = entry.metadata()?;
                    let mtime = meta.modified()?;
                    entries.push((path, mtime, meta.len()));
                }
            }
        }

        // Sort oldest first — these get evicted first
        entries.sort_by_key(|(_, mtime, _)| *mtime);

        let total: u64 = entries.iter().map(|(_, _, size)| *size).sum();
        let mut remaining = total;

        for (path, _, size) in &entries {
            if remaining <= self.max_bytes {
                break;
            }
            let _ = fs::remove_file(path);
            remaining -= size;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_load() {
        let tmp = std::env::temp_dir().join("sonus_cache_test");
        let _ = fs::remove_dir_all(&tmp);
        let cache = AudioCache::at_dir(&tmp, 1024 * 1024).unwrap();

        cache.store("abc123", b"audio data").unwrap();
        assert!(cache.has("abc123"));

        let loaded = cache.load("abc123").unwrap();
        assert_eq!(loaded, Some(b"audio data".to_vec()));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn evicts_oldest_when_over_limit() {
        let tmp = std::env::temp_dir().join("sonus_cache_evict_test");
        let _ = fs::remove_dir_all(&tmp);
        // 3 bytes max — can only hold one 2-byte file at a time
        let cache = AudioCache::at_dir(&tmp, 3).unwrap();

        cache.store("first", b"aa").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        cache.store("second", b"bb").unwrap();

        // First should have been evicted to make room
        assert!(!cache.has("first"));
        assert!(cache.has("second"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
