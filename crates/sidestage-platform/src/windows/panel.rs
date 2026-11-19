//! Win32 edge panel: no-activate tool window, AppBar space reservation,
//! DWM live thumbnails, and the second-instance signal.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Result};
use raw_window_handle::RawWindowHandle;
use sidestage_core::WindowId;
use windows::core::{BOOL, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmQueryThumbnailSourceSize, DwmRegisterThumbnail, DwmSetWindowAttribute,
    DwmUnregisterThumbnail, DwmUpdateThumbnailProperties, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
    DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_DONOTROUND, DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION,
    DWM_TNP_RECTSOURCE, DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
};
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, EnumDisplayMonitors, GetMonitorInfoW, SetWindowRgn,
    HDC, HMONITOR, MONITORINFO, MONITORINFOEXW, RGN_OR,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{
    DefSubclassProc, SHAppBarMessage, SetWindowSubclass, ABE_LEFT, ABE_RIGHT, ABM_NEW,
    ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, ABN_POSCHANGED, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, GetWindowPlacement, IsIconic, PostMessageW, RegisterWindowMessageW,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_BROADCAST,
    HWND_TOPMOST, MA_NOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE,
    GWL_STYLE, HTCLIENT, STYLESTRUCT, SW_SHOWNOACTIVATE, WINDOWPLACEMENT, WM_MOUSEACTIVATE,
    WM_NCHITTEST, WM_STYLECHANGING, WM_USER, WS_EX_APPWINDOW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

use crate::panel::{Edge, LiveThumbnails, MonitorInfo, PanelGeometry};

const APPBAR_CALLBACK: u32 = WM_USER + 0x51;

fn instance_message() -> u32 {
    static MSG: OnceLock<u32> = OnceLock::new();
    *MSG.get_or_init(|| unsafe { RegisterWindowMessageW(&HSTRING::from("Sidestage.ShowSettings")) })
}

fn quit_message() -> u32 {
    static MSG: OnceLock<u32> = OnceLock::new();
    *MSG.get_or_init(|| unsafe { RegisterWindowMessageW(&HSTRING::from("Sidestage.Quit")) })
}

/// Receives `true` for "quit", `false` for "show settings".
type Callback = Box<dyn Fn(bool) + Send>;
fn instance_callback() -> &'static Mutex<Option<Callback>> {
    static CB: OnceLock<Mutex<Option<Callback>>> = OnceLock::new();
    CB.get_or_init(|| Mutex::new(None))
}

/// Called by a second instance: asks the running one to open Settings
/// (or to quit).
pub fn signal_running_instance(quit: bool) {
    let msg = if quit {
        quit_message()
    } else {
        instance_message()
    };
    unsafe {
        let _ = PostMessageW(Some(HWND_BROADCAST), msg, WPARAM(0), LPARAM(0));
    }
}

pub fn on_instance_signal(cb: Callback) {
    *instance_callback().lock().unwrap() = Some(cb);
}

