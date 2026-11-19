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
    LaunchSection(String),
    RestoreAll(String),
    TogglePin,
    ToggleInclude(String),
    AddFolder(String),
    AddWebsite(String),
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
    /// Linear reveal progress (1 = fully shown).
    reveal: f32,
    opening: bool,
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// Damped spring from 0 to 1 with a small overshoot.
fn spring(t: f32) -> f32 {
    if t >= 1.0 {
        return 1.0;
    }
    1.0 - (-6.5 * t).exp() * (11.0 * t).cos()
}

/// Opacity for content at `order` (0 = top, 1 = bottom) while revealing,
/// so sections fade in one after another.
fn stagger(f: &Frame, order: f32) -> f32 {
    if f.reveal >= 1.0 {
        return 1.0;
    }
    if !f.opening {
        return (f.reveal * 1.6).clamp(0.0, 1.0);
    }
    let start = 0.28 + 0.38 * order.clamp(0.0, 1.0);
    let x = ((f.reveal - start) / 0.34).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

pub fn show(app: &mut SidestageApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let full = ui.max_rect();
    let reveal = app.reveal(&ctx);
    let palette = app.palette;
    let settings = app.config.settings.clone();
    let left = settings.edge == Edge::Left;

    if app.paused {
        // Fullscreen app on this monitor: draw nothing at all.
        if let Some(p) = app.panel.as_mut() {
            p.set_blur_region(None);
            if let Some(lt) = p.live_thumbnails() {
                lt.begin();
                lt.end();
            }
        }
        return;
    }
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

    // Opening: the body glides out (ease-out), the arch's swell springs into
    // shape, then content fades in top to bottom. Closing: quick ease-in.
    let opening = app.wants_shown();
    let anim = settings.animations;
    let glide = if opening {
        ease_out_cubic(reveal)
    } else {
        reveal * reveal * reveal
    };
    let swell = if !anim {
        1.0
    } else if opening {
        spring(((reveal - 0.12) / 0.88).clamp(0.0, 1.0)).min(1.07)
    } else {
        reveal
    };
    let slide = (1.0 - glide) * (settings.width + OUTER_MARGIN) * if left { -1.0 } else { 1.0 };
    let painter = ui.painter().clone();
    let arch = Arch::new(full, left, slide, swell);
    arch.paint(&painter, &palette);
    let card = arch.body;
    // Follow the shape on every frame (also while it slides and swells) so
    // the frost never lags behind or pops in after the animation.
    let blur = (reveal > 0.01).then(|| arch.blur_bands(full, ui.ctx().pixels_per_point()));
    if let Some(p) = app.panel.as_mut() {
        p.set_blur_region(blur);
    }

    let mut f = Frame {
        actions: Vec::new(),
        drop_targets: Vec::new(),
        p: &palette,
        reveal: if anim { reveal } else { 1.0 },
        opening,
    };
    // Content trails the panel slightly.
    let lag = (1.0 - stagger(&f, 0.0)) * 18.0 * if left { -1.0 } else { 1.0 };
    let inner = Rect::from_min_max(
        card.min + vec2(if left { 12.0 } else { 6.0 } + lag, 14.0),
        card.max - vec2(if left { 6.0 } else { 12.0 } - lag, 14.0),
    );
    let mut ui = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );
    ui.set_clip_rect(card);

    ui.scope(|ui| {
        ui.set_opacity(stagger(&f, 0.0));
        header(app, ui, &mut f);
    });
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

    let a = stagger(&f, 1.0);
    ui.scope(|ui| {
        ui.set_opacity(a);
        minimized_area(app, ui, &mut f);
    });

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
        Action::RestoreAll(key) => app.restore_stack(&key),
        Action::TogglePin => app.toggle_pin(),
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
        Action::LaunchSection(id) => app.launch_section(ctx, &id),
        Action::ToggleInclude(id) => {
            if let Some(f) = app.config.favourite_mut(&id) {
                f.include_in_launch = !f.include_in_launch;
                app.mark_dirty();
            }
        }
        Action::AddFolder(section) => app.spawn_pick(ctx, crate::app::PickKind::Folder, &section),
        Action::AddWebsite(section) => {
            app.settings.tab = Tab::Favourites;
            app.settings.selected_section = Some(section);
            app.settings.show_app_picker = false;
            app.settings.editing = None;
            app.settings.show_website_form = true;
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
            if app.config.settings.auto_hide {
                let pinned = app.config.settings.pinned;
                let tip = if pinned {
                    "Pinned open — click to let it auto-hide again"
                } else {
                    "Keep the sidebar open (e.g. while dragging apps in)"
                };
                if glyph_button(ui, &p, Glyph::Pin { on: pinned }, tip, pinned).clicked() {
                    f.actions.push(Action::TogglePin);
                }
            }
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
            .hint_text("Search apps and windows")
            .desired_width(f32::INFINITY)
            .margin(vec2(10.0, 6.0));
        let resp = ui.add(edit);
        if std::mem::take(&mut app.search_focus_request) {
            resp.request_focus();
        }
        if resp.changed() {
            app.search_selected = 0;
        }
        let hits = search_hits(app);
        let (enter, esc, up, down) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
            )
        });
        let typing = resp.has_focus() || resp.lost_focus();
        if typing && !hits.is_empty() {
            if down {
                app.search_selected = (app.search_selected + 1).min(hits.len() - 1);
            }
            if up {
                app.search_selected = app.search_selected.saturating_sub(1);
            }
        }
        app.search_selected = app.search_selected.min(hits.len().saturating_sub(1));
        if enter && typing {
            if let Some(hit) = hits.get(app.search_selected) {
                f.actions.push(hit_action(hit));
            }
            close_search(app, false);
        } else if esc {
            close_search(app, true);
        }
        let renaming = app.renaming_section.is_some();
        app.keyboard_wanted =
            (resp.has_focus() || resp.clicked() || resp.gained_focus() || app.search_focus_request)
                && app.search_open
                || renaming
                || (app.search_open && app.keyboard_wanted && !resp.lost_focus());
    } else if app.renaming_section.is_none() {
        app.keyboard_wanted = false;
    }
}

