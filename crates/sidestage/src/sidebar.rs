//! The sidebar: header, favourites (top) and minimized windows (bottom).

use std::path::PathBuf;
use std::time::Instant;

use egui::{
    pos2, vec2, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Layout, Pos2, Rect,
    Sense, Stroke, StrokeKind, TextureHandle, Ui, UiBuilder,
};
use sidestage_core::config::Edge;
use sidestage_core::{search, WindowGroup, WindowId, WindowInfo};
use sidestage_platform::ThumbnailMode;

use crate::app::{SidestageApp, HIDDEN_STRIP, OUTER_MARGIN};
use crate::glyphs::{self, Glyph};
use crate::settings::Tab;
use crate::theme::Palette;

/// Drag payload for moving a favourite.
#[derive(Clone, Debug)]
struct FavDrag(String);

enum Action {
    Launch(String),
    ToggleSection(String),
    StartRename(String),
    CommitRename(String, String),
    CancelRename,
    ShiftSection(String, isize),
    DeleteSection(String),
    AddSection,
    MoveFav {
        fav: String,
        section: String,
        index: usize,
    },
    RemoveFav(String),
    EditFav(String),
    Restore(WindowId),
    ToggleGroup(String),
    OpenSettings(Tab),
    AddApps(Option<String>),
    DropFiles {
        section: Option<String>,
        paths: Vec<PathBuf>,
    },
}

struct DropTarget {
    rect: Rect,
    section: String,
    index: usize,
    /// Insertion marker; `None` highlights the whole rect.
    marker: Option<[Pos2; 2]>,
}

struct Frame<'a> {
    actions: Vec<Action>,
    drop_targets: Vec<DropTarget>,
    p: &'a Palette,
}

pub fn show(app: &mut SidestageApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let full = ui.max_rect();
    let reveal = app.reveal(&ctx);
    let palette = app.palette;
    let settings = app.config.settings.clone();
    let left = settings.edge == Edge::Left;

    if settings.auto_hide && reveal < 0.002 {
        // Collapsed: a faint handle the pointer can find. On Windows the
        // window itself is only the strip; on Linux only its input shape is.
        let x = if left {
            full.left() + 1.5
        } else {
            full.right() - 1.5
        };
        let mid = full.center().y;
        // A full-height hint plus a brighter grip in the middle.
        ui.painter().line_segment(
            [pos2(x, full.top() + 8.0), pos2(x, full.bottom() - 8.0)],
            Stroke::new(3.0, palette.accent.gamma_multiply(0.25)),
        );
        ui.painter().line_segment(
            [pos2(x, mid - 70.0), pos2(x, mid + 70.0)],
            Stroke::new(3.0, palette.accent.gamma_multiply(0.9)),
        );
        let _ = HIDDEN_STRIP;
        if let Some(p) = app.panel.as_mut() {
            p.set_blur_region(None);
        }
        if let Some(lt) = app.panel.as_mut().and_then(|p| p.live_thumbnails()) {
            lt.begin();
            lt.end();
        }
        return;
    }

    let slide = (1.0 - reveal) * (settings.width + OUTER_MARGIN) * if left { -1.0 } else { 1.0 };
    let painter = ui.painter().clone();
    let arch = Arch::new(full, left, slide);
    arch.paint(&painter, &palette);
    let card = arch.body;
    let blur = (reveal >= 0.999).then(|| arch.blur_bands(full, ui.ctx().pixels_per_point()));
    if let Some(p) = app.panel.as_mut() {
        p.set_blur_region(blur);
    }

    let mut f = Frame {
        actions: Vec::new(),
        drop_targets: Vec::new(),
        p: &palette,
    };
    let inner = Rect::from_min_max(
        card.min + vec2(if left { 12.0 } else { 6.0 }, 14.0),
        card.max - vec2(if left { 6.0 } else { 12.0 }, 14.0),
    );
    let mut ui = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );
    ui.set_clip_rect(card);

    header(app, &mut ui, &mut f);
    ui.add_space(4.0);

    // Favourites take what they need, up to half the height when windows
    // are minimized, so both areas stay visible.
    let avail = ui.available_height();
    let min_area = if app.groups.is_empty() {
        110.0
    } else {
        (avail * 0.48).max(settings.preview_height + 70.0)
    };
    let fav_max = (avail - min_area).max(80.0);
    app.section_rects.clear();
    egui::ScrollArea::vertical()
        .id_salt("favourites")
        .max_height(fav_max)
        .auto_shrink([false, true])
        .show(&mut ui, |ui| {
            if app.search.trim().is_empty() {
                favourites(app, ui, &mut f);
            } else {
                search_results(app, ui, &mut f);
            }
        });

    minimized_area(app, &mut ui, &mut f);

    handle_fav_drag(&ctx, &painter, &mut f);
    handle_os_drop(app, &ctx, &painter, card, &mut f);
    toast(app, &painter, card);

    app.menu_open = egui::Popup::is_any_open(&ctx);
    let actions = std::mem::take(&mut f.actions);
    for a in actions {
        apply(app, &ctx, a);
    }
}

