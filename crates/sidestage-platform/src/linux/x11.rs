//! X11 backend using EWMH (`_NET_CLIENT_LIST`, `_NET_WM_STATE_HIDDEN`,
//! `_NET_ACTIVE_WINDOW`). Works with KWin/Mutter/Xfwm/Openbox X11 sessions.
//!
//! Minimized X11 windows are unmapped, so the compositor keeps no pixels for
//! them: previews fall back to the window icon and title.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;
use std::sync::Mutex;

use anyhow::{Context, Result};
use sidestage_core::tracker::BackendStatus;
use sidestage_core::{RgbaImage, WindowId, WindowInfo};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConnectionExt as _, EventMask, Window,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use super::AppResolver;
use crate::{ThumbnailMode, Waker, WindowBackend, WindowEvent};

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        _NET_CLIENT_LIST,
        _NET_WM_STATE,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_NORMAL,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_WM_NAME,
        _NET_WM_PID,
        _NET_WM_ICON,
        _NET_ACTIVE_WINDOW,
        UTF8_STRING,
        WM_STATE,
    }
}

pub struct X11Backend {
    conn: Mutex<RustConnection>,
    root: Window,
    atoms: Atoms,
    /// Last active window that is not Sidestage's own.
    last_active: std::sync::Arc<Mutex<Option<u32>>>,
}

impl WindowBackend for X11Backend {
    fn name(&self) -> &'static str {
        "x11-ewmh"
    }
    fn thumbnail_mode(&self) -> ThumbnailMode {
        ThumbnailMode::None
    }
    fn focus_previous(&self) {
        let prev = *self.last_active.lock().unwrap();
        if let Some(w) = prev {
            self.restore(&WindowId(w.to_string()));
        }
    }
    fn restore(&self, id: &WindowId) {
        let Ok(win) = id.0.parse::<u32>() else { return };
        let conn = self.conn.lock().unwrap();
        // EWMH WMs de-iconify and focus a window on _NET_ACTIVE_WINDOW.
        let ev = ClientMessageEvent::new(32, win, self.atoms._NET_ACTIVE_WINDOW, [2, 0, 0, 0, 0]);
        let _ = conn.send_event(
            false,
            self.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            ev,
        );
        let _ = conn.flush();
    }
}

pub fn start(tx: Sender<WindowEvent>, waker: Waker) -> Result<X11Backend> {
    let (cmd_conn, screen) = x11rb::connect(None).context("X11 connection")?;
    let root = cmd_conn.setup().roots[screen].root;
    let atoms = Atoms::new(&cmd_conn)?.reply()?;
    // Fail early if the WM is not EWMH-compliant.
    let supported = cmd_conn
        .get_property(false, root, atoms._NET_CLIENT_LIST, AtomEnum::WINDOW, 0, 1)?
        .reply()?;
    if supported.type_ == 0 {
        anyhow::bail!("window manager does not publish _NET_CLIENT_LIST");
    }
    let (conn, _) = x11rb::connect(None)?;
    let last_active = std::sync::Arc::new(Mutex::new(None));
    let last_active2 = last_active.clone();
    std::thread::Builder::new()
        .name("x11-ewmh".into())
        .spawn(move || {
            let mut w = Watcher {
                conn,
                root,
                atoms,
                tx: tx.clone(),
                waker: waker.clone(),
                known: HashMap::new(),
                iconed: HashSet::new(),
                resolver: AppResolver::new(tx.clone(), waker.clone()),
                own_pid: std::process::id(),
                last_active: last_active2,
            };
            let _ = tx.send(WindowEvent::Status(BackendStatus {
                backend: "X11 (EWMH)".into(),
                connected: true,
                message: Some("X11 cannot preview minimized windows; showing icons.".into()),
            }));
            if let Err(e) = w.run() {
                log::error!("X11 backend stopped: {e:#}");
            }
        })?;
    Ok(X11Backend {
        conn: Mutex::new(cmd_conn),
        root,
        atoms,
        last_active,
    })
}

struct Watcher {
    conn: RustConnection,
    root: Window,
    atoms: Atoms,
    tx: Sender<WindowEvent>,
    waker: Waker,
    known: HashMap<Window, Option<WindowInfo>>,
    iconed: HashSet<String>,
    resolver: AppResolver,
    own_pid: u32,
    last_active: std::sync::Arc<Mutex<Option<u32>>>,
}

