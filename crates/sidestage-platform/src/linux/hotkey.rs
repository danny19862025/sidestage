//! Global shortcut backends for Linux (see `crate::hotkey`).

use std::sync::{Arc, Mutex};

use anyhow::Result;
use sidestage_core::shortcut::Shortcut;

use crate::hotkey::{HotkeyStatus, OnPress};

const COMPONENT: &str = "sidestage";
const ACTION: &str = "search";

fn action_id() -> Vec<&'static str> {
    vec![
        COMPONENT,
        ACTION,
        "Sidestage",
        "Search favourites and windows",
    ]
}

enum Kind {
    Kde(zbus::blocking::Connection),
    Gnome(zbus::blocking::Connection),
    X11(X11Grab),
    None(String),
}

pub struct Hotkey {
    kind: Kind,
    on_press: OnPress,
    listening: bool,
    /// Current GNOME accelerator, re-applied if the extension restarts.
    gnome_accel: Arc<Mutex<Option<String>>>,
    /// Status changes found by the GNOME listener (extension enabled or
    /// restarted after Sidestage started).
    async_status: Arc<Mutex<Option<HotkeyStatus>>>,
}

impl Hotkey {
    pub fn new(on_press: OnPress) -> Hotkey {
        let kind = Self::detect();
        Hotkey {
            kind,
            on_press,
            listening: false,
            gnome_accel: Arc::new(Mutex::new(None)),
            async_status: Arc::new(Mutex::new(None)),
        }
    }

    fn detect() -> Kind {
        let bus = zbus::blocking::Connection::session().ok();
        let has = |name: &str| {
            bus.as_ref()
                .and_then(|c| zbus::blocking::fdo::DBusProxy::new(c).ok())
                .and_then(|p| p.name_has_owner(name.try_into().ok()?).ok())
                .unwrap_or(false)
        };
        if super::is_desktop("KDE") && has("org.kde.kglobalaccel") {
            return Kind::Kde(bus.unwrap());
        }
        if super::is_desktop("GNOME") {
            if let Some(c) = bus {
                return Kind::Gnome(c);
            }
        }
        if super::wayland_display().is_none() {
            match X11Grab::new() {
                Ok(g) => return Kind::X11(g),
                Err(e) => return Kind::None(format!("X11 key grab unavailable: {e:#}")),
            }
        }
        Kind::None("This desktop has no global-shortcut service Sidestage can verify. Use the search button in the sidebar.".into())
    }

    pub fn set(&mut self, s: Option<Shortcut>) -> HotkeyStatus {
        let r = match &self.kind {
            Kind::Kde(c) => kde_set(c, s),
            Kind::Gnome(c) => {
                *self.gnome_accel.lock().unwrap() = s.map(|s| s.to_gnome());
                gnome_set(c, s.map(|s| s.to_gnome()))
            }
            Kind::X11(g) => Ok(g.set(s)),
            Kind::None(msg) => {
                return if s.is_some() {
                    HotkeyStatus::Unsupported(msg.clone())
                } else {
                    HotkeyStatus::Disabled
                }
            }
        };
        let status = match r {
            Ok(st) => st,
            Err(e) => HotkeyStatus::Unsupported(format!("{e:#}")),
        };
        // GNOME: listen even when the extension is missing right now, so the
        // shortcut is registered as soon as it appears.
        let gnome_waiting = matches!(self.kind, Kind::Gnome(_)) && s.is_some();
        if (matches!(status, HotkeyStatus::Registered(_)) || gnome_waiting) && !self.listening {
            self.listening = true;
            self.listen();
        }
        status
    }

    /// A newer status found in the background, if any.
    pub fn take_status_update(&self) -> Option<HotkeyStatus> {
        self.async_status.lock().unwrap().take()
    }

    fn listen(&self) {
        let cb = self.on_press.clone();
        match &self.kind {
            Kind::Kde(c) => {
                let c = c.clone();
                std::thread::Builder::new()
                    .name("kglobalaccel".into())
                    .spawn(move || {
                        let r = (|| -> Result<()> {
                            let path = format!("/component/{COMPONENT}");
                            let p = zbus::blocking::Proxy::new(
                                &c,
                                "org.kde.kglobalaccel",
                                path.as_str(),
                                "org.kde.kglobalaccel.Component",
                            )?;
                            for msg in p.receive_signal("globalShortcutPressed")? {
                                let (_comp, action, _ts): (String, String, i64) =
                                    msg.body().deserialize()?;
                                if action == ACTION {
                                    cb();
                                }
                            }
                            Ok(())
                        })();
                        if let Err(e) = r {
                            log::warn!("KDE shortcut listener stopped: {e:#}");
                        }
                    })
                    .ok();
            }
            Kind::Gnome(c) => {
                let (c, accel, status) = (
                    c.clone(),
                    self.gnome_accel.clone(),
                    self.async_status.clone(),
                );
                std::thread::Builder::new()
                    .name("gnome-shortcut".into())
                    .spawn(move || gnome_listen(c, accel, status, cb))
                    .ok();
            }
            Kind::X11(g) => g.listen(cb),
            Kind::None(_) => {}
        }
    }
}

