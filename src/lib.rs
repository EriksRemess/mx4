//! Library interface for MX Master 4 discovery, status, settings, persistence, and reconnect logic.

pub mod config;
pub mod daemon;
pub mod device;
pub mod features;
pub mod service;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
