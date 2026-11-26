//! The app's own widgets (Obsidian Ember, docs/companion-app.md): title-bar tabs, rail items, the record button,
//! buttons, the search box, menus, the grid/list toggle, fields and the install-mode cards.

use super::theme::*;
use eframe::egui::{self, Align2, Color32, Id, Margin, Rect, Response, RichText, Sense, Stroke, TextEdit, pos2, vec2};

/// A hairline a step lighter than LINE, for controls.
pub const EDGE: Color32 = Color32::from_rgb(52, 52, 60);

/// A tab in the title bar: icon and name; the picked one in warm white with an amber line glowing under it.
pub fn top_tab(ui: &mut egui::Ui, ic: Icon, label: &str, selected: bool, height: f32) -> Response {
    let font = medium_font(15.0);
    let g = ui.painter().layout_no_wrap(label.to_string(), font.clone(), INK);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 64.0, height), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, selected, label));
    let p = ui.painter();
    let col = if selected { INK } else if resp.hovered() { rgba(220, 216, 210, 255) } else { MUTED };
    if selected {
        let base = rect.bottom() - 3.0;
        let lit = Rect::from_min_max(pos2(rect.left() + 6.0, rect.top() + 6.0), pos2(rect.right() - 6.0, base));
        p.rect_filled(lit, egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, rgba(255, 255, 255, 8));
        vgrad(p, Rect::from_min_max(pos2(lit.left(), base - 16.0), pos2(lit.right(), base)), gold_a(0), gold_a(30));
        let bar = Rect::from_min_max(pos2(rect.left() + 12.0, base), pos2(rect.right() - 12.0, base + 3.0));
        for i in 1..=4 {
            p.rect_filled(bar.expand(i as f32 * 1.5), 3.0, gold_a(16 - i * 3));
        }
        p.rect_filled(bar, 1.5, GOLD);
    }
    let x = rect.left() + 22.0;
    icon(p, pos2(x + 8.0, rect.center().y), 17.0, ic, col);
    p.text(pos2(x + 26.0, rect.center().y), Align2::LEFT_CENTER, label, font, col);
    resp
}

