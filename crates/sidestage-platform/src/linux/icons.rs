//! Freedesktop icon-theme lookup and PNG/SVG rasterisation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use sidestage_core::RgbaImage;

/// Directories that may contain `icons/` (and `applications/`).
pub fn data_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        Some(d) => out.push(PathBuf::from(d)),
        None => {
            if let Some(h) = &home {
                out.push(h.join(".local/share"));
            }
        }
    }
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    out.extend(dirs.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    // Flatpak exports are missing from XDG_DATA_DIRS in some sessions.
    if let Some(h) = &home {
        out.push(h.join(".local/share/flatpak/exports/share"));
    }
    out.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert(p.clone()));
    out
}

fn current_theme() -> Option<String> {
    if let Ok(t) = std::env::var("SIDESTAGE_ICON_THEME") {
        return Some(t);
    }
    let desktops = super::desktops();
    let kde = desktops.iter().any(|d| d.eq_ignore_ascii_case("KDE"));
    if kde {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        let cfg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or(home.join(".config"));
        if let Ok(text) = std::fs::read_to_string(cfg.join("kdeglobals")) {
            let mut in_icons = false;
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    in_icons = line == "[Icons]";
                } else if in_icons {
                    if let Some(v) = line.strip_prefix("Theme=") {
                        return Some(v.trim().to_string());
                    }
                }
            }
        }
        return Some("breeze".into());
    }
    let out = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "icon-theme"])
        .output()
        .ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().trim_matches('\'').to_string();
    (!v.is_empty()).then_some(v)
}

fn theme_dirs(theme: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(h) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(h).join(".icons").join(theme));
    }
    for d in data_dirs() {
        dirs.push(d.join("icons").join(theme));
    }
    dirs.retain(|d| d.is_dir());
    dirs
}

fn inherits(theme: &str) -> Vec<String> {
    for d in theme_dirs(theme) {
        if let Ok(text) = std::fs::read_to_string(d.join("index.theme")) {
            for line in text.lines() {
                if let Some(v) = line.trim().strip_prefix("Inherits=") {
                    return v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                }
            }
        }
    }
    Vec::new()
}

/// Ordered list of themes to search: current, its parents, then fallbacks.
fn theme_chain() -> &'static [String] {
    static CHAIN: OnceLock<Vec<String>> = OnceLock::new();
    CHAIN.get_or_init(|| {
        let mut chain: Vec<String> = Vec::new();
        let mut queue: Vec<String> = current_theme().into_iter().collect();
        queue.extend(["breeze", "Adwaita", "Papirus", "hicolor"].map(String::from));
        while let Some(t) = (!queue.is_empty()).then(|| queue.remove(0)) {
            if chain.contains(&t) {
                continue;
            }
            let parents = inherits(&t);
            chain.push(t);
            for (i, p) in parents.into_iter().enumerate() {
                queue.insert(i, p);
            }
        }
        // hicolor must be last per spec.
        chain.retain(|t| t != "hicolor");
        chain.push("hicolor".into());
        log::debug!("icon theme chain: {chain:?}");
        chain
    })
}

#[derive(Clone)]
struct Candidate {
    path: PathBuf,
    /// Nominal pixel size; `u32::MAX` for scalable.
    size: u32,
}

type ThemeIndex = HashMap<String, Vec<Candidate>>;

fn size_from_path(path: &Path) -> u32 {
    for comp in path.iter().rev().skip(1) {
        let c = comp.to_string_lossy();
        if c == "scalable" || c == "symbolic" {
            return u32::MAX;
        }
        let head = c.split(['x', '@']).next().unwrap_or("");
        if let Ok(n) = head.parse::<u32>() {
            return n;
        }
    }
    if path.extension().map(|e| e == "svg").unwrap_or(false) {
        u32::MAX
    } else {
        32
    }
}

