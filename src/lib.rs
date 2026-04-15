pub mod autostart;
pub mod config;
pub mod daemon;
pub mod device;
pub mod features;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