impl Drop for Hotkey {
    fn drop(&mut self) {
        if let Kind::Kde(c) = &self.kind {
            // Keep the user's binding but tell KDE we're gone.
            let _ = zbus::blocking::Proxy::new(
                c,
                "org.kde.kglobalaccel",
                "/kglobalaccel",
                "org.kde.KGlobalAccel",
            )
            .and_then(|p| p.call::<_, _, ()>("setInactive", &(action_id(),)));
        }
    }
}

fn kde_set(c: &zbus::blocking::Connection, s: Option<Shortcut>) -> Result<HotkeyStatus> {
    let p = zbus::blocking::Proxy::new(
        c,
        "org.kde.kglobalaccel",
        "/kglobalaccel",
        "org.kde.KGlobalAccel",
    )?;
    p.call::<_, _, ()>("doRegister", &(action_id(),))?;
    // Flags: SetPresent (2) | NoAutoloading (4): use exactly these keys.
    let Some(s) = s else {
        let _: Vec<i32> = p.call("setShortcut", &(action_id(), Vec::<i32>::new(), 6u32))?;
        return Ok(HotkeyStatus::Disabled);
    };
    let key = s.to_qt();
    let available: bool = p.call("isGlobalShortcutAvailable", &(key, COMPONENT))?;
    if !available {
        type Info = (
            String,
            String,
            String,
            String,
            String,
            String,
            Vec<i32>,
            Vec<i32>,
        );
        let owners: Vec<Info> = p
            .call("getGlobalShortcutsByKey", &(key,))
            .unwrap_or_default();
        let who = owners
            .iter()
            .find(|o| !(o.2 == COMPONENT && o.0 == ACTION))
            .map(|o| format!("{} → {}", o.3, o.1))
            .unwrap_or_else(|| "another shortcut".into());
        if owners.iter().any(|o| !(o.2 == COMPONENT && o.0 == ACTION)) {
            return Ok(HotkeyStatus::Conflict(format!(
                "{} is already used by {who}",
                s.display()
            )));
        }
    }
    let assigned: Vec<i32> = p.call("setShortcut", &(action_id(), vec![key], 6u32))?;
    if assigned.first() != Some(&key) {
        return Ok(HotkeyStatus::Conflict(format!(
            "KDE did not accept {} (it may be reserved)",
            s.display()
        )));
    }
    Ok(HotkeyStatus::Registered(format!(
        "{} via KDE global shortcuts",
        s.display()
    )))
}

fn gnome_set(c: &zbus::blocking::Connection, accel: Option<String>) -> Result<HotkeyStatus> {
    let p = zbus::blocking::Proxy::new(
        c,
        "org.sidestage.Shell",
        "/org/sidestage/Shell",
        "org.sidestage.Shell1",
    )?;
    let r: zbus::Result<(bool, String)> =
        p.call("SetShortcut", &(accel.clone().unwrap_or_default(),));
    match r {
        Ok((true, _)) if accel.is_none() => Ok(HotkeyStatus::Disabled),
        Ok((true, msg)) => Ok(HotkeyStatus::Registered(msg)),
        Ok((false, msg)) => Ok(HotkeyStatus::Conflict(msg)),
        Err(_) => Ok(HotkeyStatus::Unsupported(
            "Enable the Sidestage GNOME Shell extension for a global shortcut. The search button still works.".into(),
        )),
    }
}

