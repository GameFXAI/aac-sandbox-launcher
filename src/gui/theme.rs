//! The launcher's look: colours, fonts, the egui style, and the painting every screen draws with (the backdrop,
//! gradients, cut-corner shapes and cards). Screens take it with `use crate::gui::theme::*`.

use eframe::egui::{self, Color32, FontData, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use std::sync::Arc;

// ---------------------------------------------------------------------------------------------------------------
// Look
// ---------------------------------------------------------------------------------------------------------------

/// The accent: AA REPLAY teal (the name stays from the first look). VIOLET is the second brand colour.
pub const GOLD: Color32 = Color32::from_rgb(72, 212, 194);
pub const VIOLET: Color32 = Color32::from_rgb(150, 112, 240);
/// Warnings (the AA Classic password line).
pub const AMBER: Color32 = Color32::from_rgb(236, 186, 92);
pub const INK: Color32 = Color32::from_rgb(234, 228, 214);
pub const MUTED: Color32 = Color32::from_rgb(146, 156, 170);
pub const DARK_TEXT: Color32 = Color32::from_rgb(6, 30, 34);
pub const GREEN: Color32 = Color32::from_rgb(118, 204, 140);
pub const RED: Color32 = Color32::from_rgb(232, 112, 100);
pub const BLUE: Color32 = Color32::from_rgb(110, 170, 240);

pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub fn gold_a(a: u8) -> Color32 {
    rgba(72, 212, 194, a)
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), m(a.a(), b.a()))
}

pub fn title_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("title".into()))
}

pub fn bold_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// The bundled fonts (assets/fonts, each with its license): Open Sans for text, Open Sans SemiBold for "bold" and
/// TeX Gyre Pagella Bold, a Palatino clone, for titles. Nothing is read from Windows' font folder.
static UI_FONT: &[u8] = include_bytes!("../../assets/fonts/OpenSans-Regular.ttf");
static BOLD_FONT: &[u8] = include_bytes!("../../assets/fonts/OpenSans-SemiBold.ttf");
static TITLE_FONT: &[u8] = include_bytes!("../../assets/fonts/texgyrepagella-bold.otf");

/// Text in the "ui" font, and the "bold" and "title" families every screen asks for by name.
pub fn install_fonts(ctx: &egui::Context) {
    let mut f = egui::FontDefinitions::default();
    // Open Sans runs about 5% wider than Segoe UI: at 0.95 lines measure (and wrap) as they did with Segoe UI
    for (key, bytes, scale) in [("ui", UI_FONT, 0.95), ("bold", BOLD_FONT, 0.95), ("title", TITLE_FONT, 1.0)] {
        let tweak = egui::epaint::text::FontTweak { scale, ..Default::default() };
        f.font_data.insert(key.into(), Arc::new(FontData::from_static(bytes).tweak(tweak)));
    }
    let base = f.families[&FontFamily::Proportional].clone();
    f.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "ui".into());
    let with = |key: &str| {
        let mut v: Vec<String> = vec![key.into()];
        v.extend(base.iter().cloned());
        v
    };
    f.families.insert(FontFamily::Name("bold".into()), with("bold"));
    f.families.insert(FontFamily::Name("title".into()), with("title"));
    ctx.set_fonts(f);
}

pub fn install_style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(INK);
    v.extreme_bg_color = Color32::from_rgb(12, 16, 23);
    v.selection.bg_fill = Color32::from_rgb(28, 92, 90);
    v.selection.stroke = Stroke::new(1.0, GOLD);
    for (w, fill, stroke) in [
        (&mut v.widgets.inactive, Color32::from_rgb(26, 32, 43), Color32::from_rgb(54, 62, 78)),
        (&mut v.widgets.hovered, Color32::from_rgb(34, 41, 54), GOLD),
        (&mut v.widgets.active, Color32::from_rgb(40, 47, 60), GOLD),
        (&mut v.widgets.open, Color32::from_rgb(34, 41, 54), GOLD),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, stroke);
        w.corner_radius = 3.into();
        w.expansion = 0.0;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(44, 52, 66));
    ctx.set_visuals_of(egui::Theme::Dark, v.clone());
    ctx.set_visuals_of(egui::Theme::Light, v);
    ctx.all_styles_mut(|s| {
        use egui::TextStyle::*;
        s.spacing.item_spacing = vec2(10.0, 8.0);
        s.spacing.button_padding = vec2(14.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.text_styles.insert(Body, FontId::proportional(15.0));
        s.text_styles.insert(Button, FontId::proportional(14.5));
        s.text_styles.insert(Small, FontId::proportional(12.5));
        s.text_styles.insert(Monospace, FontId::monospace(12.5));
        s.text_styles.insert(Heading, bold_font(20.0));
    });
}

// ---------------------------------------------------------------------------------------------------------------
// Painting helpers
// ---------------------------------------------------------------------------------------------------------------

pub fn vgrad(p: &Painter, r: Rect, top: Color32, bottom: Color32) {
    let mut m = egui::Mesh::default();
    m.colored_vertex(r.left_top(), top);
    m.colored_vertex(r.right_top(), top);
    m.colored_vertex(r.left_bottom(), bottom);
    m.colored_vertex(r.right_bottom(), bottom);
    m.add_triangle(0, 1, 2);
    m.add_triangle(1, 2, 3);
    p.add(Shape::mesh(m));
}

pub fn hgrad(p: &Painter, r: Rect, left: Color32, right: Color32) {
    let mut m = egui::Mesh::default();
    m.colored_vertex(r.left_top(), left);
    m.colored_vertex(r.right_top(), right);
    m.colored_vertex(r.left_bottom(), left);
    m.colored_vertex(r.right_bottom(), right);
    m.add_triangle(0, 1, 2);
    m.add_triangle(1, 2, 3);
    p.add(Shape::mesh(m));
}

