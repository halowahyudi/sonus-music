use thiserror::Error;

pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("track not found: {0}")]
    TrackNotFound(String),

    #[error("playlist not found: {0}")]
    PlaylistNotFound(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("yt-dlp error: {0}")]
    YtDlp(String),

    #[error("playback error: {0}")]
    Playback(String),

    #[error("cache error: {0}")]
    Cache(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}