fn apply(app: &mut SidestageApp, ctx: &egui::Context, a: Action) {
    match a {
        Action::Launch(id) => app.launch(&id),
        Action::ToggleSection(id) => {
            if let Some(i) = app.config.section_index(&id) {
                app.config.sections[i].expanded = !app.config.sections[i].expanded;
                app.mark_dirty();
            }
        }
        Action::StartRename(id) => {
            let name = app
                .config
                .section_index(&id)
                .map(|i| app.config.sections[i].name.clone())
                .unwrap_or_default();
            app.renaming_section = Some((id, name));
            app.keyboard_wanted = true;
        }
        Action::CommitRename(id, name) => {
            if app.config.rename_section(&id, &name) {
                app.mark_dirty();
            }
            app.renaming_section = None;
        }
        Action::CancelRename => app.renaming_section = None,
        Action::ShiftSection(id, d) => {
            if app.config.shift_section(&id, d) {
                app.mark_dirty();
            }
        }
        Action::DeleteSection(id) => {
            if let Some(i) = app.config.section_index(&id) {
                let s = &app.config.sections[i];
                if s.items.is_empty() {
                    app.config.delete_section(&id, None);
                    app.mark_dirty();
                } else {
                    // Non-empty sections are confirmed in Settings.
                    app.settings.tab = Tab::Favourites;
                    app.settings.selected_section = Some(id);
                    app.settings.confirm_delete = true;
                    app.settings_open = true;
                }
            }
        }
        Action::AddSection => {
            let id = app.config.add_section("New section");
            app.mark_dirty();
            apply(app, ctx, Action::StartRename(id));
        }
        Action::MoveFav {
            fav,
            section,
            index,
        } => {
            if app.config.move_favourite(&fav, &section, index) {
                if let Some(i) = app.config.section_index(&section) {
                    app.config.sections[i].expanded = true;
                }
                app.mark_dirty();
            }
        }
        Action::RemoveFav(id) => {
            if let Some(f) = app.config.remove_favourite(&id) {
                app.toast(format!("Removed {}", f.name), false);
                app.mark_dirty();
            }
        }
        Action::EditFav(id) => {
            app.settings.tab = Tab::Favourites;
            app.settings.selected_section = app
                .config
                .find(&id)
                .map(|(s, _)| app.config.sections[s].id.clone());
            app.settings.editing = Some(id);
            app.settings_open = true;
            ctx.send_viewport_cmd_to(crate::settings::viewport_id(), egui::ViewportCommand::Focus);
        }
        Action::Restore(id) => app.restore(&id),
        Action::ToggleGroup(key) => {
            if !app.expanded_groups.remove(&key) {
                app.expanded_groups.insert(key);
            }
        }
        Action::OpenSettings(tab) => {
            app.settings.tab = tab;
            app.settings_open = true;
            ctx.send_viewport_cmd_to(crate::settings::viewport_id(), egui::ViewportCommand::Focus);
        }
        Action::AddApps(section) => {
            app.settings.tab = Tab::Favourites;
            if section.is_some() {
                app.settings.selected_section = section;
            }
            app.settings.show_app_picker = true;
            app.settings_open = true;
            ctx.send_viewport_cmd_to(crate::settings::viewport_id(), egui::ViewportCommand::Focus);
        }
        Action::DropFiles { section, paths } => {
            let section = section
                .or_else(|| {
                    app.config
                        .sections
                        .iter()
                        .find(|s| s.name == "Everyday")
                        .map(|s| s.id.clone())
                })
                .or_else(|| app.config.sections.first().map(|s| s.id.clone()));
            let Some(section) = section else {
                app.toast("Create a section first", true);
                return;
            };
            let mut added = 0;
            for p in paths {
                match sidestage_platform::apps::from_path(&p) {
                    Some(entry) => {
                        if app
                            .config
                            .add_favourite(&section, entry.into_favourite())
                            .is_some()
                        {
                            added += 1;
                        }
                    }
                    None => app.toast(format!("Can't add {}", p.display()), true),
                }
            }
            if added > 0 {
                app.mark_dirty();
                app.toast(
                    format!(
                        "Added {added} favourite{}",
                        if added == 1 { "" } else { "s" }
                    ),
                    false,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Header

fn glyph_button(ui: &mut Ui, p: &Palette, glyph: Glyph, tip: &str, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_with_time(resp.id, resp.hovered() || active, 0.12);
    if hover > 0.0 {
        ui.painter()
            .rect_filled(rect, 8.0, p.pressed.gamma_multiply(hover));
    }
    let color = if active { p.accent } else { p.text_dim };
    glyphs::paint(ui.painter(), rect.shrink(6.0), glyph, color);
    resp.on_hover_text(tip)
        .on_hover_cursor(CursorIcon::PointingHand)
}

fn header(app: &mut SidestageApp, ui: &mut Ui, f: &mut Frame) {
    let p = *f.p;
    ui.horizontal(|ui| {
        ui.set_height(30.0);
        if let Some(tex) = app.textures.app_icon("__brand") {
            ui.add(egui::Image::new(tex).fit_to_exact_size(vec2(20.0, 20.0)));
        }
        ui.label(
            egui::RichText::new("Sidestage")
                .size(14.5)
                .strong()
                .color(p.text),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if glyph_button(ui, &p, Glyph::Gear, "Settings", false).clicked() {
                f.actions.push(Action::OpenSettings(Tab::Appearance));
            }
            if glyph_button(ui, &p, Glyph::Plus, "Add favourites", false).clicked() {
                f.actions.push(Action::AddApps(None));
            }
            if glyph_button(ui, &p, Glyph::Search, "Search favourites", app.search_open).clicked() {
                app.search_open = !app.search_open;
                if app.search_open {
                    app.search_focus_request = true;
                } else {
                    app.search.clear();
                }
            }
        });
    });

    if app.search_open {
        ui.add_space(4.0);
        let edit = egui::TextEdit::singleline(&mut app.search)
            .hint_text("Search favourites")
            .desired_width(f32::INFINITY)
            .margin(vec2(10.0, 6.0));
        let resp = ui.add(edit);
        if std::mem::take(&mut app.search_focus_request) {
            resp.request_focus();
        }
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if enter {
            if let Some(id) = search_matches(app).first().map(|(id, _)| id.clone()) {
                f.actions.push(Action::Launch(id));
            }
            app.search.clear();
            app.search_open = false;
        } else if esc {
            app.search.clear();
            app.search_open = false;
        }
        let renaming = app.renaming_section.is_some();
        app.keyboard_wanted = (resp.has_focus() || resp.clicked() || resp.gained_focus())
            && app.search_open
            || renaming;
    } else if app.renaming_section.is_none() {
        app.keyboard_wanted = false;
    }
}

// ---------------------------------------------------------------------------
// Favourites

fn search_matches(app: &SidestageApp) -> Vec<(String, u32)> {
    let mut hits: Vec<(String, u32)> = Vec::new();
    for s in &app.config.sections {
        for fav in &s.items {
            if let Some(score) = search::score_fields(&app.search, &fav.name, &[&s.name]) {
                hits.push((fav.id.clone(), score));
            }
        }
    }
    hits.sort_by_key(|h| std::cmp::Reverse(h.1));
    hits
}

fn search_results(app: &mut SidestageApp, ui: &mut Ui, f: &mut Frame) {
    let hits = search_matches(app);
    if hits.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("No favourites match.").color(f.p.text_faint));
        return;
    }
    for (id, _) in hits {
        let Some((si, ii)) = app.config.find(&id) else {
            continue;
        };
        let fav = app.config.sections[si].items[ii].clone();
        let section = app.config.sections[si].name.clone();
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(rect, 10.0, f.p.hover);
        }
        let px = (26.0 * app.scale * 1.5) as u32;
        let icon_rect =
            Rect::from_center_size(pos2(rect.left() + 21.0, rect.center().y), vec2(26.0, 26.0));
        if let Some(tex) = app.textures.favourite(&fav, px) {
            ui.painter().image(
                tex.id(),
                icon_rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        ui.painter().text(
            pos2(rect.left() + 42.0, rect.center().y - 7.0),
            Align2::LEFT_CENTER,
            &fav.name,
            FontId::proportional(13.0),
            f.p.text,
        );
        ui.painter().text(
            pos2(rect.left() + 42.0, rect.center().y + 8.0),
            Align2::LEFT_CENTER,
            &section,
            FontId::proportional(10.5),
            f.p.text_faint,
        );
        if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
            f.actions.push(Action::Launch(fav.id.clone()));
            app.search.clear();
            app.search_open = false;
        }
    }
}

fn favourites(app: &mut SidestageApp, ui: &mut Ui, f: &mut Frame) {
    let section_count = app.config.sections.len();
    for si in 0..section_count {
        section(app, ui, f, si, section_count);
        ui.add_space(2.0);
    }
    if section_count == 0 {
        ui.label(egui::RichText::new("No sections yet.").color(f.p.text_faint));
    }
    ui.add_space(2.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    let col = if resp.hovered() {
        f.p.text_dim
    } else {
        f.p.text_faint
    };
    glyphs::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(14.0, 14.0)),
        Glyph::Plus,
        col,
    );
    ui.painter().text(
        pos2(rect.left() + 24.0, rect.center().y),
        Align2::LEFT_CENTER,
        "New section",
        FontId::proportional(12.0),
        col,
    );
    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        f.actions.push(Action::AddSection);
    }
}

