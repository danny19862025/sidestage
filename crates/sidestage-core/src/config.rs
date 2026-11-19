//! Favourites, sections and settings, persisted as one JSON document.
//!
//! The same file format is read by the GNOME Shell extension, so field names
//! are part of a public contract: add fields with `#[serde(default)]`, never
//! rename them.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const CONFIG_VERSION: u32 = 1;

fn new_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{:x}{:x}", nanos, n)
}

/// How a favourite is started.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LaunchTarget {
    /// A freedesktop `.desktop` file (Linux).
    DesktopEntry { path: PathBuf },
    /// Anything the shell can open: `.lnk`, `.exe`, `.url`, a document (Windows),
    /// or an executable / AppImage (Linux).
    Path {
        path: PathBuf,
        #[serde(default)]
        args: Vec<String>,
    },
    /// A Windows application user model id from `shell:AppsFolder`
    /// (covers Store/UWP apps and Start-menu shortcuts).
    AppsFolder { parsing_name: String },
    /// A raw command line, split into program + arguments.
    Command {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
    /// An http(s) address opened in the default browser.
    Url { url: String },
}

impl LaunchTarget {
    /// A stable string used to detect duplicate favourites.
    pub fn key(&self) -> String {
        match self {
            LaunchTarget::DesktopEntry { path } => format!("desktop:{}", path.display()),
            LaunchTarget::Path { path, args } => {
                format!("path:{}|{}", path.display(), args_key(args))
            }
            LaunchTarget::AppsFolder { parsing_name } => {
                format!("apps:{}", parsing_name.to_lowercase())
            }
            LaunchTarget::Command { program, args } => format!("cmd:{program}|{}", args_key(args)),
            LaunchTarget::Url { url } => format!("url:{}", url_key(url)),
        }
    }

    /// True for targets that open a website.
    pub fn is_url(&self) -> bool {
        matches!(self, LaunchTarget::Url { .. })
    }
}

/// Arguments encoded so `["a b"]` and `["a", "b"]` stay distinct.
fn args_key(args: &[String]) -> String {
    serde_json::to_string(args).unwrap_or_default()
}

/// Scheme and host are case-insensitive; path, query and fragment are not.
fn url_key(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
            format!(
                "{}://{}{}",
                scheme.to_ascii_lowercase(),
                rest[..end].to_ascii_lowercase(),
                &rest[end..]
            )
        }
        None => url.to_string(),
    }
}

/// Validates and normalises a website address typed or pasted by the user.
/// Adds `https://` when no scheme is given; only http and https are allowed.
pub fn normalize_web_url(input: &str) -> Result<String, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("Enter a web address".into());
    }
    if s.chars().any(char::is_whitespace) {
        return Err("A web address can't contain spaces".into());
    }
    let lower = s.to_ascii_lowercase();
    let with_scheme = if lower.starts_with("http://") || lower.starts_with("https://") {
        s.to_string()
    } else if lower.contains("://")
        || lower.starts_with("javascript:")
        || lower.starts_with("file:")
        || lower.starts_with("data:")
    {
        return Err("Only http:// and https:// addresses are supported".into());
    } else {
        format!("https://{s}")
    };
    let rest = with_scheme
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or_default();
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit('@').next().unwrap_or_default();
    let hostname = host.split(':').next().unwrap_or_default();
    if hostname.is_empty()
        || !(hostname.contains('.') || hostname == "localhost" || hostname.starts_with('['))
    {
        return Err("That doesn't look like a web address".into());
    }
    Ok(with_scheme)
}

/// A readable default name for a website: its host without `www.`.
pub fn url_display_name(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    host.trim_start_matches("www.").to_string()
}