/// Closes search; with `refocus` the previously focused window gets the
/// keyboard back (Escape), otherwise the launched/restored window takes it.
fn close_search(app: &mut SidestageApp, refocus: bool) {
    app.search.clear();
    app.search_open = false;
    app.search_selected = 0;
    app.keyboard_wanted = false;
    app.refocus_previous = refocus;
}

fn hit_action(hit: &search::SearchHit) -> Action {
    match hit {
        search::SearchHit::Favourite(id) => Action::Launch(id.clone()),
        search::SearchHit::Window(id) => Action::Restore(WindowId(id.clone())),
    }
}

/// Favourites and minimized windows matching the query, best first.
fn search_hits(app: &SidestageApp) -> Vec<search::SearchHit> {
    let favs: Vec<(&str, &str, &str)> = app
        .config
        .sections
        .iter()
        .flat_map(|s| {
            s.items
                .iter()
                .map(move |f| (f.id.as_str(), f.name.as_str(), s.name.as_str()))
        })
        .collect();
    let wins: Vec<(&str, &str, &str)> = app
        .groups
        .iter()
        .flat_map(|g| {
            g.windows
                .iter()
                .map(move |w| (w.id.0.as_str(), w.title.as_str(), g.app_name.as_str()))
        })
        .collect();
    search::search_all(&app.search, favs, wins)
}

// ---------------------------------------------------------------------------
// Favourites