/// A page in the left rail: icon and name; the picked one on a bronze fill.
pub fn rail_item(ui: &mut egui::Ui, ic: Icon, label: &str, selected: bool, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 40.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    let p = ui.painter();
    if selected {
        bronze(p, rect, 8.0, 1.2);
        p.rect_stroke(rect, 8.0, Stroke::new(1.0, gold_a(46)), egui::StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_filled(rect, 8.0, rgba(255, 255, 255, 8));
    }
    let col = if selected { INK } else if resp.hovered() { INK } else { rgba(206, 204, 200, 255) };
    icon(p, pos2(rect.left() + 22.0, rect.center().y), 17.0, ic, if selected { GOLD_HI } else { col });
    p.text(pos2(rect.left() + 44.0, rect.center().y), Align2::LEFT_CENTER, label, if selected { bold_font(14.5) } else { medium_font(14.5) }, col);
    resp
}

/// The record button at the top of the rail: a red dot (lit while recording), "Record", and its state in a chip.
pub fn record_button(ui: &mut egui::Ui, width: f32, live: bool, chip: &str, chip_col: Color32, open: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 46.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Record"));
    let p = ui.painter();
    let hov = resp.hovered() || open;
    p.rect_filled(rect, 10.0, if hov { Color32::from_rgb(30, 30, 35) } else { RAISED });
    p.rect_stroke(rect, 10.0, Stroke::new(1.0, if open { gold_a(120) } else { EDGE }), egui::StrokeKind::Inside);
    let dot = pos2(rect.left() + 24.0, rect.center().y);
    if live {
        let t = ui.input(|i| i.time) as f32;
        let a = ((t * 3.0).sin() * 0.5 + 0.5) * 60.0;
        p.circle_filled(dot, 10.0, rgba(232, 84, 84, a as u8));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
    }
    p.circle_filled(dot, 7.0, if live { Color32::from_rgb(240, 82, 82) } else { Color32::from_rgb(214, 86, 86) });
    p.text(pos2(rect.left() + 44.0, rect.center().y), Align2::LEFT_CENTER, "Record", bold_font(15.0), INK);
    let g = p.layout_no_wrap(chip.to_string(), medium_font(12.0), chip_col);
    let c = Rect::from_min_size(pos2(rect.right() - 14.0 - g.size().x - 16.0, rect.center().y - 12.0), vec2(g.size().x + 16.0, 24.0));
    p.rect_filled(c, 6.0, rgba(255, 255, 255, 8));
    p.rect_stroke(c, 6.0, Stroke::new(1.0, EDGE), egui::StrokeKind::Inside);
    p.galley(c.center() - g.size() / 2.0, g, chip_col);
    resp
}

fn button_body(ui: &mut egui::Ui, label: &str, ic: Option<Icon>, external: bool, width: f32, height: f32, enabled: bool) -> (Rect, Response, f32) {
    let font = medium_font((height * 0.38).clamp(13.0, 16.5));
    let g = ui.painter().layout_no_wrap(label.to_string(), font, INK);
    let extra = if ic.is_some() { 26.0 } else { 0.0 } + if external { 22.0 } else { 0.0 };
    let w = if width > 0.0 { width } else { g.size().x + extra + height * 0.9 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, height), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    (rect, resp, g.size().x + extra)
}

fn button_text(p: &egui::Painter, rect: Rect, label: &str, ic: Option<Icon>, external: bool, content_w: f32, col: Color32) {
    let font = medium_font((rect.height() * 0.38).clamp(13.0, 16.5));
    let mut x = rect.center().x - content_w / 2.0;
    let y = rect.center().y;
    if let Some(i) = ic {
        icon(p, pos2(x + 9.0, y), 18.0, i, col);
        x += 26.0;
    }
    let r = p.text(pos2(x, y), Align2::LEFT_CENTER, label, font, col);
    if external {
        icon(p, pos2(r.right() + 13.0, y), 14.0, Icon::External, col);
    }
}

/// The primary action: bronze, with an amber rim and a soft glow (Share clip, Play, Sign in). `width` 0 fits.
pub fn primary_button(ui: &mut egui::Ui, label: &str, ic: Option<Icon>, external: bool, width: f32, height: f32, enabled: bool) -> Response {
    let (rect, resp, cw) = button_body(ui, label, ic, external, width, height, enabled);
    let p = ui.painter();
    let (hov, down) = (enabled && resp.hovered(), enabled && resp.is_pointer_button_down_on());
    if !enabled {
        p.rect_filled(rect, 9.0, rgba(255, 255, 255, 6));
        p.rect_stroke(rect, 9.0, Stroke::new(1.0, EDGE), egui::StrokeKind::Inside);
        button_text(p, rect, label, ic, external, cw, FAINT);
        return resp;
    }
    glow(p, rect, 9.0, if hov { 1.2 } else { 0.7 });
    vgrad_round(p, rect, 9.0, if hov { rgba(150, 92, 40, 255) } else { rgba(128, 78, 34, 255) }, if down { rgba(92, 54, 22, 255) } else { rgba(84, 50, 22, 255) });
    p.rect_stroke(rect, 9.0, Stroke::new(1.5, if hov { GOLD_HI } else { GOLD }), egui::StrokeKind::Inside);
    p.line_segment([pos2(rect.left() + 9.0, rect.top() + 2.0), pos2(rect.right() - 9.0, rect.top() + 2.0)], Stroke::new(1.0, rgba(255, 214, 160, 60)));
    button_text(p, rect, label, ic, external, cw, rgba(255, 244, 228, 255));
    resp
}

/// A rounded rectangle filled top to bottom (a mesh clipped to the corners, cheaply: the middle band plus two caps).
fn vgrad_round(p: &egui::Painter, r: Rect, radius: f32, top: Color32, bottom: Color32) {
    p.rect_filled(Rect::from_min_max(r.min, pos2(r.right(), r.top() + radius * 2.0)), radius, top);
    p.rect_filled(Rect::from_min_max(pos2(r.left(), r.bottom() - radius * 2.0), r.max), radius, bottom);
    let mid = Rect::from_min_max(pos2(r.left(), r.top() + radius), pos2(r.right(), r.bottom() - radius));
    vgrad(p, mid, top, bottom);
}

/// The second action: graphite with a hairline that lights amber on hover (Jump into ArcheAge, Cancel). `danger`
/// lights it terracotta.
pub fn outline_button(ui: &mut egui::Ui, label: &str, ic: Option<Icon>, external: bool, width: f32, height: f32, danger: bool) -> Response {
    let (rect, resp, cw) = button_body(ui, label, ic, external, width, height, true);
    let p = ui.painter();
    let hov = resp.hovered();
    let lit = if danger { RED } else { GOLD };
    p.rect_filled(rect, 9.0, if hov { rgba(lit.r(), lit.g(), lit.b(), 18) } else { rgba(255, 255, 255, 5) });
    p.rect_stroke(rect, 9.0, Stroke::new(1.0, if hov { lit } else { Color32::from_rgb(70, 70, 80) }), egui::StrokeKind::Inside);
    button_text(p, rect, label, ic, external, cw, if hov && danger { RED } else { INK });
    resp
}

/// A quiet action with an icon (Rename, Delete): words only, lit on hover.
pub fn link_button(ui: &mut egui::Ui, ic: Icon, label: &str, danger: bool) -> Response {
    let font = medium_font(14.0);
    let g = ui.painter().layout_no_wrap(label.to_string(), font.clone(), INK);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 40.0, 32.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let p = ui.painter();
    let col = if resp.hovered() { if danger { RED } else { GOLD_HI } } else { rgba(214, 212, 208, 255) };
    icon(p, pos2(rect.left() + 12.0, rect.center().y), 16.0, ic, col);
    p.text(pos2(rect.left() + 32.0, rect.center().y), Align2::LEFT_CENTER, label, font, col);
    resp
}

/// The search box: a magnifier and a frameless text field in one rounded box.
pub fn search_box(ui: &mut egui::Ui, value: &mut String, hint: &str, width: f32, height: f32) -> Response {
    input_box(ui, value, Icon::Search, hint, width, height)
}

/// A text field in a rounded box with an icon at its left (the search box, Write a reply).
pub fn input_box(ui: &mut egui::Ui, value: &mut String, ic: Icon, hint: &str, width: f32, height: f32) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let id = ui.next_auto_id();
    let focused = ui.memory(|m| m.has_focus(id));
    let p = ui.painter().clone();
    p.rect_filled(rect, 9.0, Color32::from_rgb(17, 17, 20));
    p.rect_stroke(rect, 9.0, Stroke::new(1.0, if focused { gold_a(150) } else { EDGE }), egui::StrokeKind::Inside);
    icon(&p, pos2(rect.left() + 22.0, rect.center().y), 17.0, ic, MUTED);
    let field = Rect::from_min_max(pos2(rect.left() + 42.0, rect.top()), pos2(rect.right() - 10.0, rect.bottom()));
    let r = ui.put(
        field,
        TextEdit::singleline(value)
            .id(id)
            .frame(egui::Frame::NONE)
            .hint_text(RichText::new(hint).color(FAINT))
            .font(text_font(14.5))
            .text_color(INK)
            .align(Align2::LEFT_CENTER)
            .desired_width(field.width()),
    );
    r
}