/// Where a favourite's icon comes from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IconSource {
    /// Ask the platform for the target's own icon.
    #[default]
    Auto,
    /// An image file chosen by the user (PNG, SVG, ICO, or an EXE/DLL on Windows).
    File { path: PathBuf },
    /// A freedesktop icon-theme name (Linux).
    Themed { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Favourite {
    pub id: String,
    pub name: String,
    pub target: LaunchTarget,
    #[serde(default)]
    pub icon: IconSource,
    /// Started by "Launch section". Clicking always opens it.
    #[serde(default = "yes")]
    pub include_in_launch: bool,
}

impl Favourite {
    pub fn new(name: impl Into<String>, target: LaunchTarget, icon: IconSource) -> Self {
        Self {
            id: new_id("fav"),
            name: name.into(),
            target,
            icon,
            include_in_launch: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub id: String,
    pub name: String,
    #[serde(default = "yes")]
    pub expanded: bool,
    #[serde(default)]
    pub items: Vec<Favourite>,
}

fn yes() -> bool {
    true
}

impl Section {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: new_id("sec"),
            name: name.into(),
            expanded: true,
            items: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    #[default]
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// Which monitor hosts the sidebar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MonitorChoice {
    #[default]
    Primary,
    /// Matched by connector/device name first, then by index.
    Named { name: String, index: usize },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub edge: Edge,
    pub monitor: MonitorChoice,
    /// Sidebar width in logical pixels.
    pub width: f32,
    /// Height of a minimized-window preview card in logical pixels.
    pub preview_height: f32,
    /// Size of favourite icons in logical pixels.
    pub icon_size: f32,
    pub auto_hide: bool,
    /// Reserve screen space so maximized windows do not go under the sidebar.
    /// Ignored while auto-hide is on.
    pub reserve_space: bool,
    pub theme: Theme,
    pub launch_at_login: bool,
    pub show_labels: bool,
    pub animations: bool,
    /// Milliseconds the pointer must rest at the edge before an auto-hidden
    /// sidebar slides in.
    pub reveal_delay_ms: u32,
    /// Global shortcut that opens search (empty = off), e.g. `Ctrl+Alt+Space`.
    pub search_shortcut: String,
    /// Hide the sidebar (and its edge reveal and reserved space) while a
    /// fullscreen app covers its monitor.
    pub pause_in_fullscreen: bool,
    /// Favourite grid columns; 0 = as many as fit.
    pub columns: u8,
    /// Opacity of the sidebar panel (0.2 = glassy … 1.0 = solid).
    pub opacity: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            edge: Edge::Left,
            monitor: MonitorChoice::Primary,
            width: 236.0,
            preview_height: 118.0,
            icon_size: 40.0,
            auto_hide: false,
            reserve_space: true,
            theme: Theme::System,
            launch_at_login: false,
            show_labels: true,
            animations: true,
            reveal_delay_ms: 150,
            search_shortcut: "Ctrl+Alt+Space".into(),
            pause_in_fullscreen: true,
            columns: 0,
            opacity: 0.72,
        }
    }
}

impl Settings {
    pub const WIDTH_RANGE: std::ops::RangeInclusive<f32> = 180.0..=420.0;
    pub const PREVIEW_RANGE: std::ops::RangeInclusive<f32> = 72.0..=240.0;
    pub const ICON_RANGE: std::ops::RangeInclusive<f32> = 28.0..=64.0;
    pub const OPACITY_RANGE: std::ops::RangeInclusive<f32> = 0.2..=1.0;
    pub const MAX_COLUMNS: u8 = 6;

    /// Clamp values that may come from a hand-edited file.
    pub fn sanitize(&mut self) {
        let clamp = |v: &mut f32, r: &std::ops::RangeInclusive<f32>, d: f32| {
            *v = if v.is_finite() {
                v.clamp(*r.start(), *r.end())
            } else {
                d
            };
        };
        let d = Settings::default();
        clamp(&mut self.width, &Self::WIDTH_RANGE, d.width);
        clamp(
            &mut self.preview_height,
            &Self::PREVIEW_RANGE,
            d.preview_height,
        );
        clamp(&mut self.icon_size, &Self::ICON_RANGE, d.icon_size);
        self.reveal_delay_ms = self.reveal_delay_ms.min(2000);
        self.columns = self.columns.min(Self::MAX_COLUMNS);
        clamp(&mut self.opacity, &Self::OPACITY_RANGE, d.opacity);
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub sections: Vec<Section>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            settings: Settings::default(),
            sections: ["Games", "Coding", "Work", "Everyday"]
                .into_iter()
                .map(Section::new)
                .collect(),
        }
    }
}

