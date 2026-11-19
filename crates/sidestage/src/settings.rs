//! The Settings window: favourites management, appearance, behaviour and
//! integration status. A normal (focusable) OS window, unlike the sidebar.

use std::sync::mpsc::{self, Receiver};

use egui::{vec2, Align, Color32, Layout, RichText, Ui, ViewportBuilder, ViewportId};
use sidestage_core::config::{Edge, MonitorChoice, Settings, Theme};
use sidestage_core::{search, IconSource};
use sidestage_platform::apps::AppEntry;
use sidestage_platform::ThumbnailMode;

use crate::app::SidestageApp;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Favourites,
    Appearance,
    Behaviour,
    About,
}

#[derive(Default)]
pub struct SettingsUi {
    pub tab: Tab,
    pub selected_section: Option<String>,
    pub editing: Option<String>,
    pub confirm_delete: bool,
    pub show_app_picker: bool,
    apps: Option<Vec<AppEntry>>,
    apps_rx: Option<Receiver<Vec<AppEntry>>>,
    app_query: String,
    themed_name: String,
    delete_move_to: Option<String>,
    pub show_website_form: bool,
    website_name: String,
    website_url: String,
    website_error: Option<String>,
    /// URL being edited for a website favourite (id, text).
    url_edit: Option<(String, String)>,
    shortcut_edit: Option<String>,
}

pub fn viewport_id() -> ViewportId {
    ViewportId::from_hash_of("sidestage-settings")
}

pub fn show(app: &mut SidestageApp, ctx: &egui::Context) {
    let icon = crate::brand::app_icon(64);
    let builder = ViewportBuilder::default()
        .with_title("Sidestage Settings")
        .with_transparent(false)
        .with_app_id("sidestage-settings")
        .with_inner_size([780.0, 580.0])
        .with_min_inner_size([640.0, 460.0])
        .with_icon(egui::IconData {
            rgba: icon.pixels,
            width: icon.width,
            height: icon.height,
        });
    ctx.show_viewport_immediate(viewport_id(), builder, |ui, _class| {
        if ui.input(|i| i.viewport().close_requested()) {
            app.settings_open = false;
            app.settings.show_app_picker = false;
            app.settings.editing = None;
        }
        let p = app.palette;
        let fill = ui.visuals().window_fill;
        egui::Panel::left("settings-nav")
            .resizable(false)
            .exact_size(190.0)
            .frame(egui::Frame::new().fill(if p.dark {
                Color32::from_rgb(27, 28, 33)
            } else {
                Color32::from_rgb(241, 242, 246)
            }))
            .show(ui, |ui| nav(app, ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(fill)
                    .inner_margin(egui::Margin::symmetric(24, 18)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("settings", app.settings.tab as u8))
                    .auto_shrink([false, false])
                    .show(ui, |ui| match app.settings.tab {
                        Tab::Favourites => favourites(app, ui),
                        Tab::Appearance => appearance(app, ui),
                        Tab::Behaviour => behaviour(app, ui),
                        Tab::About => about(app, ui),
                    });
            });
    });
}

fn nav(app: &mut SidestageApp, ui: &mut Ui) {
    let p = app.palette;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(12, 16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(tex) = app.textures.app_icon("__brand") {
                    ui.add(egui::Image::new(tex).fit_to_exact_size(vec2(26.0, 26.0)));
                }
                ui.label(RichText::new("Sidestage").size(16.0).strong());
            });
            ui.add_space(14.0);
            for (tab, label) in [
                (Tab::Favourites, "Favourites"),
                (Tab::Appearance, "Appearance"),
                (Tab::Behaviour, "Behaviour"),
                (Tab::About, "Integration & about"),
            ] {
                let selected = app.settings.tab == tab;
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 32.0), egui::Sense::click());
                if selected {
                    ui.painter().rect_filled(rect, 8.0, p.accent_soft);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 8.0, p.hover);
                }
                ui.painter().text(
                    rect.left_center() + vec2(12.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::proportional(13.5),
                    if selected { p.accent } else { p.text },
                );
                if resp.clicked() {
                    app.settings.tab = tab;
                }
            }
        });
}

