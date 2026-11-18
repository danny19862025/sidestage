//! Installed applications from freedesktop `.desktop` files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use sidestage_core::desktop_entry::DesktopEntry;
use sidestage_core::{IconSource, LaunchTarget};

use crate::apps::AppEntry;

fn locale() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
}

/// All desktop entries, keyed by desktop-file id, first match wins (user dirs first).
fn scan() -> Vec<DesktopEntry> {
    let loc = locale();
    let mut by_id: HashMap<String, DesktopEntry> = HashMap::new();
    let mut order = Vec::new();
    for base in super::icons::data_dirs() {
        let root = base.join("applications");
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                if p.extension().map(|x| x != "desktop").unwrap_or(true) {
                    continue;
                }
                // Desktop-file ids replace '/' with '-' for subdirectories.
                let rel = p.strip_prefix(&root).unwrap_or(&p).to_string_lossy().replace('/', "-");
                let id = rel.trim_end_matches(".desktop").to_string();
                if by_id.contains_key(&id) {
                    continue;
                }
                if let Some(mut entry) = DesktopEntry::load(&p, loc.as_deref()) {
                    entry.id = id.clone();
                    order.push(id.clone());
                    by_id.insert(id, entry);
                }
            }
        }
    }
    order.into_iter().filter_map(|id| by_id.remove(&id)).collect()
}

fn cache() -> &'static Mutex<Option<Arc<Vec<DesktopEntry>>>> {
    static C: OnceLock<Mutex<Option<Arc<Vec<DesktopEntry>>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

pub fn entries() -> Arc<Vec<DesktopEntry>> {
    if let Some(e) = cache().lock().unwrap().as_ref() {
        return e.clone();
    }
    let e = Arc::new(scan());
    *cache().lock().unwrap() = Some(e.clone());
    e
}

pub fn refresh() {
    *cache().lock().unwrap() = None;
}

fn to_app(e: &DesktopEntry) -> AppEntry {
    let mut keywords = e.keywords.clone();
    keywords.extend(e.generic_name.clone());
    keywords.push(e.id.clone());
    AppEntry {
        name: e.name.clone(),
        subtitle: e.generic_name.clone().or_else(|| e.comment.clone()),
        target: LaunchTarget::DesktopEntry { path: e.path.clone() },
        icon: e.icon.clone().map(|name| IconSource::Themed { name }).unwrap_or_default(),
        keywords,
    }
}

pub fn installed_apps() -> Vec<AppEntry> {
    refresh();
    let desktops = super::desktops();
    let mut apps: Vec<AppEntry> = entries().iter().filter(|e| e.is_listable(&desktops)).map(to_app).collect();
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

pub fn from_path(path: &Path) -> Option<AppEntry> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext == "desktop" {
        let e = DesktopEntry::load(path, locale().as_deref())?;
        if e.kind == "Link" {
            // URL shortcut: open it with xdg-open.
            return Some(AppEntry {
                name: e.name.clone(),
                subtitle: None,
                target: LaunchTarget::Path { path: path.to_path_buf(), args: vec![] },
                icon: e.icon.map(|name| IconSource::Themed { name }).unwrap_or_default(),
                keywords: vec![],
            });
        }
        return Some(to_app(&e));
    }
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    Some(AppEntry {
        name,
        subtitle: Some(path.display().to_string()),
        target: LaunchTarget::Path { path: path.to_path_buf(), args: vec![] },
        icon: IconSource::Auto,
        keywords: vec![],
    })
}

/// Finds the desktop entry for a running window's app id / WM_CLASS.
pub fn find_for_app_id(app_id: &str) -> Option<DesktopEntry> {
    if app_id.is_empty() {
        return None;
    }
    let all = entries();
    let lower = app_id.to_lowercase();
    let stripped = lower.trim_end_matches(".desktop");
    let by = |pred: &dyn Fn(&DesktopEntry) -> bool| all.iter().find(|e| pred(e)).cloned();
    by(&|e| e.id.to_lowercase() == stripped)
        .or_else(|| by(&|e| e.startup_wm_class.as_deref().map(|c| c.to_lowercase() == stripped).unwrap_or(false)))
        .or_else(|| by(&|e| e.id.to_lowercase().rsplit('.').next() == Some(stripped)))
        .or_else(|| {
            by(&|e| {
                e.command_line()
                    .and_then(|argv| argv.first().cloned())
                    .map(|p| Path::new(&p).file_name().map(|f| f.to_string_lossy().to_lowercase()) == Some(stripped.to_string()))
                    .unwrap_or(false)
            })
        })
}

/// Desktop entry path for a running process id, using `/proc/<pid>/exe`.
pub fn find_for_pid(pid: u32) -> Option<DesktopEntry> {
    let exe: PathBuf = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let name = exe.file_name()?.to_string_lossy().to_lowercase();
    find_for_app_id(&name)
}
