//! The Sidestage mark, drawn procedurally so no binary assets are needed:
//! a rounded tile with a sidebar column, two favourite dots and a stacked
//! preview card.

use sidestage_core::RgbaImage;

fn rounded_rect_alpha(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let cx = px.clamp(x0 + r, x1 - r);
    let cy = py.clamp(y0 + r, y1 - r);
    let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt() - r;
    (0.5 - d).clamp(0.0, 1.0)
}

fn blend(dst: &mut [f32; 4], c: [f32; 3], a: f32) {
    let out_a = a + dst[3] * (1.0 - a);
    if out_a <= 0.0 {
        return;
    }
    for i in 0..3 {
        dst[i] = (c[i] * a + dst[i] * dst[3] * (1.0 - a)) / out_a;
    }
    dst[3] = out_a;
}

pub fn app_icon(size: u32) -> RgbaImage {
    let s = size as f32 / 64.0;
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let mut c = [0.0f32; 4];
            // Background tile: blue→violet gradient.
            let t = (px + py) / (2.0 * size as f32);
            let bg = [0.29 + 0.25 * t, 0.42 - 0.08 * t, 0.98 - 0.02 * t];
            blend(
                &mut c,
                bg,
                rounded_rect_alpha(px, py, 2.0 * s, 2.0 * s, 62.0 * s, 62.0 * s, 14.0 * s),
            );
            // Sidebar column.
            blend(
                &mut c,
                [1.0, 1.0, 1.0],
                0.92 * rounded_rect_alpha(px, py, 9.0 * s, 10.0 * s, 25.0 * s, 54.0 * s, 5.0 * s),
            );
            // Favourite dots.
            for cy in [17.0, 27.0] {
                blend(
                    &mut c,
                    bg,
                    rounded_rect_alpha(
                        px,
                        py,
                        13.0 * s,
                        (cy - 4.0) * s,
                        21.0 * s,
                        (cy + 4.0) * s,
                        4.0 * s,
                    ),
                );
            }
            // Stacked previews.
            blend(
                &mut c,
                [1.0, 1.0, 1.0],
                0.45 * rounded_rect_alpha(px, py, 32.0 * s, 20.0 * s, 54.0 * s, 36.0 * s, 4.0 * s),
            );
            blend(
                &mut c,
                [1.0, 1.0, 1.0],
                0.95 * rounded_rect_alpha(px, py, 29.0 * s, 26.0 * s, 51.0 * s, 44.0 * s, 4.0 * s),
            );
            pixels.extend(c.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
    }
    RgbaImage::new(size, size, pixels).expect("icon buffer")
}
