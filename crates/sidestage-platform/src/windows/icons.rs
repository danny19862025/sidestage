//! Shell icons and HICON/HBITMAP → RGBA conversion.

use std::path::Path;

use sidestage_core::RgbaImage;
use windows::core::HSTRING;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, HICON, ICONINFO};

/// Reads a 32-bit top-down copy of a bitmap. Returns premultiplied BGRA.
pub(crate) fn bitmap_pixels(hbm: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let mut bm = BITMAP::default();
        if GetObjectW(
            HGDIOBJ(hbm.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut _),
        ) == 0
        {
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
        if w <= 0 || h <= 0 {
            return None;
        }
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let dc = GetDC(None);
        let lines = GetDIBits(
            dc,
            hbm,
            0,
            h as u32,
            Some(px.as_mut_ptr() as *mut _),
            &mut info,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, dc);
        (lines == h).then_some((w as u32, h as u32, px))
    }
}

/// Converts an HICON (window or class icon) to RGBA. The icon is not destroyed.
pub(crate) fn hicon_to_rgba(icon: HICON) -> Option<RgbaImage> {
    unsafe {
        let mut info = ICONINFO::default();
        GetIconInfo(icon, &mut info).ok()?;
        let color = bitmap_pixels(info.hbmColor);
        let mask = bitmap_pixels(info.hbmMask);
        let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
        let (w, h, mut px) = color?;
        // Icons without alpha (old 24-bit) carry transparency in the AND mask.
        if px.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
            if let Some((mw, _, m)) = mask {
                for (i, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                    let (x, y) = (i as u32 % w, i as u32 / w);
                    let mi = ((y * mw + x) * 4) as usize;
                    p[3] = if m.get(mi).copied().unwrap_or(0) == 0 {
                        255
                    } else {
                        0
                    };
                }
            }
            return RgbaImage::from_bgra(w, h, px, false);
        }
        RgbaImage::from_bgra(w, h, px, false)
    }
}

/// Icon for a file, folder, shortcut or `shell:AppsFolder\…` item.
pub fn shell_icon(parsing_name: &str, size: u32) -> Option<RgbaImage> {
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(&HSTRING::from(parsing_name), None).ok()?;
        let s = size.clamp(16, 256) as i32;
        let hbm = factory
            .GetImage(SIZE { cx: s, cy: s }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK)
            .ok()?;
        let out = bitmap_pixels(hbm);
        let _ = DeleteObject(HGDIOBJ(hbm.0));
        let (w, h, px) = out?;
        // IShellItemImageFactory returns premultiplied alpha.
        RgbaImage::from_bgra(w, h, px, true)
    }
}

/// Resolves a bare program name using PATH and PATHEXT.
pub fn find_on_path(program: &str) -> Option<String> {
    let p = Path::new(program);
    if p.is_absolute() {
        return p.exists().then(|| program.to_string());
    }
    let exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
        .split(';')
        .map(|s| s.to_ascii_lowercase())
        .collect();
    for dir in std::env::split_paths(&std::env::var_os("PATH")?) {
        let cand = dir.join(program);
        if cand.is_file() {
            return Some(cand.display().to_string());
        }
        for ext in &exts {
            let c = dir.join(format!("{program}{ext}"));
            if c.is_file() {
                return Some(c.display().to_string());
            }
        }
    }
    None
}

/// Executable registered for http:// links (the default browser).
pub fn default_browser() -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::UI::Shell::{AssocQueryStringW, ASSOCF_NONE, ASSOCSTR_EXECUTABLE};
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    unsafe {
        AssocQueryStringW(
            ASSOCF_NONE,
            ASSOCSTR_EXECUTABLE,
            &HSTRING::from("http"),
            &HSTRING::from("open"),
            Some(PWSTR(buf.as_mut_ptr())),
            &mut len,
        )
        .ok()
        .ok()?;
    }
    let s = String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)]);
    (!s.is_empty()).then_some(s)
}
