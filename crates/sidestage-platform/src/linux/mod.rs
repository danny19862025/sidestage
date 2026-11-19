//! Linux integration.
//!
//! | Session              | Window list / restore                         | Previews                    |
//! |----------------------|-----------------------------------------------|-----------------------------|
//! | KDE Plasma Wayland   | `org_kde_plasma_window_management` protocol   | KWin ScreenShot2 snapshots  |
//! | GNOME Wayland        | Sidestage GNOME Shell extension over D-Bus    | Extension snapshots         |
//! | Any X11 (EWMH)       | `_NET_CLIENT_LIST`, `_NET_ACTIVE_WINDOW`      | Icon + title only           |
//!
//! The sidebar window itself always runs as an X11 client (XWayland on
//! Wayland sessions) so it can be placed at a screen edge, marked as a dock,
//! and reserve space via struts; see [`panel`].

pub mod apps;
mod gnome;
pub mod hotkey;
pub mod icons;
mod kde;
mod kwin_screenshot;
pub mod panel;
mod pointer_sensor;
mod reserve;
mod x11;

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

use anyhow::{anyhow, bail, Context, Result};
use sidestage_core::desktop_entry::DesktopEntry;
use sidestage_core::tracker::BackendStatus;
use sidestage_core::{IconSource, LaunchTarget, RgbaImage};

use crate::{ThumbnailMode, Waker, WindowBackend, WindowEvent, WindowId};

/// `XDG_CURRENT_DESKTOP` components, e.g. `["KDE"]` or `["ubuntu", "GNOME"]`.
pub fn desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn is_desktop(name: &str) -> bool {
    desktops().iter().any(|d| d.eq_ignore_ascii_case(name))
}

/// Wayland socket captured at startup (before the UI may hide it from winit).
pub fn wayland_display() -> Option<String> {
    static WD: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    WD.get_or_init(|| {
        std::env::var("WAYLAND_DISPLAY")
            .ok()
            .filter(|s| !s.is_empty())
    })
    .clone()
}

fn wayland_connection() -> Option<wayland_client::Connection> {
    let name = wayland_display()?;
    let path = if Path::new(&name).is_absolute() {
        std::path::PathBuf::from(name)
    } else {
        std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?).join(name)
    };
    let stream = std::os::unix::net::UnixStream::connect(path).ok()?;
    wayland_client::Connection::from_socket(stream).ok()
}

pub fn init_worker_thread() {}

pub fn session_description() -> String {
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into());
    let de = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown desktop".into());
    format!("{de} ({session})")
}

struct NullBackend;
impl WindowBackend for NullBackend {
    fn name(&self) -> &'static str {
        "none"
    }
    fn thumbnail_mode(&self) -> ThumbnailMode {
        ThumbnailMode::None
    }
    fn restore(&self, _: &WindowId) {}
}

pub fn start_window_backend(tx: Sender<WindowEvent>, waker: Waker) -> Box<dyn WindowBackend> {
    let forced = std::env::var("SIDESTAGE_BACKEND").ok();
    let want = |n: &str| forced.as_deref().map(|f| f == n).unwrap_or(true);

    if want("kwin") {
        if let Some(conn) = wayland_connection() {
            if kde::available(&conn) {
                match kde::start(conn, tx.clone(), waker.clone()) {
                    Ok(b) => return Box::new(b),
                    Err(e) => log::warn!("KWin backend unavailable: {e:#}"),
                }
            }
        }
    }
    if want("gnome") && (is_desktop("GNOME") || forced.is_some()) {
        match gnome::start(tx.clone(), waker.clone()) {
            Ok(b) => return Box::new(b),
            Err(e) => log::warn!("GNOME backend unavailable: {e:#}"),
        }
    }
    // On a Wayland session X11 only sees XWayland windows: not useful.
    if want("x11") && (wayland_display().is_none() || forced.is_some()) {
        match x11::start(tx.clone(), waker.clone()) {
            Ok(b) => return Box::new(b),
            Err(e) => log::warn!("X11 backend unavailable: {e:#}"),
        }
    }
    let msg = if is_desktop("GNOME") && wayland_display().is_some() {
        "Enable the Sidestage GNOME Shell extension to list minimized windows (Settings → Integration)."
    } else {
        "This desktop does not expose minimized windows to Sidestage."
    };
    let _ = tx.send(WindowEvent::Status(BackendStatus {
        backend: "none".into(),
        connected: false,
        message: Some(msg.into()),
    }));
    waker();
    Box::new(NullBackend)
}