fn section_menu(ui: &mut Ui, f: &mut Frame, id: &str, index: usize, count: usize) {
    if ui.button("Add apps…").clicked() {
        f.actions.push(Action::AddApps(Some(id.to_string())));
    }
    if ui.button("Rename").clicked() {
        f.actions.push(Action::StartRename(id.to_string()));
    }
    if ui
        .add_enabled(index > 0, egui::Button::new("Move up"))
        .clicked()
    {
        f.actions.push(Action::ShiftSection(id.to_string(), -1));
    }
    if ui
        .add_enabled(index + 1 < count, egui::Button::new("Move down"))
        .clicked()
    {
        f.actions.push(Action::ShiftSection(id.to_string(), 1));
    }
    ui.separator();
    if ui
        .button(egui::RichText::new("Delete section…").color(f.p.danger))
        .clicked()
    {
        f.actions.push(Action::DeleteSection(id.to_string()));
    }
}

fn section(app: &mut SidestageApp, ui: &mut Ui, f: &mut Frame, si: usize, count: usize) {
    let p = *f.p;
    let (sid, name, expanded, n) = {
        let s = &app.config.sections[si];
        (s.id.clone(), s.name.clone(), s.expanded, s.items.len())
    };
    let top = ui.cursor().top();

    // Header row
    let (hrect, hresp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    let renaming = app
        .renaming_section
        .as_ref()
        .filter(|(id, _)| *id == sid)
        .map(|(_, n)| n.clone());
    if let Some(mut buf) = renaming {
        let mut child = ui.new_child(UiBuilder::new().max_rect(hrect.shrink2(vec2(2.0, 1.0))));
        let resp = child.add(
            egui::TextEdit::singleline(&mut buf)
                .desired_width(f32::INFINITY)
                .margin(vec2(8.0, 3.0)),
        );
        if !resp.has_focus() && !resp.lost_focus() {
            resp.request_focus();
        }
        let esc = child.input(|i| i.key_pressed(egui::Key::Escape));
        if esc {
            f.actions.push(Action::CancelRename);
        } else if resp.lost_focus() {
            f.actions
                .push(Action::CommitRename(sid.clone(), buf.clone()));
        }
        app.renaming_section = Some((sid.clone(), buf));
        app.keyboard_wanted = true;
    } else {
        let hover = hresp.hovered() || hresp.context_menu_opened();
        if hover {
            ui.painter().rect_filled(hrect, 8.0, p.hover);
        }
        let open_t =
            ui.ctx()
                .animate_bool_with_time(Id::unique(("sec-open", &sid)), expanded, 0.15);
        glyphs::chevron(
            ui.painter(),
            pos2(hrect.left() + 11.0, hrect.center().y),
            4.0,
            open_t,
            Stroke::new(1.6, p.text_faint),
        );
        let galley =
            ui.painter()
                .layout_no_wrap(name.clone(), FontId::proportional(12.5), p.text_dim);
        let name_w = galley.size().x;
        ui.painter().galley(
            pos2(
                hrect.left() + 22.0,
                hrect.center().y - galley.size().y / 2.0,
            ),
            galley,
            p.text_dim,
        );
        ui.painter().text(
            pos2(hrect.left() + 28.0 + name_w, hrect.center().y),
            Align2::LEFT_CENTER,
            n.to_string(),
            FontId::proportional(11.0),
            p.text_faint,
        );
        // "⋯" menu button, visible on hover.
        let more = Rect::from_center_size(
            pos2(hrect.right() - 13.0, hrect.center().y),
            vec2(22.0, 22.0),
        );
        let more_resp = ui.interact(more, Id::unique(("sec-more", &sid)), Sense::click());
        if hover
            || more_resp.hovered()
            || egui::Popup::is_id_open(ui.ctx(), more_resp.id.with("menu"))
        {
            if more_resp.hovered() {
                ui.painter().rect_filled(more, 6.0, p.pressed);
            }
            glyphs::paint(ui.painter(), more.shrink(4.0), Glyph::More, p.text_dim);
        }
        egui::Popup::menu(&more_resp)
            .id(more_resp.id.with("menu"))
            .show(|ui| section_menu(ui, f, &sid, si, count));
        hresp.context_menu(|ui| section_menu(ui, f, &sid, si, count));
        if hresp.clicked() && !more_resp.clicked() {
            f.actions.push(Action::ToggleSection(sid.clone()));
        }
    }
    f.drop_targets.push(DropTarget {
        rect: hrect,
        section: sid.clone(),
        index: n,
        marker: None,
    });

    if expanded {
        tiles(app, ui, f, si);
    }
    let rect = Rect::from_min_max(
        pos2(ui.max_rect().left(), top),
        pos2(ui.max_rect().right(), ui.cursor().top()),
    );
    app.section_rects.push((sid, rect));
}

fn tiles(app: &mut SidestageApp, ui: &mut Ui, f: &mut Frame, si: usize) {
    let p = *f.p;
    let s = app.config.settings.clone();
    let sid = app.config.sections[si].id.clone();
    let items = app.config.sections[si].items.clone();
    let avail = ui.available_width();
    let gap = 4.0;

    if items.is_empty() {
        let (rect, resp) = ui.allocate_exact_size(vec2(avail, 40.0), Sense::click());
        let dragging = egui::DragAndDrop::has_payload_of_type::<FavDrag>(ui.ctx());
        let stroke = Stroke::new(1.0, if dragging { p.accent } else { p.border });
        ui.painter()
            .rect_stroke(rect.shrink(2.0), 10.0, stroke, StrokeKind::Inside);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            if dragging {
                "Drop here"
            } else {
                "Drop apps here or click to add"
            },
            FontId::proportional(11.5),
            p.text_faint,
        );
        if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
            f.actions.push(Action::AddApps(Some(sid.clone())));
        }
        f.drop_targets.push(DropTarget {
            rect,
            section: sid,
            index: 0,
            marker: None,
        });
        return;
    }

    let label_h = if s.show_labels { 18.0 } else { 0.0 };
    let min_tile = s.icon_size + if s.show_labels { 26.0 } else { 14.0 };
    let cols = (((avail + gap) / (min_tile + gap)).floor() as usize).max(1);
    let tile_w = (avail - gap * (cols as f32 - 1.0)) / cols as f32;
    let tile_h = s.icon_size + 14.0 + label_h;
    let rows = items.len().div_ceil(cols);
    let (grid, _) =
        ui.allocate_exact_size(vec2(avail, rows as f32 * (tile_h + gap)), Sense::hover());
    f.drop_targets.push(DropTarget {
        rect: grid,
        section: sid.clone(),
        index: items.len(),
        marker: None,
    });
    let icon_px = (s.icon_size * app.scale * 1.25).round() as u32;

    for (i, fav) in items.iter().enumerate() {
        let (r, c) = (i / cols, i % cols);
        let rect = Rect::from_min_size(
            grid.min + vec2(c as f32 * (tile_w + gap), r as f32 * (tile_h + gap)),
            vec2(tile_w, tile_h),
        );
        let id = Id::unique(("fav", &fav.id));
        let resp = ui.interact(rect, id, Sense::click_and_drag());
        let dragging_me = resp.dragged();
        if resp.drag_started() {
            resp.dnd_set_drag_payload(FavDrag(fav.id.clone()));
        }
        let hover =
            ui.ctx()
                .animate_bool_with_time(id.with("h"), resp.hovered() && !dragging_me, 0.1);
        let painter = ui.painter();
        if hover > 0.0 {
            painter.rect_filled(rect, 12.0, p.hover.gamma_multiply(hover));
        }
        // Launch feedback: a quick bounce.
        let bounce = app
            .launched
            .get(&fav.id)
            .map(|t| {
                let x = t.elapsed().as_secs_f32() / 0.9;
                (x * std::f32::consts::PI * 2.0).sin().abs() * (1.0 - x).max(0.0) * 5.0
            })
            .unwrap_or(0.0);
        let icon_rect = Rect::from_center_size(
            pos2(
                rect.center().x,
                rect.top() + 7.0 + s.icon_size / 2.0 - bounce - hover * 1.5,
            ),
            vec2(s.icon_size, s.icon_size),
        );
        let tint = if dragging_me {
            Color32::from_white_alpha(90)
        } else {
            Color32::WHITE
        };
        draw_icon(
            painter,
            app.textures.favourite(fav, icon_px),
            icon_rect,
            &fav.name,
            &p,
            tint,
        );
        if s.show_labels {
            let font = FontId::proportional(11.5);
            let label = truncate(painter, &fav.name, &font, tile_w - 4.0);
            painter.text(
                pos2(rect.center().x, icon_rect.bottom() + 11.0),
                Align2::CENTER_CENTER,
                label,
                font,
                if dragging_me {
                    p.text_faint
                } else {
                    p.text_dim
                },
            );
        }
        if dragging_me {
            // Floating copy under the pointer.
            if let Some(ptr) = ui.ctx().pointer_interact_pos() {
                let layer = egui::LayerId::new(egui::Order::Tooltip, Id::unique("fav-drag"));
                let fp = ui.ctx().layer_painter(layer);
                let r = Rect::from_center_size(ptr, vec2(s.icon_size + 12.0, s.icon_size + 12.0));
                fp.add(p.card_shadow().as_shape(r, CornerRadius::same(12)));
                fp.rect_filled(r, 12.0, p.raised);
                draw_icon(
                    &fp,
                    app.textures.favourite(fav, icon_px),
                    r.shrink(6.0),
                    &fav.name,
                    &p,
                    Color32::WHITE,
                );
            }
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        }
        let half = rect.center().x;
        let left_target = Rect::from_min_max(rect.min, pos2(half, rect.max.y));
        let right_target = Rect::from_min_max(pos2(half, rect.min.y), rect.max);
        f.drop_targets.push(DropTarget {
            rect: left_target,
            section: sid.clone(),
            index: i,
            marker: Some([
                pos2(rect.left() - gap / 2.0, rect.top() + 6.0),
                pos2(rect.left() - gap / 2.0, rect.bottom() - 6.0),
            ]),
        });
        f.drop_targets.push(DropTarget {
            rect: right_target,
            section: sid.clone(),
            index: i + 1,
            marker: Some([
                pos2(rect.right() + gap / 2.0, rect.top() + 6.0),
                pos2(rect.right() + gap / 2.0, rect.bottom() - 6.0),
            ]),
        });

        if resp.clicked() {
            f.actions.push(Action::Launch(fav.id.clone()));
        }
        let fav_id = fav.id.clone();
        let sections: Vec<(String, String)> = app
            .config
            .sections
            .iter()
            .filter(|x| x.id != sid)
            .map(|x| (x.id.clone(), x.name.clone()))
            .collect();
        resp.context_menu(|ui| {
            if ui.button("Open").clicked() {
                f.actions.push(Action::Launch(fav_id.clone()));
            }
            if ui.button("Edit name & icon…").clicked() {
                f.actions.push(Action::EditFav(fav_id.clone()));
            }
            if !sections.is_empty() {
                ui.menu_button("Move to", |ui| {
                    for (id, name) in &sections {
                        if ui.button(name).clicked() {
                            f.actions.push(Action::MoveFav {
                                fav: fav_id.clone(),
                                section: id.clone(),
                                index: usize::MAX,
                            });
                        }
                    }
                });
            }
            ui.separator();
            if ui
                .button(egui::RichText::new("Remove from favourites").color(p.danger))
                .clicked()
            {
                f.actions.push(Action::RemoveFav(fav_id.clone()));
            }
        });
        if !dragging_me {
            resp.on_hover_text_at_pointer(&fav.name)
                .on_hover_cursor(CursorIcon::PointingHand);
        }
    }
}

