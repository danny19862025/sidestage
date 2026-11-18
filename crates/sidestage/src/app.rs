//! Application state and per-frame logic (events, placement, auto-hide,
//! persistence). Drawing lives in `sidebar.rs` and `settings.rs`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use raw_window_handle::HasWindowHandle;
use sidestage_core::config::{Edge, MonitorChoice, Theme};
use sidestage_core::tracker::BackendStatus;
use sidestage_core::{Config, WindowEvent, WindowGroup, WindowId, WindowTracker};
use sidestage_platform::panel::{MonitorInfo, Panel, PanelGeometry};
use sidestage_platform::{ThumbnailMode, WindowBackend};

use crate::settings::SettingsUi;
use crate::textures::Textures;
use crate::theme::{self, Palette};
use crate::tray::{Tray, TrayCommand};

/// Width of the strip left at the screen edge while auto-hidden (logical px).
pub const HIDDEN_STRIP: f32 = 6.0;
/// Gap between the floating card and the window edges (logical px).
pub const OUTER_MARGIN: f32 = 8.0;

#[derive(Default)]
pub struct AutoHide {
    /// Sidebar fully revealed (or revealing).
    pub shown: bool,
    pub hover_since: Option<Instant>,
    pub leave_since: Option<Instant>,
}

pub struct Toast {
    pub text: String,
    pub error: bool,
    pub until: Instant,
}

pub struct SidestageApp {
    pub config: Config,
    config_path: PathBuf,
    dirty_since: Option<Instant>,

    pub tracker: WindowTracker,
    pub groups: Vec<WindowGroup>,
    groups_gen: u64,
    events: Receiver<WindowEvent>,
    pub backend: Box<dyn WindowBackend>,
    pub status: Option<BackendStatus>,
    pub thumbnail_mode: ThumbnailMode,

    pub textures: Textures,
    pub palette: Palette,
    system_dark: Arc<AtomicU8>,
    applied_dark: Option<bool>,

    pub panel: Option<Panel>,
    panel_failed: bool,
    pub monitors: Vec<MonitorInfo>,
    monitors_at: Instant,
    pub scale: f32,
    last_geometry: Option<PanelGeometry>,

    pub autohide: AutoHide,
    /// Hidden from the tray menu.
    pub visible: bool,
    pub keyboard_wanted: bool,
    keyboard_granted: bool,

    pub search: String,
    pub search_open: bool,
    pub search_focus_request: bool,
    pub expanded_groups: HashSet<String>,
    pub renaming_section: Option<(String, String)>,
    pub pending_restore: HashMap<WindowId, Instant>,
    pub launched: HashMap<String, Instant>,
    pub toast: Option<Toast>,
    pub section_rects: Vec<(String, egui::Rect)>,
    pub menu_open: bool,

    pub settings_open: bool,
    pub settings: SettingsUi,

    tray_rx: Receiver<TrayCommand>,
    extra_rx: Option<Receiver<TrayCommand>>,
    _tray: Tray,
    pub quitting: bool,
}