/// Location of a favourite: (section index, item index).
pub type Slot = (usize, usize);

impl Config {
    /// Platform config path, e.g. `~/.config/sidestage/config.json` or
    /// `%APPDATA%\Sidestage\config.json`.
    pub fn default_path() -> PathBuf {
        if let Some(p) = std::env::var_os("SIDESTAGE_CONFIG") {
            return PathBuf::from(p);
        }
        config_dir().join("config.json")
    }

    /// Loads the file; a missing file yields defaults, a corrupt file is
    /// moved aside (never silently overwritten) and defaults are returned.
    pub fn load(path: &Path) -> Config {
        match fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Config>(&text) {
                Ok(mut c) => {
                    c.settings.sanitize();
                    c.version = CONFIG_VERSION;
                    c
                }
                Err(err) => {
                    let backup = path.with_extension(format!("broken-{}.json", unix_now()));
                    log::error!(
                        "config {} is invalid ({err}); saved a copy as {}",
                        path.display(),
                        backup.display()
                    );
                    let _ = fs::copy(path, &backup);
                    Config::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(err) => {
                log::error!("cannot read {}: {err}", path.display());
                Config::default()
            }
        }
    }

    /// Atomic save: write a sibling temp file then rename over the original.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let tmp = path.with_extension("json.tmp");
        {
            let mut f =
                fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
            f.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
        Ok(())
    }

    pub fn find(&self, favourite_id: &str) -> Option<Slot> {
        self.sections.iter().enumerate().find_map(|(si, s)| {
            s.items
                .iter()
                .position(|f| f.id == favourite_id)
                .map(|ii| (si, ii))
        })
    }

    pub fn favourite(&self, id: &str) -> Option<&Favourite> {
        self.find(id).map(|(s, i)| &self.sections[s].items[i])
    }

    pub fn favourite_mut(&mut self, id: &str) -> Option<&mut Favourite> {
        self.find(id).map(|(s, i)| &mut self.sections[s].items[i])
    }

    pub fn section_index(&self, section_id: &str) -> Option<usize> {
        self.sections.iter().position(|s| s.id == section_id)
    }

    /// Favourites "Launch section" starts, in display order.
    pub fn section_launch_list(&self, section_id: &str) -> Vec<Favourite> {
        self.section_index(section_id)
            .map(|i| {
                self.sections[i]
                    .items
                    .iter()
                    .filter(|f| f.include_in_launch)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn contains_target(&self, section_id: &str, target: &LaunchTarget) -> bool {
        let key = target.key();
        self.section_index(section_id)
            .map(|i| self.sections[i].items.iter().any(|f| f.target.key() == key))
            .unwrap_or(false)
    }

    /// Adds a favourite to a section (appending), skipping exact duplicates
    /// within that section. Returns the new favourite's id.
    pub fn add_favourite(&mut self, section_id: &str, fav: Favourite) -> Option<String> {
        let si = self.section_index(section_id)?;
        if self.contains_target(section_id, &fav.target) {
            return None;
        }
        let id = fav.id.clone();
        self.sections[si].items.push(fav);
        self.sections[si].expanded = true;
        Some(id)
    }

    /// Moves a favourite to `section_id` at `index` (clamped). Works within
    /// and across sections.
    pub fn move_favourite(&mut self, favourite_id: &str, section_id: &str, index: usize) -> bool {
        let Some((from_s, from_i)) = self.find(favourite_id) else {
            return false;
        };
        let Some(to_s) = self.section_index(section_id) else {
            return false;
        };
        let fav = self.sections[from_s].items.remove(from_i);
        let mut index = index;
        if from_s == to_s && from_i < index {
            index -= 1;
        }
        let items = &mut self.sections[to_s].items;
        items.insert(index.min(items.len()), fav);
        true
    }

    pub fn remove_favourite(&mut self, favourite_id: &str) -> Option<Favourite> {
        let (s, i) = self.find(favourite_id)?;
        Some(self.sections[s].items.remove(i))
    }

    pub fn add_section(&mut self, name: &str) -> String {
        let s = Section::new(unique_name(&self.sections, name.trim()));
        let id = s.id.clone();
        self.sections.push(s);
        id
    }

    pub fn rename_section(&mut self, section_id: &str, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        match self.section_index(section_id) {
            Some(i) => {
                self.sections[i].name = name.to_string();
                true
            }
            None => false,
        }
    }

    /// Moves a section by `delta` positions (negative = up).
    pub fn shift_section(&mut self, section_id: &str, delta: isize) -> bool {
        let Some(i) = self.section_index(section_id) else {
            return false;
        };
        let j = i as isize + delta;
        if j < 0 || j as usize >= self.sections.len() {
            return false;
        }
        let s = self.sections.remove(i);
        self.sections.insert(j as usize, s);
        true
    }

    pub fn move_section(&mut self, section_id: &str, to: usize) -> bool {
        let Some(i) = self.section_index(section_id) else {
            return false;
        };
        let s = self.sections.remove(i);
        let to = if i < to { to - 1 } else { to };
        self.sections.insert(to.min(self.sections.len()), s);
        true
    }

    /// Deletes a section. Its favourites move to `move_items_to` when given,
    /// otherwise they are deleted with it.
    pub fn delete_section(&mut self, section_id: &str, move_items_to: Option<&str>) -> bool {
        let Some(i) = self.section_index(section_id) else {
            return false;
        };
        let removed = self.sections.remove(i);
        if let Some(target) = move_items_to.and_then(|t| self.section_index(t)) {
            for fav in removed.items {
                if !self.sections[target]
                    .items
                    .iter()
                    .any(|f| f.target.key() == fav.target.key())
                {
                    self.sections[target].items.push(fav);
                }
            }
        }
        true
    }
}

fn unique_name(sections: &[Section], wanted: &str) -> String {
    let base = if wanted.is_empty() {
        "New section"
    } else {
        wanted
    };
    let taken = |n: &str| sections.iter().any(|s| s.name.eq_ignore_ascii_case(n));
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|n| !taken(n))
        .unwrap()
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Per-user configuration directory for Sidestage.
pub fn config_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        base.join(crate::APP_NAME)
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join(crate::APP_ID)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fav(name: &str) -> Favourite {
        Favourite::new(
            name,
            LaunchTarget::Command {
                program: name.into(),
                args: vec![],
            },
            IconSource::Auto,
        )
    }

    #[test]
    fn distinct_targets_are_not_duplicates() {
        let url = |u: &str| LaunchTarget::Url { url: u.into() };
        assert_ne!(
            url("https://example.com/Report").key(),
            url("https://example.com/report").key()
        );
        assert_eq!(
            url("HTTPS://Example.COM/Report").key(),
            url("https://example.com/Report").key()
        );
        assert_ne!(
            url("https://x.org/?q=A").key(),
            url("https://x.org/?q=a").key()
        );
        let cmd = |a: &[&str]| LaunchTarget::Command {
            program: "p".into(),
            args: a.iter().map(|s| s.to_string()).collect(),
        };
        assert_ne!(cmd(&["a b"]).key(), cmd(&["a", "b"]).key());
        let path = |a: &[&str]| LaunchTarget::Path {
            path: "/x".into(),
            args: a.iter().map(|s| s.to_string()).collect(),
        };
        assert_ne!(path(&["a b"]).key(), path(&["a", "b"]).key());
        // Both can live in one section.
        let mut c = Config::default();
        let sec = c.sections[0].id.clone();
        assert!(c
            .add_favourite(&sec, Favourite::new("1", cmd(&["a b"]), IconSource::Auto))
            .is_some());
        assert!(c
            .add_favourite(
                &sec,
                Favourite::new("2", cmd(&["a", "b"]), IconSource::Auto)
            )
            .is_some());
        assert!(c
            .add_favourite(
                &sec,
                Favourite::new("3", url("https://e.com/Report"), IconSource::Auto)
            )
            .is_some());
        assert!(c
            .add_favourite(
                &sec,
                Favourite::new("4", url("https://e.com/report"), IconSource::Auto)
            )
            .is_some());
    }

    #[test]
    fn defaults_have_four_sections() {
        let c = Config::default();
        let names: Vec<_> = c.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Games", "Coding", "Work", "Everyday"]);
    }

    #[test]
    fn add_move_remove_favourites() {
        let mut c = Config::default();
        let games = c.sections[0].id.clone();
        let coding = c.sections[1].id.clone();
        let a = c.add_favourite(&games, fav("a")).unwrap();
        let b = c.add_favourite(&games, fav("b")).unwrap();
        assert!(
            c.add_favourite(&games, fav("a")).is_none(),
            "duplicate rejected"
        );
        // reorder within section: move a after b
        assert!(c.move_favourite(&a, &games, 2));
        assert_eq!(
            c.sections[0]
                .items
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            ["b", "a"]
        );
        // move across sections
        assert!(c.move_favourite(&b, &coding, 0));
        assert_eq!(c.find(&b), Some((1, 0)));
        assert_eq!(c.remove_favourite(&a).unwrap().name, "a");
        assert!(c.find(&a).is_none());
    }

    #[test]
    fn sections_crud() {
        let mut c = Config::default();
        let id = c.add_section("Games");
        assert_eq!(c.sections.last().unwrap().name, "Games 2");
        assert!(c.rename_section(&id, "  Music "));
        assert_eq!(c.sections.last().unwrap().name, "Music");
        assert!(!c.rename_section(&id, "   "));
        assert!(c.shift_section(&id, -1));
        assert_eq!(c.sections[3].name, "Music");
        assert!(!c.shift_section(&c.sections[0].id.clone(), -1));
        let first = c.sections[0].id.clone();
        c.add_favourite(&id, fav("x"));
        assert!(c.delete_section(&id, Some(&first)));
        assert_eq!(c.sections[0].items[0].name, "x");
        assert!(c.move_section(&first, 4));
        assert_eq!(c.sections.last().unwrap().id, first);
    }

    #[test]
    fn save_load_roundtrip_and_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/config.json");
        let mut c = Config::default();
        c.settings.width = 9999.0;
        c.settings.sanitize();
        assert_eq!(c.settings.width, 420.0);
        let s = c.sections[2].id.clone();
        c.add_favourite(&s, fav("editor"));
        c.save(&path).unwrap();
        assert_eq!(Config::load(&path), c);

        fs::write(&path, "{not json").unwrap();
        let loaded = Config::load(&path);
        assert_eq!(loaded.sections.len(), 4);
        let backups = fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(backups, 2, "corrupt file kept as a backup");
    }

    #[test]
    fn unknown_and_missing_fields_tolerated() {
        let json = r#"{"settings":{"width":300,"future_field":1},"sections":[{"id":"s","name":"A","items":[
            {"id":"f","name":"Term","target":{"kind":"command","program":"foot"}}]}]}"#;
        let c: Config = serde_json::from_str(json).unwrap();
        assert_eq!(c.settings.width, 300.0);
        assert!(c.settings.show_labels);
        assert!(c.sections[0].expanded);
        assert_eq!(c.sections[0].items[0].icon, IconSource::Auto);
    }
}
