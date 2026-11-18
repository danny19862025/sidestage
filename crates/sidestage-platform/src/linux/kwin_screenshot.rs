//! Still window previews through KWin's `org.kde.KWin.ScreenShot2` D-Bus API.
//!
//! KWin may restrict this interface to applications whose installed
//! `.desktop` file lists it in `X-KDE-DBUS-Restricted-Interfaces`; the
//! packaged `sidestage.desktop` does. When the call is refused we report
//! "no thumbnails" and the UI shows the icon-and-title fallback.

use std::collections::HashMap;
use std::io::Read;
use std::os::fd::{AsFd, OwnedFd};

use anyhow::{anyhow, bail, Context, Result};
use sidestage_core::RgbaImage;
use zbus::zvariant::{Fd, OwnedValue, Value};

pub struct KwinScreenshot {
    conn: zbus::blocking::Connection,
}

impl KwinScreenshot {
    pub fn new() -> Result<Self> {
        Ok(Self { conn: zbus::blocking::Connection::session()? })
    }

    /// Captures a window by its KWin internal UUID (`{xxxxxxxx-...}`).
    pub fn capture_window(&self, uuid: &str, max_size: u32) -> Result<RgbaImage> {
        let (reader, writer): (std::os::unix::net::UnixStream, std::os::unix::net::UnixStream) =
            std::os::unix::net::UnixStream::pair()?;
        let mut options: HashMap<&str, Value> = HashMap::new();
        options.insert("include-decoration", Value::from(false));
        options.insert("include-shadow", Value::from(false));
        options.insert("native-resolution", Value::from(false));
        let writer_fd: OwnedFd = writer.into();
        // Read concurrently: KWin writes after replying, and a full pipe
        // would otherwise block it.
        let reader_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut r = reader;
            r.read_to_end(&mut buf).map(|_| buf)
        });
        let reply = self.conn.call_method(
            Some("org.kde.KWin"),
            "/org/kde/KWin/ScreenShot2",
            Some("org.kde.KWin.ScreenShot2"),
            "CaptureWindow",
            &(uuid, options, Fd::from(writer_fd.as_fd())),
        );
        drop(writer_fd);
        let reply = reply.context("KWin refused CaptureWindow")?;
        let meta: HashMap<String, OwnedValue> = reply.body().deserialize()?;
        let data = reader_thread.join().map_err(|_| anyhow!("reader panicked"))??;
        let get = |k: &str| -> Result<u32> {
            meta.get(k)
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| anyhow!("missing {k} in screenshot metadata"))
        };
        let (w, h, stride, format) = (get("width")?, get("height")?, get("stride")?, get("format")?);
        if w == 0 || h == 0 || data.len() < (stride * h) as usize {
            bail!("short screenshot ({} bytes for {w}x{h})", data.len());
        }
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for row in 0..h as usize {
            pixels.extend_from_slice(&data[row * stride as usize..row * stride as usize + w as usize * 4]);
        }
        // QImage::Format values: 4 RGB32, 5 ARGB32, 6 ARGB32_Premultiplied
        // (BGRA bytes on little endian); 16 RGBX8888, 17 RGBA8888,
        // 18 RGBA8888_Premultiplied.
        let img = match format {
            4 | 16 => {
                for px in pixels.as_chunks_mut::<4>().0 {
                    px[3] = 255;
                }
                if format == 4 {
                    RgbaImage::from_bgra(w, h, pixels, false)
                } else {
                    RgbaImage::new(w, h, pixels)
                }
            }
            5 => RgbaImage::from_bgra(w, h, pixels, false),
            6 => RgbaImage::from_bgra(w, h, pixels, true),
            17 => RgbaImage::new(w, h, pixels),
            18 => {
                // Swap twice = no-op on order; reuse the unpremultiply path.
                for px in pixels.as_chunks_mut::<4>().0 {
                    px.swap(0, 2);
                }
                RgbaImage::from_bgra(w, h, pixels, true)
            }
            other => bail!("unsupported QImage format {other}"),
        }
        .ok_or_else(|| anyhow!("bad image buffer"))?;
        if img.is_blank() {
            bail!("KWin returned an empty image");
        }
        Ok(img.downscaled(max_size))
    }
}
