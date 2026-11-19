//! Tracks windows reported by a platform backend and derives the
//! "minimized" area of the sidebar: only minimized windows, grouped per app,
//! most recently minimized first.

use std::collections::HashMap;

use crate::image::RgbaImage;

/// Opaque, backend-specific window identifier (HWND, X11 id, KWin UUID, ...).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(pub String);

impl std::fmt::Display for WindowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the platform knows about a top-level window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowInfo {
    pub id: WindowId,
    /// Grouping key: executable path, Wayland app id, WM_CLASS, ...
    pub app_key: String,
    /// Human-readable application name for the stack header.
    pub app_name: String,
    pub title: String,
    pub minimized: bool,
    /// Hints the platform icon resolver can use (theme name, desktop id, exe).
    pub icon_hint: Option<String>,
    /// When the backend knows it: a monotonic timestamp of the moment the
    /// window was minimized (orders stacks correctly even for windows that
    /// were minimized before Sidestage started).
    pub minimized_at: Option<u64>,
}

/// Messages from a platform backend to the UI thread.
#[derive(Debug)]
pub enum WindowEvent {
    /// Replace the whole known window set (initial enumeration / resync).
    Snapshot(Vec<WindowInfo>),
    /// A window appeared or changed.
    Upsert(WindowInfo),
    /// A window was closed.
    Removed(WindowId),
    /// An application icon, keyed by `WindowInfo::app_key`.
    Icon { app_key: String, image: RgbaImage },
    /// A still preview of a window, or `None` to drop a stale one.
    Thumbnail {
        id: WindowId,
        image: Option<RgbaImage>,
    },
    /// Backend availability / diagnostics, shown in the sidebar footer.
    Status(BackendStatus),
    /// Screen areas currently covered by a fullscreen app (empty = none).
    Fullscreen(Vec<FullscreenArea>),
}

/// Where a fullscreen app is, as the backend knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FullscreenArea {
    /// Output/connector name, when known (matched first).
    pub output: Option<String>,
    /// `[x, y, w, h]` of the fullscreen window or its monitor.
    pub rect: Option<[i32; 4]>,
    /// `rect` is in compositor logical pixels (Wayland) rather than the
    /// physical pixels used for monitors.
    pub logical: bool,
}

