//! X11 / XWayland edge panel: dock window type, struts, no-focus hints.
//!
//! Wayland offers no portable way for a client to position itself or
//! reserve screen space (GNOME has no layer-shell), so the sidebar runs as
//! an X11 client. KWin and Mutter both honour `_NET_WM_WINDOW_TYPE_DOCK`
//! and `_NET_WM_STRUT_PARTIAL` for XWayland windows.

use anyhow::{anyhow, Context, Result};
use raw_window_handle::RawWindowHandle;
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt as _, EventMask, PropMode,
    Window,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use crate::panel::{Edge, LiveThumbnails, MonitorInfo, PanelGeometry};

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DOCK,
        _NET_WM_STATE,
        _NET_WM_STATE_ABOVE,
        _NET_WM_STATE_STICKY,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_STATE_SKIP_PAGER,
        _NET_WM_DESKTOP,
        _NET_WM_STRUT,
        _NET_WM_STRUT_PARTIAL,
        _NET_ACTIVE_WINDOW,
        _NET_WORKAREA,
        WM_HINTS,
        WM_PROTOCOLS,
        WM_TAKE_FOCUS,
        _KDE_NET_WM_BLUR_BEHIND_REGION,
    }
}

fn connect() -> Result<(RustConnection, usize)> {
    x11rb::connect(None).context("cannot connect to the X server (XWayland)")
}

pub fn monitors() -> Vec<MonitorInfo> {
    match monitors_inner() {
        Ok(m) if !m.is_empty() => m,
        Ok(_) => vec![],
        Err(e) => {
            log::warn!("monitor query failed: {e:#}");
            vec![]
        }
    }
}

fn monitors_inner() -> Result<Vec<MonitorInfo>> {
    let (conn, screen_num) = connect()?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let atoms = Atoms::new(&conn)?.reply()?;
    let workarea = conn
        .get_property(false, root, atoms._NET_WORKAREA, AtomEnum::CARDINAL, 0, 4)?
        .reply()
        .ok()
        .and_then(|r| r.value32().map(|v| v.collect::<Vec<u32>>()))
        .filter(|v| v.len() == 4)
        .map(|v| (v[0] as i32, v[1] as i32, v[2] as i32, v[3] as i32));
    // Xft.dpi gives the scale used by toolkits under XWayland/X11.
    let scale = xft_dpi(&conn, root).map(|d| d / 96.0).unwrap_or(1.0);
    let reply = conn.randr_get_monitors(root, true)?.reply()?;
    let mut out = Vec::new();
    for (i, m) in reply.monitors.iter().enumerate() {
        let name = conn
            .get_atom_name(m.name)?
            .reply()
            .map(|r| String::from_utf8_lossy(&r.name).into_owned())
            .unwrap_or_else(|_| format!("Monitor {}", i + 1));
        let (x, y, w, h) = (m.x as i32, m.y as i32, m.width as i32, m.height as i32);
        // _NET_WORKAREA is one rectangle for the whole desktop; use its
        // vertical extent clipped to this monitor so bottom/top panels are avoided.
        let work = match workarea {
            Some((_, wy, _, wh)) => {
                let top = wy.max(y);
                let bottom = (wy + wh).min(y + h);
                if bottom - top > h / 2 {
                    (x, top, w, bottom - top)
                } else {
                    (x, y, w, h)
                }
            }
            None => (x, y, w, h),
        };
        out.push(MonitorInfo {
            name,
            index: i,
            primary: m.primary,
            x,
            y,
            width: w,
            height: h,
            work,
            scale,
        });
    }
    if !out.iter().any(|m| m.primary) {
        if let Some(first) = out.first_mut() {
            first.primary = true;
        }
    }
    Ok(out)
}

fn xft_dpi(conn: &RustConnection, root: Window) -> Option<f32> {
    let r = conn
        .get_property(
            false,
            root,
            AtomEnum::RESOURCE_MANAGER,
            AtomEnum::STRING,
            0,
            1 << 16,
        )
        .ok()?
        .reply()
        .ok()?;
    let text = String::from_utf8_lossy(&r.value).into_owned();
    text.lines()
        .find_map(|l| l.strip_prefix("Xft.dpi:"))
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|d| *d > 0.0)
}