fn search_results(app: &mut SidestageApp, ui: &mut Ui, f: &mut Frame) {
    let hits = search_hits(app);
    if hits.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("No apps or minimized windows match.").color(f.p.text_faint));
        return;
    }
    let px = (26.0 * app.scale * 1.5) as u32;
    let mut last_kind = None;
    for (i, hit) in hits.iter().enumerate() {
        let kind = matches!(hit, search::SearchHit::Window(_));
        if last_kind != Some(kind) {
            last_kind = Some(kind);
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(if kind {
                    "Minimized windows"
                } else {
                    "Favourites"
                })
                .size(11.0)
                .color(f.p.text_faint),
            );
        }
        let (title, subtitle, icon) = match hit {
            search::SearchHit::Favourite(id) => {
                let Some((si, ii)) = app.config.find(id) else {
                    continue;
                };
                let fav = app.config.sections[si].items[ii].clone();
                let section = app.config.sections[si].name.clone();
                let tex = app.textures.favourite(&fav, px).cloned();
                (fav.name, section, tex)
            }
            search::SearchHit::Window(id) => {
                let Some((g, w)) = app
                    .groups
                    .iter()
                    .find_map(|g| g.windows.iter().find(|w| w.id.0 == *id).map(|w| (g, w)))
                else {
                    continue;
                };
                // Distinguish same-titled windows of one app.
                let same: Vec<&WindowInfo> =
                    g.windows.iter().filter(|o| o.title == w.title).collect();
                let mut sub = format!("{} · minimized", g.app_name);
                if same.len() > 1 {
                    let k = same.iter().position(|o| o.id == w.id).unwrap_or(0) + 1;
                    sub.push_str(&format!(" · {k} of {}", same.len()));
                }
                (
                    w.title.clone(),
                    sub,
                    app.textures.app_icon(&g.app_key).cloned(),
                )
            }
        };
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
        let selected = i == app.search_selected;
        if selected {
            ui.painter().rect_filled(rect, 10.0, f.p.accent_soft);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, 10.0, f.p.hover);
        }
        let icon_rect =
            Rect::from_center_size(pos2(rect.left() + 21.0, rect.center().y), vec2(26.0, 26.0));
        draw_icon(
            ui.painter(),
            icon.as_ref(),
            icon_rect,
            &title,
            f.p,
            Color32::WHITE,
        );
        let w = rect.width() - 50.0;
        let painter = ui.painter();
        let t = truncate(painter, &title, &FontId::proportional(13.0), w);
        let st = truncate(painter, &subtitle, &FontId::proportional(10.5), w);
        painter.text(
            pos2(rect.left() + 42.0, rect.center().y - 7.0),
            Align2::LEFT_CENTER,
            t,
            FontId::proportional(13.0),
            f.p.text,
        );
        painter.text(
            pos2(rect.left() + 42.0, rect.center().y + 8.0),
            Align2::LEFT_CENTER,
            st,
            FontId::proportional(10.5),
            f.p.text_faint,
        );
        if selected {
            ui.scroll_to_rect(rect, None);
        }
        if resp
            .on_hover_text(format!("{title}\n{subtitle}"))
            .on_hover_cursor(CursorIcon::PointingHand)
            .clicked()
        {
            f.actions.push(hit_action(hit));
            close_search(app, false);
        }
    }
}

