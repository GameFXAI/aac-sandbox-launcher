//! The launcher's own widgets: top-bar tabs and buttons, the big play button, captioned fields and cards.

use super::theme::*;
use eframe::egui::{self, Align2, Color32, FontId, Margin, RichText, Sense, Shape, Stroke, TextEdit, Vec2, pos2, vec2};

pub fn nav_tab(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let font = bold_font(14.0);
    let galley = ui.painter().layout_no_wrap(label.to_uppercase(), font.clone(), INK);
    let size = vec2(galley.size().x + 28.0, 52.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    let color = if selected {
        GOLD
    } else if resp.hovered() {
        INK
    } else {
        MUTED
    };
    let p = ui.painter();
    p.text(rect.center(), Align2::CENTER_CENTER, label.to_uppercase(), font, color);
    if selected {
        let y = rect.bottom() - 2.0;
        p.line_segment([pos2(rect.left() + 12.0, y), pos2(rect.right() - 12.0, y)], Stroke::new(2.0, GOLD));
    }
    resp
}

#[derive(Clone, Copy)]
pub enum WinBtn {
    Close,
    Minimize,
}

pub fn win_button(ui: &mut egui::Ui, kind: WinBtn) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(44.0, 52.0), Sense::click());
    let name = match kind {
        WinBtn::Close => "Close",
        WinBtn::Minimize => "Minimize",
    };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    let p = ui.painter();
    if resp.hovered() {
        let bg = match kind {
            WinBtn::Close => rgba(196, 52, 44, 220),
            WinBtn::Minimize => rgba(255, 255, 255, 22),
        };
        p.rect_filled(rect, 0.0, bg);
    }
    let c = rect.center();
    let s = Stroke::new(1.3, if resp.hovered() { INK } else { MUTED });
    match kind {
        WinBtn::Close => {
            p.line_segment([c + vec2(-5.0, -5.0), c + vec2(5.0, 5.0)], s);
            p.line_segment([c + vec2(5.0, -5.0), c + vec2(-5.0, 5.0)], s);
        }
        WinBtn::Minimize => {
            p.line_segment([c + vec2(-6.0, 0.0), c + vec2(6.0, 0.0)], s);
        }
    }
    resp
}

/// The gear in the top bar: Settings and everything that is not Play.
pub fn gear_button(ui: &mut egui::Ui, open: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(46.0, 52.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 0.0, rgba(255, 255, 255, 18));
    }
    let c = rect.center();
    let col = if open { GOLD } else if resp.hovered() { INK } else { MUTED };
    let teeth = 8;
    let mut pts = Vec::new();
    for i in 0..teeth * 4 {
        let a = i as f32 / (teeth * 4) as f32 * std::f32::consts::TAU;
        let r = if (i / 2) % 2 == 0 { 10.5 } else { 8.0 };
        pts.push(c + vec2(a.cos() * r, a.sin() * r));
    }
    p.add(Shape::closed_line(pts, Stroke::new(1.6, col)));
    p.circle_stroke(c, 3.6, Stroke::new(1.6, col));
    if open {
        p.line_segment([pos2(rect.left() + 10.0, rect.bottom() - 2.0), pos2(rect.right() - 10.0, rect.bottom() - 2.0)], Stroke::new(2.0, GOLD));
    }
    resp
}