fn heading(ui: &mut Ui, text: &str, sub: &str) {
    ui.label(RichText::new(text).size(19.0).strong());
    if !sub.is_empty() {
        ui.label(RichText::new(sub).weak());
    }
    ui.add_space(10.0);
}

fn group(ui: &mut Ui, app: &SidestageApp, add: impl FnOnce(&mut Ui)) {
    let p = app.palette;
    egui::Frame::new()
        .fill(if p.dark {
            Color32::from_white_alpha(6)
        } else {
            Color32::WHITE
        })
        .stroke(egui::Stroke::new(1.0, p.border))
        .corner_radius(12)
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    ui.add_space(10.0);
}

fn row(ui: &mut Ui, label: &str, hint: &str, add: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        vec2(width, 0.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.allocate_ui_with_layout(
                vec2((width * 0.48).min(280.0), 0.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.label(label);
                    if !hint.is_empty() {
                        ui.add(egui::Label::new(RichText::new(hint).small().weak()).wrap());
                    }
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), add);
        },
    );
    ui.add_space(6.0);
}

fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    // Right-to-left layout: add in reverse so they read left-to-right.
    for (v, label) in options.iter().rev() {
        if ui.selectable_label(*value == *v, *label).clicked() && *value != *v {
            *value = *v;
            changed = true;
        }
    }
    changed
}

// ---------------------------------------------------------------------------

fn appearance(app: &mut SidestageApp, ui: &mut Ui) {
    heading(ui, "Appearance", "How the sidebar looks.");
    let mut s: Settings = app.config.settings.clone();
    let before = s.clone();
    group(ui, app, |ui| {
        row(
            ui,
            "Theme",
            "System follows your desktop's light/dark setting.",
            |ui| {
                segmented(
                    ui,
                    &mut s.theme,
                    &[
                        (Theme::System, "System"),
                        (Theme::Light, "Light"),
                        (Theme::Dark, "Dark"),
                    ],
                );
            },
        );
        row(ui, "Animations", "", |ui| {
            ui.checkbox(&mut s.animations, "");
        });
    });
    group(ui, app, |ui| {
        row(ui, "Sidebar width", "", |ui| {
            ui.add(
                egui::Slider::new(&mut s.width, Settings::WIDTH_RANGE)
                    .suffix(" px")
                    .fixed_decimals(0)
                    .step_by(2.0),
            );
        });
        row(ui, "Favourite icon size", "", |ui| {
            ui.add(
                egui::Slider::new(&mut s.icon_size, Settings::ICON_RANGE)
                    .suffix(" px")
                    .fixed_decimals(0)
                    .step_by(2.0),
            );
        });
        row(ui, "Show app names", "", |ui| {
            ui.checkbox(&mut s.show_labels, "");
        });
        row(
            ui,
            "Window preview height",
            "Size of minimized-window cards.",
            |ui| {
                ui.add(
                    egui::Slider::new(&mut s.preview_height, Settings::PREVIEW_RANGE)
                        .suffix(" px")
                        .fixed_decimals(0)
                        .step_by(2.0),
                );
            },
        );
    });
    if s != before {
        if s.icon_size != before.icon_size {
            app.textures.reload_favourites();
        }
        app.config.settings = s;
        app.mark_dirty();
    }
}