fn draw_icon(
    painter: &egui::Painter,
    tex: Option<&TextureHandle>,
    rect: Rect,
    name: &str,
    p: &Palette,
    tint: Color32,
) {
    match tex {
        Some(tex) => {
            // Keep aspect ratio inside the square.
            let [w, h] = tex.size();
            let scale = (rect.width() / w as f32).min(rect.height() / h as f32);
            let r = Rect::from_center_size(rect.center(), vec2(w as f32 * scale, h as f32 * scale));
            painter.image(
                tex.id(),
                r,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                tint,
            );
        }
        None => {
            // Letter tile while loading or when no icon exists.
            painter.rect_filled(
                rect.shrink(rect.width() * 0.06),
                rect.width() * 0.24,
                p.accent_soft,
            );
            let letter: String = name
                .chars()
                .next()
                .map(|c| c.to_uppercase().collect())
                .unwrap_or_default();
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                letter,
                FontId::proportional(rect.height() * 0.45),
                p.accent,
            );
        }
    }
}

fn truncate(painter: &egui::Painter, text: &str, font: &FontId, max_w: f32) -> String {
    let width = |s: &str| {
        painter
            .layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE)
            .size()
            .x
    };
    if width(text) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let s: String = chars[..mid].iter().collect::<String>() + "…";
        if width(&s) <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo]
        .iter()
        .collect::<String>()
        .trim_end()
        .to_string()
        + "…"
}

