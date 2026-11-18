//! Windows 10/11 integration.
//!
//! * Window list: `SetWinEventHook` (minimize start/end, show/hide, destroy,
//!   name change, cloak) plus `EnumWindows` resyncs, filtered like Alt+Tab.
//! * Restore: `ShowWindowAsync(SW_RESTORE)` + `SetForegroundWindow` (allowed
//!   because the user just clicked our window).
//! * Previews: live DWM thumbnails composited into the sidebar
//!   (`DwmRegisterThumbnail`), see [`panel`].
//! * Edge panel: `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST`, and an
//!   AppBar (`SHAppBarMessage`) to reserve space.

pub mod apps;
mod backend;
pub mod icons;
pub mod panel;

use std::sync::mpsc::Sender;

use anyhow::{anyhow, bail, Result};
use sidestage_core::{IconSource, LaunchTarget, RgbaImage};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE};
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::{WindowBackend, WindowEvent, Waker};

pub use panel::{on_instance_signal, signal_running_instance};

pub fn init_worker_thread() {
    // Shell icon and AppsFolder APIs need an STA. Ignore "already initialised".
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    }
}

pub fn start_window_backend(tx: Sender<WindowEvent>, waker: Waker) -> Box<dyn WindowBackend> {
    Box::new(backend::Win32Backend::start(tx, waker))
}

pub fn session_description() -> String {
    let build = reg_string(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "CurrentBuild", true).unwrap_or_default();
    let name = if build.parse::<u32>().map(|b| b >= 22000).unwrap_or(false) { "Windows 11" } else { "Windows" };
    format!("{name} (build {build})")
}

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub fn system_prefers_dark() -> Option<bool> {
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(PERSONALIZE),
            &HSTRING::from("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    err.is_ok().then_some(value == 0)
}

fn reg_string(key: &str, value: &str, local_machine: bool) -> Option<String> {
    use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
    let root = if local_machine { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER };
    let mut buf = vec![0u16; 512];
    let mut size = (buf.len() * 2) as u32;
    let err = unsafe {
        RegGetValueW(
            root,
            &HSTRING::from(key),
            &HSTRING::from(value),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
    };
    if err.is_err() {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..len.min(buf.len())]))
}

pub fn set_launch_at_login(enabled: bool) -> Result<()> {
    if enabled {
        let exe = std::env::current_exe()?;
        let cmd = format!("\"{}\" --autostart", exe.display());
        let wide: Vec<u16> = cmd.encode_utf16().chain(Some(0)).collect();
        let err = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(RUN_KEY),
                &HSTRING::from("Sidestage"),
                REG_SZ.0,
                Some(wide.as_ptr() as *const _),
                (wide.len() * 2) as u32,
            )
        };
        err.ok().map_err(|e| anyhow!("writing the Run key: {e}"))
    } else {
        let err = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(RUN_KEY), &HSTRING::from("Sidestage")) };
        // ERROR_FILE_NOT_FOUND (2) means it was not enabled: fine.
        if err.is_ok() || err.0 == 2 {
            Ok(())
        } else {
            bail!("removing the Run key value failed ({})", err.0)
        }
    }
}

pub fn launch_at_login_enabled() -> bool {
    reg_string(RUN_KEY, "Sidestage", false).is_some()
}

fn shell_open(file: &str, params: &str, dir: Option<&str>) -> Result<()> {
    let file_w = HSTRING::from(file);
    let params_w = HSTRING::from(params);
    let dir_w = dir.map(HSTRING::from);
    let r = unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("open"),
            &file_w,
            if params.is_empty() { PCWSTR::null() } else { PCWSTR(params_w.as_ptr()) },
            dir_w.as_ref().map(|d| PCWSTR(d.as_ptr())).unwrap_or(PCWSTR::null()),
            SW_SHOWNORMAL,
        )
    };
    // Values <= 32 are errors (SE_ERR_*).
    if r.0 as isize <= 32 {
        bail!("Windows could not open {file} (error {})", r.0 as isize);
    }
    Ok(())
}

fn quote_args(args: &[String]) -> String {
    args.iter()
        .map(|a| if a.is_empty() || a.contains([' ', '\t', '"']) { format!("\"{}\"", a.replace('"', "\\\"")) } else { a.clone() })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn launch(target: &LaunchTarget) -> Result<()> {
    match target {
        LaunchTarget::AppsFolder { parsing_name } => shell_open(&format!("shell:AppsFolder\\{parsing_name}"), "", None),
        LaunchTarget::Path { path, args } => {
            if !path.exists() {
                bail!("{} no longer exists", path.display());
            }
            let dir = path.parent().map(|p| p.display().to_string());
            shell_open(&path.display().to_string(), &quote_args(args), dir.as_deref())
        }
        LaunchTarget::Command { program, args } => shell_open(program, &quote_args(args), None),
        LaunchTarget::DesktopEntry { .. } => bail!("Linux .desktop shortcuts cannot be launched on Windows"),
    }
}

pub fn favourite_icon(icon: &IconSource, target: &LaunchTarget, size: u32) -> Option<RgbaImage> {
    match icon {
        IconSource::File { path } => {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if matches!(ext.as_str(), "png" | "ico" | "jpg" | "jpeg" | "bmp") {
                image::open(path).ok().and_then(|i| {
                    let i = i.into_rgba8();
                    let (w, h) = i.dimensions();
                    RgbaImage::new(w, h, i.into_raw()).map(|img| img.downscaled(size * 2))
                })
            } else {
                icons::shell_icon(&path.display().to_string(), size)
            }
        }
        IconSource::Themed { .. } => None,
        IconSource::Auto => match target {
            LaunchTarget::AppsFolder { parsing_name } => icons::shell_icon(&format!("shell:AppsFolder\\{parsing_name}"), size),
            LaunchTarget::Path { path, .. } => icons::shell_icon(&path.display().to_string(), size),
            LaunchTarget::Command { program, .. } => icons::find_on_path(program).and_then(|p| icons::shell_icon(&p, size)),
            LaunchTarget::DesktopEntry { .. } => None,
        },
    }
}