/// Opens or closes the menu `id` when `anchor` is clicked and draws it under the anchor while it is open (closing on
/// a click anywhere else). `right` lines it up with the anchor's right edge.
pub fn popover<R>(ui: &egui::Ui, id: Id, anchor: &Response, width: f32, right: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> Option<R> {
    let ctx = ui.ctx().clone();
    let mut open = ctx.data(|d| d.get_temp::<bool>(id).unwrap_or(false));
    if anchor.clicked() {
        open = !open;
    }
    if !open {
        ctx.data_mut(|d| d.insert_temp(id, false));
        return None;
    }
    let x = if right { anchor.rect.right() - width } else { anchor.rect.left() };
    let shown = egui::Area::new(id.with("menu")).order(egui::Order::Foreground).fixed_pos(pos2(x, anchor.rect.bottom() + 6.0)).show(&ctx, |ui| {
        egui::Frame::new()
            .fill(Color32::from_rgb(24, 24, 28))
            .stroke(Stroke::new(1.0, EDGE))
            .corner_radius(10)
            .inner_margin(Margin::same(6))
            .shadow(egui::Shadow { offset: [0, 10], blur: 28, spread: 0, color: rgba(0, 0, 0, 160) })
            .show(ui, |ui| {
                ui.set_width(width - 12.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                add(ui)
            })
            .inner
    });
    if !anchor.clicked() && ctx.input(|i| i.pointer.any_click()) {
        let inside = ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| shown.response.rect.contains(p));
        if !inside {
            open = false;
        }
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        open = false;
    }
    let still = ctx.data(|d| d.get_temp::<bool>(id).unwrap_or(true));
    ctx.data_mut(|d| d.insert_temp(id, open && still));
    Some(shown.inner)
}

/// Closes a popover from inside it (an item was chosen).
pub fn close_popover(ui: &egui::Ui, id: Id) {
    ui.ctx().data_mut(|d| d.insert_temp(id, false));
}

/// A line in a menu: an icon, the words, and a check when it is the picked one.
pub fn menu_item(ui: &mut egui::Ui, ic: Option<Icon>, label: &str, picked: bool, danger: bool) -> Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 34.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::MenuItem, true, picked, label));
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 7.0, if danger { rgba(224, 118, 90, 22) } else { rgba(255, 255, 255, 10) });
    }
    let col = if danger && resp.hovered() { RED } else if picked { GOLD_HI } else { INK };
    let mut x = rect.left() + 10.0;
    if let Some(i) = ic {
        icon(p, pos2(x + 8.0, rect.center().y), 16.0, i, if danger { RED } else if picked { GOLD } else { MUTED });
        x += 26.0;
    }
    p.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, label, medium_font(14.0), col);
    if picked {
        icon(p, pos2(rect.right() - 16.0, rect.center().y), 14.0, Icon::Check, GOLD);
    }
    resp
}