fn handle_fav_drag(ctx: &egui::Context, painter: &egui::Painter, f: &mut Frame) {
    let Some(payload) = egui::DragAndDrop::payload::<FavDrag>(ctx) else {
        return;
    };
    let Some(ptr) = ctx.pointer_interact_pos() else {
        return;
    };
    // Later targets are more specific (tiles after their section).
    let target = f.drop_targets.iter().rev().find(|t| t.rect.contains(ptr));
    if let Some(t) = target {
        match t.marker {
            Some(m) => {
                painter.line_segment(m, Stroke::new(2.5, f.p.accent));
            }
            None => {
                painter.rect_filled(t.rect, 10.0, f.p.drop);
            }
        }
        if ctx.input(|i| i.pointer.any_released()) {
            f.actions.push(Action::MoveFav {
                fav: payload.0.clone(),
                section: t.section.clone(),
                index: t.index,
            });
            egui::DragAndDrop::clear_payload(ctx);
        }
    } else if ctx.input(|i| i.pointer.any_released()) {
        egui::DragAndDrop::clear_payload(ctx);
    }
}

fn handle_os_drop(
    app: &mut SidestageApp,
    ctx: &egui::Context,
    painter: &egui::Painter,
    card: Rect,
    f: &mut Frame,
) {
    let (hovering, dropped, ptr) = ctx.input(|i| {
        (
            !i.raw.hovered_files.is_empty(),
            i.raw
                .dropped_files
                .iter()
                .map(|d| d.path().to_path_buf())
                .collect::<Vec<_>>(),
            i.pointer.latest_pos(),
        )
    });
    let section_at = |pos: Option<Pos2>| {
        pos.and_then(|p| {
            app.section_rects
                .iter()
                .find(|(_, r)| r.contains(p))
                .map(|(id, r)| (id.clone(), *r))
        })
    };
    if hovering {
        match section_at(ptr) {
            Some((_, r)) => {
                painter.rect_filled(r, 10.0, f.p.drop);
            }
            None => {
                painter.rect_stroke(
                    card.shrink(3.0),
                    16.0,
                    Stroke::new(2.0, f.p.accent),
                    StrokeKind::Inside,
                );
            }
        }
        ctx.request_repaint();
    }
    if !dropped.is_empty() {
        let section = section_at(ptr).map(|(id, _)| id);
        f.actions.push(Action::DropFiles {
            section,
            paths: dropped,
        });
    }
}