fn behaviour(app: &mut SidestageApp, ui: &mut Ui) {
    heading(ui, "Behaviour", "Placement and startup.");
    let mut s: Settings = app.config.settings.clone();
    let before = s.clone();
    let monitors = app.monitors.clone();
    group(ui, app, |ui| {
        row(ui, "Screen edge", "", |ui| {
            segmented(
                ui,
                &mut s.edge,
                &[(Edge::Left, "Left"), (Edge::Right, "Right")],
            );
        });
        row(
            ui,
            "Display",
            "With several monitors, choose where the sidebar lives.",
            |ui| {
                let current = match &s.monitor {
                    MonitorChoice::Primary => "Primary display".to_string(),
                    MonitorChoice::Named { name, .. } => name.clone(),
                };
                egui::ComboBox::from_id_salt("monitor")
                    .selected_text(current)
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut s.monitor,
                            MonitorChoice::Primary,
                            "Primary display",
                        );
                        for m in &monitors {
                            let label = format!(
                                "{} — {}×{}{}",
                                m.name,
                                m.width,
                                m.height,
                                if m.primary { " (primary)" } else { "" }
                            );
                            ui.selectable_value(
                                &mut s.monitor,
                                MonitorChoice::Named {
                                    name: m.name.clone(),
                                    index: m.index,
                                },
                                label,
                            );
                        }
                    });
            },
        );
    });
    group(ui, app, |ui| {
        row(
            ui,
            "Auto-hide",
            "Slide away when not in use; point at the screen edge to reveal.",
            |ui| {
                ui.checkbox(&mut s.auto_hide, "");
            },
        );
        ui.add_enabled_ui(s.auto_hide, |ui| {
            row(ui, "Reveal delay", "", |ui| {
                ui.add(
                    egui::Slider::new(&mut s.reveal_delay_ms, 0..=1000)
                        .suffix(" ms")
                        .step_by(25.0),
                );
            });
        });
        ui.add_enabled_ui(!s.auto_hide, |ui| {
            row(
                ui,
                "Reserve screen space",
                "Maximized windows stop at the sidebar instead of going under it.",
                |ui| {
                    ui.checkbox(&mut s.reserve_space, "");
                },
            );
        });
    });
    group(ui, app, |ui| {
        row(ui, "Launch at login", "", |ui| {
            ui.checkbox(&mut s.launch_at_login, "");
        });
    });
    shortcut_group(app, ui);
    if s.launch_at_login != before.launch_at_login {
        if let Err(e) = sidestage_platform::autostart::set_enabled(s.launch_at_login) {
            app.toast(format!("Couldn't change launch at login: {e}"), true);
            s.launch_at_login = before.launch_at_login;
        }
    }
    if s != before {
        app.config.settings = s;
        app.mark_dirty();
    }
}

fn about(app: &mut SidestageApp, ui: &mut Ui) {
    heading(ui, "Integration & about", "");
    let p = app.palette;
    let status = app.status.clone();
    let backend = app.backend.name();
    let mode = app.thumbnail_mode;
    let path = app.config_path().display().to_string();
    group(ui, app, |ui| {
        egui::Grid::new("about")
            .num_columns(2)
            .spacing([18.0, 8.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Version").weak());
                ui.label(env!("CARGO_PKG_VERSION"));
                ui.end_row();
                ui.label(RichText::new("Session").weak());
                ui.label(sidestage_platform::session_description());
                ui.end_row();
                ui.label(RichText::new("Window integration").weak());
                let (ok, text) = match &status {
                    Some(s) => (s.connected, format!("{} ({backend})", s.backend)),
                    None => (false, format!("starting ({backend})")),
                };
                ui.label(RichText::new(text).color(if ok { p.text } else { p.danger }));
                ui.end_row();
                ui.label(RichText::new("Window previews").weak());
                ui.label(match mode {
                    ThumbnailMode::Live => "Live (compositor thumbnails)",
                    ThumbnailMode::Snapshot => "Still image captured when a window is minimized",
                    ThumbnailMode::None => "Not available — icon and title shown",
                });
                ui.end_row();
                ui.label(RichText::new("Settings file").weak());
                ui.label(RichText::new(&path).monospace());
                ui.end_row();
            });
        if let Some(msg) = status.as_ref().and_then(|s| s.message.clone()) {
            ui.add_space(6.0);
            ui.label(RichText::new(msg).color(p.text_dim));
        }
    });
    group(ui, app, |ui| {
        ui.label(RichText::new("How Sidestage sees your windows").strong());
        ui.add_space(4.0);
        for line in [
            "Windows: Win32 window events and DWM live thumbnails.",
            "KDE Plasma (Wayland): KWin's plasma-window-management protocol; previews via KWin ScreenShot2.",
            "GNOME: requires the bundled Sidestage GNOME Shell extension (GNOME does not let apps see other windows).",
            "Other X11 desktops: EWMH; minimized X11 windows have no pixels, so icon + title are shown.",
        ] {
            ui.label(RichText::new(format!("• {line}")).color(p.text_dim));
        }
    });
}

// ---------------------------------------------------------------------------
// Favourites management

