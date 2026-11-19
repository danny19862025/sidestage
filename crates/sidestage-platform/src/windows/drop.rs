//! The sidebar's own OLE drop target.
//!
//! winit's drop target only understands `CF_HDROP` (file paths) and reports
//! no pointer position during a drag. Apps dragged from the Start menu carry
//! shell items without a file path, and without a position a drop can't land
//! in the section under the pointer. This target reads any shell items via
//! `SHCreateShellItemArrayFromDataObject` and tracks the position itself.

use std::sync::Mutex;

use windows::core::{implement, Interface, Ref};
use windows::Win32::Foundation::{HWND, POINT, POINTL};
use windows::Win32::System::Com::{CoTaskMemFree, IDataObject};
use windows::Win32::System::Ole::{
    IDropTarget, IDropTarget_Impl, RegisterDragDrop, RevokeDragDrop, DROPEFFECT, DROPEFFECT_COPY,
    DROPEFFECT_LINK, DROPEFFECT_NONE,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{
    IShellItem, IShellItem2, IShellItemArray, SHCreateShellItemArrayFromDataObject,
    SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY,
};
use windows::Win32::Graphics::Gdi::ScreenToClient;

use crate::apps::AppEntry;
use crate::panel::NativeDrag;

struct State {
    hovering: bool,
    pos: Option<[f32; 2]>,
    dropped: Vec<Result<AppEntry, String>>,
    drop_pos: Option<[f32; 2]>,
    waker: Option<crate::Waker>,
}

static STATE: Mutex<State> = Mutex::new(State {
    hovering: false,
    pos: None,
    dropped: Vec::new(),
    drop_pos: None,
    waker: None,
});

fn update(f: impl FnOnce(&mut State)) {
    let mut s = STATE.lock().unwrap();
    f(&mut s);
    if let Some(w) = s.waker.as_ref() {
        w();
    }
}

pub fn set_waker(w: crate::Waker) {
    STATE.lock().unwrap().waker = Some(w);
}

/// Current drag state; dropped items are handed over once.
pub fn poll() -> NativeDrag {
    let mut s = STATE.lock().unwrap();
    NativeDrag {
        hovering: s.hovering,
        pos: s.pos,
        dropped: std::mem::take(&mut s.dropped),
        drop_pos: s.drop_pos.take(),
    }
}

/// Replaces winit's drop target with ours.
pub fn register(hwnd: HWND) {
    unsafe {
        let _ = RevokeDragDrop(hwnd);
        let target: IDropTarget = Target { hwnd: hwnd.0 as isize }.into();
        match RegisterDragDrop(hwnd, &target) {
            Ok(()) => log::debug!("sidebar drop target registered"),
            Err(e) => log::warn!("cannot register the sidebar as a drop target: {e}"),
        }
    }
}

pub fn revoke(hwnd: HWND) {
    unsafe {
        let _ = RevokeDragDrop(hwnd);
    }
}

// Not agile: an agile object hands COM the free-threaded marshaler, and
// cross-process drag calls from Explorer / the Start menu never arrive. A
// plain apartment-bound object (like winit's) gets them on the UI thread.
#[implement(IDropTarget, Agile = false)]
struct Target {
    hwnd: isize,
}

impl Target {
    fn client_pos(&self, pt: &POINTL) -> [f32; 2] {
        let mut p = POINT { x: pt.x, y: pt.y };
        unsafe {
            let _ = ScreenToClient(HWND(self.hwnd as *mut _), &mut p);
        }
        [p.x as f32, p.y as f32]
    }
}

/// Link when the source allows it (a shortcut, nothing moves), else copy.
/// Never "move": the source would delete what was dragged.
fn choose(effect: *mut DROPEFFECT) {
    if effect.is_null() {
        return;
    }
    unsafe {
        let allowed = *effect;
        *effect = if allowed.0 & DROPEFFECT_LINK.0 != 0 {
            DROPEFFECT_LINK
        } else if allowed.0 & DROPEFFECT_COPY.0 != 0 {
            DROPEFFECT_COPY
        } else {
            DROPEFFECT_NONE
        };
    }
}

fn items(data: &IDataObject) -> Option<IShellItemArray> {
    unsafe { SHCreateShellItemArrayFromDataObject::<_, IShellItemArray>(data).ok() }
}

fn display_name(item: &IShellItem, kind: windows::Win32::UI::Shell::SIGDN) -> Option<String> {
    unsafe {
        let p = item.GetDisplayName(kind).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

/// A file, folder or shortcut becomes a path favourite; a Start-menu app
/// (no file path, but an AppUserModelID) becomes an AppsFolder favourite.
fn entry(item: &IShellItem) -> Result<AppEntry, String> {
    if let Some(path) = display_name(item, SIGDN_FILESYSPATH) {
        return super::apps::from_path(std::path::Path::new(&path)).ok_or(path);
    }
    let name = display_name(item, SIGDN_NORMALDISPLAY).unwrap_or_else(|| "that item".into());
    let aumid = item.cast::<IShellItem2>().ok().and_then(|i2| unsafe {
        let p = i2.GetString(&super::apps::PKEY_APP_USER_MODEL_ID).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    });
    match aumid.filter(|a| !a.is_empty()) {
        Some(aumid) => Ok(super::apps::apps_folder_entry(name, aumid)),
        None => Err(name),
    }
}

impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let ok = data.as_ref().and_then(items).is_some();
        log::debug!("drag enter: shell items readable = {ok}");
        if ok {
            choose(effect);
            let pos = self.client_pos(pt);
            update(|s| {
                s.hovering = true;
                s.pos = Some(pos);
            });
        } else if !effect.is_null() {
            unsafe { *effect = DROPEFFECT_NONE };
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let hovering = STATE.lock().unwrap().hovering;
        if hovering {
            choose(effect);
            let pos = self.client_pos(pt);
            update(|s| s.pos = Some(pos));
        } else if !effect.is_null() {
            unsafe { *effect = DROPEFFECT_NONE };
        }
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        update(|s| {
            s.hovering = false;
            s.pos = None;
        });
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let pos = self.client_pos(pt);
        let mut out = Vec::new();
        if let Some(arr) = data.as_ref().and_then(items) {
            let n = unsafe { arr.GetCount() }.unwrap_or(0);
            for i in 0..n {
                if let Ok(item) = unsafe { arr.GetItemAt(i) } {
                    out.push(entry(&item));
                }
            }
        }
        log::debug!("drop: {} item(s): {:?}", out.len(), out.iter().map(|r| match r { Ok(e) => e.name.clone(), Err(e) => format!("unsupported {e}") }).collect::<Vec<_>>());
        if out.is_empty() {
            if !effect.is_null() {
                unsafe { *effect = DROPEFFECT_NONE };
            }
        } else {
            choose(effect);
        }
        update(|s| {
            s.hovering = false;
            s.pos = None;
            s.dropped.extend(out);
            s.drop_pos = Some(pos);
        });
        Ok(())
    }
}
