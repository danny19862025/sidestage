//! KDE Plasma (Wayland) backend using KWin's `org_kde_plasma_window_management`
//! protocol for the window list and restore, plus KWin ScreenShot2 for
//! still previews.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use sidestage_core::tracker::BackendStatus;
use sidestage_core::{WindowId, WindowInfo};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_plasma::plasma_window_management::client::{
    org_kde_plasma_window::{self, OrgKdePlasmaWindow},
    org_kde_plasma_window_management::{self, OrgKdePlasmaWindowManagement},
};

use super::kwin_screenshot::KwinScreenshot;
use super::AppResolver;
use crate::{ThumbnailMode, Waker, WindowBackend, WindowEvent};

const STATE_ACTIVE: u32 = 0x1;
const STATE_MINIMIZED: u32 = 0x2;
const STATE_SKIPTASKBAR: u32 = 0x1000;
const THUMB_MAX: u32 = 480;

enum Command {
    Restore(String),
    Thumbnail(String),
}

pub struct KdeBackend {
    last_active: Arc<Mutex<Option<String>>>,
    cmd: Sender<Command>,
    wake: Mutex<UnixStream>,
    thumbs: bool,
}

impl WindowBackend for KdeBackend {
    fn name(&self) -> &'static str {
        "kwin-wayland"
    }
    fn thumbnail_mode(&self) -> ThumbnailMode {
        if self.thumbs {
            ThumbnailMode::Snapshot
        } else {
            ThumbnailMode::None
        }
    }
    fn restore(&self, id: &WindowId) {
        let _ = self.cmd.send(Command::Restore(id.0.clone()));
        let _ = self.wake.lock().unwrap().write_all(b"r");
    }
    fn detects_fullscreen(&self) -> bool {
        true
    }
    fn focus_previous(&self) {
        let prev = self.last_active.lock().unwrap().clone();
        if let Some(uuid) = prev {
            let _ = self.cmd.send(Command::Restore(uuid));
            let _ = self.wake.lock().unwrap().write_all(b"f");
        }
    }
    fn refresh_thumbnail(&self, id: &WindowId) {
        let _ = self.cmd.send(Command::Thumbnail(id.0.clone()));
        let _ = self.wake.lock().unwrap().write_all(b"t");
    }
}

/// Returns `true` if the compositor advertises the protocol we need.
pub fn available(conn: &Connection) -> bool {
    match registry_queue_init::<Probe>(conn) {
        Ok((globals, _)) => globals.contents().with_list(|l| {
            l.iter()
                .any(|g| g.interface == OrgKdePlasmaWindowManagement::interface().name)
        }),
        Err(_) => false,
    }
}

