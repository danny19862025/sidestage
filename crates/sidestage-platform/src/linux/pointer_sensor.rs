//! Compositor-side "is the pointer over the sidebar?" for Wayland sessions.
//!
//! The sidebar is an XWayland window, and XWayland does not send
//! LeaveNotify when the pointer moves onto a native Wayland window (seen
//! with KWin 6.8 and Mutter 51). Auto-hide therefore asks the compositor:
//!
//! * KDE: a tiny KWin script, regenerated whenever the sidebar moves, calls
//!   `org.sidestage.Sidebar1.PointerInside(b)` on our D-Bus service when the
//!   cursor crosses the sidebar rectangle (event driven, no polling).
//! * GNOME: the Sidestage extension watches Mutter's cursor tracker and
//!   emits `org.sidestage.Shell1.PointerInside(b)`.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};

const UNKNOWN: u8 = 2;
const SERVICE: &str = "org.sidestage.Sidebar";
const PATH: &str = "/org/sidestage/Sidebar";
const SCRIPT_NAME: &str = "sidestage-pointer";

struct SidebarIface {
    state: Arc<AtomicU8>,
}

#[zbus::interface(name = "org.sidestage.Sidebar1")]
impl SidebarIface {
    #[zbus(name = "PointerInside")]
    fn pointer_inside(&self, inside: bool) {
        self.state.store(inside as u8, Ordering::Relaxed);
    }
}

enum Kind {
    KWin { conn: zbus::blocking::Connection, script: std::path::PathBuf },
    Gnome { conn: zbus::blocking::Connection },
}

pub struct PointerSensor {
    state: Arc<AtomicU8>,
    kind: Kind,
    rect: Option<[i32; 4]>,
}

impl PointerSensor {
    /// Creates the sensor for the current Wayland desktop, if supported.
    pub fn new() -> Option<PointerSensor> {
        super::wayland_display()?;
        let state = Arc::new(AtomicU8::new(UNKNOWN));
        let r = if super::is_desktop("KDE") {
            Self::kwin(state.clone())
        } else if super::is_desktop("GNOME") {
            Self::gnome(state.clone())
        } else {
            return None;
        };
        match r {
            Ok(kind) => Some(PointerSensor { state, kind, rect: None }),
            Err(e) => {
                log::info!("pointer sensor unavailable: {e:#}");
                None
            }
        }
    }

    fn kwin(state: Arc<AtomicU8>) -> Result<Kind> {
        let conn = zbus::blocking::connection::Builder::session()?
            .name(SERVICE)?
            .serve_at(PATH, SidebarIface { state })?
            .build()
            .context("registering org.sidestage.Sidebar")?;
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let script = dir.join(format!("sidestage-pointer-{}.js", std::process::id()));
        Ok(Kind::KWin { conn, script })
    }

    fn gnome(state: Arc<AtomicU8>) -> Result<Kind> {
        let conn = zbus::blocking::Connection::session()?;
        let c2 = conn.clone();
        std::thread::Builder::new().name("gnome-pointer".into()).spawn(move || loop {
            let r = (|| -> Result<()> {
                let proxy = zbus::blocking::Proxy::new(&c2, "org.sidestage.Shell", "/org/sidestage/Shell", "org.sidestage.Shell1")?;
                for msg in proxy.receive_signal("PointerInside")? {
                    let (inside,): (bool,) = msg.body().deserialize()?;
                    state.store(inside as u8, Ordering::Relaxed);
                }
                Ok(())
            })();
            if let Err(e) = r {
                log::debug!("GNOME pointer signal stream: {e:#}");
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        })?;
        Ok(Kind::Gnome { conn })
    }

    /// Sets the watched rectangle in compositor (logical) coordinates.
    pub fn set_rect(&mut self, rect: Option<[i32; 4]>) {
        if self.rect == rect {
            return;
        }
        self.rect = rect;
        self.state.store(UNKNOWN, Ordering::Relaxed);
        let r = match &self.kind {
            Kind::KWin { conn, script } => kwin_load(conn, script, rect),
            Kind::Gnome { conn } => {
                let [x, y, w, h] = rect.unwrap_or([0, 0, 0, 0]);
                zbus::blocking::Proxy::new(conn, "org.sidestage.Shell", "/org/sidestage/Shell", "org.sidestage.Shell1")
                    .and_then(|p| p.call::<_, _, ()>("WatchPointer", &(x, y, w, h)))
                    .map_err(Into::into)
            }
        };
        if let Err(e) = r {
            log::warn!("pointer sensor update failed: {e:#}");
        }
    }

    pub fn inside(&self) -> Option<bool> {
        match self.state.load(Ordering::Relaxed) {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
}

impl Drop for PointerSensor {
    fn drop(&mut self) {
        self.set_rect(None);
    }
}

fn kwin_load(conn: &zbus::blocking::Connection, script: &std::path::Path, rect: Option<[i32; 4]>) -> Result<()> {
    let scripting = zbus::blocking::Proxy::new(conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting")?;
    let _: bool = scripting.call("unloadScript", &(SCRIPT_NAME,)).unwrap_or(false);
    let Some([x, y, w, h]) = rect else {
        let _ = std::fs::remove_file(script);
        return Ok(());
    };
    std::fs::write(
        script,
        format!(
            r#"// Generated by Sidestage: reports when the cursor enters/leaves the sidebar.
const R = {{ x: {x}, y: {y}, w: {w}, h: {h} }};
let last = null;
function check() {{
    const p = workspace.cursorPos;
    const inside = p.x >= R.x && p.x < R.x + R.w && p.y >= R.y && p.y < R.y + R.h;
    if (inside !== last) {{
        last = inside;
        callDBus("{SERVICE}", "{PATH}", "org.sidestage.Sidebar1", "PointerInside", inside);
    }}
}}
workspace.cursorPosChanged.connect(check);
check();
"#
        ),
    )?;
    let id: i32 = scripting.call("loadScript", &(script.display().to_string(), SCRIPT_NAME))?;
    if id < 0 {
        anyhow::bail!("KWin refused the pointer script");
    }
    // KWin 6 exposes loaded scripts at /Scripting/Script<N>.
    let path = format!("/Scripting/Script{id}");
    zbus::blocking::Proxy::new(conn, "org.kde.KWin", path.as_str(), "org.kde.kwin.Script")?.call::<_, _, ()>("run", &())?;
    Ok(())
}
