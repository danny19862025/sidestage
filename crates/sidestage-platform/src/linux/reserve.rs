//! Reserving screen space on Wayland compositors that support
//! `wlr-layer-shell` (KWin, wlroots compositors).
//!
//! KWin ignores `_NET_WM_STRUT*` from XWayland windows, so on Wayland the
//! (XWayland) sidebar is paired with an invisible, input-transparent
//! layer-shell surface whose exclusive zone keeps maximized windows clear
//! of the sidebar. GNOME has no layer-shell; there the Shell extension
//! reserves space instead.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Receiver, Sender};

use anyhow::{Context, Result};
use sidestage_core::config::Edge;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_output, wl_region, wl_registry, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

#[derive(Clone, Debug, PartialEq)]
pub struct Reservation {
    /// wl_output name (matches XWayland's RandR monitor name).
    pub output: String,
    pub edge: Edge,
    /// Logical (Wayland) pixels.
    pub size: u32,
}

pub struct LayerReserver {
    tx: Sender<Option<Reservation>>,
    wake: UnixStream,
    last: Option<Option<Reservation>>,
}

impl LayerReserver {
    /// Connects to the compositor; fails if layer-shell is unavailable.
    pub fn new() -> Result<LayerReserver> {
        let conn = super::wayland_connection().context("no Wayland connection")?;
        let (globals, queue) = registry_queue_init::<State>(&conn)?;
        let qh = queue.handle();
        let layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = globals
            .bind(&qh, 1..=4, ())
            .context("compositor has no wlr-layer-shell")?;
        let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 4..=6, ())?;
        let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ())?;
        let mut outputs = HashMap::new();
        for g in globals.contents().clone_list() {
            if g.interface == wl_output::WlOutput::interface().name && g.version >= 4 {
                let o: wl_output::WlOutput = globals.registry().bind(g.name, 4, &qh, g.name);
                outputs.insert(g.name, (o, String::new()));
            }
        }
        let (tx, rx) = mpsc::channel();
        let (wake_r, wake_w) = UnixStream::pair()?;
        wake_r.set_nonblocking(true)?;
        let mut state = State {
            layer_shell,
            compositor,
            shm,
            outputs,
            current: None,
            wanted: None,
        };
        std::thread::Builder::new()
            .name("layer-reserve".into())
            .spawn(move || {
                if let Err(e) = run(conn, queue, &mut state, rx, wake_r) {
                    log::warn!("space reservation stopped: {e:#}");
                }
            })?;
        Ok(LayerReserver {
            tx,
            wake: wake_w,
            last: None,
        })
    }

    pub fn set(&mut self, r: Option<Reservation>) {
        if self.last.as_ref() == Some(&r) {
            return;
        }
        self.last = Some(r.clone());
        let _ = self.tx.send(r);
        let _ = self.wake.write_all(b"x");
    }
}

impl Drop for LayerReserver {
    fn drop(&mut self) {
        let _ = self.tx.send(None);
        let _ = self.wake.write_all(b"x");
    }
}

struct Active {
    surface: wl_surface::WlSurface,
    layer: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    buffer: Option<wl_buffer::WlBuffer>,
}

struct State {
    layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1,
    compositor: wl_compositor::WlCompositor,
    shm: wl_shm::WlShm,
    outputs: HashMap<u32, (wl_output::WlOutput, String)>,
    current: Option<Active>,
    wanted: Option<Reservation>,
}

