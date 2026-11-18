//! GNOME Shell (Wayland or X11) backend.
//!
//! Mutter does not expose other clients' windows to applications, so the
//! window list, restore and previews come from the companion GNOME Shell
//! extension (`integrations/gnome/`), which runs inside gnome-shell and
//! exports `org.sidestage.Shell1` on the session bus.

use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use sidestage_core::tracker::BackendStatus;
use sidestage_core::{WindowId, WindowInfo};

use super::AppResolver;
use crate::{ThumbnailMode, Waker, WindowBackend, WindowEvent};

const BUS_NAME: &str = "org.sidestage.Shell";
const PATH: &str = "/org/sidestage/Shell";
const IFACE: &str = "org.sidestage.Shell1";
const THUMB_MAX: u32 = 480;

#[derive(Deserialize)]
struct ExtWindow {
    id: String,
    #[serde(default)]
    app_id: String,
    #[serde(default)]
    app_name: String,
    #[serde(default)]
    title: String,
    minimized: bool,
    #[serde(default)]
    icon: String,
}

pub struct GnomeBackend {
    conn: zbus::blocking::Connection,
    tx: Sender<WindowEvent>,
    waker: Waker,
}

impl GnomeBackend {
    fn proxy(&self) -> zbus::Result<zbus::blocking::Proxy<'static>> {
        zbus::blocking::Proxy::new(&self.conn, BUS_NAME, PATH, IFACE)
    }
}

impl WindowBackend for GnomeBackend {
    fn name(&self) -> &'static str {
        "gnome-shell"
    }
    fn thumbnail_mode(&self) -> ThumbnailMode {
        ThumbnailMode::Snapshot
    }
    fn restore(&self, id: &WindowId) {
        if let Err(e) = self
            .proxy()
            .and_then(|p| p.call::<_, _, ()>("Activate", &(id.0.as_str(),)))
        {
            log::warn!("GNOME extension Activate failed: {e}");
        }
    }
    fn refresh_thumbnail(&self, id: &WindowId) {
        request_thumbnail(&self.conn, &self.tx, &self.waker, &id.0);
    }
}

fn request_thumbnail(
    conn: &zbus::blocking::Connection,
    tx: &Sender<WindowEvent>,
    waker: &Waker,
    id: &str,
) {
    let (conn, tx, waker, id) = (conn.clone(), tx.clone(), waker.clone(), id.to_string());
    std::thread::spawn(move || {
        let path: zbus::Result<String> = zbus::blocking::Proxy::new(&conn, BUS_NAME, PATH, IFACE)
            .and_then(|p| p.call("Snapshot", &(id.as_str(), THUMB_MAX)));
        match path {
            Ok(path) if !path.is_empty() => {
                if let Some(img) =
                    super::icons::load_image_file(std::path::Path::new(&path), THUMB_MAX)
                {
                    let _ = tx.send(WindowEvent::Thumbnail {
                        id: WindowId(id),
                        image: Some(img),
                    });
                    waker();
                }
                let _ = std::fs::remove_file(&path);
            }
            Ok(_) => {}
            Err(e) => log::debug!("snapshot {id}: {e}"),
        }
    });
}

pub fn start(tx: Sender<WindowEvent>, waker: Waker) -> Result<GnomeBackend> {
    let conn = zbus::blocking::Connection::session().context("session bus")?;
    let backend = GnomeBackend {
        conn: conn.clone(),
        tx: tx.clone(),
        waker: waker.clone(),
    };
    std::thread::Builder::new().name("gnome-ext".into()).spawn(move || {
        let mut resolver = AppResolver::new(tx.clone(), waker.clone());
        let mut minimized_before = std::collections::HashSet::<String>::new();
        let mut announced: Option<bool> = None;
        loop {
            let present = has_owner(&conn);
            if announced != Some(present) {
                announced = Some(present);
                let _ = tx.send(WindowEvent::Status(BackendStatus {
                    backend: "GNOME Shell extension".into(),
                    connected: present,
                    message: (!present).then(|| {
                        "Enable the Sidestage GNOME Shell extension to list minimized windows.".to_string()
                    }),
                }));
                if !present {
                    let _ = tx.send(WindowEvent::Snapshot(vec![]));
                }
                waker();
            }
            if !present {
                std::thread::sleep(Duration::from_secs(3));
                continue;
            }
            if let Err(e) = follow(&conn, &tx, &waker, &mut resolver, &mut minimized_before) {
                log::info!("GNOME extension connection ended: {e:#}");
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    })?;
    Ok(backend)
}

fn has_owner(conn: &zbus::blocking::Connection) -> bool {
    zbus::blocking::fdo::DBusProxy::new(conn)
        .ok()
        .and_then(|p| p.name_has_owner(BUS_NAME.try_into().unwrap()).ok())
        .unwrap_or(false)
}

/// Lists windows, then re-lists on every `Changed` signal until the
/// extension disappears.
fn follow(
    conn: &zbus::blocking::Connection,
    tx: &Sender<WindowEvent>,
    waker: &Waker,
    resolver: &mut AppResolver,
    minimized_before: &mut std::collections::HashSet<String>,
) -> Result<()> {
    let proxy = zbus::blocking::Proxy::new(conn, BUS_NAME, PATH, IFACE)?;
    let signals = proxy.receive_signal("Changed")?;
    let sync = |resolver: &mut AppResolver,
                minimized_before: &mut std::collections::HashSet<String>|
     -> Result<()> {
        let json: String = proxy.call("ListWindows", &())?;
        let list: Vec<ExtWindow> = serde_json::from_str(&json)?;
        let mut infos = Vec::with_capacity(list.len());
        let mut now_min = std::collections::HashSet::new();
        for w in list {
            let key = if w.app_id.is_empty() {
                format!("gnome:{}", w.id)
            } else {
                w.app_id.trim_end_matches(".desktop").to_string()
            };
            let app = resolver.resolve(&key, None, &w.icon);
            if w.minimized {
                now_min.insert(w.id.clone());
                if !minimized_before.contains(&w.id) {
                    request_thumbnail(conn, tx, waker, &w.id);
                }
            }
            infos.push(WindowInfo {
                id: WindowId(w.id),
                app_key: key,
                app_name: if w.app_name.is_empty() {
                    app.name
                } else {
                    w.app_name
                },
                title: if w.title.is_empty() {
                    "Untitled window".into()
                } else {
                    w.title
                },
                minimized: w.minimized,
                icon_hint: app.icon,
            });
        }
        *minimized_before = now_min;
        let _ = tx.send(WindowEvent::Snapshot(infos));
        waker();
        Ok(())
    };
    sync(resolver, minimized_before)?;
    for _ in signals {
        sync(resolver, minimized_before)?;
    }
    anyhow::bail!("signal stream closed")
}

/// Asks the GNOME extension to reserve `size` logical pixels at an edge of
/// a monitor (0 releases it). No-op when the extension is not running.
pub fn set_reserved_space(monitor: &str, edge: sidestage_core::config::Edge, size: u32) {
    let edge = match edge {
        sidestage_core::config::Edge::Left => "left",
        sidestage_core::config::Edge::Right => "right",
    };
    let (monitor, edge) = (monitor.to_string(), edge.to_string());
    std::thread::spawn(move || {
        let r = zbus::blocking::Connection::session().and_then(|c| {
            zbus::blocking::Proxy::new(&c, BUS_NAME, PATH, IFACE)?
                .call::<_, _, ()>("SetReservedSpace", &(monitor.as_str(), edge.as_str(), size))
        });
        if let Err(e) = r {
            log::debug!("GNOME extension SetReservedSpace: {e}");
        }
    });
}
