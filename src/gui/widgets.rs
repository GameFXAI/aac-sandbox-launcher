//! The launcher's own widgets: top-bar tabs and buttons, the big play button, captioned fields and cards.

use super::theme::*;
use eframe::egui::{self, Align2, Color32, FontId, Margin, Rect, RichText, Sense, Shape, Stroke, TextEdit, Vec2, pos2, vec2};

/// A tab in the top bar, as the Arena window's: the name in the title serif, gold with a glowing gold line under it
/// when picked.
pub fn nav_tab(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let font = title_font(17.0);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font.clone(), INK);
    let size = vec2(galley.size().x + 40.0, 52.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    let color = if selected {
        GOLD_HI
    } else if resp.hovered() {
        INK
    } else {
        rgba(214, 200, 176, 255)
    };
    let p = ui.painter();
    if selected {
        // a glow rising from the line, and the line itself fading out at both ends
        let base = rect.bottom() - 2.0;
        vgrad(p, Rect::from_min_max(pos2(rect.left() + 8.0, base - 18.0), pos2(rect.right() - 8.0, base)), gold_a(0), gold_a(34));
        let mid = rect.center().x;
        hgrad(p, Rect::from_min_max(pos2(rect.left() + 4.0, base), pos2(mid, base + 2.0)), gold_a(0), GOLD);
        hgrad(p, Rect::from_min_max(pos2(mid, base), pos2(rect.right() - 4.0, base + 2.0)), GOLD, gold_a(0));
    }
    p.text(rect.center() + vec2(0.0, 1.0), Align2::CENTER_CENTER, label, font, color);
    resp
}

/// The game's gold button (Apply, Enter the ring): a gold bar with dark serif words. `width` 0 fits the words.
pub fn gold_button(ui: &mut egui::Ui, label: &str, width: f32, height: f32, enabled: bool) -> egui::Response {
    let font = title_font((height * 0.5).clamp(13.0, 22.0));
    let g = ui.painter().layout_no_wrap(label.to_string(), font.clone(), DARK_TEXT);
    let w = if width > 0.0 { width } else { g.size().x + height * 1.3 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, height), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let p = ui.painter();
    let (hov, down) = (enabled && resp.hovered(), enabled && resp.is_pointer_button_down_on());
    let (top, bottom, text) = if !enabled {
        (rgba(96, 86, 72, 255), rgba(66, 58, 48, 255), rgba(40, 34, 28, 255))
    } else if down {
        (rgba(222, 172, 96, 255), rgba(196, 142, 64, 255), DARK_TEXT)
    } else if hov {
        (rgba(255, 238, 186, 255), rgba(232, 178, 96, 255), DARK_TEXT)
    } else {
        (rgba(250, 224, 158, 255), rgba(214, 156, 76, 255), DARK_TEXT)
    };
    grad_shape(p, rect, 3.0, top, bottom, Stroke::new(1.0, rgba(110, 72, 28, 255)));
    if enabled {
        p.line_segment([pos2(rect.left() + 3.0, rect.top() + 1.5), pos2(rect.right() - 3.0, rect.top() + 1.5)], Stroke::new(1.0, rgba(255, 248, 220, 170)));
    }
    p.text(rect.center() + vec2(0.0, if down { 1.5 } else { 0.5 }), Align2::CENTER_CENTER, label, font, text);
    resp
}

/// The game's dark button (Share, Follow): smoked glass with a faint gold edge that lights on hover. `danger` lights
/// it red.
pub fn ghost_button(ui: &mut egui::Ui, label: &str, width: f32, height: f32, danger: bool) -> egui::Response {
    let font = title_font((height * 0.48).clamp(13.0, 20.0));
    let g = ui.painter().layout_no_wrap(label.to_string(), font.clone(), INK);
    let w = if width > 0.0 { width } else { g.size().x + height * 1.2 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, height), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let p = ui.painter();
    let hov = resp.hovered();
    let lit = if danger { RED } else { GOLD };
    p.rect_filled(rect, 3.0, if hov { rgba(lit.r(), lit.g(), lit.b(), 26) } else { rgba(0, 0, 0, 150) });
    p.rect_stroke(rect, 3.0, Stroke::new(1.0, if hov { lit } else { gold_a(64) }), egui::StrokeKind::Inside);
    p.text(rect.center() + vec2(0.0, 0.5), Align2::CENTER_CENTER, label, font, if hov { if danger { RED } else { GOLD_HI } } else { INK });
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

/// The big gold button in the bottom bar (play / install / update).
pub fn big_button(ui: &mut egui::Ui, size: Vec2, label: &str, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let p = ui.painter();
    let hov = enabled && resp.hovered();
    if enabled {
        // a warm glow around it
        for i in 1..=7 {
            let g = rect.expand(i as f32 * 2.2);
            p.rect_filled(g, 4.0 + i as f32 * 2.0, rgba(240, 170, 70, if hov { 13 } else { 7 }));
        }
    }
    let (top, bottom, edge, text) = if !enabled {
        (Color32::from_rgb(82, 74, 62), Color32::from_rgb(54, 48, 40), Color32::from_rgb(100, 90, 74), MUTED)
    } else if hov {
        (Color32::from_rgb(255, 238, 186), Color32::from_rgb(232, 176, 92), Color32::from_rgb(255, 240, 200), DARK_TEXT)
    } else {
        (Color32::from_rgb(250, 224, 158), Color32::from_rgb(210, 150, 70), Color32::from_rgb(255, 232, 180), DARK_TEXT)
    };
    let r = if enabled && resp.is_pointer_button_down_on() { rect.shrink(1.0) } else { rect };
    grad_shape(p, r, 4.0, top, bottom, Stroke::new(1.0, rgba(110, 72, 28, 255)));
    let inner = r.shrink(3.5);
    p.add(Shape::closed_line(chamfer(inner, 2.5), Stroke::new(1.0, rgba(edge.r(), edge.g(), edge.b(), if enabled { 120 } else { 30 }))));
    p.text(r.center() + vec2(0.0, 1.0), Align2::CENTER_CENTER, sentence(label), title_font(28.0), text);
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
        Stroke::new(1.0, gold_a(40))
    };
    let (t, b) = if selected { (rgba(58, 42, 22, 235), rgba(28, 20, 12, 240)) } else { (rgba(24, 19, 14, 225), rgba(14, 11, 8, 232)) };
    grad_shape(p, rect, 8.0, t, b, edge);
    // radio dot
    let dot = rect.left_top() + vec2(22.0, 26.0);
    p.circle_stroke(dot, 7.0, Stroke::new(1.3, if selected { GOLD } else { MUTED }));
    if selected {
        p.circle_filled(dot, 3.5, GOLD);
    }
    p.text(rect.left_top() + vec2(40.0, 14.0), Align2::LEFT_TOP, title, title_font(16.5), if selected { GOLD_HI } else { INK });
    p.text(rect.right_top() + vec2(-16.0, 18.0), Align2::RIGHT_TOP, size, bold_font(13.0), if selected { GOLD } else { MUTED });
    let g = p.layout(detail.to_string(), FontId::proportional(13.0), MUTED, width - 56.0);
    p.galley(rect.left_top() + vec2(40.0, 44.0), g, MUTED);
    enabled && resp.clicked()
}

/// "INSTALL GAME" -> "Install game": the game's buttons speak in sentences, not capitals.
pub fn sentence(label: &str) -> String {
    let low = label.to_lowercase();
    let mut c = low.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
