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
    /// Windows: how far (physical px) the whole window is moved past the
    /// screen edge for the slide animation; 0 = fully shown, the window
    /// width = fully hidden. Windows can't show the GL surface's transparent
    /// pixels (they come out black), so instead of drawing the panel at an
    /// offset inside a fixed window, the shaped window itself slides.
    /// Linux ignores it.
    pub slide: i32,
    /// A fullscreen app covers this monitor: no drawing, input, reserved
    /// space or edge reveal until it ends.
    pub paused: bool,
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

/// A file or app drag the platform tracks itself (Windows: the sidebar's own
/// OLE drop target, which also accepts Start-menu apps). Positions are in
/// client-area physical pixels.
#[derive(Default)]
pub struct NativeDrag {
    /// Something droppable is over the sidebar.
    pub hovering: bool,
    pub pos: Option<[f32; 2]>,
    /// Items dropped since the last poll; `Err` names what couldn't be added.
    pub dropped: Vec<Result<crate::apps::AppEntry, String>>,
    /// Where the last drop happened.
    pub drop_pos: Option<[f32; 2]>,
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

    /// Takes keyboard focus now that the window manager activated the
    /// sidebar (needed for X11's globally-active input model).
    pub fn claim_keyboard(&mut self) {
        self.0.claim_keyboard()
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

    /// Windows: trims the window to these rectangles (physical px,
    /// `[x, y, w, h]`, window-relative) so no transparent pixel is ever
    /// shown (they would come out black). Set once for the panel's resting
    /// shape; repeated identical calls are free. Linux: no-op.
    pub fn set_shape(&mut self, rects: Vec<[i32; 4]>) {
        self.0.set_shape(rects)
    }

    /// Drag-and-drop the platform handles itself, or `None` to use the
    /// toolkit's file-drop events (Linux).
    pub fn poll_drag(&mut self) -> Option<NativeDrag> {
        self.0.poll_drag()
    }

    /// Called whenever the native drag state changes, to wake the UI.
    pub fn set_drag_waker(&mut self, waker: crate::Waker) {
        self.0.set_drag_waker(waker)
    }

    /// Releases reserved space (call before exit).
    pub fn detach(&mut self) {
        self.0.detach()
    }
}