/// Rectangle with cut corners.
pub fn chamfer(r: Rect, c: f32) -> Vec<Pos2> {
    vec![
        pos2(r.left() + c, r.top()),
        pos2(r.right() - c, r.top()),
        pos2(r.right(), r.top() + c),
        pos2(r.right(), r.bottom() - c),
        pos2(r.right() - c, r.bottom()),
        pos2(r.left() + c, r.bottom()),
        pos2(r.left(), r.bottom() - c),
        pos2(r.left(), r.top() + c),
    ]
}

/// Cut-corner shape filled with a vertical gradient.
pub fn grad_shape(p: &Painter, r: Rect, c: f32, top: Color32, bottom: Color32, stroke: Stroke) {
    let pts = chamfer(r, c);
    let mut m = egui::Mesh::default();
    m.colored_vertex(r.center(), lerp(top, bottom, 0.5));
    for q in &pts {
        m.colored_vertex(*q, lerp(top, bottom, (q.y - r.top()) / r.height()));
    }
    for i in 0..pts.len() as u32 {
        m.add_triangle(0, 1 + i, 1 + (i + 1) % pts.len() as u32);
    }
    p.add(Shape::mesh(m));
    if stroke.width > 0.0 {
        p.add(Shape::closed_line(pts, stroke));
    }
}

/// Dark glass card with a thin gold edge and gold corner marks.
pub fn card(p: &Painter, r: Rect) {
    grad_shape(p, r, 12.0, rgba(14, 19, 28, 232), rgba(8, 11, 17, 240), Stroke::new(1.0, gold_a(70)));
    let k = 16.0;
    let s = Stroke::new(1.5, gold_a(200));
    for (c, dx, dy) in [
        (r.left_top() + vec2(4.0, 4.0), 1.0, 1.0),
        (r.right_top() + vec2(-4.0, 4.0), -1.0, 1.0),
        (r.left_bottom() + vec2(4.0, -4.0), 1.0, -1.0),
        (r.right_bottom() + vec2(-4.0, -4.0), -1.0, -1.0),
    ] {
        p.line_segment([c, c + vec2(dx * k, 0.0)], s);
        p.line_segment([c, c + vec2(0.0, dy * k)], s);
    }
}

/// Night sky, moon and three mountain ranges, painted in code.
pub fn backdrop(p: &Painter, r: Rect) {
    vgrad(p, Rect::from_min_max(r.min, pos2(r.right(), r.top() + r.height() * 0.62)), Color32::from_rgb(16, 26, 46), Color32::from_rgb(40, 46, 66));
    vgrad(p, Rect::from_min_max(pos2(r.left(), r.top() + r.height() * 0.62), r.max), Color32::from_rgb(40, 46, 66), Color32::from_rgb(8, 10, 16));

    // stars (fixed seed: the same sky every time)
    let mut s: u32 = 0x9e37_79b9;
    let mut rnd = || {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        (s % 10_000) as f32 / 10_000.0
    };
    for _ in 0..140 {
        let pos = pos2(r.left() + rnd() * r.width(), r.top() + rnd() * r.height() * 0.55);
        let a = (40.0 + rnd() * 160.0) as u8;
        let size = if rnd() > 0.93 { 1.6 } else { 0.9 };
        p.circle_filled(pos, size, rgba(230, 230, 255, a));
    }

    // moon with a warm glow
    let moon = pos2(r.left() + r.width() * 0.52, r.top() + r.height() * 0.19);
    for i in (0..16).rev() {
        p.circle_filled(moon, 36.0 + i as f32 * 16.0, rgba(140, 200, 240, 3 + (15 - i) as u8 / 2));
    }
    p.circle_filled(moon, 38.0, Color32::from_rgb(246, 236, 210));
    p.circle_filled(moon + vec2(-10.0, -6.0), 7.0, rgba(214, 200, 170, 90));
    p.circle_filled(moon + vec2(12.0, 10.0), 5.0, rgba(214, 200, 170, 80));

    ridge(p, r, 0.50, 80.0, 0.8, 1.3, rgba(58, 68, 96, 255), rgba(30, 36, 52, 255));
    ridge(p, r, 0.63, 64.0, 1.4, 4.1, rgba(32, 40, 58, 255), rgba(16, 20, 30, 255));
    ridge(p, r, 0.76, 46.0, 2.3, 2.7, rgba(16, 20, 30, 255), rgba(7, 9, 14, 255));

    // shade the left side so the text reads well
    hgrad(p, Rect::from_min_max(r.min, pos2(r.left() + r.width() * 0.55, r.bottom())), rgba(4, 6, 10, 200), rgba(4, 6, 10, 0));
}

pub fn ridge(p: &Painter, r: Rect, base: f32, amp: f32, freq: f32, seed: f32, top: Color32, bottom: Color32) {
    let n = 120u32;
    let mut m = egui::Mesh::default();
    for i in 0..=n {
        let fx = i as f32 / n as f32;
        let x = r.left() + fx * r.width();
        let h = (fx * freq * 6.0 + seed).sin() * 0.55
            + (fx * freq * 13.0 + seed * 2.1).sin() * 0.25
            + (fx * freq * 31.0 + seed * 0.7).sin().abs() * 0.15;
        let y = r.top() + r.height() * base - h * amp;
        m.colored_vertex(pos2(x, y), top);
        m.colored_vertex(pos2(x, r.bottom()), bottom);
        if i > 0 {
            let k = i * 2;
            m.add_triangle(k - 2, k - 1, k);
            m.add_triangle(k - 1, k, k + 1);
        }
    }
    p.add(Shape::mesh(m));
}
