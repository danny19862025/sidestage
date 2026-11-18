//! Test helper: synthesises pointer input with XTEST (X11/XWayland only).
//! `xinput move X Y` | `xinput click X Y [button]` | `xinput geometry WINDOW_NAME`

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("this test helper is Linux-only");
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    imp::main()
}

#[cfg(target_os = "linux")]
mod imp {

    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{self, ConnectionExt as _};
    use x11rb::protocol::xtest::ConnectionExt as _;

    pub fn main() -> Result<(), Box<dyn std::error::Error>> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        let num = |i: usize| args.get(i).and_then(|s| s.parse::<i16>().ok()).unwrap_or(0);
        match args.first().map(String::as_str) {
            Some("move") => {
                conn.xtest_fake_input(xproto::MOTION_NOTIFY_EVENT, 0, 0, root, num(1), num(2), 0)?;
            }
            Some("click") => {
                let button = args.get(3).and_then(|s| s.parse::<u8>().ok()).unwrap_or(1);
                conn.xtest_fake_input(xproto::MOTION_NOTIFY_EVENT, 0, 0, root, num(1), num(2), 0)?;
                conn.flush()?;
                std::thread::sleep(std::time::Duration::from_millis(120));
                conn.xtest_fake_input(xproto::BUTTON_PRESS_EVENT, button, 0, root, 0, 0, 0)?;
                conn.flush()?;
                std::thread::sleep(std::time::Duration::from_millis(60));
                conn.xtest_fake_input(xproto::BUTTON_RELEASE_EVENT, button, 0, root, 0, 0, 0)?;
            }
            _ => eprintln!("usage: xinput move X Y | click X Y [button]"),
        }
        conn.flush()?;
        conn.get_input_focus()?.reply()?;
        Ok(())
    }
}