struct Probe;
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Probe {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

pub fn start(conn: Connection, tx: Sender<WindowEvent>, waker: Waker) -> Result<KdeBackend> {
    let (globals, queue) = registry_queue_init::<State>(&conn).context("wayland registry")?;
    let qh = queue.handle();
    let mgr: OrgKdePlasmaWindowManagement = globals
        .bind(&qh, 13..=18, ())
        .context("org_kde_plasma_window_management (v13+) not offered by the compositor")?;
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (wake_r, wake_w) = UnixStream::pair()?;
    wake_r.set_nonblocking(true)?;

    // Probe ScreenShot2 once; a refusal means icon-only previews.
    let shot = KwinScreenshot::new().ok();
    let thumbs = shot.is_some() && std::env::var_os("SIDESTAGE_NO_THUMBNAILS").is_none();
    let (thumb_tx, thumb_handle) =
        spawn_thumbnailer(shot.filter(|_| thumbs), tx.clone(), waker.clone());

    let last_active = Arc::new(Mutex::new(None));
    let mut state = State {
        tx: tx.clone(),
        waker: waker.clone(),
        windows: HashMap::new(),
        sent: HashSet::new(),
        own_pid: std::process::id(),
        resolver: AppResolver::new(tx.clone(), waker.clone()),
        thumbs: thumb_tx,
        last_active: last_active.clone(),
        activation_seq: 0,
        fullscreen: Vec::new(),
        _mgr: mgr,
    };
    let _ = tx.send(WindowEvent::Status(BackendStatus {
        backend: "KWin (Wayland)".into(),
        connected: true,
        message: (!thumbs).then(|| "Window previews unavailable; showing icons.".into()),
    }));
    waker();

    std::thread::Builder::new()
        .name("kwin-wayland".into())
        .spawn(move || {
            let _keep = thumb_handle;
            if let Err(e) = run(conn, queue, &mut state, cmd_rx, wake_r) {
                log::error!("KWin backend stopped: {e:#}");
                let _ = state.tx.send(WindowEvent::Status(BackendStatus {
                    backend: "KWin (Wayland)".into(),
                    connected: false,
                    message: Some(format!("Lost connection to KWin: {e}")),
                }));
                (state.waker)();
            }
        })?;
    Ok(KdeBackend {
        last_active,
        cmd: cmd_tx,
        wake: Mutex::new(wake_w),
        thumbs,
    })
}

fn spawn_thumbnailer(
    shot: Option<KwinScreenshot>,
    tx: Sender<WindowEvent>,
    waker: Waker,
) -> (Option<Sender<String>>, Option<std::thread::JoinHandle<()>>) {
    let Some(shot) = shot else {
        return (None, None);
    };
    let (ttx, trx) = mpsc::channel::<String>();
    let h = std::thread::Builder::new()
        .name("kwin-thumbs".into())
        .spawn(move || {
            while let Ok(uuid) = trx.recv() {
                // Coalesce bursts (e.g. several windows minimized at once).
                let mut batch = vec![uuid];
                while let Ok(more) = trx.try_recv() {
                    if !batch.contains(&more) {
                        batch.push(more);
                    }
                }
                for uuid in batch {
                    match shot.capture_window(&uuid, THUMB_MAX) {
                        Ok(img) => {
                            let _ = tx.send(WindowEvent::Thumbnail {
                                id: WindowId(uuid),
                                image: Some(img),
                            });
                            waker();
                        }
                        Err(e) => log::debug!("no preview for {uuid}: {e:#}"),
                    }
                }
            }
        })
        .ok();
    (Some(ttx), h)
}

fn run(
    conn: Connection,
    mut queue: wayland_client::EventQueue<State>,
    state: &mut State,
    cmd_rx: Receiver<Command>,
    mut wake_r: UnixStream,
) -> Result<()> {
    queue.roundtrip(state)?;
    loop {
        queue.dispatch_pending(state)?;
        conn.flush()?;
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let mut fds = [
            libc::pollfd {
                fd: guard.connection_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: wake_r.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err.into());
        }
        if fds[0].revents & (libc::POLLIN | libc::POLLERR | libc::POLLHUP) != 0 {
            guard.read()?;
        } else {
            drop(guard);
        }
        if fds[1].revents & libc::POLLIN != 0 {
            let mut buf = [0u8; 64];
            while matches!(wake_r.read(&mut buf), Ok(n) if n > 0) {}
            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    Command::Restore(uuid) => {
                        if let Some(w) = state.windows.get(&uuid) {
                            // Clear "minimized" and request "active" in one go.
                            w.proxy
                                .set_state(STATE_MINIMIZED | STATE_ACTIVE, STATE_ACTIVE);
                        }
                    }
                    Command::Thumbnail(uuid) => {
                        if let Some(t) = &state.thumbs {
                            let _ = t.send(uuid);
                        }
                    }
                }
            }
        }
    }
}

struct Win {
    proxy: OrgKdePlasmaWindow,
    title: String,
    app_id: String,
    resource_name: String,
    icon_name: String,
    pid: u32,
    flags: u32,
    ready: bool,
    /// Logical `[x, y, w, h]`.
    geom: Option<[i32; 4]>,
    /// Increases each time the window becomes active.
    activated: u64,
}

struct State {
    tx: Sender<WindowEvent>,
    waker: Waker,
    windows: HashMap<String, Win>,
    /// Windows currently known to the UI.
    sent: HashSet<String>,
    own_pid: u32,
    resolver: AppResolver,
    thumbs: Option<Sender<String>>,
    last_active: Arc<Mutex<Option<String>>>,
    activation_seq: u64,
    fullscreen: Vec<sidestage_core::FullscreenArea>,
    _mgr: OrgKdePlasmaWindowManagement,
}

impl State {
    /// A fullscreen, non-minimized window covers its screen until another
    /// window on that screen is activated after it.
    fn update_fullscreen(&mut self) {
        const STATE_FULLSCREEN: u32 = 0x8;
        let visible = |w: &Win| w.ready && w.flags & STATE_MINIMIZED == 0 && w.pid != self.own_pid;
        let center = |g: [i32; 4]| (g[0] + g[2] / 2, g[1] + g[3] / 2);
        let inside = |p: (i32, i32), g: [i32; 4]| {
            p.0 >= g[0] && p.0 < g[0] + g[2] && p.1 >= g[1] && p.1 < g[1] + g[3]
        };
        let mut areas: Vec<sidestage_core::FullscreenArea> = self
            .windows
            .values()
            .filter(|w| visible(w) && w.flags & STATE_FULLSCREEN != 0)
            .filter_map(|f| {
                let g = f.geom?;
                let covered = self.windows.values().any(|o| {
                    visible(o)
                        && o.activated > f.activated
                        && o.geom.map(|og| inside(center(og), g)).unwrap_or(false)
                });
                (!covered).then_some(sidestage_core::FullscreenArea {
                    output: None,
                    rect: Some(g),
                    logical: true,
                })
            })
            .collect();
        areas.sort_by_key(|a| a.rect);
        if areas != self.fullscreen {
            log::debug!("fullscreen areas: {areas:?}");
            self.fullscreen = areas.clone();
            let _ = self.tx.send(WindowEvent::Fullscreen(areas));
            (self.waker)();
        }
    }

