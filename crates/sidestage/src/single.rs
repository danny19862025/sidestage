//! Single-instance guard. A second launch asks the running instance to
//! open Settings and exits.

use std::sync::mpsc::Sender;

use crate::tray::TrayCommand;

pub enum Instance {
    Primary(Guard),
    Secondary,
}

pub struct Guard {
    #[cfg(target_os = "linux")]
    listener: Option<std::os::unix::net::UnixListener>,
    #[cfg(windows)]
    _mutex: Option<windows_mutex::Mutex>,
}

#[cfg(target_os = "linux")]
fn socket_name() -> String {
    // Abstract socket: per user and per display, no file to clean up.
    let uid = unsafe { libc_getuid() };
    let display = std::env::var("WAYLAND_DISPLAY").or_else(|_| std::env::var("DISPLAY")).unwrap_or_default();
    format!("sidestage-{uid}-{display}")
}

#[cfg(target_os = "linux")]
extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

/// What a second launch asks the running instance to do.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Settings,
    Quit,
}

#[cfg(target_os = "linux")]
pub fn acquire(req: Request) -> Instance {
    use std::io::Write;
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
    let name = socket_name();
    let Ok(addr) = SocketAddr::from_abstract_name(name.as_bytes()) else {
        return Instance::Primary(Guard { listener: None });
    };
    match UnixListener::bind_addr(&addr) {
        Ok(l) => Instance::Primary(Guard { listener: Some(l) }),
        Err(_) => {
            if let Ok(mut s) = UnixStream::connect_addr(&addr) {
                let _ = s.write_all(if req == Request::Quit { b"quit\n" } else { b"settings\n" });
                return Instance::Secondary;
            }
            Instance::Primary(Guard { listener: None })
        }
    }
}

#[cfg(target_os = "linux")]
impl Guard {
    pub fn listen(&mut self, tx: Sender<TrayCommand>, ctx: egui::Context) {
        let Some(l) = self.listener.take() else { return };
        std::thread::Builder::new()
            .name("single-instance".into())
            .spawn(move || {
                use std::io::Read;
                for mut s in l.incoming().flatten() {
                    let mut buf = [0u8; 32];
                    let n = s.read(&mut buf).unwrap_or(0);
                    let cmd = if buf[..n].starts_with(b"settings") {
                        TrayCommand::Settings
                    } else if buf[..n].starts_with(b"quit") {
                        TrayCommand::Quit
                    } else {
                        continue;
                    };
                    let _ = tx.send(cmd);
                    ctx.request_repaint();
                }
            })
            .ok();
    }
}

#[cfg(windows)]
mod windows_mutex {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    pub struct Mutex(HANDLE);
    unsafe impl Send for Mutex {}

    impl Mutex {
        /// `None` if another instance already owns the name.
        pub fn create(name: &str) -> Option<Option<Mutex>> {
            let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            let h = unsafe { CreateMutexW(std::ptr::null(), 1, wide.as_ptr()) };
            if h.is_null() {
                return Some(None);
            }
            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                unsafe { CloseHandle(h) };
                return None;
            }
            Some(Some(Mutex(h)))
        }
    }

    impl Drop for Mutex {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(windows)]
pub fn acquire(req: Request) -> Instance {
    match windows_mutex::Mutex::create("Local\\Sidestage.SingleInstance") {
        Some(m) => Instance::Primary(Guard { _mutex: m }),
        None => {
            // Ask the running instance to show Settings via a registered message.
            sidestage_platform::signal_running_instance(req == Request::Quit);
            Instance::Secondary
        }
    }
}

#[cfg(windows)]
impl Guard {
    pub fn listen(&mut self, tx: Sender<TrayCommand>, ctx: egui::Context) {
        sidestage_platform::on_instance_signal(Box::new(move |quit| {
            let _ = tx.send(if quit { TrayCommand::Quit } else { TrayCommand::Settings });
            ctx.request_repaint();
        }));
    }
}
