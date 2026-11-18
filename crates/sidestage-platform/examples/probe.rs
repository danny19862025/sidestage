//! Diagnostic: prints window events from the detected backend.
//!
//! `cargo run -p sidestage-platform --example probe -- [seconds] [restore-title-substring] [thumb-dir]`
//! With a title substring, the first matching window that becomes minimized
//! is restored after 2 seconds (exercises the real restore path). With a
//! directory, received previews are written there as PNG.

use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sidestage_core::{WindowEvent, WindowTracker};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let secs: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(5);
    let restore_title = args.get(2).cloned().filter(|s| !s.is_empty());
    let thumb_dir = args.get(3).cloned();
    let (tx, rx) = mpsc::channel();
    let backend = sidestage_platform::start_window_backend(tx, Arc::new(|| {}));
    println!("backend={} thumbnails={:?} session={}", backend.name(), backend.thumbnail_mode(), sidestage_platform::session_description());
    let mut tracker = WindowTracker::new();
    let end = Instant::now() + Duration::from_secs(secs);
    let mut restore_at: Option<(Instant, sidestage_core::WindowId)> = None;
    while Instant::now() < end {
        if let Some((t, id)) = restore_at.clone() {
            if Instant::now() >= t {
                println!("RESTORE {id}");
                backend.restore(&id);
                restore_at = None;
            }
        }
        let Ok(ev) = rx.recv_timeout(Duration::from_millis(100)) else { continue };
        match &ev {
            WindowEvent::Upsert(w) => {
                println!("UPSERT min={} app={} ({}) title={:?} id={}", w.minimized, w.app_key, w.app_name, w.title, w.id);
                if w.minimized {
                    if let Some(sub) = &restore_title {
                        if w.title.contains(sub.as_str()) && restore_at.is_none() {
                            restore_at = Some((Instant::now() + Duration::from_secs(2), w.id.clone()));
                        }
                    }
                }
            }
            WindowEvent::Removed(id) => println!("REMOVED {id}"),
            WindowEvent::Snapshot(l) => {
                println!("SNAPSHOT {} windows", l.len());
                for w in l {
                    println!("  min={} app={} ({}) title={:?} id={}", w.minimized, w.app_key, w.app_name, w.title, w.id);
                    if let (true, Some(sub), None) = (w.minimized, &restore_title, &restore_at) {
                        if w.title.contains(sub.as_str()) {
                            restore_at = Some((Instant::now() + Duration::from_secs(2), w.id.clone()));
                        }
                    }
                }
            }
            WindowEvent::Icon { app_key, image } => println!("ICON {app_key} {}x{}", image.width, image.height),
            WindowEvent::Thumbnail { id, image } => {
                println!("THUMB {id} {:?} blank={}", image, image.as_ref().map(|i| i.is_blank()).unwrap_or(true));
                if let (Some(dir), Some(img)) = (&thumb_dir, image) {
                    let path = format!("{dir}/thumb-{}.png", id.0.trim_matches(|c| c == '{' || c == '}'));
                    let _ = image_save(&path, img);
                    println!("  saved {path}");
                }
            }
            WindowEvent::Status(s) => println!("STATUS {s:?}"),
        }
        if tracker.apply(&ev) {
            let g = tracker.groups();
            println!("  -> minimized groups: {:?}", g.iter().map(|g| (g.app_name.clone(), g.windows.len())).collect::<Vec<_>>());
        }
    }
}

fn image_save(path: &str, img: &sidestage_core::RgbaImage) -> std::io::Result<()> {
    // Minimal uncompressed PNG writer to avoid extra deps in the example.
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn adler(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &d in data {
            a = (a + d as u32) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }
    let mut raw = Vec::new();
    for row in img.pixels.chunks_exact(img.width as usize * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    for (i, chunk) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(last as u8);
        z.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(chunk.len() as u16)).to_le_bytes());
        z.extend_from_slice(chunk);
    }
    z.extend_from_slice(&adler(&raw).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |ty: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = ty.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&img.width.to_be_bytes());
    ihdr.extend_from_slice(&img.height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    std::fs::write(path, out)
}