fn index_theme(theme: &str) -> ThemeIndex {
    let mut index: ThemeIndex = HashMap::new();
    for root in theme_dirs(theme) {
        let mut stack = vec![(root, 0)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for entry in rd.flatten() {
                let p = entry.path();
                let ft = match entry.file_type() {
                    Ok(f) => f,
                    Err(_) => continue,
                };
                if ft.is_dir() || (ft.is_symlink() && p.is_dir()) {
                    if depth < 3 {
                        stack.push((p, depth + 1));
                    }
                    continue;
                }
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !matches!(ext, "png" | "svg") {
                    continue;
                }
                // Prefer full-colour app icons; skip symbolic variants.
                let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                if stem.ends_with("-symbolic") {
                    continue;
                }
                let size = size_from_path(&p);
                index.entry(stem).or_default().push(Candidate { path: p, size });
            }
        }
    }
    index
}

fn theme_index(theme: &str) -> std::sync::Arc<ThemeIndex> {
    static CACHE: OnceLock<Mutex<HashMap<String, std::sync::Arc<ThemeIndex>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(i) = cache.lock().unwrap().get(theme) {
        return i.clone();
    }
    let idx = std::sync::Arc::new(index_theme(theme));
    cache.lock().unwrap().insert(theme.to_string(), idx.clone());
    idx
}

/// Resolves an icon name (or absolute path) to a file.
pub fn find_icon_file(name: &str, size: u32) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.is_absolute() {
        return p.is_file().then(|| p.to_path_buf());
    }
    // Some entries wrongly include an extension.
    let name = name.strip_suffix(".png").or_else(|| name.strip_suffix(".svg")).unwrap_or(name);
    for theme in theme_chain() {
        let idx = theme_index(theme);
        if let Some(cands) = idx.get(name) {
            let best = cands.iter().min_by_key(|c| {
                if c.size == u32::MAX {
                    1 // scalable renders crisply at any size
                } else if c.size >= size {
                    (c.size - size) * 4 + 2
                } else {
                    (size - c.size) * 16 + 2
                }
            });
            if let Some(c) = best {
                return Some(c.path.clone());
            }
        }
    }
    for d in data_dirs() {
        for ext in ["png", "svg", "xpm"] {
            let f = d.join("pixmaps").join(format!("{name}.{ext}"));
            if f.is_file() && ext != "xpm" {
                return Some(f);
            }
        }
    }
    None
}

/// Loads a PNG/JPEG/ICO/SVG file, rasterising SVGs at `size` pixels.
pub fn load_image_file(path: &Path, size: u32) -> Option<RgbaImage> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "svg" || ext == "svgz" {
        return render_svg(path, size);
    }
    let img = image::open(path).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    RgbaImage::new(w, h, img.into_raw()).map(|i| i.downscaled(size.max(16) * 2))
}

fn render_svg(path: &Path, size: u32) -> Option<RgbaImage> {
    let data = std::fs::read(path).ok()?;
    let opts = resvg::usvg::Options { resources_dir: path.parent().map(Path::to_path_buf), ..Default::default() };
    let tree = resvg::usvg::Tree::from_data(&data, &opts).ok()?;
    let ts = tree.size();
    let scale = size as f32 / ts.width().max(ts.height()).max(1.0);
    let w = ((ts.width() * scale).round() as u32).max(1);
    let h = ((ts.height() * scale).round() as u32).max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    // tiny-skia stores premultiplied RGBA.
    let mut px = pixmap.take();
    for p in px.as_chunks_mut::<4>().0 {
        let a = p[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    RgbaImage::new(w, h, px)
}

/// Icon name or path → bitmap.
pub fn load_named(name: &str, size: u32) -> Option<RgbaImage> {
    find_icon_file(name, size).and_then(|p| load_image_file(&p, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_parsing() {
        assert_eq!(size_from_path(Path::new("/i/hicolor/48x48/apps/a.png")), 48);
        assert_eq!(size_from_path(Path::new("/i/breeze/apps/48/a.svg")), 48);
        assert_eq!(size_from_path(Path::new("/i/hicolor/scalable/apps/a.svg")), u32::MAX);
        assert_eq!(size_from_path(Path::new("/i/hicolor/256x256@2/apps/a.png")), 256);
    }
}