pub struct Panel {
    conn: RustConnection,
    root: Window,
    root_size: (i32, i32),
    window: Window,
    atoms: Atoms,
    last: Option<PanelGeometry>,
    verified_at: std::time::Instant,
    /// Wayland session: space is reserved by a layer-shell surface (KWin,
    /// wlroots) or the GNOME extension, because compositors ignore X11
    /// struts from XWayland clients.
    reserver: Option<super::reserve::LayerReserver>,
    reserver_tried: bool,
    /// Pointer inside the window, from our own Enter/Leave subscription
    /// (winit misses some XWayland leaves).
    inside: Option<bool>,
    sensor: Option<super::pointer_sensor::PointerSensor>,
    sensor_tried: bool,
    blur: Option<Option<Vec<[i32; 4]>>>,
    /// KDE: script that keeps the sidebar on all virtual desktops.
    sticky: Option<super::kwin_script::KwinScript>,
}

impl Panel {
    pub fn attach(handle: RawWindowHandle) -> Result<Panel> {
        let window = match handle {
            RawWindowHandle::Xlib(h) => h.window as Window,
            RawWindowHandle::Xcb(h) => h.window.get(),
            other => return Err(anyhow!("sidebar is not an X11 window ({other:?})")),
        };
        let (conn, screen_num) = connect()?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;
        let root_size = (
            screen.width_in_pixels as i32,
            screen.height_in_pixels as i32,
        );
        let atoms = Atoms::new(&conn)?.reply()?;
        let mut p = Panel {
            conn,
            root,
            root_size,
            window,
            atoms,
            last: None,
            verified_at: std::time::Instant::now(),
            reserver: None,
            reserver_tried: false,
            inside: None,
            sensor: None,
            sensor_tried: false,
            blur: None,
            sticky: None,
        };
        p.init()?;
        if super::is_desktop("KDE") {
            // Set up right away; the periodic check only re-tries failures.
            match super::kwin_script::sticky_sidebar() {
                Ok(s) => p.sticky = Some(s),
                Err(e) => log::warn!("cannot keep the sidebar on all desktops: {e:#}"),
            }
        }
        Ok(p)
    }

    fn init(&self) -> Result<()> {
        let a = &self.atoms;
        self.conn.change_property32(
            PropMode::REPLACE,
            self.window,
            a._NET_WM_WINDOW_TYPE,
            AtomEnum::ATOM,
            &[a._NET_WM_WINDOW_TYPE_DOCK],
        )?;
        self.set_input_hint(false)?;
        // KWin's (and Mutter's) Wayland→XWayland drag-and-drop bridge only
        // targets windows that accept input. "Globally active" (input=false
        // + WM_TAKE_FOCUS) qualifies; focus taken by a click is handed back
        // by the app (see SidestageApp::return_unwanted_focus).
        self.add_take_focus()?;
        // The window is already mapped, so state changes go through the WM.
        for state in [
            a._NET_WM_STATE_ABOVE,
            a._NET_WM_STATE_STICKY,
            a._NET_WM_STATE_SKIP_TASKBAR,
            a._NET_WM_STATE_SKIP_PAGER,
        ] {
            self.send_wm(a._NET_WM_STATE, [1, state, 0, 1, 0])?;
        }
        self.send_wm(a._NET_WM_DESKTOP, [0xFFFF_FFFF, 1, 0, 0, 0])?;
        // Also set the properties directly for WMs that read them on map.
        self.conn.change_property32(
            PropMode::REPLACE,
            self.window,
            a._NET_WM_STATE,
            AtomEnum::ATOM,
            &[
                a._NET_WM_STATE_ABOVE,
                a._NET_WM_STATE_STICKY,
                a._NET_WM_STATE_SKIP_TASKBAR,
                a._NET_WM_STATE_SKIP_PAGER,
            ],
        )?;
        self.conn.change_property32(
            PropMode::REPLACE,
            self.window,
            a._NET_WM_DESKTOP,
            AtomEnum::CARDINAL,
            &[0xFFFF_FFFF],
        )?;
        // Our own crossing events, independent of the toolkit's.
        self.conn.change_window_attributes(
            self.window,
            &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                .event_mask(EventMask::ENTER_WINDOW | EventMask::LEAVE_WINDOW),
        )?;
        self.conn.flush()?;
        Ok(())
    }

    /// Latest pointer-inside state from Enter/Leave events, if any arrived.
    pub fn pointer_inside(&mut self) -> Option<bool> {
        if let Some(v) = self.sensor.as_ref().and_then(|s| s.inside()) {
            return Some(v);
        }
        use x11rb::protocol::Event;
        while let Ok(Some(ev)) = self.conn.poll_for_event() {
            match ev {
                // Ignore crossings into/out of our own child windows.
                Event::EnterNotify(e)
                    if e.detail != x11rb::protocol::xproto::NotifyDetail::INFERIOR =>
                {
                    self.inside = Some(true)
                }
                Event::LeaveNotify(e)
                    if e.detail != x11rb::protocol::xproto::NotifyDetail::INFERIOR =>
                {
                    self.inside = Some(false)
                }
                _ => {}
            }
        }
        self.inside
    }