// ---------------------------------------------------------------------------
// Minimized windows

fn minimized_area(app: &mut SidestageApp, ui: &mut Ui, f: &mut Frame) {
    let p = *f.p;
    ui.add_space(6.0);
    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().line_segment(
        [line.left_center(), line.right_center()],
        Stroke::new(1.0, p.border),
    );
    ui.add_space(6.0);
    let count = app.tracker.minimized_count();
    let (hrect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    glyphs::paint(
        ui.painter(),
        Rect::from_center_size(pos2(hrect.left() + 9.0, hrect.center().y), vec2(16.0, 16.0)),
        Glyph::Windows,
        p.text_faint,
    );
    ui.painter().text(
        pos2(hrect.left() + 22.0, hrect.center().y),
        Align2::LEFT_CENTER,
        "Minimized",
        FontId::proportional(12.5),
        p.text_dim,
    );
    if count > 0 {
        glyphs::count_badge(
            ui.painter(),
            pos2(hrect.right() - 12.0, hrect.center().y),
            count,
            p.accent_soft,
            p.accent,
        );
    }
    ui.add_space(4.0);

    // Borrow the panel separately from `app` while cards are drawn.
    let mut panel = app.panel.take();
    let mut live = if app.thumbnail_mode == ThumbnailMode::Live {
        panel.as_mut().and_then(|p| p.live_thumbnails())
    } else {
        None
    };
    if let Some(lt) = live.as_mut() {
        lt.begin();
    }
    let ppp = ui.ctx().pixels_per_point();
    let hide_live = app.menu_open || egui::DragAndDrop::has_any_payload(ui.ctx());

    if app.groups.is_empty() {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 84.0), Sense::hover());
        ui.painter()
            .rect_stroke(rect, 12.0, Stroke::new(1.0, p.border), StrokeKind::Inside);
        let (title, body) = match &app.status {
            Some(s) if !s.connected => (
                "Window integration unavailable",
                s.message.clone().unwrap_or_default(),
            ),
            None => ("Connecting…", String::new()),
            _ => (
                "Nothing minimized",
                "Minimize a window and it appears here.".to_string(),
            ),
        };
        ui.painter().text(
            rect.center() - vec2(0.0, 10.0),
            Align2::CENTER_CENTER,
            title,
            FontId::proportional(12.5),
            p.text_dim,
        );
        let galley = ui.painter().layout(
            body,
            FontId::proportional(11.0),
            p.text_faint,
            rect.width() - 20.0,
        );
        let gpos = pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y + 2.0,
        );
        ui.painter().galley(gpos, galley, p.text_faint);
    } else {
        let groups = app.groups.clone();
        egui::ScrollArea::vertical()
            .id_salt("minimized")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let clip = ui.clip_rect();
                let clip_px = [
                    (clip.left() * ppp) as i32,
                    (clip.top() * ppp) as i32,
                    (clip.width() * ppp) as i32,
                    (clip.height() * ppp) as i32,
                ];
                for g in &groups {
                    group(app, ui, f, g, &mut live, clip_px, ppp, hide_live);
                    ui.add_space(8.0);
                }
            });
    }
    if let Some(lt) = live.as_mut() {
        if hide_live {
            lt.hide_all();
        }
        lt.end();
    }
    let _ = live;
    app.panel = panel;
    if let Some(s) = app
        .status
        .as_ref()
        .filter(|s| s.connected && s.message.is_some())
    {
        if !app.groups.is_empty() {
            ui.label(
                egui::RichText::new(s.message.as_deref().unwrap_or_default())
                    .size(10.5)
                    .color(p.text_faint),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn group(
    app: &mut SidestageApp,
    ui: &mut Ui,
    f: &mut Frame,
    g: &WindowGroup,
    live: &mut Option<&mut dyn sidestage_platform::panel::LiveThumbnails>,
    clip_px: [i32; 4],
    ppp: f32,
    hide_live: bool,
) {
    let p = *f.p;
    if g.windows.len() == 1 {
        let w = &g.windows[0];
        if window_card(app, ui, f, w, 1, &g.app_name, live, clip_px, ppp, hide_live).clicked() {
            f.actions.push(Action::Restore(w.id.clone()));
        }
        return;
    }
    let expanded = app.expanded_groups.contains(&g.app_key);
    if !expanded {
        let resp = window_card(
            app,
            ui,
            f,
            &g.windows[0],
            g.windows.len(),
            &g.app_name,
            live,
            clip_px,
            ppp,
            hide_live,
        );
        if resp
            .on_hover_text(format!(
                "{} — {} windows. Click to expand.",
                g.app_name,
                g.windows.len()
            ))
            .clicked()
        {
            f.actions.push(Action::ToggleGroup(g.app_key.clone()));
        }
        return;
    }
    // Expanded stack: header + one card per window.
    let (hrect, hresp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    if hresp.hovered() {
        ui.painter().rect_filled(hrect, 8.0, p.hover);
    }
    let icon_rect = Rect::from_center_size(
        pos2(hrect.left() + 14.0, hrect.center().y),
        vec2(18.0, 18.0),
    );
    draw_icon(
        ui.painter(),
        app.textures.app_icon(&g.app_key),
        icon_rect,
        &g.app_name,
        &p,
        Color32::WHITE,
    );
    ui.painter().text(
        pos2(hrect.left() + 30.0, hrect.center().y),
        Align2::LEFT_CENTER,
        &g.app_name,
        FontId::proportional(12.5),
        p.text,
    );
    glyphs::count_badge(
        ui.painter(),
        pos2(hrect.right() - 34.0, hrect.center().y),
        g.windows.len(),
        p.accent_soft,
        p.accent,
    );
    glyphs::chevron(
        ui.painter(),
        pos2(hrect.right() - 12.0, hrect.center().y),
        4.0,
        -1.0,
        Stroke::new(1.6, p.text_dim),
    );
    if hresp.on_hover_text("Collapse").clicked() {
        f.actions.push(Action::ToggleGroup(g.app_key.clone()));
    }
    ui.add_space(4.0);
    let indent = 10.0;
    for w in &g.windows {
        let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_max(
            ui.cursor().min + vec2(indent, 0.0),
            pos2(ui.max_rect().right(), ui.max_rect().bottom()),
        )));
        let resp = window_card(
            app,
            &mut child,
            f,
            w,
            1,
            &g.app_name,
            live,
            clip_px,
            ppp,
            hide_live,
        );
        let used = child.min_rect().height();
        ui.allocate_exact_size(vec2(ui.available_width(), used), Sense::hover());
        if resp.clicked() {
            f.actions.push(Action::Restore(w.id.clone()));
        }
        ui.add_space(6.0);
    }
}

#[allow(clippy::too_many_arguments)]
fn window_card(
    app: &mut SidestageApp,
    ui: &mut Ui,
    f: &mut Frame,
    w: &WindowInfo,
    stack: usize,
    app_name: &str,
    live: &mut Option<&mut dyn sidestage_platform::panel::LiveThumbnails>,
    clip_px: [i32; 4],
    ppp: f32,
    hide_live: bool,
) -> egui::Response {
    let p = *f.p;
    let preview_h = app.config.settings.preview_height;
    let stack_pad = if stack > 1 { 8.0 } else { 0.0 };
    let title_h = 22.0;
    let width = ui.available_width();
    let (outer, resp) =
        ui.allocate_exact_size(vec2(width, stack_pad + preview_h + title_h), Sense::click());
    let id = Id::unique(("win", &w.id));
    let hover = ui.ctx().animate_bool_with_time(id, resp.hovered(), 0.12);
    let pending = app.pending_restore.contains_key(&w.id);
    let fade = ui
        .ctx()
        .animate_bool_with_time(id.with("pending"), pending, 0.2);
    let mut painter = ui.painter().clone();
    painter.set_opacity(1.0 - 0.6 * fade);

    let thumb = Rect::from_min_size(outer.min + vec2(0.0, stack_pad), vec2(width, preview_h))
        .translate(vec2(0.0, -2.0 * hover));
    // Ghost cards behind a stack.
    if stack > 1 {
        for (k, a) in [(2.0, 0.45), (1.0, 0.75)] {
            let r = Rect::from_min_max(
                thumb.min + vec2(8.0 * k, -4.0 * k),
                pos2(thumb.right() - 8.0 * k, thumb.top() + 10.0),
            );
            painter.rect(
                r,
                10.0,
                p.raised.gamma_multiply(a),
                Stroke::new(1.0, p.border),
                StrokeKind::Inside,
            );
        }
    }
    if hover > 0.0 {
        let mut sh = p.card_shadow();
        sh.color = sh.color.gamma_multiply(hover);
        painter.add(sh.as_shape(thumb, CornerRadius::same(10)));
    }
    painter.rect_filled(thumb, 10.0, p.raised);

    // Preview content.
    let mut drew_live = false;
    if let Some(lt) = live.as_mut() {
        if !hide_live && fade < 0.5 {
            let inset = thumb.shrink(4.0);
            let dest = [
                (inset.left() * ppp) as i32,
                (inset.top() * ppp) as i32,
                (inset.width() * ppp) as i32,
                (inset.height() * ppp) as i32,
            ];
            drew_live = lt.place(&w.id, dest, clip_px).is_some();
        }
    }
    if !drew_live {
        if let Some((tex, [tw, th])) = app.textures.thumbnail(&w.id) {
            let inset = thumb.shrink(4.0);
            let s = (inset.width() / *tw as f32).min(inset.height() / *th as f32);
            let r = Rect::from_center_size(inset.center(), vec2(*tw as f32 * s, *th as f32 * s));
            painter.rect_filled(
                inset,
                7.0,
                if p.dark {
                    Color32::from_black_alpha(60)
                } else {
                    Color32::from_black_alpha(12)
                },
            );
            painter.add(
                egui::epaint::RectShape::filled(r, CornerRadius::same(6), Color32::WHITE)
                    .with_texture(tex.id(), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))),
            );
        } else {
            // Icon + title fallback.
            let tint = p.accent_soft;
            painter.rect_filled(thumb.shrink(4.0), 7.0, tint.gamma_multiply(0.6));
            let size = (preview_h * 0.42).clamp(28.0, 64.0);
            let ir = Rect::from_center_size(
                thumb.center() - vec2(0.0, if preview_h > 100.0 { 10.0 } else { 0.0 }),
                vec2(size, size),
            );
            draw_icon(
                &painter,
                app.textures.app_icon(&w.app_key),
                ir,
                app_name,
                &p,
                Color32::WHITE,
            );
            if preview_h > 100.0 {
                painter.text(
                    pos2(thumb.center().x, ir.bottom() + 14.0),
                    Align2::CENTER_CENTER,
                    app_name,
                    FontId::proportional(11.5),
                    p.text_dim,
                );
            }
        }
    }
    let border = if hover > 0.0 {
        p.accent.gamma_multiply(0.35 + 0.5 * hover)
    } else {
        p.border
    };
    painter.rect_stroke(
        thumb,
        10.0,
        Stroke::new(1.0 + hover * 0.5, border),
        StrokeKind::Inside,
    );

    // App badge.
    let badge = Rect::from_center_size(
        pos2(thumb.left() + 17.0, thumb.bottom() - 15.0),
        vec2(26.0, 26.0),
    );
    painter.circle_filled(badge.center(), 14.0, p.surface);
    draw_icon(
        &painter,
        app.textures.app_icon(&w.app_key),
        badge.shrink(4.0),
        app_name,
        &p,
        Color32::WHITE,
    );
    if stack > 1 {
        glyphs::count_badge(
            &painter,
            pos2(thumb.right() - 16.0, thumb.top() + 14.0),
            stack,
            p.accent,
            Color32::WHITE,
        );
    }

    // Title line.
    let font = FontId::proportional(12.0);
    let label = if stack > 1 {
        format!("{app_name} · {stack} windows")
    } else {
        w.title.clone()
    };
    let text = truncate(&painter, &label, &font, width - 6.0);
    painter.text(
        pos2(outer.left() + 3.0, thumb.bottom() + 12.0),
        Align2::LEFT_CENTER,
        text,
        font,
        if hover > 0.0 { p.text } else { p.text_dim },
    );

    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if stack == 1 {
        resp.on_hover_text(format!("{}\n{}", w.title, app_name))
    } else {
        resp
    }
}

