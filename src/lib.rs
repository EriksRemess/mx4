//! Library interface for MX Master 4 discovery, status, settings, persistence, and reconnect logic.
//!
//! Use the feature modules' `read_status` functions for structured values. Their `status` and
//! `print_best_effort` helpers write to standard output for CLI use. Feature setters change the
//! device only; persist settings separately through [`config`].
//!
//! Device requests require HID access and a writable configuration directory for the shared
//! transport lock, even when only reading status. See [`device::req`] for synchronization details.
//!
//! ```no_run
//! let battery = mx4::features::battery::read_status()?;
//! println!("Battery: {}%", battery.pct);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod config;
pub mod daemon;
pub mod device;
pub mod features;
pub mod service;
mod transport_lock;

/// A result carrying device, I/O, parsing, or validation errors.
///
/// Error messages are intended for display; their wording is not a stable machine-readable API.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