    fn send_wm(&self, msg_type: u32, data: [u32; 5]) -> Result<()> {
        let ev = ClientMessageEvent::new(32, self.window, msg_type, data);
        self.conn.send_event(
            false,
            self.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            ev,
        )?;
        Ok(())
    }

    /// Adds WM_TAKE_FOCUS to WM_PROTOCOLS ("globally active" input model):
    /// the WM asks before focusing us, and we simply don't accept.
    fn add_take_focus(&self) -> Result<()> {
        let a = &self.atoms;
        let mut protos: Vec<u32> = self
            .conn
            .get_property(false, self.window, a.WM_PROTOCOLS, AtomEnum::ATOM, 0, 32)?
            .reply()?
            .value32()
            .map(|v| v.collect())
            .unwrap_or_default();
        if !protos.contains(&a.WM_TAKE_FOCUS) {
            protos.push(a.WM_TAKE_FOCUS);
            self.conn.change_property32(
                PropMode::REPLACE,
                self.window,
                a.WM_PROTOCOLS,
                AtomEnum::ATOM,
                &protos,
            )?;
        }
        Ok(())
    }

    fn remove_take_focus(&self) -> Result<()> {
        let a = &self.atoms;
        let protos: Vec<u32> = self
            .conn
            .get_property(false, self.window, a.WM_PROTOCOLS, AtomEnum::ATOM, 0, 32)?
            .reply()?
            .value32()
            .map(|v| v.filter(|p| *p != a.WM_TAKE_FOCUS).collect())
            .unwrap_or_default();
        self.conn.change_property32(
            PropMode::REPLACE,
            self.window,
            a.WM_PROTOCOLS,
            AtomEnum::ATOM,
            &protos,
        )?;
        Ok(())
    }

    fn set_input_hint(&self, input: bool) -> Result<()> {
        // WM_HINTS: flags (InputHint = 1), input, initial_state, ... (9 CARD32).
        let hints = [1u32, input as u32, 1, 0, 0, 0, 0, 0, 0];
        self.conn.change_property32(
            PropMode::REPLACE,
            self.window,
            self.atoms.WM_HINTS,
            self.atoms.WM_HINTS,
            &hints,
        )?;
        Ok(())
    }

