//! Launch at login: `HKCU\...\Run` on Windows, XDG autostart on Linux.

pub fn set_enabled(enabled: bool) -> anyhow::Result<()> {
    crate::imp::set_launch_at_login(enabled)
}

pub fn is_enabled() -> bool {
    crate::imp::launch_at_login_enabled()
}
