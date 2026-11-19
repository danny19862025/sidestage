//! `RegisterHotKey` on a dedicated message-loop thread.

use std::sync::mpsc::{self, Receiver, Sender};

use sidestage_core::shortcut::Shortcut;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, PeekMessageW, PostThreadMessageW, MSG, PM_NOREMOVE, WM_APP, WM_HOTKEY,
};

use crate::hotkey::{HotkeyStatus, OnPress};

const HOTKEY_ID: i32 = 0x5157;
const ERROR_HOTKEY_ALREADY_REGISTERED: u32 = 1409;

pub struct Hotkey {
    thread_id: u32,
    req: Sender<Option<Shortcut>>,
    resp: Receiver<HotkeyStatus>,
}

impl Hotkey {
    pub fn new(on_press: OnPress) -> Hotkey {
        let (req_tx, req_rx) = mpsc::channel::<Option<Shortcut>>();
        let (resp_tx, resp_rx) = mpsc::channel();
        let (id_tx, id_rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("win-hotkey".into())
            .spawn(move || run(on_press, req_rx, resp_tx, id_tx))
            .expect("hotkey thread");
        let thread_id = id_rx.recv().unwrap_or(0);
        Hotkey {
            thread_id,
            req: req_tx,
            resp: resp_rx,
        }
    }

    pub fn set(&mut self, s: Option<Shortcut>) -> HotkeyStatus {
        if self.req.send(s).is_err() {
            return HotkeyStatus::Unsupported("hotkey thread stopped".into());
        }
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_APP, WPARAM(0), LPARAM(0));
        }
        self.resp
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap_or_else(|_| HotkeyStatus::Unsupported("Windows did not answer".into()))
    }
}

fn run(
    on_press: OnPress,
    req: Receiver<Option<Shortcut>>,
    resp: Sender<HotkeyStatus>,
    id: Sender<u32>,
) {
    let mut msg = MSG::default();
    unsafe {
        // Create the message queue before announcing the thread id.
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        let _ = id.send(GetCurrentThreadId());
        let mut active = false;
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_HOTKEY && msg.wParam.0 as i32 == HOTKEY_ID {
                on_press();
            } else if msg.message == WM_APP {
                while let Ok(s) = req.try_recv() {
                    if active {
                        let _ = UnregisterHotKey(None, HOTKEY_ID);
                        active = false;
                    }
                    let status = match s {
                        None => HotkeyStatus::Disabled,
                        Some(s) => {
                            let (mods, vk) = s.to_win32();
                            match RegisterHotKey(
                                None,
                                HOTKEY_ID,
                                HOT_KEY_MODIFIERS(mods) | MOD_NOREPEAT,
                                vk,
                            ) {
                                Ok(()) => {
                                    active = true;
                                    HotkeyStatus::Registered(format!(
                                        "{} via Windows hotkey",
                                        s.display()
                                    ))
                                }
                                // The error carries HRESULT_FROM_WIN32(code).
                                Err(e)
                                    if (e.code().0 as u32 & 0xFFFF)
                                        == ERROR_HOTKEY_ALREADY_REGISTERED =>
                                {
                                    HotkeyStatus::Conflict(format!(
                                        "{} is already used by another application",
                                        s.display()
                                    ))
                                }
                                Err(e) => HotkeyStatus::Conflict(format!(
                                    "Windows refused {}: {e}",
                                    s.display()
                                )),
                            }
                        }
                    };
                    let _ = resp.send(status);
                }
            }
        }
    }
}
