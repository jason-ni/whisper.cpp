use thiserror::Error;
use crate::rb::RbError;

#[derive(Error, Debug)]
pub enum WhisperError {
    #[error("An error occurred: {0}")]
    AnyhowError(#[from] anyhow::Error),
    #[error("IoError: {0}")]
    IoError(#[from] std::io::Error),
    #[error("RbError: {0}")]
    RbError(#[from] RbError),
    #[error("FFmpegError: {0}")]
    FFmpegError(#[from] ffmpeg_next::Error),
}