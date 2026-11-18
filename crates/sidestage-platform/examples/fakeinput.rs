//! Test helper for nested KWin sessions: pointer input through KWin's
//! `org_kde_kwin_fake_input` protocol (no XTEST permission prompt).
//! `fakeinput click X Y [right]` | `move X Y` | `glide X1 Y1 X2 Y2` | `drag X1 Y1 X2 Y2`

use std::time::Duration;

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_plasma::fake_input::client::org_kde_kwin_fake_input::OrgKdeKwinFakeInput;

struct S;
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for S {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<OrgKdeKwinFakeInput, ()> for S {
    fn event(_: &mut Self, _: &OrgKdeKwinFakeInput, _: <OrgKdeKwinFakeInput as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let n = |i: usize| args.get(i).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<S>(&conn)?;
    let qh = queue.handle();
    let fi: OrgKdeKwinFakeInput = globals.bind(&qh, 4..=5, ())?;
    fi.authenticate("sidestage-tests".into(), "UI tests in a nested session".into());
    let sync = |queue: &mut wayland_client::EventQueue<S>| {
        queue.roundtrip(&mut S).ok();
    };
    let pause = |ms| std::thread::sleep(Duration::from_millis(ms));
    match args.first().map(String::as_str) {
        Some("move") => fi.pointer_motion_absolute(n(1), n(2)),
        Some("click") => {
            let b = if args.get(3).map(|s| s == "right").unwrap_or(false) { BTN_RIGHT } else { BTN_LEFT };
            fi.pointer_motion_absolute(n(1), n(2));
            sync(&mut queue);
            pause(150);
            fi.button(b, 1);
            sync(&mut queue);
            pause(70);
            fi.button(b, 0);
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
