//! Test helper for nested KWin sessions: pointer input through KWin's
//! `org_kde_kwin_fake_input` protocol (no XTEST permission prompt).
//! `fakeinput click X Y [right]` | `move X Y` | `glide X1 Y1 X2 Y2` | `drag X1 Y1 X2 Y2`

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

    use std::time::Duration;

    use wayland_client::globals::{registry_queue_init, GlobalListContents};
    use wayland_client::protocol::wl_registry;
    use wayland_client::{Connection, Dispatch, QueueHandle};
    use wayland_protocols_plasma::fake_input::client::org_kde_kwin_fake_input::OrgKdeKwinFakeInput;

    struct S;
    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for S {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    impl Dispatch<OrgKdeKwinFakeInput, ()> for S {
        fn event(
            _: &mut Self,
            _: &OrgKdeKwinFakeInput,
            _: <OrgKdeKwinFakeInput as wayland_client::Proxy>::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    const BTN_LEFT: u32 = 0x110;
    const BTN_RIGHT: u32 = 0x111;

    pub fn main() -> Result<(), Box<dyn std::error::Error>> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let n = |i: usize| {
            args.get(i)
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0)
        };
        let conn = Connection::connect_to_env()?;
        let (globals, mut queue) = registry_queue_init::<S>(&conn)?;
        let qh = queue.handle();
        let fi: OrgKdeKwinFakeInput = globals.bind(&qh, 4..=5, ())?;
        fi.authenticate(
            "sidestage-tests".into(),
            "UI tests in a nested session".into(),
        );
        let sync = |queue: &mut wayland_client::EventQueue<S>| {
            queue.roundtrip(&mut S).ok();
        };
        let pause = |ms| std::thread::sleep(Duration::from_millis(ms));
        match args.first().map(String::as_str) {
            Some("move") => fi.pointer_motion_absolute(n(1), n(2)),
            Some("click") => {
                let b = if args.get(3).map(|s| s == "right").unwrap_or(false) {
                    BTN_RIGHT
                } else {
                    BTN_LEFT
                };
                fi.pointer_motion_absolute(n(1), n(2));
                sync(&mut queue);
                pause(150);
                fi.button(b, 1);
                sync(&mut queue);
                pause(70);
                fi.button(b, 0);
            }
            Some("keys") | Some("type") => {
                // `keys ctrl+alt+space` presses a chord; `type text` types it.
                let code = |k: &str| -> Option<u32> {
                    Some(match k {
                        "ctrl" => 29,
                        "alt" => 56,
                        "shift" => 42,
                        "meta" | "super" => 125,
                        "space" | " " => 57,
                        "esc" | "escape" => 1,
                        "enter" | "return" => 28,
                        "up" => 103,
                        "down" => 108,
                        "tab" => 15,
                        "backspace" => 14,
                        "." => 52,
                        "-" => 12,
                        _ if k.len() == 1 => {
                            let c = k.chars().next()?;
                            let row = "qwertyuiop"
                                .find(c)
                                .map(|i| 16 + i as u32)
                                .or_else(|| "asdfghjkl".find(c).map(|i| 30 + i as u32))
                                .or_else(|| "zxcvbnm".find(c).map(|i| 44 + i as u32))
                                .or_else(|| "1234567890".find(c).map(|i| 2 + i as u32));
                            row?
                        }
                        _ => return None,
                    })
                };
                let arg = args.get(1).cloned().unwrap_or_default();
                let groups: Vec<Vec<u32>> = if args[0] == "keys" {
                    vec![arg
                        .split('+')
                        .filter_map(|k| code(&k.to_lowercase()))
                        .collect()]
                } else {
                    arg.chars()
                        .filter_map(|c| code(&c.to_lowercase().to_string()))
                        .map(|c| vec![c])
                        .collect()
                };
                for chord in groups {
                    for k in &chord {
                        fi.keyboard_key(*k, 1);
                        sync(&mut queue);
                        pause(20);
                    }
                    for k in chord.iter().rev() {
                        fi.keyboard_key(*k, 0);
                        sync(&mut queue);
                        pause(20);
                    }
                    pause(30);
                }
            }
            Some("glide") => {
                // Continuous motion like a real mouse.
                for i in 0..=30 {
                    let t = i as f64 / 30.0;
                    fi.pointer_motion_absolute(n(1) + (n(3) - n(1)) * t, n(2) + (n(4) - n(2)) * t);
                    sync(&mut queue);
                    pause(15);
                }
            }
            Some("dragpath") => {
                // dragpath X0 Y0 X1 Y1 ... : press at the first point, glide
                // through each waypoint (lingering ~1 s), release at the last.
                let pts: Vec<(f64, f64)> = (1..args.len())
                    .step_by(2)
                    .map(|i| (n(i), n(i + 1)))
                    .collect();
                fi.pointer_motion_absolute(pts[0].0, pts[0].1);
                sync(&mut queue);
                pause(150);
                fi.button(BTN_LEFT, 1);
                sync(&mut queue);
                for w in pts.windows(2) {
                    let ((ax, ay), (bx, by)) = (w[0], w[1]);
                    for i in 1..=20 {
                        let t = i as f64 / 20.0;
                        fi.pointer_motion_absolute(ax + (bx - ax) * t, ay + (by - ay) * t);
                        sync(&mut queue);
                        pause(25);
                    }
                    for k in 0..12 {
                        fi.pointer_motion_absolute(bx + if k % 2 == 0 { 1.0 } else { -1.0 }, by);
                        sync(&mut queue);
                        pause(80);
                    }
                }
                pause(300);
                fi.button(BTN_LEFT, 0);
            }
            Some("drag") => {
                fi.pointer_motion_absolute(n(1), n(2));
                sync(&mut queue);
                pause(150);
                fi.button(BTN_LEFT, 1);
                sync(&mut queue);
                for i in 1..=20 {
                    let t = i as f64 / 20.0;
                    fi.pointer_motion_absolute(n(1) + (n(3) - n(1)) * t, n(2) + (n(4) - n(2)) * t);
                    sync(&mut queue);
                    pause(25);
                }
                pause(150);
                fi.button(BTN_LEFT, 0);
            }
            _ => eprintln!("usage: fakeinput click X Y [right] | move X Y | drag X1 Y1 X2 Y2"),
        }
        sync(&mut queue);
        Ok(())
    }
}