impl SidestageApp {
    pub fn new(cc: &eframe::CreationContext<'_>, open_settings: bool) -> Self {
        let ctx = cc.egui_ctx.clone();
        let config_path = Config::default_path();
        let mut config = Config::load(&config_path);
        // The autostart entry is the source of truth for launch at login.
        config.settings.launch_at_login = sidestage_platform::autostart::is_enabled();

        let (tx, events) = mpsc::channel();
        let wake_ctx = ctx.clone();
        let backend = sidestage_platform::start_window_backend(tx, Arc::new(move || wake_ctx.request_repaint()));
        log::info!("window backend: {} ({:?})", backend.name(), backend.thumbnail_mode());
        let thumbnail_mode = backend.thumbnail_mode();

        // Poll the desktop colour scheme in the background (D-Bus/registry).
        let system_dark = Arc::new(AtomicU8::new(2));
        {
            let flag = system_dark.clone();
            let ctx = ctx.clone();
            std::thread::Builder::new()
                .name("theme-watch".into())
                .spawn(move || loop {
                    let v = match sidestage_platform::system_prefers_dark() {
                        Some(true) => 1,
                        Some(false) => 0,
                        None => 2,
                    };
                    if flag.swap(v, Ordering::Relaxed) != v {
                        ctx.request_repaint();
                    }
                    std::thread::sleep(Duration::from_secs(5));
                })
                .ok();
        }

        let (tray_tx, tray_rx) = mpsc::channel();
        let tray = Tray::new(tray_tx, ctx.clone());

        let palette = Palette::new(true);
        theme::apply(&ctx, &palette);

        Self {
            config,
            config_path,
            dirty_since: None,
            tracker: WindowTracker::new(),
            groups: Vec::new(),
            groups_gen: u64::MAX,
            events,
            backend,
            status: None,
            thumbnail_mode,
            textures: Textures::new(ctx.clone()),
            palette,
            system_dark,
            applied_dark: None,
            panel: None,
            panel_failed: false,
            monitors: sidestage_platform::panel::monitors(),
            monitors_at: Instant::now(),
            scale: 1.0,
            last_geometry: None,
            autohide: AutoHide::default(),
            visible: true,
            keyboard_wanted: false,
            keyboard_granted: false,
            search: String::new(),
            search_open: false,
            search_focus_request: false,
            expanded_groups: HashSet::new(),
            renaming_section: None,
            pending_restore: HashMap::new(),
            launched: HashMap::new(),
            toast: None,
            section_rects: Vec::new(),
            menu_open: false,
            settings_open: open_settings,
            settings: SettingsUi::default(),
            tray_rx,
            extra_rx: None,
            _tray: tray,
            quitting: false,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty_since.get_or_insert_with(Instant::now);
    }

    pub fn save_now(&mut self) {
        self.dirty_since = None;
        if let Err(e) = self.config.save(&self.config_path) {
            log::error!("saving config: {e:#}");
            self.toast(format!("Could not save settings: {e}"), true);
        }
    }

    pub fn config_path(&self) -> &std::path::Path {
        &self.config_path
    }

    pub fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toast = Some(Toast { text: text.into(), error, until: Instant::now() + Duration::from_secs(if error { 6 } else { 3 }) });
    }

    pub fn launch(&mut self, favourite_id: &str) {
        let Some(fav) = self.config.favourite(favourite_id).cloned() else { return };
        log::info!("launching {} ({:?})", fav.name, fav.target);
        match sidestage_platform::launch::launch(&fav.target) {
            Ok(()) => {
                self.launched.insert(fav.id.clone(), Instant::now());
            }
            Err(e) => {
                log::warn!("launch failed: {e:#}");
                self.toast(format!("Couldn't open {}: {e}", fav.name), true);
            }
        }
    }

    pub fn restore(&mut self, id: &WindowId) {
        log::info!("restoring {id}");
        self.pending_restore.insert(id.clone(), Instant::now());
        self.backend.restore(id);
    }

    pub fn dark(&self) -> bool {
        match self.config.settings.theme {
            Theme::Light => false,
            Theme::Dark => true,
            Theme::System => self.system_dark.load(Ordering::Relaxed) != 0,
        }
    }