fn favourites(app: &mut SidestageApp, ui: &mut Ui) {
    heading(
        ui,
        "Favourites",
        "Organise shortcuts into sections. Drag apps between sections in the sidebar.",
    );
    if app.config.sections.is_empty() {
        if ui.button("Create a section").clicked() {
            let id = app.config.add_section("Everyday");
            app.settings.selected_section = Some(id);
            app.mark_dirty();
        }
        return;
    }
    let selected = app
        .settings
        .selected_section
        .clone()
        .filter(|id| app.config.section_index(id).is_some())
        .unwrap_or_else(|| app.config.sections[0].id.clone());
    app.settings.selected_section = Some(selected.clone());

    ui.horizontal_top(|ui| {
        // Section list
        ui.vertical(|ui| {
            ui.set_width(190.0);
            section_list(app, ui, &selected);
        });
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            if app.settings.confirm_delete {
                confirm_delete(app, ui, &selected);
            } else if app.settings.show_app_picker {
                app_picker(app, ui, &selected);
            } else if let Some(fid) = app
                .settings
                .editing
                .clone()
                .filter(|f| app.config.find(f).is_some())
            {
                edit_favourite(app, ui, &fid);
            } else {
                section_items(app, ui, &selected);
            }
        });
    });
}

fn section_list(app: &mut SidestageApp, ui: &mut Ui, selected: &str) {
    let p = app.palette;
    let n = app.config.sections.len();
    let mut action: Option<(usize, i8)> = None;
    for i in 0..n {
        let (id, name, count) = {
            let s = &app.config.sections[i];
            (s.id.clone(), s.name.clone(), s.items.len())
        };
        let is_sel = id == selected;
        let (rect, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), 30.0), egui::Sense::click());
        if is_sel {
            ui.painter().rect_filled(rect, 8.0, p.accent_soft);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, 8.0, p.hover);
        }
        ui.painter().text(
            rect.left_center() + vec2(10.0, 0.0),
            egui::Align2::LEFT_CENTER,
            &name,
            egui::FontId::proportional(13.0),
            if is_sel { p.accent } else { p.text },
        );
        ui.painter().text(
            rect.right_center() - vec2(10.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            count.to_string(),
            egui::FontId::proportional(11.0),
            p.text_faint,
        );
        if resp.clicked() {
            app.settings.selected_section = Some(id.clone());
            app.settings.editing = None;
            app.settings.show_app_picker = false;
            app.settings.confirm_delete = false;
        }
        if is_sel {
            action = action.or(Some((i, 0)));
        }
    }
    ui.add_space(8.0);
    if ui.button("+ New section").clicked() {
        let id = app.config.add_section("New section");
        app.settings.selected_section = Some(id);
        app.mark_dirty();
    }
    // Rename / reorder / delete the selected section.
    if let Some((i, _)) = action {
        ui.add_space(12.0);
        ui.label(RichText::new("Selected section").small().weak());
        let mut name = app.config.sections[i].name.clone();
        let resp = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
        if resp.changed() && !name.trim().is_empty() {
            let id = app.config.sections[i].id.clone();
            app.config.rename_section(&id, &name);
            app.mark_dirty();
        }
        ui.horizontal(|ui| {
            let id = app.config.sections[i].id.clone();
            if ui
                .add_enabled(i > 0, egui::Button::new("Move up"))
                .clicked()
            {
                app.config.shift_section(&id, -1);
                app.mark_dirty();
            }
            if ui
                .add_enabled(i + 1 < n, egui::Button::new("Move down"))
                .clicked()
            {
                app.config.shift_section(&id, 1);
                app.mark_dirty();
            }
            if ui.button(RichText::new("Delete").color(p.danger)).clicked() {
                app.settings.confirm_delete = true;
            }
        });
    }
}

