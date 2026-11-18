//! Platform-independent logic shared by every Sidestage build.
//!
//! Nothing in this crate talks to a window manager. Platform crates feed
//! [`tracker::WindowTracker`] with [`tracker::WindowInfo`] values and the UI
//! reads favourites and settings through [`config::Config`].

pub mod config;
pub mod desktop_entry;
pub mod image;
pub mod search;
pub mod tracker;

pub use config::{Config, Favourite, IconSource, LaunchTarget, Section, Settings};
pub use image::RgbaImage;
pub use tracker::{BackendStatus, WindowEvent, WindowGroup, WindowId, WindowInfo, WindowTracker};

/// Short product name used for config directories, D-Bus names and UI.
pub const APP_NAME: &str = "Sidestage";
/// Lower-case identifier used for paths and service names.
pub const APP_ID: &str = "sidestage";
