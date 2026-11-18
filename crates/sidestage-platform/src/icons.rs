//! Icon loading for favourites.

use sidestage_core::{IconSource, LaunchTarget, RgbaImage};

/// Loads the icon for a favourite at roughly `size` physical pixels.
/// May block on disk/COM; call from a worker thread.
pub fn favourite_icon(icon: &IconSource, target: &LaunchTarget, size: u32) -> Option<RgbaImage> {
    crate::imp::favourite_icon(icon, target, size)
}