fn confirm_delete(app: &mut SidestageApp, ui: &mut Ui, selected: &str) {
    let Some(i) = app.config.section_index(selected) else {
        app.settings.confirm_delete = false;
        return;
    };
    let (name, count) = (
        app.config.sections[i].name.clone(),
        app.config.sections[i].items.len(),
    );
    let others: Vec<(String, String)> = app
        .config
        .sections
        .iter()
        .filter(|s| s.id != selected)
        .map(|s| (s.id.clone(), s.name.clone()))
        .collect();
    ui.label(
        RichText::new(format!("Delete “{name}”?"))
            .size(16.0)
            .strong(),
    );
    ui.add_space(6.0);
    if count > 0 {
        ui.label(format!(
            "It contains {count} favourite{}.",
            if count == 1 { "" } else { "s" }
        ));
        let mv = &mut app.settings.delete_move_to;
        if mv
            .as_ref()
            .map(|id| !others.iter().any(|(o, _)| o == id))
            .unwrap_or(false)
        {
            *mv = None;
        }
        ui.radio_value(mv, None, "Delete them too");
        for (id, n) in &others {
            ui.radio_value(mv, Some(id.clone()), format!("Move them to {n}"));
        }
    }
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if ui
            .button(RichText::new("Delete section").color(app.palette.danger))
            .clicked()
        {
            let to = app.settings.delete_move_to.take();
            app.config.delete_section(selected, to.as_deref());
            app.settings.confirm_delete = false;
            app.settings.selected_section = to;
            app.mark_dirty();
        }
        if ui.button("Cancel").clicked() {
            app.settings.confirm_delete = false;
        }
    });
}

fn section_items(app: &mut SidestageApp, ui: &mut Ui, selected: &str) {
    let Some(si) = app.config.section_index(selected) else {
        return;
    };
    let p = app.palette;
    ui.label(
        RichText::new(&app.config.sections[si].name)
            .size(16.0)
            .strong(),
    );
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if ui.button("+ Add apps").clicked() {
            app.settings.show_app_picker = true;
        }
        if ui
            .button("Add folder…")
            .on_hover_text("Opens in your file manager")
            .clicked()
        {
            app.spawn_pick(ui.ctx(), crate::app::PickKind::Folder, selected);
        }
        if ui
            .button("Add website…")
            .on_hover_text("Opens in your default browser")
            .clicked()
        {
            app.settings.show_website_form = true;
            app.settings.website_error = None;
        }
        if ui
            .button("Add file or program…")
            .on_hover_text("Choose a shortcut, executable, .desktop or .url file, or a document")
            .clicked()
        {
            app.spawn_pick(ui.ctx(), crate::app::PickKind::File, selected);
        }
    });
    if app.settings.show_website_form {
        ui.add_space(8.0);
        website_form(app, ui, selected);
    }
    ui.add_space(8.0);
    let items = app.config.sections[si].items.clone();
    if items.is_empty() {
        ui.label(RichText::new("No favourites yet. Add installed apps, a folder or a website, or drag a file or folder onto the sidebar.").color(p.text_dim));
        return;
    }
    let n = items.len();
    let px = (32.0 * app.scale * 1.25) as u32;
    for (i, fav) in items.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.set_height(40.0);
            let tex = app.textures.favourite(fav, px).cloned();
            match tex {
                Some(t) => {
                    ui.add(egui::Image::new(&t).fit_to_exact_size(vec2(30.0, 30.0)));
                }
                None => {
                    ui.add_space(34.0);
                }
            }
            ui.allocate_ui_with_layout(
                vec2((ui.available_width() - 260.0).max(80.0), 36.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.add(egui::Label::new(&fav.name).truncate());
                    ui.add(
                        egui::Label::new(RichText::new(target_text(&fav.target)).small().weak())
                            .truncate(),
                    );
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Remove").clicked() {
                    app.config.remove_favourite(&fav.id);
                    app.mark_dirty();
                }
                if ui.small_button("Edit").clicked() {
                    app.settings.editing = Some(fav.id.clone());
                    app.settings.themed_name.clear();
                }
                if ui
                    .add_enabled(i + 1 < n, egui::Button::new("Down").small())
                    .clicked()
                {
                    app.config.move_favourite(&fav.id, selected, i + 2);
                    app.mark_dirty();
                }
                if ui
                    .add_enabled(i > 0, egui::Button::new("Up").small())
                    .clicked()
                {
                    app.config.move_favourite(&fav.id, selected, i - 1);
                    app.mark_dirty();
                }
                if ui.small_button("Open").clicked() {
                    app.launch(&fav.id);
                }
            });
        });
        ui.separator();
    }
}