fn toast(app: &SidestageApp, painter: &egui::Painter, card: Rect) {
    let Some(t) = &app.toast else { return };
    let p = &app.palette;
    let remaining = t
        .until
        .saturating_duration_since(Instant::now())
        .as_secs_f32();
    let alpha = remaining.min(0.3) / 0.3;
    let galley = painter.layout(
        t.text.clone(),
        FontId::proportional(12.0),
        Color32::WHITE,
        card.width() - 44.0,
    );
    let size = galley.size() + vec2(20.0, 14.0);
    let r = Rect::from_center_size(
        pos2(card.center().x, card.bottom() - 18.0 - size.y / 2.0),
        size,
    );
    let fill = if t.error {
        p.danger
    } else if p.dark {
        Color32::from_rgb(70, 74, 88)
    } else {
        Color32::from_rgb(40, 44, 56)
    };
    painter.rect_filled(r, 10.0, fill.gamma_multiply(alpha));
    painter.galley(
        r.min + vec2(10.0, 7.0),
        galley,
        Color32::WHITE.gamma_multiply(alpha),
    );
}

// ---------------------------------------------------------------------------
// The arch: flush with the screen edge, rounded top and bottom, and an inner
// edge that bows gently toward the middle of the screen.

/// How far the inner edge bulges at mid-height (logical px).
const ARCH_BULGE: f32 = 24.0;
const ARCH_RADIUS: f32 = 44.0;