/// A picker in the toolbar (All visibility, Newest first): the picked option, a chevron, and the options in a menu.
/// Returns true when the pick changed.
pub fn dropdown(ui: &mut egui::Ui, id: &str, ic: Icon, options: &[&str], picked: &mut usize, width: f32, height: f32, framed: bool) -> bool {
    let label = options.get(*picked).copied().unwrap_or("");
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, label));
    let pid = ui.make_persistent_id(id);
    let open = ui.ctx().data(|d| d.get_temp::<bool>(pid).unwrap_or(false));
    let p = ui.painter();
    let hov = resp.hovered() || open;
    if framed {
        p.rect_filled(rect, 9.0, if hov { Color32::from_rgb(28, 28, 33) } else { Color32::from_rgb(20, 20, 24) });
        p.rect_stroke(rect, 9.0, Stroke::new(1.0, if open { gold_a(140) } else { EDGE }), egui::StrokeKind::Inside);
    } else if hov {
        p.rect_filled(rect, 9.0, rgba(255, 255, 255, 8));
    }
    icon(p, pos2(rect.left() + 20.0, rect.center().y), 16.0, ic, INK);
    p.text(pos2(rect.left() + 38.0, rect.center().y), Align2::LEFT_CENTER, label, medium_font(14.5), INK);
    icon(p, pos2(rect.right() - 18.0, rect.center().y), 13.0, Icon::ChevronDown, MUTED);
    let mut changed = false;
    popover(ui, pid, &resp, width.max(200.0), true, |ui| {
        for (i, o) in options.iter().enumerate() {
            if menu_item(ui, None, o, i == *picked, false).clicked() {
                changed = i != *picked;
                *picked = i;
                close_popover(ui, pid);
            }
        }
    });
    changed
}

