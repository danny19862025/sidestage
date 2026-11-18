//! Launching favourites.

use sidestage_core::LaunchTarget;

/// Starts the target detached from Sidestage.
pub fn launch(target: &LaunchTarget) -> anyhow::Result<()> {
    crate::imp::launch(target)
}
