//! Edge-panel placement for the sidebar window.

use raw_window_handle::RawWindowHandle;
pub use sidestage_core::config::Edge;
use sidestage_core::WindowId;

/// A monitor in physical pixels (virtual-desktop coordinates).
#[derive(Clone, Debug, PartialEq)]
pub struct MonitorInfo {
    pub name: String,
    pub index: usize,
    pub primary: bool,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// Usable area excluding taskbars/panels, when known.
    pub work: (i32, i32, i32, i32),
    /// Display scale factor (1.0 = 96 DPI) when known.
    pub scale: f32,
}

pub fn monitors() -> Vec<MonitorInfo> {
    crate::imp::panel::monitors()
}

/// Where the sidebar should be, in physical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelGeometry {
    pub monitor: MonitorInfo,
    pub edge: Edge,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// Width to reserve at the edge so maximized windows avoid the sidebar
    /// (0 = overlay without reserving).
    pub reserve: i32,
    /// Auto-hidden: only a strip this wide (at the screen edge) should react
    /// to the pointer. Linux keeps the window size and narrows its input
    /// shape; Windows shrinks the window.
    pub collapsed_strip: Option<i32>,
    /// Auto-hide is enabled (the platform may need to watch the pointer).
    pub auto_hide: bool,
}

/// Draws live previews into rectangles of the sidebar (Windows DWM).
pub trait LiveThumbnails {
    /// Starts a frame; previews not placed again before [`end`](Self::end) are removed.
    fn begin(&mut self);
    /// Shows `id` inside `dest` (client-area physical pixels, `[x, y, w, h]`)
    /// clipped to `clip`. Returns the source window size when the preview is
    /// available, `None` if the compositor cannot provide one.
    fn place(&mut self, id: &WindowId, dest: [i32; 4], clip: [i32; 4]) -> Option<(i32, i32)>;
    /// Hides all previews this frame (e.g. a menu is open over them).
    fn hide_all(&mut self);
    fn end(&mut self);
}

/// Platform handle to the sidebar's native window.
pub struct Panel(crate::imp::panel::Panel);

impl Panel {
    /// Marks the window as a dock/tool window that does not take focus.
    pub fn attach(handle: RawWindowHandle) -> anyhow::Result<Panel> {
        crate::imp::panel::Panel::attach(handle).map(Panel)
    }

    /// Moves/resizes the window and updates reserved space.
    pub fn apply(&mut self, geometry: &PanelGeometry) {
        self.0.apply(geometry)
    }

    /// Allow (search field focused) or forbid keyboard focus.
    pub fn set_keyboard_focus(&mut self, wanted: bool) {
        self.0.set_keyboard_focus(wanted)
    }

    pub fn live_thumbnails(&mut self) -> Option<&mut dyn LiveThumbnails> {
        self.0.live_thumbnails()
    }

    /// Pointer-inside state tracked by the platform when the toolkit's own
    /// enter/leave reporting is unreliable (XWayland). `None` = use the toolkit.
    pub fn pointer_inside(&mut self) -> Option<bool> {
        self.0.pointer_inside()
    }

    /// Blur the desktop behind these window rectangles (physical px,
    /// `[x, y, w, h]`), or turn blur off. KDE only; elsewhere a no-op.
    pub fn set_blur_region(&mut self, rects: Option<Vec<[i32; 4]>>) {
        self.0.set_blur_region(rects)
    }

    /// Releases reserved space (call before exit).
    pub fn detach(&mut self) {
        self.0.detach()
    }
}