fn target_text(t: &sidestage_core::LaunchTarget) -> String {
    use sidestage_core::LaunchTarget::*;
    match t {
        DesktopEntry { path } | Path { path, .. } => path.display().to_string(),
        AppsFolder { parsing_name } => parsing_name.clone(),
        Url { url } => url.clone(),
        Command { program, args } => std::iter::once(program.clone())
            .chain(args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn website_form(app: &mut SidestageApp, ui: &mut Ui, section: &str) {
    let p = app.palette;
    egui::Frame::new()
        .fill(p.hover)
        .corner_radius(10)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.label(RichText::new("Add a website").strong());
            egui::Grid::new("website-form")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Address");
                    let url = ui.add(
                        egui::TextEdit::singleline(&mut app.settings.website_url)
                            .hint_text("https://example.com")
                            .desired_width(320.0),
                    );
                    if url.changed() {
                        app.settings.website_error = None;
                    }
                    ui.end_row();
                    ui.label("Name");
                    ui.add(
                        egui::TextEdit::singleline(&mut app.settings.website_name)
                            .hint_text("Optional – defaults to the site name")
                            .desired_width(320.0),
                    );
                    ui.end_row();
                });
            if let Some(e) = &app.settings.website_error {
                ui.label(RichText::new(e).color(p.danger));
            }
            ui.horizontal(|ui| {
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Add").clicked() || enter {
                    match sidestage_platform::apps::website_entry(
                        &app.settings.website_name,
                        &app.settings.website_url,
                    ) {
                        Ok(entry) => {
                            app.add_entry(section, entry);
                            app.settings.website_name.clear();
                            app.settings.website_url.clear();
                            app.settings.show_website_form = false;
                        }
                        Err(e) => app.settings.website_error = Some(e),
                    }
                }
                if ui.button("Cancel").clicked() {
                    app.settings.show_website_form = false;
                    app.settings.website_error = None;
                }
            });
        });
}

fn app_picker(app: &mut SidestageApp, ui: &mut Ui, selected: &str) {
    if app.settings.apps.is_none() && app.settings.apps_rx.is_none() {
        let (tx, rx) = mpsc::channel();
        let ctx = ui.ctx().clone();
        std::thread::spawn(move || {
            sidestage_platform::init_worker_thread();
            let _ = tx.send(sidestage_platform::apps::installed_apps());
            ctx.request_repaint();
        });
        app.settings.apps_rx = Some(rx);
    }
    if let Some(rx) = &app.settings.apps_rx {
        if let Ok(list) = rx.try_recv() {
            app.settings.apps = Some(list);
            app.settings.apps_rx = None;
        }
    }
    let section_name = app
        .config
        .section_index(selected)
        .map(|i| app.config.sections[i].name.clone())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("Add apps to {section_name}"))
                .size(16.0)
                .strong(),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Done").clicked() {
                app.settings.show_app_picker = false;
            }
            if ui.button("Refresh").clicked() {
                app.settings.apps = None;
            }
        });
    });
    ui.add_space(6.0);
    let resp = ui.add(
        egui::TextEdit::singleline(&mut app.settings.app_query)
            .hint_text("Search installed apps")
            .desired_width(f32::INFINITY),
    );
    if !resp.has_focus()
        && app.settings.app_query.is_empty()
        && ui.memory(|m| m.focused().is_none())
    {
        resp.request_focus();
    }
    ui.add_space(6.0);
    let Some(apps) = app.settings.apps.clone() else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Finding installed apps…");
        });
        return;
    };
    let q = app.settings.app_query.trim().to_string();
    let mut hits: Vec<(&AppEntry, u32)> = apps
        .iter()
        .filter_map(|a| {
            let kw: Vec<&str> = a.keywords.iter().map(String::as_str).collect();
            search::score_fields(&q, &a.name, &kw).map(|s| (a, s))
        })
        .collect();
    if !q.is_empty() {
        hits.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.cmp(&b.0.name)));
    }
    ui.label(RichText::new(format!("{} apps", hits.len())).small().weak());
    let px = (28.0 * app.scale * 1.25) as u32;
    egui::ScrollArea::vertical()
        .id_salt("picker")
        .max_height(ui.available_height().max(260.0))
        .show_rows(ui, 38.0, hits.len(), |ui, range| {
            for (entry, _) in &hits[range] {
                let already = app.config.contains_target(selected, &entry.target);
                ui.horizontal(|ui| {
                    ui.set_height(34.0);
                    let key = format!("app-entry:{}", entry.target.key());
                    let tex = app
                        .textures
                        .preview(&key, &entry.icon, &entry.target, px)
                        .cloned();
                    match tex {
                        Some(t) => {
                            ui.add(egui::Image::new(&t).fit_to_exact_size(vec2(26.0, 26.0)));
                        }
                        None => {
                            ui.add_space(30.0);
                        }
                    }
                    // Fixed-height rows (the list is virtualised): one line each.
                    ui.allocate_ui_with_layout(
                        vec2((ui.available_width() - 70.0).max(80.0), 34.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.add(egui::Label::new(&entry.name).truncate());
                            if let Some(sub) = &entry.subtitle {
                                ui.add(
                                    egui::Label::new(RichText::new(sub).small().weak()).truncate(),
                                );
                            }
                        },
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(10.0); // keep clear of the scrollbar
                        if already {
                            ui.label(RichText::new("Added").weak());
                        } else if ui.button("Add").clicked() {
                            app.config
                                .add_favourite(selected, (*entry).clone().into_favourite());
                            app.mark_dirty();
                        }
                    });
                });
            }
        });
}