/// Shared with the subclass procedure.
static KEYBOARD_WANTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static APPBAR_DIRTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Frame bits winit keeps on undecorated windows (caption, resize frame,
/// system menu, min/max boxes). With them, DWM falls back to drawing a
/// classic frame behind the sidebar whenever a window region is set.
const FRAME_BITS: u32 = 0x00C0_0000 | 0x0004_0000 | 0x0008_0000 | 0x0002_0000 | 0x0001_0000;
const WS_POPUP_BIT: u32 = 0x8000_0000;

/// The sidebar is a plain popup: no frame of any kind.
fn panel_style(style: u32) -> u32 {
    (style & !FRAME_BITS) | WS_POPUP_BIT
}

/// Tool window (no taskbar/Alt+Tab), topmost, and no-activate unless the
/// search field or a rename wants the keyboard.
fn panel_ex_style(ex: u32) -> u32 {
    let mut ex = (ex | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0) & !WS_EX_APPWINDOW.0;
    if KEYBOARD_WANTED.load(std::sync::atomic::Ordering::Relaxed) {
        ex &= !WS_EX_NOACTIVATE.0;
    } else {
        ex |= WS_EX_NOACTIVATE.0;
    }
    ex
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_STYLECHANGING && lparam.0 != 0 {
        // winit re-applies its own styles on resize and other updates;
        // adjust them as they are set so a frame never flashes back.
        let ss = &mut *(lparam.0 as *mut STYLESTRUCT);
        match wparam.0 as i32 {
            x if x == GWL_STYLE.0 => ss.styleNew = panel_style(ss.styleNew),
            x if x == GWL_EXSTYLE.0 => ss.styleNew = panel_ex_style(ss.styleNew),
            _ => {}
        }
    }
    if msg == WM_MOUSEACTIVATE && !KEYBOARD_WANTED.load(std::sync::atomic::Ordering::Relaxed) {
        // Clicks on the sidebar must not take focus from the user's window.
        return LRESULT(MA_NOACTIVATE as isize);
    }
    if msg == WM_NCHITTEST {
        // The undecorated window keeps WS_THICKFRAME, so its outer pixels
        // hit-test as resize borders. The sidebar never resizes, and the
        // auto-hide strip is only a few pixels wide: treat it all as client
        // so the pointer at the very screen edge reveals the sidebar.
        return LRESULT(HTCLIENT as isize);
    }
    if msg == APPBAR_CALLBACK && wparam.0 as u32 == ABN_POSCHANGED {
        APPBAR_DIRTY.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if msg == instance_message() || msg == quit_message() {
        if let Some(cb) = instance_callback().lock().unwrap().as_ref() {
            cb(msg == quit_message());
        }
        return LRESULT(0);
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

pub fn monitors() -> Vec<MonitorInfo> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    unsafe extern "system" fn cb(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        (*(data.0 as *mut Vec<HMONITOR>)).push(h);
        BOOL(1)
    }
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(cb),
            LPARAM(&mut handles as *mut _ as isize),
        );
    }
    let mut out = Vec::new();
    for (index, h) in handles.into_iter().enumerate() {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !unsafe { GetMonitorInfoW(h, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO) }
            .as_bool()
        {
            continue;
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = unsafe { GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) };
        let name_len = info
            .szDevice
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(info.szDevice.len());
        let r = info.monitorInfo.rcMonitor;
        let w = info.monitorInfo.rcWork;
        out.push(MonitorInfo {
            name: String::from_utf16_lossy(&info.szDevice[..name_len]),
            index,
            primary: info.monitorInfo.dwFlags & 1 != 0,
            x: r.left,
            y: r.top,
            width: r.right - r.left,
            height: r.bottom - r.top,
            work: (w.left, w.top, w.right - w.left, w.bottom - w.top),
            scale: dx as f32 / 96.0,
        });
    }
    out
}

pub struct Panel {
    hwnd: HWND,
    last: Option<PanelGeometry>,
    appbar: bool,
    hidden: bool,
    /// Last window region sent (`None` = never set).
    shape: Option<Option<Vec<[i32; 4]>>>,
    thumbs: DwmThumbs,
}

impl Panel {
    pub fn attach(handle: RawWindowHandle) -> Result<Panel> {
        let RawWindowHandle::Win32(h) = handle else {
            return Err(anyhow!("not a Win32 window"));
        };
        let hwnd = HWND(h.hwnd.get() as *mut _);
        unsafe {
            // Subclass first so WM_STYLECHANGING keeps our styles from now on.
            let _ = SetWindowSubclass(hwnd, Some(subclass_proc), 0x5151, 0);
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            SetWindowLongPtrW(hwnd, GWL_STYLE, panel_style(style) as isize);
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, panel_ex_style(ex) as isize);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            // We draw our own rounded card; stop Windows 11 rounding the window.
            let pref = DWMWCP_DONOTROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &pref as *const _ as *const _,
                4,
            );
            // Windows 11 also draws a 1px border round every framed window,
            // which shows as a ghost outline round our transparent window.
            let no_border = DWMWA_COLOR_NONE;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &no_border as *const _ as *const _,
                4,
            );
        }
        Ok(Panel {
            hwnd,
            last: None,
            appbar: false,
            hidden: false,
            shape: None,
            thumbs: DwmThumbs {
                dest: hwnd,
                map: HashMap::new(),
                used: HashSet::new(),
            },
        })
    }

    /// Safety net behind WM_STYLECHANGING: if a style ever slips through
    /// (frame bits, or losing tool-window/no-activate, which would put the
    /// sidebar in the taskbar and Alt+Tab), put it right.
    fn keep_styles(&self) {
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32;
            let (want, want_ex) = (panel_style(style), panel_ex_style(ex));
            if want == style && want_ex == ex {
                return;
            }
            SetWindowLongPtrW(self.hwnd, GWL_STYLE, want as isize);
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, want_ex as isize);
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }

    fn appbar_data(&self) -> APPBARDATA {
        APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: self.hwnd,
            uCallbackMessage: APPBAR_CALLBACK,
            ..Default::default()
        }
    }

    pub fn apply(&mut self, g: &PanelGeometry) {
        self.keep_styles();
        let dirty = APPBAR_DIRTY.swap(false, std::sync::atomic::Ordering::Relaxed);
        if self.last.as_ref() == Some(g) && !dirty {
            return;
        }
        self.last = Some(g.clone());
        let (mut x, y, w, h) = (g.x, g.y, g.width, g.height);
        if g.paused {
            // Fullscreen app on this monitor: drop the AppBar and hide.
            unsafe {
                if self.appbar {
                    let mut abd = self.appbar_data();
                    SHAppBarMessage(ABM_REMOVE, &mut abd);
                    self.appbar = false;
                }
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.hidden = true;
            return;
        }
        if std::mem::take(&mut self.hidden) {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
        }
        // Auto-hidden: the window keeps its size and the sidebar clips it to
        // the edge strip with `set_shape`. Resizing a GL window between the
        // strip and full width flashes a stale frame on every reveal.
        unsafe {
            if g.reserve > 0 {
                let mut abd = self.appbar_data();
                if !self.appbar {
                    SHAppBarMessage(ABM_NEW, &mut abd);
                    self.appbar = true;
                }
                let m = &g.monitor;
                abd.uEdge = if g.edge == Edge::Left {
                    ABE_LEFT
                } else {
                    ABE_RIGHT
                };
                abd.rc = RECT {
                    left: m.x,
                    top: m.y,
                    right: m.x + m.width,
                    bottom: m.y + m.height,
                };
                if g.edge == Edge::Left {
                    abd.rc.right = abd.rc.left + g.reserve;
                } else {
                    abd.rc.left = abd.rc.right - g.reserve;
                }
                SHAppBarMessage(ABM_QUERYPOS, &mut abd);
                // The shell may shift us next to another appbar.
                if g.edge == Edge::Left {
                    abd.rc.right = abd.rc.left + g.reserve;
                } else {
                    abd.rc.left = abd.rc.right - g.reserve;
                }
                SHAppBarMessage(ABM_SETPOS, &mut abd);
                // The AppBar reserves only the body; the window keeps its
                // full width so the arch's swell can float over other
                // windows. Follow the shell's position, not its width.
                x = if g.edge == Edge::Left {
                    abd.rc.left
                } else {
                    abd.rc.right - w
                };
            } else if self.appbar {
                let mut abd = self.appbar_data();
                SHAppBarMessage(ABM_REMOVE, &mut abd);
                self.appbar = false;
            }
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
        }
    }

    pub fn set_keyboard_focus(&mut self, wanted: bool) {
        KEYBOARD_WANTED.store(wanted, std::sync::atomic::Ordering::Relaxed);
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32;
            let ex = if wanted {
                ex & !WS_EX_NOACTIVATE.0
            } else {
                ex | WS_EX_NOACTIVATE.0
            };
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex as isize);
            if wanted {
                let _ = SetForegroundWindow(self.hwnd);
            }
        }
    }

    pub fn set_blur_region(&mut self, _rects: Option<Vec<[i32; 4]>>) {}

    /// Clips the window to the arch with a window region, so the
    /// transparent corners around it pass clicks to the windows below.
    pub fn set_shape(&mut self, rects: Option<Vec<[i32; 4]>>) {
        if self.shape.as_ref() == Some(&rects) {
            return;
        }
        unsafe {
            match &rects {
                Some(v) if !v.is_empty() => {
                    let rgn = CreateRectRgn(0, 0, 0, 0);
                    for r in v {
                        let part = CreateRectRgn(r[0], r[1], r[0] + r[2], r[1] + r[3]);
                        CombineRgn(Some(rgn), Some(rgn), Some(part), RGN_OR);
                        let _ = DeleteObject(part.into());
                    }
                    // On success the system owns the region.
                    if SetWindowRgn(self.hwnd, Some(rgn), true) == 0 {
                        let _ = DeleteObject(rgn.into());
                    }
                }
                _ => {
                    SetWindowRgn(self.hwnd, None, true);
                }
            }
        }
        self.shape = Some(rects);
    }

    /// Foreground activation already gives keyboard focus on Windows.
    pub fn claim_keyboard(&mut self) {}

    /// Win32 delivers WM_MOUSELEAVE reliably through winit; no override.
    pub fn pointer_inside(&mut self) -> Option<bool> {
        None
    }

    pub fn live_thumbnails(&mut self) -> Option<&mut dyn LiveThumbnails> {
        Some(&mut self.thumbs)
    }

    pub fn detach(&mut self) {
        self.thumbs.clear();
        if self.appbar {
            let mut abd = self.appbar_data();
            unsafe { SHAppBarMessage(ABM_REMOVE, &mut abd) };
            self.appbar = false;
        }
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.detach();
    }
}