impl State {
    fn rebuild(&mut self, qh: &QueueHandle<State>) {
        if let Some(a) = self.current.take() {
            a.layer.destroy();
            a.surface.destroy();
            if let Some(b) = a.buffer {
                b.destroy();
            }
        }
        let Some(r) = self.wanted.clone() else { return };
        if r.size == 0 {
            return;
        }
        let output = self
            .outputs
            .values()
            .find(|(_, n)| *n == r.output)
            .map(|(o, _)| o.clone());
        if output.is_none() {
            log::debug!(
                "output {:?} not found among {:?}; letting the compositor choose",
                r.output,
                self.outputs
                    .values()
                    .map(|(_, n)| n.clone())
                    .collect::<Vec<_>>()
            );
        }
        let surface = self.compositor.create_surface(qh, ());
        // Empty input region: clicks fall through to whatever is below.
        let region = self.compositor.create_region(qh, ());
        surface.set_input_region(Some(&region));
        region.destroy();
        let layer = self.layer_shell.get_layer_surface(
            &surface,
            output.as_ref(),
            zwlr_layer_shell_v1::Layer::Background,
            "sidestage-reserve".into(),
            qh,
            (),
        );
        use zwlr_layer_surface_v1::Anchor;
        let side = match r.edge {
            Edge::Left => Anchor::Left,
            Edge::Right => Anchor::Right,
        };
        layer.set_anchor(side | Anchor::Top | Anchor::Bottom);
        layer.set_size(1, 0);
        layer.set_exclusive_zone(r.size as i32);
        layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
        surface.commit();
        self.current = Some(Active {
            surface,
            layer,
            buffer: None,
        });
    }

    fn attach(&mut self, qh: &QueueHandle<State>, width: u32, height: u32) {
        let Some(active) = self.current.as_mut() else {
            return;
        };
        let (w, h) = (width.max(1), height.max(1));
        let size = (w * h * 4) as usize;
        let fd = unsafe { libc::memfd_create(c"sidestage-reserve".as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            return;
        }
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        // A zero-filled ARGB buffer is fully transparent.
        if unsafe { libc::ftruncate(fd.as_raw_fd(), size as i64) } != 0 {
            return;
        }
        let pool = self.shm.create_pool(fd.as_fd(), size as i32, qh, ());
        let buffer = pool.create_buffer(
            0,
            w as i32,
            h as i32,
            (w * 4) as i32,
            wl_shm::Format::Argb8888,
            qh,
            (),
        );
        pool.destroy();
        active.surface.attach(Some(&buffer), 0, 0);
        active.surface.damage_buffer(0, 0, w as i32, h as i32);
        active.surface.commit();
        if let Some(old) = active.buffer.replace(buffer) {
            old.destroy();
        }
    }
}

fn run(
    conn: Connection,
    mut queue: wayland_client::EventQueue<State>,
    state: &mut State,
    rx: Receiver<Option<Reservation>>,
    mut wake: UnixStream,
) -> Result<()> {
    let qh = queue.handle();
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
                fd: wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(std::io::Error::last_os_error().into());
        }
        if fds[0].revents != 0 {
            guard.read()?;
        } else {
            drop(guard);
        }
        if fds[1].revents != 0 {
            let mut buf = [0u8; 32];
            while matches!(wake.read(&mut buf), Ok(n) if n > 0) {}
            let mut latest = None;
            let mut disconnected = false;
            loop {
                match rx.try_recv() {
                    Ok(r) => latest = Some(r),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
            if let Some(r) = latest {
                state.wanted = r;
                state.rebuild(&qh);
            }
            if disconnected {
                state.wanted = None;
                state.rebuild(&qh);
                conn.flush()?;
                return Ok(());
            }
        }
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

impl Dispatch<wl_output::WlOutput, u32> for State {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        global: &u32,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event {
            if let Some(entry) = state.outputs.get_mut(global) {
                entry.1 = name;
            }
            // An output we were waiting for may just have been named.
            if state.current.is_some() {
                state.rebuild(qh);
            }
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for State {
    fn event(
        state: &mut Self,
        layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                layer.ack_configure(serial);
                state.attach(qh, width, height);
            }
            zwlr_layer_surface_v1::Event::Closed => {
                state.current = None;
            }
            _ => {}
        }
    }
}

macro_rules! ignore {
    ($($t:ty),*) => {$(
        impl Dispatch<$t, ()> for State {
            fn event(_: &mut Self, _: &$t, _: <$t as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore!(
    zwlr_layer_shell_v1::ZwlrLayerShellV1,
    wl_compositor::WlCompositor,
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_buffer::WlBuffer,
    wl_surface::WlSurface,
    wl_region::WlRegion
);
