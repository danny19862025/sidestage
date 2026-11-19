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

/// A favourite for a folder, opened in the default file manager.
pub fn folder_entry(path: &Path) -> AppEntry {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| path.display().to_string());
    AppEntry {
        name,
        subtitle: Some(path.display().to_string()),
        target: LaunchTarget::Path {
            path: path.to_path_buf(),
            args: vec![],
        },
        icon: IconSource::Auto,
        keywords: vec![],
    }
}

/// A website favourite from a typed or pasted address.
pub fn website_entry(name: &str, url: &str) -> Result<AppEntry, String> {
    let url = sidestage_core::config::normalize_web_url(url)?;
    let name = if name.trim().is_empty() {
        sidestage_core::config::url_display_name(&url)
    } else {
        name.trim().to_string()
    };
    Ok(AppEntry {
        name,
        subtitle: Some(url.clone()),
        target: LaunchTarget::Url { url },
        icon: IconSource::Auto,
        keywords: vec![],
    })
}

/// Reads `URL=` from a Windows Internet Shortcut (`.url`) file.
pub fn internet_shortcut_url(text: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line.eq_ignore_ascii_case("[InternetShortcut]");
        } else if in_section {
            if let Some(v) = line.strip_prefix("URL=") {
                return sidestage_core::config::normalize_web_url(v).ok();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internet_shortcut_parsing() {
        let t = "[{000214A0-0000-0000-C000-000000000046}]\r\nProp3=19,11\r\n[InternetShortcut]\r\nIDList=\r\nURL=https://github.com/danny19862025/sidestage\r\n";
        assert_eq!(
            internet_shortcut_url(t).as_deref(),
            Some("https://github.com/danny19862025/sidestage")
        );
        assert_eq!(
            internet_shortcut_url("[InternetShortcut]\nURL=file:///C:/x"),
            None
        );
    }

    #[test]
    fn website_entries() {
        let e = website_entry("", "www.rust-lang.org/learn").unwrap();
        assert_eq!(e.name, "rust-lang.org");
        assert_eq!(
            e.target,
            LaunchTarget::Url {
                url: "https://www.rust-lang.org/learn".into()
            }
        );
        assert!(website_entry("x", "ftp://a.b").is_err());
    }

    #[test]
    fn folder_entries_use_the_folder_name() {
        let dir = std::env::temp_dir();
        let e = folder_entry(&dir);
        assert_eq!(
            e.target,
            LaunchTarget::Path {
                path: dir.clone(),
                args: vec![]
            }
        );
        assert!(!e.name.is_empty());
    }
}
