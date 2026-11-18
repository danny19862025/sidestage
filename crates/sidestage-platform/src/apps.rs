//! Installed applications and dropped-file conversion.

use std::path::Path;

use sidestage_core::{Favourite, IconSource, LaunchTarget};

use crate::imp;

/// An application the user can add as a favourite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppEntry {
    pub name: String,
    pub subtitle: Option<String>,
    pub target: LaunchTarget,
    pub icon: IconSource,
    pub keywords: Vec<String>,
}

impl AppEntry {
    pub fn into_favourite(self) -> Favourite {
        // Keep `Auto` for Windows so the shell icon follows app updates.
        Favourite::new(self.name, self.target, self.icon)
    }
}

/// Start-menu / AppsFolder entries on Windows, `.desktop` entries on Linux.
/// Slow on first call (filesystem/COM scan); call off the UI thread.
pub fn installed_apps() -> Vec<AppEntry> {
    imp::apps::installed_apps()
}

/// Turns a file dropped onto the sidebar (`.lnk`, `.exe`, `.url`,
/// `.desktop`, executable, document) into an app entry.
pub fn from_path(path: &Path) -> Option<AppEntry> {
    imp::apps::from_path(path)
}
