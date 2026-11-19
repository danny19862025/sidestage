//! Platform integration for Sidestage.
//!
//! Everything that talks to a window manager, shell or OS service lives
//! here behind a small, OS-neutral API so `sidestage-core` and the UI stay
//! portable:
//!
//! * [`start_window_backend`] — reports top-level windows (minimized state,
//!   title, app) as [`WindowEvent`]s and restores/focuses them on request.
//! * [`apps`] — installed applications for the "Add favourite" picker and
//!   for turning a dropped file into a favourite.
//! * [`launch`] — starts a favourite.
//! * [`icons`] — loads app icons as RGBA.
//! * [`panel`] — places the sidebar window at a screen edge, reserves space,
//!   and keeps it from taking keyboard focus.
//! * [`autostart`] — launch at login.

use std::sync::mpsc::Sender;
use std::sync::Arc;

pub use sidestage_core::tracker::{BackendStatus, WindowEvent};
pub use sidestage_core::{RgbaImage, WindowId};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as imp;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as imp;

pub mod apps;
pub mod autostart;
pub mod icons;
pub mod launch;
pub mod panel;

/// Called by backends after sending events so the UI thread wakes up.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// How previews of minimized windows are produced on this backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThumbnailMode {
    /// No previews; the UI shows the app icon and title.
    None,
    /// Still images delivered as [`WindowEvent::Thumbnail`].
    Snapshot,
    /// The compositor draws live previews into rectangles of the sidebar
    /// window (Windows DWM thumbnails); see [`panel::LiveThumbnails`].
    Live,
}

/// A running window-integration backend.
pub trait WindowBackend: Send {
    /// Short identifier, e.g. `"win32"`, `"kwin-wayland"`, `"gnome-shell"`, `"x11-ewmh"`.
    fn name(&self) -> &'static str;
    fn thumbnail_mode(&self) -> ThumbnailMode;
    /// Un-minimizes and focuses a window.
    fn restore(&self, id: &WindowId);
    /// Asks for a fresh still preview (Snapshot mode only). Best effort.
    fn refresh_thumbnail(&self, _id: &WindowId) {}
    /// Restores several windows; the last one ends up focused. Ids of
    /// windows that have closed meanwhile are ignored.
    fn restore_many(&self, ids: &[WindowId]) {
        for id in ids {
            self.restore(id);
        }
    }
}

/// Picks and starts the best backend for the current session.
///
/// On Linux: KWin Wayland protocol → GNOME Shell extension → X11/EWMH.
/// Events go to `tx`; `waker` is invoked after each batch.
pub fn start_window_backend(tx: Sender<WindowEvent>, waker: Waker) -> Box<dyn WindowBackend> {
    imp::start_window_backend(tx, waker)
}

/// Light/dark preference of the desktop, if it can be read.
pub fn system_prefers_dark() -> Option<bool> {
    imp::system_prefers_dark()
}

/// Prepares a background thread for platform calls (COM on Windows).
pub fn init_worker_thread() {
    imp::init_worker_thread()
}

/// Records the session's Wayland socket before the UI hides it (Linux).
#[cfg(target_os = "linux")]
pub fn linux_wayland_display() -> Option<String> {
    linux::wayland_display()
}

/// Windows: ask an already-running instance to open Settings or quit.
#[cfg(windows)]
pub fn signal_running_instance(quit: bool) {
    imp::signal_running_instance(quit)
}

/// Windows: called when another instance signals this one (`true` = quit).
#[cfg(windows)]
pub fn on_instance_signal(cb: Box<dyn Fn(bool) + Send>) {
    imp::on_instance_signal(cb)
}

/// Human-readable description of the session, for the About/diagnostics view.
pub fn session_description() -> String {
    imp::session_description()
}