fn favourites(app: &mut SidestageApp, ui: &mut Ui, f: &mut Frame) {
    let section_count = app.config.sections.len();
    for si in 0..section_count {
        let a = stagger(f, 0.1 + 0.6 * si as f32 / section_count.max(1) as f32);
        ui.scope(|ui| {
            ui.set_opacity(a);
            section(app, ui, f, si, section_count);
        });
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

fn section_menu(
    ui: &mut Ui,
    f: &mut Frame,
    id: &str,
    index: usize,
    count: usize,
    launch: Result<usize, &str>,
) {
    let label = match launch {
        Ok(n) => format!("Launch section ({n})"),
        Err(why) => format!("Launch section · {why}"),
    };
    if ui
        .add_enabled(launch.is_ok(), egui::Button::new(label))
        .on_hover_text("Open every favourite in this section that is included in section launch")
        .on_disabled_hover_text(
            "No favourites are included in section launch, or a launch is in progress",
        )
        .clicked()
    {
        f.actions.push(Action::LaunchSection(id.to_string()));
        ui.close();
    }
    ui.separator();
    if ui.button("Add apps…").clicked() {
        f.actions.push(Action::AddApps(Some(id.to_string())));
    }
    if ui.button("Add folder…").clicked() {
        f.actions.push(Action::AddFolder(id.to_string()));
    }
    if ui.button("Add website…").clicked() {
        f.actions.push(Action::AddWebsite(id.to_string()));
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
    let launch = app.section_launch_status(&sid);
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
            .show(|ui| section_menu(ui, f, &sid, si, count, launch));
        hresp.context_menu(|ui| section_menu(ui, f, &sid, si, count, launch));
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
    let cols = if s.columns > 0 {
        s.columns as usize
    } else {
        (((avail + gap) / (min_tile + gap)).floor() as usize).max(1)
    };
    let tile_w = (avail - gap * (cols as f32 - 1.0)) / cols as f32;
    // Fixed column counts shrink icons to fit narrow tiles.
    let icon = s.icon_size.min(tile_w - 10.0).max(18.0);
    let tile_h = icon + 14.0 + label_h;
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
                rect.top() + 7.0 + icon / 2.0 - bounce - hover * 1.5,
            ),
            vec2(icon, icon),
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
                let r = Rect::from_center_size(ptr, vec2(icon + 12.0, icon + 12.0));
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
        let include_in_launch = fav.include_in_launch;
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
            let mut include = include_in_launch;
            if ui
                .checkbox(&mut include, "Include in section launch")
                .clicked()
            {
                f.actions.push(Action::ToggleInclude(fav_id.clone()));
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
            let tip = match &fav.target {
                sidestage_core::LaunchTarget::Url { url } => format!("{}\n{url}", fav.name),
                sidestage_core::LaunchTarget::Path { path, .. } if path.is_dir() => {
                    format!("{}\n{}", fav.name, path.display())
                }
                _ => fav.name.clone(),
            };
            resp.on_hover_text_at_pointer(tip)
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
        stack_menu(&resp, f, g);
        if resp
            .on_hover_text(format!(
                "{} — {} windows. Click to expand, right-click to restore all.",
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
    stack_menu(&hresp, f, g);
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
    painter.multiply_opacity(1.0 - 0.6 * fade);

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
const ARCH_BULGE: f32 = 54.0;
const ARCH_RADIUS: f32 = 26.0;

struct Arch {
    /// Bounding box of the straight part; content lives here.
    body: Rect,
    points: Vec<Pos2>,
    left: bool,
}

impl Arch {
    fn new(full: Rect, left: bool, slide: f32, swell: f32) -> Arch {
        let top = full.top() + 6.0;
        let bottom = full.bottom() - 6.0;
        let inner = full.width() - OUTER_MARGIN + 4.0; // straight inner edge, from the screen edge
        let r = ARCH_RADIUS.min((bottom - top) / 4.0);
        // Build for the left edge, in window-local x (0 = screen edge).
        let mut pts: Vec<(f32, f32)> = vec![(0.0, top)];
        let steps = 10;
        for i in 0..=steps {
            let a = -std::f32::consts::FRAC_PI_2
                + i as f32 / steps as f32 * std::f32::consts::FRAC_PI_2;
            pts.push((inner - r + r * a.cos(), top + r + r * a.sin()));
        }
        let curve_steps = 48;
        let (y0, y1) = (top + r, bottom - r);
        for i in 1..curve_steps {
            let t = i as f32 / curve_steps as f32;
            pts.push((
                inner + ARCH_BULGE * swell * (t * std::f32::consts::PI).sin(),
                y0 + (y1 - y0) * t,
            ));
        }
        for i in 0..=steps {
            let a = i as f32 / steps as f32 * std::f32::consts::FRAC_PI_2;
            pts.push((inner - r + r * a.cos(), bottom - r + r * a.sin()));
        }
        pts.push((0.0, bottom));
        let map = |(x, y): (f32, f32)| {
            let x = if left {
                full.left() + x
            } else {
                full.right() - x
            };
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
        painter.add(egui::Shape::line(
            shifted,
            Stroke::new(3.0, p.shadow.gamma_multiply(0.45)),
        ));
        painter.add(egui::epaint::PathShape::convex_polygon(
            self.points.clone(),
            p.surface,
            Stroke::NONE,
        ));
        // Thin light rim reads as glass on both themes.
        let rim = if p.dark {
            Color32::from_white_alpha(34)
        } else {
            Color32::from_white_alpha(170)
        };
        painter.add(egui::Shape::line(edge, Stroke::new(1.2, rim)));
    }

    /// Horizontal bands approximating the shape, in physical window pixels,
    /// for the compositor's blur-behind region.
    /// Distance from the screen edge to the arch's inner edge at height
    /// `y` (logical px), or `None` above/below the shape.
    fn reach_at(&self, full: Rect, y: f32) -> Option<f32> {
        let mut best: Option<f32> = None;
        for w in self.points.windows(2) {
            let (a, b) = (w[0], w[1]);
            if (a.y - b.y).abs() < f32::EPSILON || y < a.y.min(b.y) || y > a.y.max(b.y) {
                continue;
            }
            let t = (y - a.y) / (b.y - a.y);
            let x = a.x + (b.x - a.x) * t;
            let reach = if self.left {
                x - full.left()
            } else {
                full.right() - x
            };
            best = Some(best.map_or(reach, |r: f32| r.max(reach)));
        }
        best.filter(|r| *r > 0.5)
    }

    /// Rectangles (physical window pixels) that follow the arch outline
    /// closely, for KWin's blur-behind region: 2 px bands measured at the
    /// exact curve, merged where consecutive bands have the same width.
    fn blur_bands(&self, full: Rect, ppp: f32) -> Vec<[i32; 4]> {
        const STEP: i32 = 2;
        let top = ((self.body.top() - full.top()) * ppp).floor() as i32;
        let bottom = ((self.body.bottom() - full.top()) * ppp).ceil() as i32;
        let width_px = (full.width() * ppp).round() as i32;
        let mut out: Vec<[i32; 4]> = Vec::new();
        let mut py = top.max(0);
        while py < bottom {
            // Narrower of the band's top and bottom, rounded down, so the
            // frost never spills past the drawn outline.
            let y0 = full.top() + py as f32 / ppp;
            let y1 = full.top() + (py + STEP) as f32 / ppp;
            let reach = match (self.reach_at(full, y0), self.reach_at(full, y1)) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            if let Some(reach) = reach {
                let w = ((reach * ppp).floor() as i32).clamp(1, width_px);
                let x = if self.left { 0 } else { width_px - w };
                match out.last_mut() {
                    Some(last) if last[0] == x && last[2] == w && last[1] + last[3] == py => {
                        last[3] += STEP
                    }
                    _ => out.push([x, py, w, STEP]),
                }
            }
            py += STEP;
        }
        out
    }
}

/// Right-click menu of a minimized app stack.
fn stack_menu(resp: &egui::Response, f: &mut Frame, g: &WindowGroup) {
    resp.context_menu(|ui| {
        if ui
            .button(format!("Restore all windows ({})", g.windows.len()))
            .clicked()
        {
            f.actions.push(Action::RestoreAll(g.app_key.clone()));
        }
        if ui.button("Show windows").clicked() {
            f.actions.push(Action::ToggleGroup(g.app_key.clone()));
        }
    });
}