pub fn system_prefers_dark() -> Option<bool> {
    // freedesktop portal setting: 0 none, 1 dark, 2 light.
    let conn = zbus::blocking::Connection::session().ok()?;
    let reply = conn
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.Settings"),
            "ReadOne",
            &("org.freedesktop.appearance", "color-scheme"),
        )
        .ok()?;
    let v: zbus::zvariant::OwnedValue = reply.body().deserialize().ok()?;
    let n = u32::try_from(&v).ok().or_else(|| match &*v {
        // Older portals wrap the value in another variant.
        zbus::zvariant::Value::Value(inner) => u32::try_from(&**inner).ok(),
        _ => None,
    })?;
    match n {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// App metadata for running windows

pub(crate) struct ResolvedApp {
    pub name: String,
    pub icon: Option<String>,
}

/// Maps a window's app id to a display name and icon, sending the icon
/// bitmap asynchronously as [`WindowEvent::Icon`].
pub(crate) struct AppResolver {
    cache: HashMap<String, (String, Option<String>)>,
    tx: Sender<WindowEvent>,
    waker: Waker,
}

impl AppResolver {
    pub fn new(tx: Sender<WindowEvent>, waker: Waker) -> Self {
        Self {
            cache: HashMap::new(),
            tx,
            waker,
        }
    }

    pub fn resolve(&mut self, app_key: &str, pid: Option<u32>, icon_hint: &str) -> ResolvedApp {
        if let Some((name, icon)) = self.cache.get(app_key) {
            return ResolvedApp {
                name: name.clone(),
                icon: icon.clone(),
            };
        }
        let entry: Option<DesktopEntry> = apps::find_for_app_id(app_key)
            .or_else(|| pid.filter(|p| *p > 0).and_then(apps::find_for_pid));
        let name = entry
            .as_ref()
            .map(|e| e.name.clone())
            .unwrap_or_else(|| pretty_app_name(app_key));
        let icon = entry
            .as_ref()
            .and_then(|e| e.icon.clone())
            .or_else(|| (!icon_hint.is_empty()).then(|| icon_hint.to_string()))
            .or_else(|| (!app_key.is_empty()).then(|| app_key.to_lowercase()));
        self.cache
            .insert(app_key.to_string(), (name.clone(), icon.clone()));
        if let Some(icon_name) = icon.clone() {
            let (tx, waker, key) = (self.tx.clone(), self.waker.clone(), app_key.to_string());
            std::thread::spawn(move || {
                let img = icons::load_named(&icon_name, 64)
                    .or_else(|| icons::load_named("application-x-executable", 64));
                if let Some(image) = img {
                    let _ = tx.send(WindowEvent::Icon {
                        app_key: key,
                        image,
                    });
                    waker();
                }
            });
        }
        ResolvedApp { name, icon }
    }
}

/// `org.gnome.TextEditor` → `Text Editor`, `firefox` → `Firefox`.
fn pretty_app_name(app_key: &str) -> String {
    // Reverse-DNS ids (org.gnome.TextEditor) use their last segment;
    // otherwise drop a file extension (test-window.py -> test-window).
    let last = if app_key.matches('.').count() >= 2 {
        app_key.rsplit('.').next().unwrap_or(app_key)
    } else {
        app_key.split('.').next().unwrap_or(app_key)
    };
    if last.is_empty() {
        return "Application".into();
    }
    let mut out = String::new();
    for (i, c) in last.chars().enumerate() {
        if i > 0 && c.is_uppercase() && !out.ends_with(' ') {
            out.push(' ');
        }
        if i == 0 {
            out.extend(c.to_uppercase());
        } else if c == '-' || c == '_' {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Launching

fn spawn_detached(argv: &[String], cwd: Option<&str>) -> Result<()> {
    let (prog, args) = argv.split_first().ok_or_else(|| anyhow!("empty command"))?;
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = cwd.filter(|d| Path::new(d).is_dir()) {
        cmd.current_dir(dir);
    }
    // Launched apps must see the real session, even though the sidebar
    // itself may run with WAYLAND_DISPLAY hidden (XWayland panel).
    if let Some(wd) = wayland_display() {
        cmd.env("WAYLAND_DISPLAY", wd);
    }
    cmd.env_remove("SIDESTAGE_CONFIG");
    unsafe {
        use std::os::unix::process::CommandExt;
        // New session so the app outlives the sidebar and is not killed with it.
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn().with_context(|| format!("starting {prog}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn terminal_command() -> Option<Vec<String>> {
    let candidates: &[(&str, &[&str])] = &[
        ("xdg-terminal-exec", &[]),
        ("kgx", &["--"]),
        ("gnome-terminal", &["--"]),
        ("konsole", &["-e"]),
        ("ptyxis", &["--"]),
        ("foot", &[]),
        ("alacritty", &["-e"]),
        ("kitty", &[]),
        ("xterm", &["-e"]),
    ];
    let path = std::env::var_os("PATH")?;
    for (bin, args) in candidates {
        if std::env::split_paths(&path).any(|d| d.join(bin).is_file()) {
            let mut v = vec![bin.to_string()];
            v.extend(args.iter().map(|s| s.to_string()));
            return Some(v);
        }
    }
    None
}

fn launch_entry(e: &DesktopEntry) -> Result<()> {
    let mut argv = e
        .command_line()
        .ok_or_else(|| anyhow!("{} has no usable Exec line", e.path.display()))?;
    if e.terminal {
        let mut t = terminal_command().ok_or_else(|| anyhow!("no terminal emulator found"))?;
        t.append(&mut argv);
        argv = t;
    }
    spawn_detached(&argv, e.working_dir.as_deref())
}

pub fn launch(target: &LaunchTarget) -> Result<()> {
    match target {
        LaunchTarget::DesktopEntry { path } => {
            let e = DesktopEntry::load(path, None)
                .ok_or_else(|| anyhow!("cannot read {}", path.display()))?;
            launch_entry(&e)
        }
        LaunchTarget::Path { path, args } => {
            if !path.exists() {
                bail!("{} no longer exists", path.display());
            }
            if path.extension().map(|e| e == "desktop").unwrap_or(false) {
                let e = DesktopEntry::load(path, None)
                    .ok_or_else(|| anyhow!("cannot read {}", path.display()))?;
                if e.kind == "Application" {
                    return launch_entry(&e);
                }
                return spawn_detached(&["xdg-open".into(), path.display().to_string()], None);
            }
            use std::os::unix::fs::PermissionsExt;
            let exec = path.is_file()
                && path
                    .metadata()
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false);
            if exec {
                let mut argv = vec![path.display().to_string()];
                argv.extend(args.iter().cloned());
                spawn_detached(&argv, path.parent().and_then(|p| p.to_str()))
            } else {
                spawn_detached(&["xdg-open".into(), path.display().to_string()], None)
            }
        }
        LaunchTarget::Command { program, args } => {
            let mut argv = vec![program.clone()];
            argv.extend(args.iter().cloned());
            spawn_detached(&argv, None)
        }
        LaunchTarget::AppsFolder { .. } => bail!("Windows Store apps cannot be launched on Linux"),
        LaunchTarget::Url { url } => {
            let url = sidestage_core::config::normalize_web_url(url).map_err(|e| anyhow!(e))?;
            spawn_detached(&["xdg-open".into(), url], None)
        }
    }
}

// ---------------------------------------------------------------------------
// Favourite icons

pub fn favourite_icon(icon: &IconSource, target: &LaunchTarget, size: u32) -> Option<RgbaImage> {
    match icon {
        IconSource::File { path } => icons::load_image_file(path, size),
        IconSource::Themed { name } => icons::load_named(name, size),
        IconSource::Auto => {
            let from_entry = |p: &Path| {
                DesktopEntry::load(p, None)
                    .and_then(|e| e.icon)
                    .and_then(|i| icons::load_named(&i, size))
            };
            let found = match target {
                LaunchTarget::DesktopEntry { path } => from_entry(path),
                LaunchTarget::Path { path, .. } if path.is_dir() => {
                    icons::load_named("folder", size)
                        .or_else(|| icons::load_named("inode-directory", size))
                }
                LaunchTarget::Path { path, .. } => {
                    if path.extension().map(|e| e == "desktop").unwrap_or(false) {
                        from_entry(path)
                    } else if let Some(stem) =
                        path.file_stem().map(|s| s.to_string_lossy().to_lowercase())
                    {
                        apps::find_for_app_id(&stem)
                            .and_then(|e| e.icon)
                            .and_then(|i| icons::load_named(&i, size))
                            .or_else(|| {
                                if path.is_dir() {
                                    icons::load_named("folder", size)
                                } else {
                                    None
                                }
                            })
                    } else {
                        None
                    }
                }
                LaunchTarget::Command { program, .. } => {
                    let base = Path::new(program)
                        .file_name()
                        .map(|s| s.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    apps::find_for_app_id(&base)
                        .and_then(|e| e.icon)
                        .and_then(|i| icons::load_named(&i, size))
                        .or_else(|| icons::load_named(&base, size))
                }
                LaunchTarget::AppsFolder { .. } => None,
                LaunchTarget::Url { .. } => [
                    "web-browser",
                    "applications-internet",
                    "internet-web-browser",
                ]
                .iter()
                .find_map(|n| icons::load_named(n, size)),
            };
            found.or_else(|| icons::load_named("application-x-executable", size))
        }
    }
}

// ---------------------------------------------------------------------------
// Launch at login (XDG autostart)

fn autostart_file() -> std::path::PathBuf {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or(home.join(".config"))
        .join("autostart/sidestage.desktop")
}

pub fn set_launch_at_login(enabled: bool) -> Result<()> {
    let file = autostart_file();
    if !enabled {
        match std::fs::remove_file(&file) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        }
    }
    let exe = std::env::current_exe()?;
    std::fs::create_dir_all(file.parent().unwrap())?;
    let quoted = format!(
        "\"{}\"",
        exe.display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    );
    std::fs::write(
        &file,
        format!(
            "[Desktop Entry]\nType=Application\nName=Sidestage\nComment=Favourites and minimized windows sidebar\n\
             Exec={quoted} --autostart\nIcon=sidestage\nX-GNOME-Autostart-enabled=true\nX-KDE-autostart-phase=2\nNoDisplay=true\n"
        ),
    )?;
    Ok(())
}

pub fn launch_at_login_enabled() -> bool {
    autostart_file().is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_names() {
        assert_eq!(pretty_app_name("org.gnome.TextEditor"), "Text Editor");
        assert_eq!(pretty_app_name("firefox"), "Firefox");
        assert_eq!(pretty_app_name("my-app"), "My app");
    }
}