struct Arch {
    /// Bounding box of the straight part; content lives here.
    body: Rect,
    points: Vec<Pos2>,
    left: bool,
}

impl Arch {
    fn new(full: Rect, left: bool, slide: f32) -> Arch {
        let top = full.top() + 6.0;
        let bottom = full.bottom() - 6.0;
        let inner = full.width() - OUTER_MARGIN + 2.0; // straight inner edge, from the screen edge
        let r = ARCH_RADIUS.min((bottom - top) / 4.0);
        // Build for the left edge, in window-local x (0 = screen edge).
        let mut pts: Vec<(f32, f32)> = vec![(0.0, top)];
        let steps = 10;
        for i in 0..=steps {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 / steps as f32 * std::f32::consts::FRAC_PI_2;
            pts.push((inner - r + r * a.cos(), top + r + r * a.sin()));
        }
        let curve_steps = 48;
        let (y0, y1) = (top + r, bottom - r);
        for i in 1..curve_steps {
            let t = i as f32 / curve_steps as f32;
            pts.push((inner + ARCH_BULGE * (t * std::f32::consts::PI).sin(), y0 + (y1 - y0) * t));
        }
        for i in 0..=steps {
            let a = i as f32 / steps as f32 * std::f32::consts::FRAC_PI_2;
            pts.push((inner - r + r * a.cos(), bottom - r + r * a.sin()));
        }
        pts.push((0.0, bottom));
        let map = |(x, y): (f32, f32)| {
            let x = if left { full.left() + x } else { full.right() - x };
            pos2(x + slide, y)
        };
        let points: Vec<Pos2> = pts.into_iter().map(map).collect();
        let body = Rect::from_min_max(map((0.0, top)), map((inner, bottom)));
        let body = Rect::from_two_pos(body.min, body.max);
        Arch { body, points, left }
    }

    fn paint(&self, painter: &egui::Painter, p: &Palette) {
        // Soft glow/shadow along the curved edge only (the edge side is flush).
        let edge: Vec<Pos2> = self.points[1..self.points.len() - 1].to_vec();
        let dir = if self.left { 1.0 } else { -1.0 };
        let shifted: Vec<Pos2> = edge.iter().map(|q| pos2(q.x + dir * 1.5, q.y)).collect();
        painter.add(egui::Shape::line(shifted, Stroke::new(3.0, p.shadow.gamma_multiply(0.45))));
        painter.add(egui::epaint::PathShape::convex_polygon(self.points.clone(), p.surface, Stroke::NONE));
        // Thin light rim reads as glass on both themes.
        let rim = if p.dark { Color32::from_white_alpha(34) } else { Color32::from_white_alpha(170) };
        painter.add(egui::Shape::line(edge, Stroke::new(1.2, rim)));
    }

    /// Horizontal bands approximating the shape, in physical window pixels,
    /// for the compositor's blur-behind region.
    fn blur_bands(&self, full: Rect, ppp: f32) -> Vec<[i32; 4]> {
        let ys = (self.body.top() as i32..self.body.bottom() as i32).step_by(6);
        let mut out = Vec::new();
        for y in ys {
            let (y0, y1) = (y as f32, (y + 6) as f32);
            // Narrowest extent of the shape within this band.
            let mut reach = f32::MAX;
            for w in self.points.windows(2) {
                let (a, b) = (w[0], w[1]);
                if (a.y.max(b.y) >= y0) && (a.y.min(b.y) <= y1) {
                    let x = if self.left { a.x.min(b.x) - full.left() } else { full.right() - a.x.max(b.x) };
                    if x > 1.0 {
                        reach = reach.min(x);
                    }
                }
            }
            if reach == f32::MAX {
                continue;
            }
            let x = if self.left { 0.0 } else { full.width() - reach };
            out.push([(x * ppp) as i32, ((y0 - full.top()) * ppp) as i32, (reach * ppp).ceil() as i32, (6.0 * ppp).ceil() as i32]);
        }
        out
    }
}
