pub mod capture;
pub mod cli;
pub mod config;
pub mod control;
pub mod device;
pub mod gpu;
pub mod kms;
pub mod runtime;
pub mod sampling;

pub use config::Config;

#[derive(Debug)]
pub struct AppError {
    pub message: String,
    pub temporary: bool,
}

impl AppError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            temporary: false,
        }
    }

    pub fn temporary(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            temporary: true,
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

pub type Result<T, E = AppError> = std::result::Result<T, E>;