struct DwmThumbs {
    dest: HWND,
    map: HashMap<WindowId, isize>,
    used: HashSet<WindowId>,
}

impl DwmThumbs {
    fn clear(&mut self) {
        for (_, h) in self.map.drain() {
            unsafe {
                let _ = DwmUnregisterThumbnail(h);
            }
        }
    }

    /// Size of the window as it looks when restored (minimized windows
    /// report a tiny caption-only size).
    fn source_size(&self, src: HWND, thumb: isize) -> Option<(i32, i32)> {
        unsafe {
            if IsIconic(src).as_bool() {
                let mut wp = WINDOWPLACEMENT {
                    length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                    ..Default::default()
                };
                if GetWindowPlacement(src, &mut wp).is_ok() {
                    let r = wp.rcNormalPosition;
                    if r.right > r.left && r.bottom > r.top {
                        return Some((r.right - r.left, r.bottom - r.top));
                    }
                }
            }
            let s = DwmQueryThumbnailSourceSize(thumb).ok()?;
            (s.cx > 0 && s.cy > 0).then_some((s.cx, s.cy))
        }
    }
}

impl LiveThumbnails for DwmThumbs {
    fn begin(&mut self) {
        self.used.clear();
    }

    fn place(&mut self, id: &WindowId, dest: [i32; 4], clip: [i32; 4]) -> Option<(i32, i32)> {
        let src = HWND(id.0.parse::<isize>().ok()? as *mut _);
        let thumb = match self.map.get(id) {
            Some(t) => *t,
            None => {
                let t = unsafe { DwmRegisterThumbnail(self.dest, src) }.ok()?;
                self.map.insert(id.clone(), t);
                t
            }
        };
        self.used.insert(id.clone());
        let (sw, sh) = self.source_size(src, thumb)?;
        // Fit (contain) inside dest.
        let scale = (dest[2] as f32 / sw as f32).min(dest[3] as f32 / sh as f32);
        let (fw, fh) = ((sw as f32 * scale) as i32, (sh as f32 * scale) as i32);
        let fx = dest[0] + (dest[2] - fw) / 2;
        let fy = dest[1] + (dest[3] - fh) / 2;
        let full = RECT {
            left: fx,
            top: fy,
            right: fx + fw,
            bottom: fy + fh,
        };
        let clipr = RECT {
            left: clip[0],
            top: clip[1],
            right: clip[0] + clip[2],
            bottom: clip[1] + clip[3],
        };
        let vis = RECT {
            left: full.left.max(clipr.left),
            top: full.top.max(clipr.top),
            right: full.right.min(clipr.right),
            bottom: full.bottom.min(clipr.bottom),
        };
        let visible = vis.right > vis.left && vis.bottom > vis.top;
        // Crop the source in proportion to what the scroll area clips.
        let to_src = |v: i32, origin: i32, len: i32, slen: i32| {
            ((v - origin) as f32 / len.max(1) as f32 * slen as f32) as i32
        };
        let src_rect = RECT {
            left: to_src(vis.left, full.left, fw, sw),
            top: to_src(vis.top, full.top, fh, sh),
            right: to_src(vis.right, full.left, fw, sw),
            bottom: to_src(vis.bottom, full.top, fh, sh),
        };
        let props = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: DWM_TNP_RECTDESTINATION
                | DWM_TNP_RECTSOURCE
                | DWM_TNP_VISIBLE
                | DWM_TNP_OPACITY
                | DWM_TNP_SOURCECLIENTAREAONLY,
            rcDestination: vis,
            rcSource: src_rect,
            opacity: 255,
            fVisible: visible.into(),
            fSourceClientAreaOnly: false.into(),
        };
        unsafe { DwmUpdateThumbnailProperties(thumb, &props) }.ok()?;
        Some((sw, sh))
    }

    fn hide_all(&mut self) {
        for t in self.map.values() {
            let props = DWM_THUMBNAIL_PROPERTIES {
                dwFlags: DWM_TNP_VISIBLE,
                fVisible: false.into(),
                ..Default::default()
            };
            unsafe {
                let _ = DwmUpdateThumbnailProperties(*t, &props);
            }
        }
    }

    fn end(&mut self) {
        let stale: Vec<WindowId> = self
            .map
            .keys()
            .filter(|k| !self.used.contains(*k))
            .cloned()
            .collect();
        for k in stale {
            if let Some(t) = self.map.remove(&k) {
                unsafe {
                    let _ = DwmUnregisterThumbnail(t);
                }
            }
        }
    }
}