/// The grid / list toggle: two icon halves in one box, the picked half on bronze.
pub fn view_toggle(ui: &mut egui::Ui, list: &mut bool, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(height * 2.0 + 8.0, height), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, 9.0, Color32::from_rgb(20, 20, 24));
    p.rect_stroke(rect, 9.0, Stroke::new(1.0, EDGE), egui::StrokeKind::Inside);
    for (i, (ic, is_list, tip)) in [(Icon::Grid, false, "Grid"), (Icon::List, true, "List")].into_iter().enumerate() {
        let half = Rect::from_min_size(pos2(rect.left() + 4.0 + i as f32 * height, rect.top() + 4.0), vec2(height, height - 8.0));
        let resp = ui.interact(half, ui.scope_id().with(("view", i)), Sense::click()).on_hover_text(tip);
        let on = *list == is_list;
        if on {
            bronze(&p, half, 7.0, 1.4);
            p.rect_stroke(half, 7.0, Stroke::new(1.0, gold_a(110)), egui::StrokeKind::Inside);
        } else if resp.hovered() {
            p.rect_filled(half, 7.0, rgba(255, 255, 255, 8));
        }
        icon(&p, half.center(), 17.0, ic, if on { GOLD_HI } else { MUTED });
        if resp.clicked() {
            *list = is_list;
        }
    }
}

/// A window button in the title bar (minimise, maximise or restore, close).
pub fn win_button(ui: &mut egui::Ui, kind: Icon, height: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(46.0, height), Sense::click());
    let name = match kind {
        Icon::Close => "Close",
        Icon::Minimize => "Minimize",
        Icon::Restore => "Restore",
        _ => "Maximize",
    };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 0.0, if kind == Icon::Close { rgba(196, 52, 44, 230) } else { rgba(255, 255, 255, 14) });
    }
    icon(p, rect.center(), 15.0, kind, if resp.hovered() { INK } else { rgba(200, 200, 206, 255) });
    resp
}

/// A small dark square badge on a thumbnail's corner (lock: private, share: shared).
pub fn thumb_badge(p: &egui::Painter, at: egui::Pos2, ic: Icon) {
    let r = Rect::from_min_size(at, vec2(28.0, 28.0));
    p.rect_filled(r, 7.0, rgba(10, 10, 12, 190));
    p.rect_stroke(r, 7.0, Stroke::new(1.0, rgba(255, 255, 255, 40)), egui::StrokeKind::Inside);
    icon(p, r.center(), 15.0, ic, INK);
}

/// Text field with a small caption above it.
pub fn field(ui: &mut egui::Ui, caption: &str, value: &mut String, password: bool, hint: &str) -> Response {
    let label = ui.label(RichText::new(caption).font(medium_font(13.0)).color(MUTED));
    let r = ui
        .add(
            TextEdit::singleline(value)
                .password(password)
                .hint_text(RichText::new(hint).color(FAINT))
                .desired_width(f32::INFINITY)
                .margin(Margin::symmetric(12, 10))
                .font(text_font(15.0)),
        )
        .labelled_by(label.id);
    ui.add_space(6.0);
    r
}

/// Selectable card for an install mode.
pub fn mode_card(ui: &mut egui::Ui, width: f32, selected: bool, enabled: bool, title: &str, size: &str, detail: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 104.0), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, enabled, selected, title));
    let p = ui.painter();
    if selected {
        bronze(p, rect, 10.0, 1.0);
        picked_rim(p, rect, 10.0);
    } else {
        p.rect_filled(rect, 10.0, RAISED);
        p.rect_stroke(rect, 10.0, Stroke::new(1.0, if resp.hovered() && enabled { Color32::from_rgb(80, 80, 90) } else { EDGE }), egui::StrokeKind::Inside);
    }
    let dot = rect.left_top() + vec2(22.0, 25.0);
    p.circle_stroke(dot, 7.0, Stroke::new(1.3, if selected { GOLD } else { MUTED }));
    if selected {
        p.circle_filled(dot, 3.5, GOLD);
    }
    p.text(rect.left_top() + vec2(40.0, 15.0), Align2::LEFT_TOP, title, bold_font(15.0), INK);
    p.text(rect.right_top() + vec2(-16.0, 17.0), Align2::RIGHT_TOP, size, medium_font(12.5), if selected { GOLD } else { MUTED });
    let g = p.layout(detail.to_string(), text_font(12.5), MUTED, width - 56.0);
    p.galley(rect.left_top() + vec2(40.0, 44.0), g, MUTED);
    enabled && resp.clicked()
}

