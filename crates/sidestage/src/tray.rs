//! System tray / status-notifier menu with Settings and Quit.

use std::sync::mpsc::Sender;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCommand {
    Settings,
    ToggleSidebar,
    Quit,
}

pub struct Tray {
    #[cfg(target_os = "linux")]
    _handle: Option<ksni::blocking::Handle<LinuxTray>>,
    #[cfg(windows)]
    _icon: Option<tray_icon::TrayIcon>,
}

#[cfg(target_os = "linux")]
pub struct LinuxTray {
    tx: Sender<TrayCommand>,
    ctx: egui::Context,
}

#[cfg(target_os = "linux")]
impl LinuxTray {
    fn send(&self, c: TrayCommand) {
        let _ = self.tx.send(c);
        self.ctx.request_repaint();
    }
}

#[cfg(target_os = "linux")]
impl ksni::Tray for LinuxTray {
    fn id(&self) -> String {
        "sidestage".into()
    }
    fn title(&self) -> String {
        "Sidestage".into()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        [22u32, 32, 64]
            .iter()
            .map(|&s| {
                let img = crate::brand::app_icon(s);
                // SNI wants ARGB32 in network byte order.
                let data = img.pixels.as_chunks::<4>().0.iter().flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
                ksni::Icon { width: s as i32, height: s as i32, data }
            })
            .collect()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: "Sidestage".into(), description: "Favourites and minimized windows".into(), ..Default::default() }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayCommand::ToggleSidebar);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: "Show / hide sidebar".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayCommand::ToggleSidebar)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Settings…".into(),
                icon_name: "configure".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayCommand::Settings)),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Quit Sidestage".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayCommand::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

impl Tray {
    #[cfg(target_os = "linux")]
    pub fn new(tx: Sender<TrayCommand>, ctx: egui::Context) -> Tray {
        use ksni::blocking::TrayMethods;
        let handle = match (LinuxTray { tx, ctx }).spawn() {
            Ok(h) => Some(h),
            Err(e) => {
                // GNOME needs the AppIndicator extension for SNI; the
                // Sidestage GNOME extension also offers a panel menu.
                log::warn!("system tray unavailable: {e}");
                None
            }
        };
        Tray { _handle: handle }
    }

    #[cfg(windows)]
    pub fn new(tx: Sender<TrayCommand>, ctx: egui::Context) -> Tray {
        use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
        let menu = Menu::new();
        let toggle = MenuItem::new("Show / hide sidebar", true, None);
        let settings = MenuItem::new("Settings…", true, None);
        let quit = MenuItem::new("Quit Sidestage", true, None);
        let _ = menu.append_items(&[&toggle, &settings, &PredefinedMenuItem::separator(), &quit]);
        let (toggle_id, settings_id, quit_id) = (toggle.id().clone(), settings.id().clone(), quit.id().clone());
        let menu_tx = tx.clone();
        let menu_ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let cmd = if e.id == toggle_id {
                TrayCommand::ToggleSidebar
            } else if e.id == settings_id {
                TrayCommand::Settings
            } else if e.id == quit_id {
                TrayCommand::Quit
            } else {
                return;
            };
            let _ = menu_tx.send(cmd);
            menu_ctx.request_repaint();
        }));
        tray_icon::TrayIconEvent::set_event_handler(Some(move |e: tray_icon::TrayIconEvent| {
            if let tray_icon::TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                button_state: tray_icon::MouseButtonState::Up,
                ..
            } = e
            {
                let _ = tx.send(TrayCommand::ToggleSidebar);
                ctx.request_repaint();
            }
        }));
        let img = crate::brand::app_icon(32);
        let icon = tray_icon::Icon::from_rgba(img.pixels, img.width, img.height).ok();
        let mut builder = tray_icon::TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("Sidestage");
        if let Some(icon) = icon {
            builder = builder.with_icon(icon);
        }
        let icon = match builder.build() {
            Ok(t) => Some(t),
            Err(e) => {
                log::warn!("tray icon unavailable: {e}");
                None
            }
        };
        Tray { _icon: icon }
    }
}