impl Watcher {
    fn note_active(&self) {
        let win = self
            .conn
            .get_property(
                false,
                self.root,
                self.atoms._NET_ACTIVE_WINDOW,
                AtomEnum::WINDOW,
                0,
                1,
            )
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().and_then(|mut v| v.next()))
            .filter(|w| *w != 0);
        let Some(win) = win else { return };
        let pid = self
            .conn
            .get_property(false, win, self.atoms._NET_WM_PID, AtomEnum::CARDINAL, 0, 1)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().and_then(|mut v| v.next()));
        if pid != Some(self.own_pid) {
            *self.last_active.lock().unwrap() = Some(win);
        }
    }

    fn run(&mut self) -> Result<()> {
        self.conn.change_window_attributes(
            self.root,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?;
        self.sync_clients()?;
        self.conn.flush()?;
        loop {
            let ev = self.conn.wait_for_event()?;
            if let Event::PropertyNotify(p) = ev {
                if p.window == self.root {
                    if p.atom == self.atoms._NET_CLIENT_LIST {
                        self.sync_clients()?;
                    } else if p.atom == self.atoms._NET_ACTIVE_WINDOW {
                        self.note_active();
                    }
                } else if [
                    self.atoms._NET_WM_STATE,
                    self.atoms._NET_WM_NAME,
                    self.atoms.WM_STATE,
                    u32::from(AtomEnum::WM_NAME),
                ]
                .contains(&p.atom)
                {
                    self.refresh(p.window);
                }
            }
        }
    }

    fn sync_clients(&mut self) -> Result<()> {
        let list: Vec<Window> = self
            .conn
            .get_property(
                false,
                self.root,
                self.atoms._NET_CLIENT_LIST,
                AtomEnum::WINDOW,
                0,
                4096,
            )?
            .reply()?
            .value32()
            .map(|v| v.collect())
            .unwrap_or_default();
        let current: HashSet<Window> = list.iter().copied().collect();
        let gone: Vec<Window> = self
            .known
            .keys()
            .filter(|w| !current.contains(w))
            .copied()
            .collect();
        for w in gone {
            if let Some(Some(_)) = self.known.remove(&w) {
                let _ = self.tx.send(WindowEvent::Removed(WindowId(w.to_string())));
            }
        }
        for w in list {
            if !self.known.contains_key(&w) {
                let _ = self.conn.change_window_attributes(
                    w,
                    &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
                );
                self.known.insert(w, None);
                self.refresh(w);
            }
        }
        self.conn.flush()?;
        (self.waker)();
        Ok(())
    }

    fn atoms_of(&self, w: Window, prop: u32) -> Vec<u32> {
        self.conn
            .get_property(false, w, prop, AtomEnum::ATOM, 0, 64)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|v| v.collect()))
            .unwrap_or_default()
    }

    fn string_prop(&self, w: Window, prop: u32, ty: u32) -> Option<String> {
        let r = self
            .conn
            .get_property(false, w, prop, ty, 0, 1024)
            .ok()?
            .reply()
            .ok()?;
        (!r.value.is_empty()).then(|| String::from_utf8_lossy(&r.value).into_owned())
    }

    fn refresh(&mut self, w: Window) {
        if !self.known.contains_key(&w) {
            return;
        }
        let info = self.read(w);
        let prev = self.known.get(&w).cloned().flatten();
        match (&info, &prev) {
            (Some(i), _) if prev.as_ref() != Some(i) => {
                let _ = self.tx.send(WindowEvent::Upsert(i.clone()));
            }
            (None, Some(_)) => {
                let _ = self.tx.send(WindowEvent::Removed(WindowId(w.to_string())));
            }
            _ => {}
        }
        self.known.insert(w, info);
        (self.waker)();
    }

    fn read(&mut self, w: Window) -> Option<WindowInfo> {
        let a = &self.atoms;
        let types = self.atoms_of(w, a._NET_WM_WINDOW_TYPE);
        if !types.is_empty()
            && !types
                .iter()
                .any(|t| *t == a._NET_WM_WINDOW_TYPE_NORMAL || *t == a._NET_WM_WINDOW_TYPE_DIALOG)
        {
            return None;
        }
        let states = self.atoms_of(w, a._NET_WM_STATE);
        if states.contains(&a._NET_WM_STATE_SKIP_TASKBAR) {
            return None;
        }
        let pid = self
            .conn
            .get_property(false, w, a._NET_WM_PID, AtomEnum::CARDINAL, 0, 1)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().and_then(|mut v| v.next()));
        if pid == Some(self.own_pid) {
            return None;
        }
        let iconic = self
            .conn
            .get_property(false, w, a.WM_STATE, a.WM_STATE, 0, 2)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().and_then(|mut v| v.next()))
            == Some(3);
        let minimized = states.contains(&a._NET_WM_STATE_HIDDEN) || iconic;
        let title = self
            .string_prop(w, a._NET_WM_NAME, a.UTF8_STRING)
            .or_else(|| self.string_prop(w, AtomEnum::WM_NAME.into(), AtomEnum::STRING.into()))
            .unwrap_or_else(|| "Untitled window".into());
        let class = self
            .conn
            .get_property(false, w, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| {
                let parts: Vec<String> = r
                    .value
                    .split(|b| *b == 0)
                    .map(|s| String::from_utf8_lossy(s).into_owned())
                    .collect();
                parts
                    .get(1)
                    .cloned()
                    .filter(|s| !s.is_empty())
                    .or_else(|| parts.first().cloned())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let app_key = if class.is_empty() {
            format!("x11:{w}")
        } else {
            class.clone()
        };
        let app = self.resolver.resolve(&app_key, pid, "");
        if !self.iconed.contains(&app_key) {
            // Prefer the window's own _NET_WM_ICON over the theme lookup.
            if let Some(img) = self.net_wm_icon(w, 64) {
                self.iconed.insert(app_key.clone());
                let _ = self.tx.send(WindowEvent::Icon {
                    app_key: app_key.clone(),
                    image: img,
                });
            }
        }
        Some(WindowInfo {
            id: WindowId(w.to_string()),
            app_key,
            app_name: app.name,
            title,
            minimized,
            icon_hint: app.icon,
            minimized_at: None,
        })
    }

    fn net_wm_icon(&self, w: Window, want: u32) -> Option<RgbaImage> {
        let r = self
            .conn
            .get_property(
                false,
                w,
                self.atoms._NET_WM_ICON,
                AtomEnum::CARDINAL,
                0,
                1 << 20,
            )
            .ok()?
            .reply()
            .ok()?;
        let data: Vec<u32> = r.value32()?.collect();
        best_icon(&data, want)
    }
}