    fn drain_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.events.try_recv() {
            match &ev {
                WindowEvent::Icon { app_key, image } => self.textures.set_app_icon(ctx, app_key, image),
                WindowEvent::Thumbnail { id, image } => self.textures.set_thumbnail(ctx, id, image.as_ref()),
                WindowEvent::Status(s) => {
                    log::info!("backend status: {s:?}");
                    self.status = Some(s.clone());
                }
                WindowEvent::Upsert(w) if !w.minimized => {
                    self.pending_restore.remove(&w.id);
                }
                WindowEvent::Removed(id) => {
                    self.pending_restore.remove(id);
                }
                _ => {}
            }
            self.tracker.apply(&ev);
        }
        if self.tracker.generation() != self.groups_gen {
            self.groups_gen = self.tracker.generation();
            self.groups = self.tracker.groups();
            let tracker = &self.tracker;
            self.textures.retain_thumbnails(|id| tracker.get(id).map(|w| w.minimized).unwrap_or(false));
            let keys: HashSet<&str> = self.groups.iter().map(|g| g.app_key.as_str()).collect();
            self.expanded_groups.retain(|k| keys.contains(k.as_str()));
        }
        // A restore that did not happen within 4s is shown normally again.
        self.pending_restore.retain(|_, t| t.elapsed() < Duration::from_secs(4));
        self.launched.retain(|_, t| t.elapsed() < Duration::from_millis(900));
    }

    /// Commands from another source (second instance launch).
    pub fn extra_commands(&mut self, rx: Receiver<TrayCommand>) {
        self.extra_rx = Some(rx);
    }

    fn handle_tray(&mut self, ctx: &egui::Context) {
        let mut cmds: Vec<TrayCommand> = self.tray_rx.try_iter().collect();
        if let Some(rx) = &self.extra_rx {
            cmds.extend(rx.try_iter());
        }
        for cmd in cmds {
            match cmd {
                TrayCommand::Settings => {
                    self.settings_open = true;
                    ctx.send_viewport_cmd_to(crate::settings::viewport_id(), egui::ViewportCommand::Focus);
                }
                TrayCommand::ToggleSidebar => {
                    self.visible = !self.visible;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.visible));
                    if self.visible {
                        // Window managers may forget dock hints on unmap.
                        self.panel = None;
                    }
                }
                TrayCommand::Quit => self.quit(ctx),
            }
        }
    }

    pub fn quit(&mut self, ctx: &egui::Context) {
        self.quitting = true;
        if let Some(p) = self.panel.as_mut() {
            p.detach();
        }
        if self.dirty_since.is_some() {
            self.save_now();
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    pub fn target_monitor(&self) -> Option<&MonitorInfo> {
        let primary = || self.monitors.iter().find(|m| m.primary).or(self.monitors.first());
        match &self.config.settings.monitor {
            MonitorChoice::Primary => primary(),
            MonitorChoice::Named { name, index } => self
                .monitors
                .iter()
                .find(|m| &m.name == name)
                .or_else(|| self.monitors.get(*index))
                .or_else(primary),
        }
    }

    /// Whether the sidebar should currently be fully shown.
    fn wants_shown(&self) -> bool {
        !self.config.settings.auto_hide || self.autohide.shown
    }

    fn update_autohide(&mut self, ctx: &egui::Context) {
        let s = &self.config.settings;
        if !s.auto_hide {
            self.autohide = AutoHide { shown: true, ..Default::default() };
            return;
        }
        let toolkit_hover = ctx.input(|i| i.pointer.hover_pos().is_some());
        let files = ctx.input(|i| !i.raw.hovered_files.is_empty());
        // XWayland may not report the pointer leaving to a Wayland window;
        // the panel's own Enter/Leave tracking wins when it has an answer.
        let platform_hover = self.panel.as_mut().and_then(|p| p.pointer_inside());
        let hovering = files || platform_hover.unwrap_or(toolkit_hover);
        let busy = self.keyboard_wanted || self.menu_open || ctx.dragged_id().is_some() || self.renaming_section.is_some();
        let now = Instant::now();
        log::trace!(
            "autohide hovering={hovering} platform={platform_hover:?} pos={:?} busy={busy} (kbd={} menu={} drag={}) shown={}",
            ctx.input(|i| i.pointer.latest_pos()),
            self.keyboard_wanted,
            self.menu_open,
            ctx.dragged_id().is_some(),
            self.autohide.shown
        );
        if self.autohide.shown {
            // Leave events can arrive while egui is idle; keep checking.
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if hovering || busy {
            self.autohide.leave_since = None;
            if !self.autohide.shown {
                let since = *self.autohide.hover_since.get_or_insert(now);
                if since.elapsed() >= Duration::from_millis(s.reveal_delay_ms as u64) {
                    self.autohide.shown = true;
                } else {
                    ctx.request_repaint_after(Duration::from_millis(30));
                }
            }
        } else {
            self.autohide.hover_since = None;
            if self.autohide.shown {
                ctx.request_repaint_after(Duration::from_millis(120));
                let since = *self.autohide.leave_since.get_or_insert(now);
                if since.elapsed() >= Duration::from_millis(550) {
                    self.autohide.shown = false;
                } else {
                    ctx.request_repaint_after(Duration::from_millis(60));
                }
            }
        }
    }

    /// Reveal progress 0..1, animated.
    pub fn reveal(&self, ctx: &egui::Context) -> f32 {
        let anim = if self.config.settings.animations { 0.18 } else { 0.0 };
        ctx.animate_bool_with_time(egui::Id::unique("reveal"), self.wants_shown(), anim)
    }

    fn place_panel(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if self.monitors_at.elapsed() > Duration::from_secs(4) || self.monitors.is_empty() {
            self.monitors = sidestage_platform::panel::monitors();
            self.monitors_at = Instant::now();
        }
        if self.panel.is_none() && !self.panel_failed {
            match frame.window_handle().map(|h| h.as_raw()) {
                Ok(raw) => match Panel::attach(raw) {
                    Ok(p) => self.panel = Some(p),
                    Err(e) => {
                        log::error!("cannot manage sidebar window: {e:#}");
                        self.panel_failed = true;
                    }
                },
                Err(e) => log::debug!("window handle not ready: {e}"),
            }
        }
        let Some(mon) = self.target_monitor().cloned() else {
            if !self.panel_failed {
                log::warn!("no monitors reported; leaving the sidebar where it is");
                self.panel_failed = self.panel.is_none();
            }
            return;
        };
        self.scale = if mon.scale > 0.0 { mon.scale } else { ctx.native_pixels_per_point().unwrap_or(1.0) };
        let s = &self.config.settings;
        let full_w = ((s.width + OUTER_MARGIN) * self.scale).round() as i32;
        let reveal = self.reveal(ctx);
        let collapsed = s.auto_hide && reveal <= 0.001 && !self.autohide.shown;
        let w = full_w;
        let collapsed_strip = collapsed.then(|| (HIDDEN_STRIP * self.scale).round().max(2.0) as i32);
        let (_, wy, _, wh) = mon.work;
        let x = match s.edge {
            Edge::Left => mon.x,
            Edge::Right => mon.x + mon.width - w,
        };
        let reserve = if s.auto_hide || !s.reserve_space { 0 } else { full_w };
        let geometry = PanelGeometry { monitor: mon, edge: s.edge, x, y: wy, width: w, height: wh, reserve, collapsed_strip, auto_hide: s.auto_hide };
        if let Some(p) = self.panel.as_mut() {
            if self.last_geometry.as_ref() != Some(&geometry) {
                log::debug!("panel geometry {geometry:?}");
                self.last_geometry = Some(geometry.clone());
            }
            p.apply(&geometry);
        }
    }

    fn sync_keyboard(&mut self) {
        if self.keyboard_wanted != self.keyboard_granted {
            self.keyboard_granted = self.keyboard_wanted;
            if let Some(p) = self.panel.as_mut() {
                p.set_keyboard_focus(self.keyboard_wanted);
            }
        }
    }
}

impl eframe::App for SidestageApp {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let dark = self.dark();
        if self.applied_dark != Some(dark) {
            self.applied_dark = Some(dark);
            self.palette = Palette::new(dark);
            theme::apply(ctx, &self.palette);
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            // The sidebar has no close button; a WM close means quit.
            self.quit(ctx);
        }
        self.textures.poll(ctx);
        self.drain_events(ctx);
        self.handle_tray(ctx);
        self.update_autohide(ctx);
        self.place_panel(ctx, frame);
        self.sync_keyboard();
        if let Some(t) = self.dirty_since {
            if t.elapsed() > Duration::from_millis(400) {
                self.save_now();
            } else {
                ctx.request_repaint_after(Duration::from_millis(450));
            }
        }
        if let Some(t) = &self.toast {
            if Instant::now() > t.until {
                self.toast = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
        if !self.pending_restore.is_empty() || !self.launched.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        crate::sidebar::show(self, ui);
        if self.settings_open {
            crate::settings::show(self, ui.ctx());
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(p) = self.panel.as_mut() {
            p.detach();
        }
        if self.dirty_since.is_some() {
            self.save_now();
        }
    }
}