fn edit_favourite(app: &mut SidestageApp, ui: &mut Ui, fid: &str) {
    let Some(fav) = app.config.favourite(fid).cloned() else {
        return;
    };
    let p = app.palette;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Edit favourite").size(16.0).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Done").clicked() {
                app.settings.editing = None;
            }
        });
    });
    ui.add_space(10.0);
    let px = (64.0 * app.scale) as u32;
    ui.horizontal(|ui| {
        let tex = app.textures.favourite(&fav, px).cloned();
        egui::Frame::new()
            .fill(p.hover)
            .corner_radius(14)
            .inner_margin(10)
            .show(ui, |ui| match tex {
                Some(t) => {
                    ui.add(egui::Image::new(&t).fit_to_exact_size(vec2(56.0, 56.0)));
                }
                None => {
                    ui.add_space(56.0);
                }
            });
        ui.vertical(|ui| {
            ui.label(RichText::new("Name").small().weak());
            let mut name = fav.name.clone();
            if ui
                .add(egui::TextEdit::singleline(&mut name).desired_width(280.0))
                .changed()
                && !name.trim().is_empty()
            {
                if let Some(f) = app.config.favourite_mut(fid) {
                    f.name = name;
                }
                app.mark_dirty();
            }
            if let sidestage_core::LaunchTarget::Url { url } = &fav.target {
                ui.label(RichText::new("Address").small().weak());
                let editing = app
                    .settings
                    .url_edit
                    .get_or_insert_with(|| (fid.to_string(), url.clone()));
                if editing.0 != fid {
                    *editing = (fid.to_string(), url.clone());
                }
                let resp = ui.add(egui::TextEdit::singleline(&mut editing.1).desired_width(280.0));
                if resp.lost_focus()
                    || ui.input(|i| i.key_pressed(egui::Key::Enter)) && resp.has_focus()
                {
                    match sidestage_core::config::normalize_web_url(&editing.1) {
                        Ok(u) => {
                            editing.1 = u.clone();
                            if let Some(f) = app.config.favourite_mut(fid) {
                                f.target = sidestage_core::LaunchTarget::Url { url: u };
                            }
                            app.mark_dirty();
                        }
                        Err(e) => app.toast(e, true),
                    }
                }
            } else {
                ui.label(RichText::new(target_text(&fav.target)).small().weak());
            }
        });
    });
    ui.add_space(10.0);
    let mut include = fav.include_in_launch;
    if ui
        .checkbox(&mut include, "Include in section launch")
        .on_hover_text("“Launch section” opens every included favourite. Clicking this favourite always opens it.")
        .changed()
    {
        if let Some(f) = app.config.favourite_mut(fid) {
            f.include_in_launch = include;
        }
        app.mark_dirty();
    }
    ui.add_space(12.0);
    ui.label(RichText::new("Icon").strong());
    ui.horizontal(|ui| {
        if ui.button("Choose image…").clicked() {
            let mut d = rfd::FileDialog::new()
                .set_title("Choose an icon")
                .add_filter("Images", &["png", "svg", "ico", "jpg", "jpeg", "bmp"]);
            if cfg!(windows) {
                d = d.add_filter("Programs (use their icon)", &["exe", "dll", "lnk"]);
            }
            if let Some(path) = d.pick_file() {
                set_icon(app, fid, IconSource::File { path });
            }
        }
        if ui
            .add_enabled(
                fav.icon != IconSource::Auto,
                egui::Button::new("Use app's own icon"),
            )
            .clicked()
        {
            set_icon(app, fid, IconSource::Auto);
        }
    });
    if cfg!(target_os = "linux") {
        ui.horizontal(|ui| {
            ui.label("Icon theme name:");
            ui.add(
                egui::TextEdit::singleline(&mut app.settings.themed_name)
                    .hint_text("e.g. utilities-terminal")
                    .desired_width(200.0),
            );
            if ui
                .add_enabled(
                    !app.settings.themed_name.trim().is_empty(),
                    egui::Button::new("Apply"),
                )
                .clicked()
            {
                let name = app.settings.themed_name.trim().to_string();
                set_icon(app, fid, IconSource::Themed { name });
            }
        });
    }
    ui.add_space(12.0);
    ui.label(RichText::new("Section").strong());
    let current = app
        .config
        .find(fid)
        .map(|(s, _)| app.config.sections[s].id.clone());
    let sections: Vec<(String, String)> = app
        .config
        .sections
        .iter()
        .map(|s| (s.id.clone(), s.name.clone()))
        .collect();
    ui.horizontal_wrapped(|ui| {
        for (id, name) in sections {
            if ui
                .selectable_label(current.as_deref() == Some(id.as_str()), name)
                .clicked()
                && current.as_deref() != Some(id.as_str())
            {
                app.config.move_favourite(fid, &id, usize::MAX);
                app.settings.selected_section = Some(id);
                app.mark_dirty();
            }
        }
    });
    ui.add_space(16.0);
    if ui
        .button(RichText::new("Remove from favourites").color(p.danger))
        .clicked()
    {
        app.config.remove_favourite(fid);
        app.settings.editing = None;
        app.mark_dirty();
    }
}