/// Picks the `_NET_WM_ICON` entry closest to (and preferably above) `want`.
fn best_icon(data: &[u32], want: u32) -> Option<RgbaImage> {
    let mut i = 0;
    let mut best: Option<(u32, u32, usize)> = None;
    while i + 2 <= data.len() {
        let (w, h) = (data[i], data[i + 1]);
        let len = (w as usize).checked_mul(h as usize)?;
        if w == 0 || h == 0 || i + 2 + len > data.len() {
            break;
        }
        let score = |s: u32| if s >= want { s - want } else { (want - s) * 4 };
        if best.map(|(bw, _, _)| score(w) < score(bw)).unwrap_or(true) {
            best = Some((w, h, i + 2));
        }
        i += 2 + len;
    }
    let (w, h, off) = best?;
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for &argb in &data[off..off + (w * h) as usize] {
        px.extend_from_slice(&[
            (argb >> 16) as u8,
            (argb >> 8) as u8,
            argb as u8,
            (argb >> 24) as u8,
        ]);
    }
    RgbaImage::new(w, h, px)
}

#[cfg(test)]
mod tests {
    use super::best_icon;

    #[test]
    fn picks_closest_icon() {
        let mut data = vec![2, 2];
        data.extend([0xFF0000FFu32; 4]);
        data.extend([4, 4]);
        data.extend([0x80FF0000u32; 16]);
        let img = best_icon(&data, 4).unwrap();
        assert_eq!((img.width, img.height), (4, 4));
        assert_eq!(&img.pixels[..4], &[0xFF, 0, 0, 0x80]);
        assert!(best_icon(&[5, 5, 1], 4).is_none());
    }
}
