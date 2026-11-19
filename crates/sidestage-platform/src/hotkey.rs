//! Configurable global shortcut that opens Sidestage search.
//!
//! * KDE (Wayland and X11): KGlobalAccel over D-Bus — conflicts are
//!   detected and the owning application is named.
//! * GNOME: the Sidestage Shell extension grabs the accelerator; Mutter
//!   refuses accelerators that are already taken.
//! * Other X11 desktops: `XGrabKey` on the root window (BadAccess = taken).
//! * Windows: `RegisterHotKey` (fails if another app owns the combination).
//! * Anything else: reported as unsupported; the search button still works.

use std::sync::Arc;

/// Outcome of registering the shortcut, shown in Settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotkeyStatus {
    Disabled,
    /// Active; the text names the mechanism.
    Registered(String),
    /// Another application or the desktop already uses the combination.
    Conflict(String),
    /// This desktop offers no verifiable way to register it.
    Unsupported(String),
    /// The text could not be parsed.
    Invalid(String),
}

impl HotkeyStatus {
    pub fn is_ok(&self) -> bool {
        matches!(self, HotkeyStatus::Registered(_) | HotkeyStatus::Disabled)
    }
}

pub type OnPress = Arc<dyn Fn() + Send + Sync>;

pub struct Hotkey(crate::imp::hotkey::Hotkey);

impl Hotkey {
    pub fn new(on_press: OnPress) -> Hotkey {
        Hotkey(crate::imp::hotkey::Hotkey::new(on_press))
    }

    /// A status change detected in the background (e.g. the GNOME
    /// extension was enabled after start-up), if any since the last call.
    pub fn take_status_update(&self) -> Option<HotkeyStatus> {
        self.0.take_status_update()
    }

    /// Registers `text` (e.g. `Ctrl+Alt+Space`); an empty string disables it.
    pub fn set(&mut self, text: &str) -> HotkeyStatus {
        if text.trim().is_empty() {
            self.0.set(None);
            return HotkeyStatus::Disabled;
        }
        match sidestage_core::shortcut::Shortcut::parse(text) {
            Ok(s) => self.0.set(Some(s)),
            Err(e) => HotkeyStatus::Invalid(e),
        }
    }
}