    /// The WM may assign the dock to one desktop when it manages it (KWin
    /// overwrites `_NET_WM_DESKTOP` on map); ask again until it sticks.
    fn ensure_all_desktops(&mut self) {
        if self.sticky.is_none() && super::is_desktop("KDE") {
            match super::kwin_script::sticky_sidebar() {
                Ok(s) => self.sticky = Some(s),
                Err(e) => log::warn!("cannot keep the sidebar on all desktops: {e:#}"),
            }
        }
        const ALL: u32 = 0xFFFF_FFFF;
        let current = self
            .conn
            .get_property(
                false,
                self.window,
                self.atoms._NET_WM_DESKTOP,
                AtomEnum::CARDINAL,
                0,
                1,
            )
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().and_then(|mut v| v.next()));
        if current.is_some() && current != Some(ALL) {
            log::debug!("sidebar is on desktop {current:?}; asking for all desktops");
            let _ = self.send_wm(self.atoms._NET_WM_DESKTOP, [ALL, 2, 0, 0, 0]);
            let _ = self.conn.flush();
        }
    }

    /// Actual position/size of the window (root coordinates).
    fn actual(&self) -> Option<(i32, i32, i32, i32)> {
        let geo = self.conn.get_geometry(self.window).ok()?.reply().ok()?;
        let tr = self
            .conn
            .translate_coordinates(self.window, self.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        Some((
            tr.dst_x as i32,
            tr.dst_y as i32,
            geo.width as i32,
            geo.height as i32,
        ))
    }

    pub fn apply(&mut self, g: &PanelGeometry) {
        if self.last.as_ref() == Some(g) {
            // The WM or toolkit may override our first request (e.g. when it
            // arrives before the window is managed); re-check now and then.
            if self.verified_at.elapsed() < std::time::Duration::from_millis(700) {
                return;
            }
            self.verified_at = std::time::Instant::now();
            self.ensure_all_desktops();
            if self.actual() == Some((g.x, g.y, g.width.max(1), g.height.max(1))) {
                return;
            }
            log::debug!(
                "sidebar geometry drifted to {:?}; re-applying",
                self.actual()
            );
        }
        if let Err(e) = self.apply_inner(g) {
            log::warn!("panel placement failed: {e:#}");
        }
        self.last = Some(g.clone());
    }

    fn reserve_wayland(&mut self, g: &PanelGeometry) {
        if !self.reserver_tried {
            self.reserver_tried = true;
            match super::reserve::LayerReserver::new() {
                Ok(r) => self.reserver = Some(r),
                Err(e) => log::info!(
                    "layer-shell reservation unavailable ({e:#}); trying the GNOME extension"
                ),
            }
        }
        let scale = if g.monitor.scale > 0.0 {
            g.monitor.scale
        } else {
            1.0
        };
        let logical = (g.reserve as f32 / scale).round() as u32;
        match self.reserver.as_mut() {
            Some(r) => r.set((logical > 0).then(|| super::reserve::Reservation {
                output: g.monitor.name.clone(),
                edge: g.edge,
                size: logical,
            })),
            None => super::gnome::set_reserved_space(&g.monitor.name, g.edge, logical),
        }
    }

    fn apply_inner(&mut self, g: &PanelGeometry) -> Result<()> {
        if super::wayland_display().is_some() {
            self.reserve_wayland(g);
            if g.auto_hide && !self.sensor_tried {
                self.sensor_tried = true;
                self.sensor = super::pointer_sensor::PointerSensor::new();
            }
            if let Some(sensor) = self.sensor.as_mut() {
                let s = if g.monitor.scale > 0.0 {
                    g.monitor.scale
                } else {
                    1.0
                };
                let l = |v: i32| (v as f32 / s).round() as i32;
                // Collapsed: only the edge strip reveals the sidebar.
                // Revealed: the whole sidebar keeps it open.
                let (x, w) = match g.collapsed_strip {
                    Some(strip) if g.edge == Edge::Left => (g.x, strip),
                    Some(strip) => (g.x + g.width - strip, strip),
                    None => (g.x, g.width),
                };
                sensor.set_rect(
                    g.auto_hide
                        .then(|| [l(x), l(g.y), l(w).max(1), l(g.height)]),
                );
            }
        }
        self.conn.configure_window(
            self.window,
            &ConfigureWindowAux::new()
                .x(g.x)
                .y(g.y)
                .width(g.width.max(1) as u32)
                .height(g.height.max(1) as u32),
        )?;
        // Auto-hidden: keep the window size (resizing an XWayland dock under
        // the pointer confuses pointer focus) and narrow the input shape.
        match g.collapsed_strip {
            _ if g.paused => {
                // No input at all while a fullscreen app owns the monitor.
                self.conn.shape_rectangles(
                    shape::SO::SET,
                    shape::SK::INPUT,
                    x11rb::protocol::xproto::ClipOrdering::UNSORTED,
                    self.window,
                    0,
                    0,
                    &[],
                )?;
            }
            Some(strip) => {
                let x = if g.edge == Edge::Left {
                    0
                } else {
                    (g.width - strip) as i16
                };
                let rect = x11rb::protocol::xproto::Rectangle {
                    x,
                    y: 0,
                    width: strip.max(1) as u16,
                    height: g.height.max(1) as u16,
                };
                self.conn.shape_rectangles(
                    shape::SO::SET,
                    shape::SK::INPUT,
                    x11rb::protocol::xproto::ClipOrdering::UNSORTED,
                    self.window,
                    0,
                    0,
                    &[rect],
                )?;
            }
            None => {
                self.conn.shape_mask(
                    shape::SO::SET,
                    shape::SK::INPUT,
                    self.window,
                    0,
                    0,
                    x11rb::NONE,
                )?;
            }
        }
        let mut partial = [0u32; 12];
        let m = &g.monitor;
        if g.reserve > 0 && super::wayland_display().is_none() {
            // Struts are measured from the edge of the whole X screen, so a
            // sidebar can only reserve space on an outer monitor edge.
            match g.edge {
                Edge::Left if m.x == 0 => {
                    partial[0] = (m.x + g.reserve) as u32;
                    partial[4] = m.y as u32;
                    partial[5] = (m.y + m.height - 1) as u32;
                }
                Edge::Right if m.x + m.width == self.root_size.0 => {
                    partial[1] = g.reserve as u32;
                    partial[6] = m.y as u32;
                    partial[7] = (m.y + m.height - 1) as u32;
                }
                _ => log::info!("not reserving space: sidebar edge is between two monitors"),
            }
        }
        let a = &self.atoms;
        self.conn.change_property32(
            PropMode::REPLACE,
            self.window,
            a._NET_WM_STRUT_PARTIAL,
            AtomEnum::CARDINAL,
            &partial,
        )?;
        self.conn.change_property32(
            PropMode::REPLACE,
            self.window,
            a._NET_WM_STRUT,
            AtomEnum::CARDINAL,
            &partial[..4],
        )?;
        self.conn.flush()?;
        Ok(())
    }

    pub fn set_keyboard_focus(&mut self, wanted: bool) {
        let r = (|| -> Result<()> {
            // Passive model (input=true, no WM_TAKE_FOCUS) while typing so
            // the WM really gives us the keyboard; globally active otherwise.
            if wanted {
                self.remove_take_focus()?;
            } else {
                self.add_take_focus()?;
            }
            self.set_input_hint(wanted)?;
            if wanted {
                // Source indication 2 = pager/taskbar: honoured without
                // focus-stealing prevention because the user clicked us.
                self.send_wm(self.atoms._NET_ACTIVE_WINDOW, [2, 0, 0, 0, 0])?;
            }
            self.conn.flush()?;
            Ok(())
        })();
        if let Err(e) = r {
            log::warn!("focus change failed: {e:#}");
        }
    }

    /// Window shaping is a Windows feature; Linux narrows the input shape
    /// only while auto-hidden.
    pub fn set_shape(&mut self, _rects: Option<Vec<[i32; 4]>>) {}

    /// Linux uses the toolkit's (XDND) file-drop events.
    pub fn poll_drag(&mut self) -> Option<crate::panel::NativeDrag> {
        None
    }

    pub fn set_drag_waker(&mut self, _waker: crate::Waker) {}

    /// KWin's blur effect reads `_KDE_NET_WM_BLUR_BEHIND_REGION` (also for
    /// XWayland windows). An empty value would blur the whole window, so
    /// "off" deletes the property.
    pub fn set_blur_region(&mut self, rects: Option<Vec<[i32; 4]>>) {
        if self.blur.as_ref() == Some(&rects) {
            return;
        }
        let atom = self.atoms._KDE_NET_WM_BLUR_BEHIND_REGION;
        let r = match &rects {
            Some(v) if !v.is_empty() => {
                let data: Vec<u32> = v
                    .iter()
                    .flat_map(|r| r.iter().map(|x| (*x).max(0) as u32))
                    .collect();
                self.conn
                    .change_property32(
                        PropMode::REPLACE,
                        self.window,
                        atom,
                        AtomEnum::CARDINAL,
                        &data,
                    )
                    .map(|_| ())
            }
            _ => self.conn.delete_property(self.window, atom).map(|_| ()),
        };
        let _ = r;
        // The same region is the input shape while revealed, so clicks
        // outside the curved outline reach the windows underneath.
        if self
            .last
            .as_ref()
            .map(|g| g.collapsed_strip.is_none())
            .unwrap_or(true)
        {
            let shaped = match &rects {
                Some(v) if !v.is_empty() => {
                    let list: Vec<x11rb::protocol::xproto::Rectangle> = v
                        .iter()
                        .map(|r| x11rb::protocol::xproto::Rectangle {
                            x: r[0] as i16,
                            y: r[1] as i16,
                            width: r[2].max(1) as u16,
                            height: r[3].max(1) as u16,
                        })
                        .collect();
                    self.conn.shape_rectangles(
                        shape::SO::SET,
                        shape::SK::INPUT,
                        x11rb::protocol::xproto::ClipOrdering::UNSORTED,
                        self.window,
                        0,
                        0,
                        &list,
                    )
                }
                _ => self.conn.shape_mask(
                    shape::SO::SET,
                    shape::SK::INPUT,
                    self.window,
                    0,
                    0,
                    x11rb::NONE,
                ),
            };
            let _ = shaped;
        }
        let _ = self.conn.flush();
        self.blur = Some(rects);
    }

    pub fn live_thumbnails(&mut self) -> Option<&mut dyn LiveThumbnails> {
        None
    }

    /// Globally-active input model: once the WM has activated us, the
    /// client sets X input focus itself.
    pub fn claim_keyboard(&mut self) {
        let _ = self.conn.set_input_focus(
            x11rb::protocol::xproto::InputFocus::PARENT,
            self.window,
            x11rb::CURRENT_TIME,
        );
        let _ = self.conn.flush();
    }

    pub fn detach(&mut self) {
        self.sticky = None;
        if let Some(r) = self.reserver.as_mut() {
            r.set(None);
        } else if super::wayland_display().is_some() {
            super::gnome::set_reserved_space("", Edge::Left, 0);
        }
        let a = &self.atoms;
        let _ = self
            .conn
            .delete_property(self.window, a._NET_WM_STRUT_PARTIAL);
        let _ = self.conn.delete_property(self.window, a._NET_WM_STRUT);
        let _ = self.conn.flush();
    }
}