/// The big gold button (play / install / update).
pub fn big_button(ui: &mut egui::Ui, size: Vec2, label: &str, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let p = ui.painter();
    let hov = enabled && resp.hovered();
    if enabled {
        for i in 1..=6 {
            let g = rect.expand(i as f32 * 2.5);
            p.add(Shape::convex_polygon(chamfer(g, 10.0 + i as f32 * 2.0), gold_a(if hov { 16 } else { 9 }), Stroke::NONE));
        }
    }
    let (top, bottom, edge, text) = if !enabled {
        (Color32::from_rgb(64, 62, 58), Color32::from_rgb(42, 41, 38), Color32::from_rgb(80, 76, 68), MUTED)
    } else if hov {
        (Color32::from_rgb(150, 246, 230), Color32::from_rgb(56, 178, 176), Color32::from_rgb(190, 255, 244), DARK_TEXT)
    } else {
        (Color32::from_rgb(118, 234, 216), Color32::from_rgb(40, 156, 160), Color32::from_rgb(160, 246, 232), DARK_TEXT)
    };
    let r = if enabled && resp.is_pointer_button_down_on() { rect.shrink(1.0) } else { rect };
    grad_shape(p, r, 10.0, top, bottom, Stroke::new(1.0, edge));
    let inner = r.shrink(4.0);
    p.add(Shape::closed_line(chamfer(inner, 7.0), Stroke::new(1.0, rgba(220, 255, 250, if enabled { 70 } else { 20 }))));
    p.text(r.center(), Align2::CENTER_CENTER, label, title_font(24.0), text);
    resp
}

/// Text field with a small caption above it.
pub fn field(ui: &mut egui::Ui, caption: &str, value: &mut String, password: bool, hint: &str) -> egui::Response {
    let label = ui.label(RichText::new(caption.to_uppercase()).size(11.5).color(MUTED).extra_letter_spacing(1.2));
    let r = ui.add(
        TextEdit::singleline(value)
            .password(password)
            .hint_text(hint)
            .desired_width(f32::INFINITY)
            .margin(Margin::symmetric(10, 8))
            .font(FontId::proportional(15.5)),
    )
    .labelled_by(label.id);
    ui.add_space(4.0);
    r
}

pub fn tick_row(ui: &mut egui::Ui, title: &str, detail: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
        let p = ui.painter();
        p.add(Shape::convex_polygon(
            vec![rect.center_top(), rect.right_center(), rect.center_bottom(), rect.left_center()],
            gold_a(36),
            Stroke::new(1.0, GOLD),
        ));
        let c = rect.center();
        p.line_segment([c + vec2(-4.0, 0.0), c + vec2(-1.0, 3.0)], Stroke::new(1.6, GOLD));
        p.line_segment([c + vec2(-1.0, 3.0), c + vec2(4.5, -3.5)], Stroke::new(1.6, GOLD));
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(title).font(bold_font(15.0)).color(INK));
            ui.label(RichText::new(detail).size(13.0).color(MUTED));
        });
    });
    ui.add_space(6.0);
}

/// Selectable card for an install mode.
pub fn mode_card(ui: &mut egui::Ui, width: f32, selected: bool, enabled: bool, title: &str, size: &str, detail: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 104.0), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, enabled, selected, title));
    let p = ui.painter();
    let edge = if selected {
        Stroke::new(1.5, GOLD)
    } else if resp.hovered() && enabled {
        Stroke::new(1.0, gold_a(140))
    } else {
        Stroke::new(1.0, Color32::from_rgb(52, 60, 76))
    };
    let (t, b) = if selected { (rgba(18, 52, 54, 230), rgba(12, 24, 28, 235)) } else { (rgba(24, 30, 40, 220), rgba(14, 18, 25, 230)) };
    grad_shape(p, rect, 8.0, t, b, edge);
    // radio dot
    let dot = rect.left_top() + vec2(22.0, 26.0);
    p.circle_stroke(dot, 7.0, Stroke::new(1.3, if selected { GOLD } else { MUTED }));
    if selected {
        p.circle_filled(dot, 3.5, GOLD);
    }
    p.text(rect.left_top() + vec2(40.0, 16.0), Align2::LEFT_TOP, title, bold_font(16.0), INK);
    p.text(rect.right_top() + vec2(-16.0, 18.0), Align2::RIGHT_TOP, size, bold_font(13.0), if selected { GOLD } else { MUTED });
    let g = p.layout(detail.to_string(), FontId::proportional(13.0), MUTED, width - 56.0);
    p.galley(rect.left_top() + vec2(40.0, 44.0), g, MUTED);
    enabled && resp.clicked()
}
