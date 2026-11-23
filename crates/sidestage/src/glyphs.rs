//! Small vector glyphs drawn with the painter so they stay crisp at any
//! scale and do not depend on icon fonts.

use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Search,
    Plus,
    Gear,
    More,
    Windows,
    Pin { on: bool },
    /// Settings navigation: Favourites.
    Star,
    /// Settings navigation: Appearance (half-filled circle).
    Contrast,
    /// Settings navigation: Behaviour (two sliders).
    Sliders,
    /// Settings navigation: Integration (two linked rings).
    Link,
    /// Settings navigation: About.
    Info,
    /// Buy Me a Coffee.
    Coffee,
    /// Update available / download.
    Download,
}

pub fn paint(p: &Painter, rect: Rect, glyph: Glyph, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let w = (s * 0.1).max(1.4);
    let stroke = Stroke::new(w, color);
    match glyph {
        Glyph::Search => {
            let r = s * 0.27;
            let cc = c + vec2(-s * 0.06, -s * 0.06);
            p.circle_stroke(cc, r, stroke);
            let d = std::f32::consts::FRAC_1_SQRT_2;
            p.line_segment(
                [cc + vec2(r * d, r * d), c + vec2(s * 0.32, s * 0.32)],
                stroke,
            );
        }
        Glyph::Plus => {
            let h = s * 0.3;
            p.line_segment([c - vec2(h, 0.0), c + vec2(h, 0.0)], stroke);
            p.line_segment([c - vec2(0.0, h), c + vec2(0.0, h)], stroke);
        }
        Glyph::Gear => {
            let teeth = 8;
            let (r_out, r_in) = (s * 0.36, s * 0.27);
            let mut pts = Vec::new();
            for i in 0..teeth * 4 {
                let a = i as f32 / (teeth * 4) as f32 * std::f32::consts::TAU;
                let r = if (i / 2) % 2 == 0 { r_out } else { r_in };
                pts.push(c + vec2(a.cos(), a.sin()) * r);
            }
            p.add(Shape::closed_line(pts, stroke));
            p.circle_stroke(c, s * 0.11, stroke);
        }
        Glyph::Pin { on } => {
            // Push-pin: cap, body, needle. Leans when free, upright and
            // filled when pinned.
            let tilt = if on { 0.0 } else { std::f32::consts::FRAC_PI_4 };
            let rot = |v: egui::Vec2| {
                c + vec2(
                    v.x * tilt.cos() - v.y * tilt.sin(),
                    v.x * tilt.sin() + v.y * tilt.cos(),
                )
            };
            let cap = Stroke::new(w * 1.6, color);
            p.line_segment(
                [rot(vec2(-s * 0.2, -s * 0.3)), rot(vec2(s * 0.2, -s * 0.3))],
                cap,
            );
            let body = vec![
                rot(vec2(-s * 0.11, -s * 0.28)),
                rot(vec2(s * 0.11, -s * 0.28)),
                rot(vec2(s * 0.16, s * 0.05)),
                rot(vec2(-s * 0.16, s * 0.05)),
            ];
            if on {
                p.add(egui::Shape::convex_polygon(body, color, Stroke::NONE));
            } else {
                p.add(egui::Shape::closed_line(body, stroke));
            }
            p.line_segment(
                [
                    rot(vec2(-s * 0.26, s * 0.06)),
                    rot(vec2(s * 0.26, s * 0.06)),
                ],
                stroke,
            );
            p.line_segment([rot(vec2(0.0, s * 0.06)), rot(vec2(0.0, s * 0.4))], stroke);
        }
        Glyph::More => {
            for dx in [-1.0, 0.0, 1.0] {
                p.circle_filled(c + vec2(dx * s * 0.24, 0.0), w * 0.95, color);
            }
        }
        Glyph::Windows => {
            let a = Rect::from_center_size(c + vec2(-s * 0.06, s * 0.06), vec2(s * 0.5, s * 0.38));
            let b = a.translate(vec2(s * 0.14, -s * 0.14));
            p.rect_stroke(b, 2.0, stroke, egui::StrokeKind::Middle);
            p.rect_filled(a.expand(w * 0.5), 2.0, Color32::TRANSPARENT);
            p.rect_stroke(a, 2.0, stroke, egui::StrokeKind::Middle);
        }
        Glyph::Star => {
            let (r_out, r_in) = (s * 0.38, s * 0.16);
            let pts: Vec<Pos2> = (0..10)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                    let r = if i % 2 == 0 { r_out } else { r_in };
                    c + vec2(a.cos(), a.sin()) * r + vec2(0.0, s * 0.03)
                })
                .collect();
            p.add(Shape::closed_line(pts, stroke));
        }
        Glyph::Contrast => {
            let r = s * 0.32;
            p.circle_stroke(c, r, stroke);
            // Left half filled.
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = std::f32::consts::FRAC_PI_2 + i as f32 / 16.0 * std::f32::consts::PI;
                    c + vec2(a.cos(), a.sin()) * r
                })
                .collect();
            p.add(Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        Glyph::Sliders => {
            for (dy, knob) in [(-0.17, -0.12), (0.17, 0.14)] {
                let y = c.y + s * dy;
                p.line_segment([pos2(c.x - s * 0.34, y), pos2(c.x + s * 0.34, y)], stroke);
                p.circle_filled(pos2(c.x + s * knob, y), s * 0.09, color);
            }
        }
        Glyph::Link => {
            let r = s * 0.17;
            p.circle_stroke(c + vec2(-s * 0.12, s * 0.06), r, stroke);
            p.circle_stroke(c + vec2(s * 0.12, -s * 0.06), r, stroke);
        }
        Glyph::Info => {
            p.circle_stroke(c, s * 0.34, stroke);
            p.circle_filled(c + vec2(0.0, -s * 0.15), w * 0.8, color);
            p.line_segment([c + vec2(0.0, -s * 0.03), c + vec2(0.0, s * 0.18)], stroke);
        }
        Glyph::Coffee => {
            // Cup, handle and two wisps of steam.
            let cup = Rect::from_min_max(c + vec2(-s * 0.3, -s * 0.04), c + vec2(s * 0.14, s * 0.34));
            p.rect_filled(cup, egui::CornerRadius { nw: 1, ne: 1, sw: 5, se: 5 }, color);
            p.circle_stroke(c + vec2(s * 0.2, s * 0.13), s * 0.09, stroke);
            for dx in [-0.16, 0.0] {
                let x = c.x + s * dx;
                p.add(Shape::line(
                    vec![
                        pos2(x, c.y - s * 0.12),
                        pos2(x + s * 0.05, c.y - s * 0.22),
                        pos2(x, c.y - s * 0.32),
                    ],
                    Stroke::new(w * 0.8, color),
                ));
            }
        }
        Glyph::Download => {
            p.line_segment([c + vec2(0.0, -s * 0.32), c + vec2(0.0, s * 0.12)], stroke);
            p.add(Shape::line(
                vec![c + vec2(-s * 0.16, -s * 0.04), c + vec2(0.0, s * 0.12), c + vec2(s * 0.16, -s * 0.04)],
                stroke,
            ));
            p.line_segment([c + vec2(-s * 0.3, s * 0.32), c + vec2(s * 0.3, s * 0.32)], stroke);
        }
    }
}

/// A chevron that rotates from pointing right (`t = 0`) to down (`t = 1`).
pub fn chevron(p: &Painter, c: Pos2, h: f32, t: f32, stroke: Stroke) {
    let angle = t * std::f32::consts::FRAC_PI_2;
    let rot = |v: egui::Vec2| {
        vec2(
            v.x * angle.cos() - v.y * angle.sin(),
            v.x * angle.sin() + v.y * angle.cos(),
        )
    };
    let pts = [vec2(-h * 0.5, -h), vec2(h * 0.5, 0.0), vec2(-h * 0.5, h)].map(|v| c + rot(v));
    p.add(Shape::line(pts.to_vec(), stroke));
}

/// Rounded "pill" badge with a number.
pub fn count_badge(p: &Painter, anchor: Pos2, n: usize, fill: Color32, text: Color32) -> Rect {
    let label = n.to_string();
    let galley = p.layout_no_wrap(label, egui::FontId::proportional(10.5), text);
    let size = vec2((galley.size().x + 9.0).max(17.0), 17.0);
    let rect = Rect::from_center_size(anchor, size);
    p.rect_filled(rect, 8.5, fill);
    p.galley(
        pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        text,
    );
    rect
}