fn gnome_listen(
    c: zbus::blocking::Connection,
    accel: Arc<Mutex<Option<String>>>,
    status: Arc<Mutex<Option<HotkeyStatus>>>,
    cb: OnPress,
) {
    // Re-applies the accelerator when the extension (re)appears, and
    // forwards activations.
    let mut owner: Option<String> = None;
    loop {
        let now = zbus::blocking::fdo::DBusProxy::new(&c)
            .ok()
            .and_then(|p| {
                p.get_name_owner("org.sidestage.Shell".try_into().ok()?)
                    .ok()
            })
            .map(|n| n.to_string());
        if now.is_some() && now != owner {
            owner = now.clone();
            let current = accel.lock().unwrap().clone();
            if current.is_some() {
                let st = gnome_set(&c, current)
                    .unwrap_or_else(|e| HotkeyStatus::Unsupported(format!("{e:#}")));
                log::info!("GNOME extension available: search shortcut {st:?}");
                *status.lock().unwrap() = Some(st);
            }
            let (c2, cb2) = (c.clone(), cb.clone());
            std::thread::spawn(move || {
                let r = (|| -> Result<()> {
                    let p = zbus::blocking::Proxy::new(
                        &c2,
                        "org.sidestage.Shell",
                        "/org/sidestage/Shell",
                        "org.sidestage.Shell1",
                    )?;
                    for _ in p.receive_signal("ShortcutActivated")? {
                        cb2();
                    }
                    Ok(())
                })();
                if let Err(e) = r {
                    log::debug!("GNOME shortcut signal stream: {e:#}");
                }
            });
        } else if now.is_none() && owner.is_some() {
            owner = None;
            if accel.lock().unwrap().is_some() {
                *status.lock().unwrap() = Some(HotkeyStatus::Unsupported(
                    "The Sidestage GNOME extension stopped; the shortcut returns when it is enabled again.".into(),
                ));
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
}

// ---------------------------------------------------------------------------
// X11 XGrabKey

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, GrabMode, ModMask};

struct X11Grab {
    conn: Arc<x11rb::rust_connection::RustConnection>,
    root: u32,
    current: Mutex<Option<(u8, u16)>>,
}

impl X11Grab {
    fn new() -> Result<X11Grab> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        Ok(X11Grab {
            conn: Arc::new(conn),
            root,
            current: Mutex::new(None),
        })
    }

    fn keycode(&self, keysym: u32) -> Option<u8> {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let map = self
            .conn
            .get_keyboard_mapping(min, max - min + 1)
            .ok()?
            .reply()
            .ok()?;
        let per = map.keysyms_per_keycode as usize;
        map.keysyms
            .chunks(per)
            .position(|syms| syms.contains(&keysym))
            .map(|i| min + i as u8)
    }

    /// Grab variants so Caps Lock / Num Lock don't break the shortcut.
    fn variants(mods: u16) -> [u16; 4] {
        let lock = u16::from(ModMask::LOCK);
        let num = u16::from(ModMask::M2);
        [mods, mods | lock, mods | num, mods | lock | num]
    }

    fn set(&self, s: Option<Shortcut>) -> HotkeyStatus {
        let mut cur = self.current.lock().unwrap();
        if let Some((kc, mods)) = cur.take() {
            for m in Self::variants(mods) {
                let _ = self.conn.ungrab_key(kc, self.root, ModMask::from(m));
            }
        }
        let Some(s) = s else {
            let _ = self.conn.flush();
            return HotkeyStatus::Disabled;
        };
        let (mods, sym) = s.to_x11();
        let Some(kc) = self.keycode(sym) else {
            return HotkeyStatus::Invalid(format!(
                "{} is not on this keyboard layout",
                s.display()
            ));
        };
        for m in Self::variants(mods) {
            let r = self
                .conn
                .grab_key(
                    true,
                    self.root,
                    ModMask::from(m),
                    kc,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                )
                .map_err(anyhow::Error::from)
                .and_then(|c| c.check().map_err(anyhow::Error::from));
            if r.is_err() {
                for m2 in Self::variants(mods) {
                    let _ = self.conn.ungrab_key(kc, self.root, ModMask::from(m2));
                }
                let _ = self.conn.flush();
                return HotkeyStatus::Conflict(format!(
                    "{} is already grabbed by another application",
                    s.display()
                ));
            }
        }
        *cur = Some((kc, mods));
        let _ = self.conn.flush();
        HotkeyStatus::Registered(format!("{} via X11 key grab", s.display()))
    }

    fn listen(&self, cb: OnPress) {
        let conn = self.conn.clone();
        std::thread::Builder::new()
            .name("x11-hotkey".into())
            .spawn(move || {
                while let Ok(ev) = conn.wait_for_event() {
                    if let x11rb::protocol::Event::KeyPress(_) = ev {
                        cb();
                    }
                }
            })
            .ok();
    }
}
