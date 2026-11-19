//! Win32 window tracking with WinEvent hooks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::mpsc::Sender;

use sidestage_core::tracker::BackendStatus;
use sidestage_core::{WindowId, WindowInfo};
use windows::core::{BOOL, HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumWindows, GetAncestor, GetClassLongPtrW, GetMessageW, GetWindow,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindow, IsWindowVisible, SendMessageTimeoutW, SetForegroundWindow, SetTimer, ShowWindowAsync,
    TranslateMessage, CHILDID_SELF, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE,
    EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_FOREGROUND,
    EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, GA_ROOTOWNER, GCLP_HICON, GCLP_HICONSM,
    GWL_EXSTYLE, GW_OWNER, HICON, ICON_BIG, ICON_SMALL2, MSG, OBJID_WINDOW, SMTO_ABORTIFHUNG,
    SW_RESTORE, SW_SHOWNOACTIVATE, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_GETICON,
    WM_TIMER, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

use super::icons;
use crate::{ThumbnailMode, Waker, WindowBackend, WindowEvent};

pub struct Win32Backend;

/// Most recent foreground window of another process.
static LAST_FOREGROUND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

impl Win32Backend {
    pub fn start(tx: Sender<WindowEvent>, waker: Waker) -> Self {
        std::thread::Builder::new()
            .name("win32-windows".into())
            .spawn(move || {
                super::init_worker_thread();
                run(tx, waker);
            })
            .expect("spawn window thread");
        Win32Backend
    }
}

fn hwnd_of(id: &WindowId) -> Option<HWND> {
    id.0.parse::<isize>().ok().map(|v| HWND(v as *mut _))
}

impl WindowBackend for Win32Backend {
    fn name(&self) -> &'static str {
        "win32"
    }
    fn thumbnail_mode(&self) -> ThumbnailMode {
        ThumbnailMode::Live
    }
    fn restore(&self, id: &WindowId) {
        let Some(hwnd) = hwnd_of(id) else { return };
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                return;
            }
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindowAsync(hwnd, SW_RESTORE);
            }
            // The click on the sidebar gives this process the foreground
            // right, so the restored window may be activated.
            let _ = SetForegroundWindow(hwnd);
        }
    }

    fn focus_previous(&self) {
        let prev = LAST_FOREGROUND.load(std::sync::atomic::Ordering::Relaxed);
        if prev != 0 {
            unsafe {
                let hwnd = HWND(prev as *mut _);
                if IsWindow(Some(hwnd)).as_bool() {
                    let _ = SetForegroundWindow(hwnd);
                }
            }
        }
    }

    fn restore_many(&self, ids: &[WindowId]) {
        let Some((last, rest)) = ids.split_last() else {
            return;
        };
        for id in rest {
            let Some(hwnd) = hwnd_of(id) else { continue };
            unsafe {
                // Restore without activating, so only the last one takes focus.
                if IsWindow(Some(hwnd)).as_bool() && IsIconic(hwnd).as_bool() {
                    let _ = ShowWindowAsync(hwnd, SW_SHOWNOACTIVATE);
                }
            }
        }
        self.restore(last);
    }
}

struct Tracked {
    info: WindowInfo,
}