    fn publish(&mut self, uuid: &str, was_minimized: bool) {
        let Some(w) = self.windows.get(uuid) else {
            return;
        };
        if !w.ready {
            return;
        }
        let hidden = w.flags & STATE_SKIPTASKBAR != 0 || w.pid == self.own_pid;
        if hidden {
            if self.sent.remove(uuid) {
                let _ = self.tx.send(WindowEvent::Removed(WindowId(uuid.into())));
                (self.waker)();
            }
            return;
        }
        let key = if !w.app_id.is_empty() {
            w.app_id.clone()
        } else {
            w.resource_name.clone()
        };
        let app = self.resolver.resolve(&key, Some(w.pid), &w.icon_name);
        let minimized = w.flags & STATE_MINIMIZED != 0;
        let info = WindowInfo {
            id: WindowId(uuid.into()),
            app_key: if key.is_empty() {
                format!("pid:{}", w.pid)
            } else {
                key
            },
            app_name: app.name,
            title: if w.title.is_empty() {
                "Untitled window".into()
            } else {
                w.title.clone()
            },
            minimized,
            icon_hint: app.icon,
            minimized_at: None,
        };
        self.sent.insert(uuid.to_string());
        let _ = self.tx.send(WindowEvent::Upsert(info));
        if minimized && !was_minimized {
            if let Some(t) = &self.thumbs {
                let _ = t.send(uuid.to_string());
            }
        }
        (self.waker)();
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<OrgKdePlasmaWindowManagement, ()> for State {
    fn event(
        state: &mut Self,
        mgr: &OrgKdePlasmaWindowManagement,
        event: org_kde_plasma_window_management::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let org_kde_plasma_window_management::Event::WindowWithUuid { uuid, .. } = event {
            if state.windows.contains_key(&uuid) {
                return;
            }
            let proxy = mgr.get_window_by_uuid(uuid.clone(), qh, uuid.clone());
            state.windows.insert(
                uuid,
                Win {
                    proxy,
                    title: String::new(),
                    app_id: String::new(),
                    resource_name: String::new(),
                    icon_name: String::new(),
                    pid: 0,
                    flags: 0,
                    ready: false,
                    geom: None,
                    activated: 0,
                },
            );
        }
    }
}

impl Dispatch<OrgKdePlasmaWindow, String> for State {
    fn event(
        state: &mut Self,
        _: &OrgKdePlasmaWindow,
        event: org_kde_plasma_window::Event,
        uuid: &String,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use org_kde_plasma_window::Event as E;
        let Some(w) = state.windows.get_mut(uuid) else {
            return;
        };
        let was_min = w.flags & STATE_MINIMIZED != 0;
        let mut changed = true;
        let mut fs_dirty = false;
        match event {
            E::TitleChanged { title } => w.title = title,
            E::AppIdChanged { app_id } => w.app_id = app_id,
            E::ResourceNameChanged { resource_name } => w.resource_name = resource_name,
            E::ThemedIconNameChanged { name } => w.icon_name = name,
            E::PidChanged { pid } => w.pid = pid,
            E::StateChanged { flags } => {
                if flags & STATE_ACTIVE != 0 && w.flags & STATE_ACTIVE == 0 {
                    state.activation_seq += 1;
                    w.activated = state.activation_seq;
                }
                if flags & STATE_ACTIVE != 0 && w.pid != state.own_pid && w.pid != 0 {
                    *state.last_active.lock().unwrap() = Some(uuid.clone());
                }
                fs_dirty = true;
                // Ignore pure focus changes for non-minimized windows.
                changed = (w.flags ^ flags) & !STATE_ACTIVE != 0;
                w.flags = flags;
            }
            E::InitialState => w.ready = true,
            E::Geometry {
                x,
                y,
                width,
                height,
            } => {
                w.geom = Some([x, y, width as i32, height as i32]);
                changed = false;
                fs_dirty = true;
            }
            E::Unmapped => {
                let w = state.windows.remove(uuid).unwrap();
                w.proxy.destroy();
                if state.sent.remove(uuid) {
                    let _ = state.tx.send(WindowEvent::Removed(WindowId(uuid.clone())));
                    (state.waker)();
                }
                state.update_fullscreen();
                return;
            }
            _ => changed = false,
        }
        if changed {
            let uuid = uuid.clone();
            state.publish(&uuid, was_min);
        }
        if fs_dirty {
            state.update_fullscreen();
        }
    }
}