impl FullscreenArea {
    /// Whether this area is on the monitor at `monitor` (`[x, y, w, h]`,
    /// physical pixels) named `name` with display `scale`.
    pub fn covers(&self, name: &str, monitor: [i32; 4], scale: f32) -> bool {
        if let Some(o) = &self.output {
            return o == name;
        }
        let Some([x, y, w, h]) = self.rect else {
            return false;
        };
        let k = if self.logical && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let cx = ((x as f32 + w as f32 / 2.0) * k) as i32;
        let cy = ((y as f32 + h as f32 / 2.0) * k) as i32;
        let [mx, my, mw, mh] = monitor;
        cx >= mx && cx < mx + mw && cy >= my && cy < my + mh
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendStatus {
    pub backend: String,
    pub connected: bool,
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowGroup {
    pub app_key: String,
    pub app_name: String,
    /// Most recently minimized first.
    pub windows: Vec<WindowInfo>,
}

#[derive(Debug)]
struct Tracked {
    info: WindowInfo,
    /// Sequence number taken when the window became minimized.
    minimized_seq: u64,
}

#[derive(Debug, Default)]
pub struct WindowTracker {
    windows: HashMap<WindowId, Tracked>,
    seq: u64,
    generation: u64,
}

/// Older-minimized first. Backend timestamps win over observation order.
fn minimized_cmp(a: &Tracked, b: &Tracked) -> std::cmp::Ordering {
    match (a.info.minimized_at, b.info.minimized_at) {
        (Some(x), Some(y)) if x != y => x.cmp(&y),
        _ => a.minimized_seq.cmp(&b.minimized_seq),
    }
    .then_with(|| a.info.id.cmp(&b.info.id))
}

impl WindowTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Increments whenever the visible minimized set changes; the UI uses it
    /// to skip regrouping on frames where nothing happened.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn get(&self, id: &WindowId) -> Option<&WindowInfo> {
        self.windows.get(id).map(|t| &t.info)
    }

    pub fn len(&self) -> usize {
        self.windows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    /// Applies an event. Returns `true` if the minimized area changed.
    pub fn apply(&mut self, event: &WindowEvent) -> bool {
        let changed = match event {
            WindowEvent::Snapshot(list) => {
                let mut changed = false;
                let incoming: std::collections::HashSet<_> =
                    list.iter().map(|w| w.id.clone()).collect();
                self.windows.retain(|id, t| {
                    let keep = incoming.contains(id);
                    changed |= !keep && t.info.minimized;
                    keep
                });
                for w in list {
                    changed |= self.upsert(w.clone());
                }
                changed
            }
            WindowEvent::Upsert(w) => self.upsert(w.clone()),
            WindowEvent::Removed(id) => self
                .windows
                .remove(id)
                .map(|t| t.info.minimized)
                .unwrap_or(false),
            _ => false,
        };
        if changed {
            self.generation += 1;
        }
        changed
    }

    fn upsert(&mut self, info: WindowInfo) -> bool {
        self.seq += 1;
        let seq = self.seq;
        match self.windows.get_mut(&info.id) {
            Some(t) => {
                let was = t.info.minimized;
                if info.minimized && !was {
                    t.minimized_seq = seq;
                }
                let visible_change = (was || info.minimized) && t.info != info;
                t.info = info;
                visible_change
            }
            None => {
                let minimized = info.minimized;
                self.windows.insert(
                    info.id.clone(),
                    Tracked {
                        info,
                        minimized_seq: seq,
                    },
                );
                minimized
            }
        }
    }

    /// Minimized windows grouped by application. Groups are ordered by their
    /// most recently minimized member so the newest stays on top.
    /// Windows of one app's stack that are still minimized, oldest
    /// minimized first: restoring in this order leaves the most recently
    /// minimized window on top and focused.
    pub fn restore_all_order(&self, app_key: &str) -> Vec<WindowId> {
        let mut v: Vec<&Tracked> = self
            .windows
            .values()
            .filter(|t| t.info.minimized && t.info.app_key == app_key)
            .collect();
        v.sort_by(|a, b| minimized_cmp(a, b));
        v.into_iter().map(|t| t.info.id.clone()).collect()
    }

    pub fn groups(&self) -> Vec<WindowGroup> {
        let mut minimized: Vec<&Tracked> =
            self.windows.values().filter(|t| t.info.minimized).collect();
        minimized.sort_by(|a, b| minimized_cmp(b, a));
        let mut order: Vec<String> = Vec::new();
        let mut by_app: HashMap<&str, WindowGroup> = HashMap::new();
        for t in minimized {
            let g = by_app.entry(&t.info.app_key).or_insert_with(|| {
                order.push(t.info.app_key.clone());
                WindowGroup {
                    app_key: t.info.app_key.clone(),
                    app_name: t.info.app_name.clone(),
                    windows: vec![],
                }
            });
            g.windows.push(t.info.clone());
        }
        order
            .into_iter()
            .filter_map(|k| by_app.remove(k.as_str()))
            .collect()
    }

    pub fn minimized_count(&self) -> usize {
        self.windows.values().filter(|t| t.info.minimized).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(id: &str, app: &str, min: bool) -> WindowInfo {
        WindowInfo {
            id: WindowId(id.into()),
            app_key: app.into(),
            app_name: app.to_uppercase(),
            title: format!("{id} title"),
            minimized: min,
            icon_hint: None,
            minimized_at: None,
        }
    }

    #[test]
    fn only_minimized_windows_are_listed() {
        let mut t = WindowTracker::new();
        assert!(!t.apply(&WindowEvent::Upsert(w("1", "a", false))));
        assert!(t.groups().is_empty());
        assert!(t.apply(&WindowEvent::Upsert(w("1", "a", true))));
        assert_eq!(t.groups().len(), 1);
        // restored -> disappears
        assert!(t.apply(&WindowEvent::Upsert(w("1", "a", false))));
        assert!(t.groups().is_empty());
        // minimized then closed -> disappears
        t.apply(&WindowEvent::Upsert(w("1", "a", true)));
        assert!(t.apply(&WindowEvent::Removed(WindowId("1".into()))));
        assert!(t.groups().is_empty());
        // title change of a normal window does not touch the sidebar
        t.apply(&WindowEvent::Upsert(w("2", "a", false)));
        let mut renamed = w("2", "a", false);
        renamed.title = "x".into();
        assert!(!t.apply(&WindowEvent::Upsert(renamed)));
    }

    #[test]
    fn groups_by_app_newest_first() {
        let mut t = WindowTracker::new();
        for (id, app) in [("1", "editor"), ("2", "browser"), ("3", "editor")] {
            t.apply(&WindowEvent::Upsert(w(id, app, false)));
        }
        t.apply(&WindowEvent::Upsert(w("1", "editor", true)));
        t.apply(&WindowEvent::Upsert(w("2", "browser", true)));
        t.apply(&WindowEvent::Upsert(w("3", "editor", true)));
        let g = t.groups();
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].app_key, "editor");
        assert_eq!(
            g[0].windows
                .iter()
                .map(|w| w.id.0.as_str())
                .collect::<Vec<_>>(),
            ["3", "1"]
        );
        assert_eq!(g[1].app_key, "browser");
    }

    #[test]
    fn restore_all_order_is_oldest_first_and_only_minimized() {
        let mut t = WindowTracker::new();
        for (id, app) in [
            ("1", "editor"),
            ("2", "editor"),
            ("3", "editor"),
            ("4", "browser"),
        ] {
            t.apply(&WindowEvent::Upsert(w(id, app, false)));
        }
        t.apply(&WindowEvent::Upsert(w("2", "editor", true)));
        t.apply(&WindowEvent::Upsert(w("4", "browser", true)));
        t.apply(&WindowEvent::Upsert(w("1", "editor", true)));
        let ids = |v: Vec<WindowId>| v.into_iter().map(|i| i.0).collect::<Vec<_>>();
        assert_eq!(
            ids(t.restore_all_order("editor")),
            ["2", "1"],
            "window 3 is not minimized"
        );
        // A window closing mid-way simply drops out.
        t.apply(&WindowEvent::Removed(WindowId("2".into())));
        assert_eq!(ids(t.restore_all_order("editor")), ["1"]);
        assert!(t.restore_all_order("nothing").is_empty());
    }

    #[test]
    fn backend_timestamps_order_stacks() {
        let mut t = WindowTracker::new();
        let mut a = w("a", "app", true);
        a.minimized_at = Some(200);
        let mut b = w("b", "app", true);
        b.minimized_at = Some(100);
        // Seen in one snapshot: observation order says a, b; timestamps say b, a.
        t.apply(&WindowEvent::Snapshot(vec![a, b]));
        let order: Vec<_> = t
            .restore_all_order("app")
            .into_iter()
            .map(|i| i.0)
            .collect();
        assert_eq!(order, ["b", "a"]);
        assert_eq!(
            t.groups()[0].windows[0].id.0,
            "a",
            "newest first in the sidebar"
        );
    }

    #[test]
    fn fullscreen_area_matching() {
        let left = [0, 0, 1920, 1080];
        let right = [1920, 0, 2560, 1440];
        let named = FullscreenArea {
            output: Some("DP-1".into()),
            rect: None,
            logical: false,
        };
        assert!(named.covers("DP-1", right, 1.0));
        assert!(!named.covers("eDP-1", left, 1.0));
        // Physical rect on the right monitor.
        let r = FullscreenArea {
            output: None,
            rect: Some([1920, 0, 2560, 1440]),
            logical: false,
        };
        assert!(r.covers("x", right, 1.0) && !r.covers("y", left, 1.0));
        // Logical rect at 2x: (960..2240) logical = right monitor physically.
        let l = FullscreenArea {
            output: None,
            rect: Some([960, 0, 1280, 720]),
            logical: true,
        };
        assert!(l.covers("x", right, 2.0));
        assert!(!l.covers("x", left, 2.0));
        assert!(!FullscreenArea {
            output: None,
            rect: None,
            logical: false
        }
        .covers("x", left, 1.0));
    }

    #[test]
    fn snapshot_replaces_state() {
        let mut t = WindowTracker::new();
        t.apply(&WindowEvent::Upsert(w("1", "a", true)));
        let gen = t.generation();
        assert!(t.apply(&WindowEvent::Snapshot(vec![w("2", "b", true)])));
        assert!(t.generation() > gen);
        let g = t.groups();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].app_key, "b");
        assert!(t.apply(&WindowEvent::Snapshot(vec![])));
        assert!(t.groups().is_empty());
    }
}