struct State {
    tx: Sender<WindowEvent>,
    waker: Waker,
    windows: HashMap<isize, Tracked>,
    apps: HashMap<String, String>,
    iconed: std::collections::HashSet<String>,
    own_pid: u32,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn run(tx: Sender<WindowEvent>, waker: Waker) {
    let state = State {
        tx: tx.clone(),
        waker: waker.clone(),
        windows: HashMap::new(),
        apps: HashMap::new(),
        iconed: Default::default(),
        own_pid: unsafe { GetCurrentProcessId() },
    };
    STATE.with(|s| *s.borrow_mut() = Some(state));
    let _ = tx.send(WindowEvent::Status(BackendStatus {
        backend: "Windows".into(),
        connected: true,
        message: None,
    }));
    resync();

    let ranges = [
        (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
        (EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE),
        (EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE),
        (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
        (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
    ];
    let hooks: Vec<HWINEVENTHOOK> = ranges
        .iter()
        .map(|(a, b)| unsafe {
            SetWinEventHook(
                *a,
                *b,
                None,
                Some(on_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        })
        .collect();
    // Safety net: periodic resync for anything the hooks miss.
    unsafe { SetTimer(None, 0, 5000, None) };
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_TIMER {
                resync();
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        for h in hooks {
            let _ = UnhookWinEvent(h);
        }
    }
}

unsafe extern "system" fn on_event(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _: u32,
    _: u32,
) {
    if hwnd.0.is_null() || id_object != OBJID_WINDOW.0 || id_child != CHILDID_SELF as i32 {
        return;
    }
    let key = hwnd.0 as isize;
    STATE.with(|s| {
        let mut guard = s.borrow_mut();
        let Some(state) = guard.as_mut() else { return };
        match event {
            // WINEVENT_SKIPOWNPROCESS: this is never the sidebar itself.
            EVENT_SYSTEM_FOREGROUND => {
                LAST_FOREGROUND.store(key, std::sync::atomic::Ordering::Relaxed);
            }
            EVENT_OBJECT_DESTROY => {
                if state.windows.remove(&key).is_some() {
                    let _ = state
                        .tx
                        .send(WindowEvent::Removed(WindowId(key.to_string())));
                    (state.waker)();
                }
            }
            _ => state.refresh(hwnd),
        }
    });
}

fn resync() {
    let mut found: Vec<HWND> = Vec::new();
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let v = &mut *(lparam.0 as *mut Vec<HWND>);
        v.push(hwnd);
        BOOL(1)
    }
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut found as *mut _ as isize));
    }
    STATE.with(|s| {
        let mut guard = s.borrow_mut();
        let Some(state) = guard.as_mut() else { return };
        let mut list = Vec::new();
        let mut seen = HashMap::new();
        for hwnd in found {
            if let Some(info) = state.describe(hwnd) {
                seen.insert(hwnd.0 as isize, ());
                list.push(info);
            }
        }
        state.windows.retain(|k, _| seen.contains_key(k));
        for info in &list {
            let key: isize = info.id.0.parse().unwrap_or_default();
            state.windows.insert(key, Tracked { info: info.clone() });
        }
        let _ = state.tx.send(WindowEvent::Snapshot(list));
        (state.waker)();
    });
}

impl State {
    fn refresh(&mut self, hwnd: HWND) {
        let key = hwnd.0 as isize;
        match self.describe(hwnd) {
            Some(info) => {
                let changed = self
                    .windows
                    .get(&key)
                    .map(|t| t.info != info)
                    .unwrap_or(true);
                if changed {
                    self.windows.insert(key, Tracked { info: info.clone() });
                    let _ = self.tx.send(WindowEvent::Upsert(info));
                    (self.waker)();
                }
            }
            None => {
                if self.windows.remove(&key).is_some() {
                    let _ = self
                        .tx
                        .send(WindowEvent::Removed(WindowId(key.to_string())));
                    (self.waker)();
                }
            }
        }
    }

    /// Returns info for windows that would appear in Alt+Tab.
    fn describe(&mut self, hwnd: HWND) -> Option<WindowInfo> {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return None;
            }
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            if ex & WS_EX_TOOLWINDOW.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0 {
                return None;
            }
            if ex & WS_EX_NOACTIVATE.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0 {
                return None;
            }
            // Owned windows (dialogs) are skipped unless they force a taskbar button.
            let owner = GetWindow(hwnd, GW_OWNER).unwrap_or_default();
            if !owner.0.is_null() && ex & WS_EX_APPWINDOW.0 == 0 {
                return None;
            }
            if GetAncestor(hwnd, GA_ROOTOWNER) != hwnd && ex & WS_EX_APPWINDOW.0 == 0 {
                return None;
            }
            let mut cloaked: u32 = 0;
            let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4);
            let minimized = IsIconic(hwnd).as_bool();
            // Cloaked = on another virtual desktop or a suspended UWP frame.
            if cloaked != 0 && !minimized {
                return None;
            }
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return None;
            }
            let mut buf = vec![0u16; len as usize + 1];
            let n = GetWindowTextW(hwnd, &mut buf);
            let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == self.own_pid {
                return None;
            }
            let exe = process_path(pid).unwrap_or_default();
            // Store apps are hosted by ApplicationFrameHost; group them by AUMID.
            let app_key = window_aumid(hwnd).unwrap_or_else(|| exe.to_lowercase());
            let app_name = self
                .apps
                .entry(app_key.clone())
                .or_insert_with(|| app_display_name(&app_key, &exe))
                .clone();
            if !self.iconed.contains(&app_key) {
                self.iconed.insert(app_key.clone());
                // Shell icons are large and crisp; WM_GETICON is often 32px.
                let shell = if app_key.contains('!') {
                    icons::shell_icon(&format!("shell:AppsFolder\\{app_key}"), 64)
                } else {
                    icons::shell_icon(&exe, 64)
                };
                let icon = shell.or_else(|| window_icon(hwnd));
                if let Some(image) = icon {
                    let _ = self.tx.send(WindowEvent::Icon {
                        app_key: app_key.clone(),
                        image,
                    });
                }
            }
            Some(WindowInfo {
                id: WindowId((hwnd.0 as isize).to_string()),
                app_key,
                app_name,
                title,
                minimized,
                icon_hint: Some(exe),
                minimized_at: None,
            })
        }
    }
}

fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let r =
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        r.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn window_aumid(hwnd: HWND) -> Option<String> {
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
        let v = store.GetValue(&super::apps::PKEY_APP_USER_MODEL_ID).ok()?;
        let s = windows::core::BSTR::try_from(&v).ok()?.to_string();
        (!s.is_empty()).then_some(s)
    }
}

/// FileDescription from the executable's version resource, else the file stem.
fn app_display_name(app_key: &str, exe: &str) -> String {
    if app_key.contains('!') {
        // Store app: ask the shell for its display name.
        if let Some(n) = shell_display_name(&format!("shell:AppsFolder\\{app_key}")) {
            return n;
        }
    }
    file_description(exe).unwrap_or_else(|| {
        std::path::Path::new(exe)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Application".into())
    })
}

fn shell_display_name(parsing: &str) -> Option<String> {
    use windows::Win32::UI::Shell::{IShellItem, SHCreateItemFromParsingName, SIGDN_NORMALDISPLAY};
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(parsing), None).ok()?;
        let p = item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?;
        let s = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

fn file_description(exe: &str) -> Option<String> {
    unsafe {
        let path = HSTRING::from(exe);
        let size = GetFileVersionInfoSizeW(&path, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&path, None, size, data.as_mut_ptr() as *mut _).ok()?;
        let mut ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let mut lang_code = (0x0409u16, 0x04B0u16);
        if VerQueryValueW(
            data.as_ptr() as *const _,
            &HSTRING::from(r"\VarFileInfo\Translation"),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            && len >= 4
        {
            let pair = std::slice::from_raw_parts(ptr as *const u16, 2);
            lang_code = (pair[0], pair[1]);
        }
        let key = format!(
            r"\StringFileInfo\{:04x}{:04x}\FileDescription",
            lang_code.0, lang_code.1
        );
        if !VerQueryValueW(
            data.as_ptr() as *const _,
            &HSTRING::from(key),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            || len == 0
        {
            return None;
        }
        let s = std::slice::from_raw_parts(ptr as *const u16, len as usize);
        let text = String::from_utf16_lossy(s)
            .trim_end_matches('\0')
            .trim()
            .to_string();
        (!text.is_empty()).then_some(text)
    }
}

fn window_icon(hwnd: HWND) -> Option<sidestage_core::RgbaImage> {
    unsafe {
        let mut result = 0usize;
        for kind in [ICON_BIG, ICON_SMALL2] {
            let ok = SendMessageTimeoutW(
                hwnd,
                WM_GETICON,
                WPARAM(kind as usize),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                100,
                Some(&mut result),
            );
            if ok.0 != 0 && result != 0 {
                if let Some(img) = icons::hicon_to_rgba(HICON(result as *mut _)) {
                    return Some(img);
                }
            }
        }
        for idx in [GCLP_HICON, GCLP_HICONSM] {
            let h = GetClassLongPtrW(hwnd, idx);
            if h != 0 {
                if let Some(img) = icons::hicon_to_rgba(HICON(h as *mut _)) {
                    return Some(img);
                }
            }
        }
        None
    }
}