/// A tab or filter you pick one of: bronze when picked; `framed` draws a box around the others too.
pub fn chip(ui: &mut egui::Ui, label: &str, picked: bool, framed: bool, height: f32) -> Response {
    let g = ui.painter().layout_no_wrap(label.to_string(), medium_font(14.5), INK);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 34.0, height), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, picked, label));
    let p = ui.painter();
    let r = if framed { 9.0 } else { height / 2.0 };
    if picked {
        bronze(p, rect, r, 1.0);
        p.rect_stroke(rect, r, Stroke::new(1.2, gold_a(170)), egui::StrokeKind::Inside);
    } else {
        if resp.hovered() {
            p.rect_filled(rect, r, rgba(255, 255, 255, 10));
        }
        if framed {
            p.rect_stroke(rect, r, Stroke::new(1.0, EDGE), egui::StrokeKind::Inside);
        }
    }
    p.galley(rect.center() - g.size() / 2.0, g, if picked { INK } else { rgba(214, 212, 208, 255) });
    resp
}

/// A small rounded label: "Needs review" (gold), "Reviewed" (a tick, outlined).
pub fn pill(p: &egui::Painter, right_center: egui::Pos2, label: &str, done: bool) -> Rect {
    let g = p.layout_no_wrap(label.to_string(), medium_font(13.5), if done { INK } else { GOLD_HI });
    let w = g.size().x + if done { 52.0 } else { 34.0 };
    let r = Rect::from_min_size(pos2(right_center.x - w, right_center.y - 17.0), vec2(w, 34.0));
    if done {
        p.rect_stroke(r, 9.0, Stroke::new(1.0, EDGE), egui::StrokeKind::Inside);
        icon(p, pos2(r.left() + 21.0, r.center().y), 16.0, Icon::CheckCircle, INK);
        p.galley(pos2(r.left() + 38.0, r.center().y - g.size().y / 2.0), g, INK);
    } else {
        p.rect_filled(r, 9.0, gold_a(30));
        p.rect_stroke(r, 9.0, Stroke::new(1.0, gold_a(110)), egui::StrokeKind::Inside);
        p.galley(r.center() - g.size() / 2.0, g, GOLD_HI);
    }
    r
}

/// "INSTALL GAME" -> "Install game": buttons speak in sentences, not capitals.
pub fn sentence(label: &str) -> String {
    let low = label.to_lowercase();
    let mut c = low.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// A big round icon, a title and a line under it: what a page with nothing to show says.
pub fn empty_state(ui: &mut egui::Ui, area: Rect, ic: Icon, title: &str, sub: &str) {
    let c = pos2(area.center().x, area.top() + area.height() * 0.36);
    let p = ui.painter();
    p.circle_filled(c, 38.0, gold_a(14));
    p.circle_stroke(c, 38.0, Stroke::new(1.0, gold_a(60)));
    icon(p, c, 34.0, ic, GOLD);
    p.text(c + vec2(0.0, 62.0), Align2::CENTER_TOP, title, bold_font(19.0), INK);
    let mut job = egui::text::LayoutJob::simple(sub.to_string(), text_font(13.5), MUTED, (area.width() - 80.0).min(460.0));
    job.halign = egui::Align::Center;
    let g = p.fonts_mut(|f| f.layout_job(job));
    p.galley(pos2(c.x, c.y + 92.0), g, MUTED);
}