fn set_icon(app: &mut SidestageApp, fid: &str, icon: IconSource) {
    if let Some(f) = app.config.favourite_mut(fid) {
        f.icon = icon;
    }
    app.mark_dirty();
}

fn shortcut_group(app: &mut SidestageApp, ui: &mut Ui) {
    use sidestage_platform::hotkey::HotkeyStatus;
    let p = app.palette;
    let current = app.config.settings.search_shortcut.clone();
    let mut apply: Option<String> = None;
    let mut buf = app
        .settings
        .shortcut_edit
        .take()
        .unwrap_or_else(|| current.clone());
    let status = app.hotkey_status.clone();
    group(ui, app, |ui| {
        row(
            ui,
            "Search shortcut",
            "Opens search from anywhere. Leave empty to turn it off; the search button always works.",
            |ui| {
                if ui.button("Apply").clicked() {
                    apply = Some(buf.clone());
                }
                let resp = ui.add(egui::TextEdit::singleline(&mut buf).hint_text("Ctrl+Alt+Space").desired_width(150.0));
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    apply = Some(buf.clone());
                }
            },
        );
        let (text, color) = match &status {
            HotkeyStatus::Disabled => ("Off".to_string(), p.text_faint),
            HotkeyStatus::Registered(how) => (format!("Active: {how}"), p.text_dim),
            HotkeyStatus::Conflict(m) => (
                format!("Not set: {m}. Choose another combination."),
                p.danger,
            ),
            HotkeyStatus::Unsupported(m) => (m.clone(), p.text_dim),
            HotkeyStatus::Invalid(m) => (format!("Invalid: {m}"), p.danger),
        };
        ui.label(RichText::new(text).small().color(color));
    });
    app.settings.shortcut_edit = Some(buf);
    if let Some(text) = apply {
        let text = text.trim().to_string();
        let normalized = sidestage_core::shortcut::Shortcut::parse(&text)
            .map(|s| s.display())
            .unwrap_or(text);
        app.config.settings.search_shortcut = normalized.clone();
        app.settings.shortcut_edit = Some(normalized);
        app.apply_hotkey();
        app.mark_dirty();
    }
}
