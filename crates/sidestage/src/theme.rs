//! Colours and egui style for the light and dark themes.

use egui::{Color32, CornerRadius, FontFamily, FontId, Shadow, Stroke, TextStyle, Visuals};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    /// Sidebar card background.
    pub surface: Color32,
    /// Raised element (preview card, tile hover).
    pub raised: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub drop: Color32,
    pub shadow: Color32,
    pub danger: Color32,
}

impl Palette {
    pub fn new(dark: bool) -> Self {
        if dark {
            Palette {
                dark,
                surface: Color32::from_rgba_unmultiplied(28, 29, 34, 238),
                raised: Color32::from_rgb(44, 46, 54),
                hover: Color32::from_rgba_unmultiplied(255, 255, 255, 18),
                pressed: Color32::from_rgba_unmultiplied(255, 255, 255, 30),
                border: Color32::from_rgba_unmultiplied(255, 255, 255, 22),
                text: Color32::from_rgb(236, 237, 242),
                text_dim: Color32::from_rgb(168, 172, 186),
                text_faint: Color32::from_rgb(112, 117, 132),
                accent: Color32::from_rgb(122, 162, 255),
                accent_soft: Color32::from_rgba_unmultiplied(122, 162, 255, 40),
                drop: Color32::from_rgba_unmultiplied(122, 162, 255, 70),
                shadow: Color32::from_black_alpha(110),
                danger: Color32::from_rgb(255, 118, 118),
            }
        } else {
            Palette {
                dark,
                surface: Color32::from_rgba_unmultiplied(246, 247, 250, 242),
                raised: Color32::from_rgb(255, 255, 255),
                hover: Color32::from_rgba_unmultiplied(20, 30, 60, 14),
                pressed: Color32::from_rgba_unmultiplied(20, 30, 60, 26),
                border: Color32::from_rgba_unmultiplied(20, 30, 60, 24),
                text: Color32::from_rgb(24, 27, 36),
                text_dim: Color32::from_rgb(88, 94, 110),
                text_faint: Color32::from_rgb(140, 146, 160),
                accent: Color32::from_rgb(52, 104, 232),
                accent_soft: Color32::from_rgba_unmultiplied(52, 104, 232, 30),
                drop: Color32::from_rgba_unmultiplied(52, 104, 232, 56),
                shadow: Color32::from_black_alpha(46),
                danger: Color32::from_rgb(200, 52, 52),
            }
        }
    }

    pub fn card_shadow(&self) -> Shadow {
        Shadow {
            offset: [0, 3],
            blur: 12,
            spread: 0,
            color: self.shadow,
        }
    }
}

pub const RADIUS: u8 = 12;

pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut v = if p.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = p.surface;
    v.window_fill = if p.dark {
        Color32::from_rgb(32, 33, 39)
    } else {
        Color32::from_rgb(250, 250, 252)
    };
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(RADIUS);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_shadow = Shadow {
        offset: [0, 6],
        blur: 24,
        spread: 0,
        color: p.shadow,
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: p.shadow,
    };
    v.override_text_color = Some(p.text);
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.extreme_bg_color = if p.dark {
        Color32::from_rgb(22, 23, 28)
    } else {
        Color32::from_rgb(236, 238, 243)
    };
    v.faint_bg_color = p.hover;
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
        &mut v.widgets.noninteractive,
    ] {
        w.corner_radius = CornerRadius::same(8);
    }
    v.widgets.inactive.weak_bg_fill = p.hover;
    v.widgets.inactive.bg_fill = p.hover;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.weak_bg_fill = p.pressed;
    v.widgets.hovered.bg_fill = p.pressed;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.active.weak_bg_fill = p.accent_soft;
    v.widgets.active.bg_fill = p.accent_soft;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    let theme = if p.dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(theme);
    ctx.set_visuals_of(theme, v);

    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size.y = 26.0;
        s.spacing.scroll.bar_width = 6.0;
        s.spacing.scroll.floating = true;
        s.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(17.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(13.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(11.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12.5, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}
