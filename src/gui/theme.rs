//! The launcher's look: colours, fonts, the egui style, and the painting every screen draws with (the backdrop,
//! gradients, cut-corner shapes and cards). Screens take it with `use crate::gui::theme::*`.

use eframe::egui::{self, Color32, FontData, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use std::sync::Arc;

// ---------------------------------------------------------------------------------------------------------------
// Look
// ---------------------------------------------------------------------------------------------------------------

// The arena's look (the in-game Arena window, pts_lobby): warm black glass, antique gold, cream text, the owner's
// paintings behind it all.

/// The accent: antique gold (the picked tab, edges, names).
pub const GOLD: Color32 = Color32::from_rgb(234, 196, 124);
pub const GOLD_HI: Color32 = Color32::from_rgb(255, 230, 172);
pub const GOLD_DEEP: Color32 = Color32::from_rgb(196, 140, 64);
/// The window's background: what every picture fades into.
pub const BG: Color32 = Color32::from_rgb(15, 11, 8);
/// Warnings (the AA Classic password line, a plan in its last days).
pub const AMBER: Color32 = Color32::from_rgb(240, 178, 84);
pub const INK: Color32 = Color32::from_rgb(240, 230, 210);
pub const MUTED: Color32 = Color32::from_rgb(178, 162, 138);
/// Text on gold.
pub const DARK_TEXT: Color32 = Color32::from_rgb(52, 32, 10);
pub const GREEN: Color32 = Color32::from_rgb(158, 206, 120);
pub const RED: Color32 = Color32::from_rgb(224, 98, 82);
/// Links and the second kind of status (pale gold: the arena has no blue).
pub const BLUE: Color32 = Color32::from_rgb(232, 206, 150);

pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub fn gold_a(a: u8) -> Color32 {
    rgba(234, 196, 124, a)
}

pub fn bg_a(a: u8) -> Color32 {
    rgba(15, 11, 8, a)
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
    v.panel_fill = BG;
    v.window_fill = Color32::from_rgb(22, 17, 12);
    v.window_stroke = Stroke::new(1.0, gold_a(70));
    v.extreme_bg_color = Color32::from_rgb(9, 7, 5);
    v.selection.bg_fill = Color32::from_rgb(112, 80, 34);
    v.selection.stroke = Stroke::new(1.0, GOLD);
    v.hyperlink_color = BLUE;
    for (w, fill, stroke) in [
        (&mut v.widgets.inactive, rgba(30, 23, 17, 235), Color32::from_rgb(92, 72, 46)),
        (&mut v.widgets.hovered, rgba(44, 34, 23, 240), GOLD),
        (&mut v.widgets.active, rgba(54, 41, 27, 245), GOLD_HI),
        (&mut v.widgets.open, rgba(44, 34, 23, 240), GOLD),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, stroke);
        w.corner_radius = 3.into();
        w.expansion = 0.0;
    }
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, GOLD_HI);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(58, 46, 32));
    ctx.set_visuals_of(egui::Theme::Dark, v.clone());
    ctx.set_visuals_of(egui::Theme::Light, v);
    ctx.all_styles_mut(|s| {
        use egui::TextStyle::*;
        s.spacing.item_spacing = vec2(10.0, 8.0);
        s.spacing.button_padding = vec2(16.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.text_styles.insert(Body, FontId::proportional(15.0));
        // buttons in the title serif, as the game's are
        s.text_styles.insert(Button, title_font(15.0));
        s.text_styles.insert(Small, FontId::proportional(12.5));
        s.text_styles.insert(Monospace, FontId::monospace(12.5));
        s.text_styles.insert(Heading, title_font(21.0));
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

/// The arena's panel: dark smoked glass, a faint gold edge, a gold line along the top that is brightest in the middle,
/// and small gold brackets in the corners.
pub fn card(p: &Painter, r: Rect) {
    p.rect_filled(r, 2.0, rgba(13, 10, 7, 228));
    p.rect_stroke(r, 2.0, Stroke::new(1.0, gold_a(34)), egui::StrokeKind::Inside);
    top_edge(p, r);
    corners(p, r, 11.0, gold_a(200));
}

/// The gold line along the top of a panel or card.
pub fn top_edge(p: &Painter, r: Rect) {
    let y = r.top();
    let mid = r.center().x;
    hgrad(p, Rect::from_min_max(pos2(r.left(), y), pos2(mid, y + 1.5)), gold_a(20), gold_a(210));
    hgrad(p, Rect::from_min_max(pos2(mid, y), pos2(r.right(), y + 1.5)), gold_a(210), gold_a(20));
}

/// Gold brackets on the four corners of `r`, `k` long.
pub fn corners(p: &Painter, r: Rect, k: f32, color: Color32) {
    let s = Stroke::new(1.5, color);
    for (c, dx, dy) in [(r.left_top(), 1.0, 1.0), (r.right_top(), -1.0, 1.0), (r.left_bottom(), 1.0, -1.0), (r.right_bottom(), -1.0, -1.0)] {
        p.line_segment([c, c + vec2(dx * k, 0.0)], s);
        p.line_segment([c, c + vec2(0.0, dy * k)], s);
    }
}

/// The window's background behind the bars: warm black.
pub fn backdrop(p: &Painter, r: Rect) {
    p.rect_filled(r, 0.0, BG);
}

/// A screen's painting behind its body, as the Arena window's front: the picture on the right, fading into the
/// background on the left (where the words are) and at the bottom. `light` 0..1 is how bright it stays (the Play
/// screen's is bright; screens full of cards keep it dim).
pub fn scene(p: &Painter, tex: &egui::TextureHandle, body: Rect, light: f32) {
    let g = (255.0 * light.clamp(0.0, 1.0)) as u8;
    crate::gui::pictures::draw(p, tex, body, Color32::from_gray(g));
    let x = |f: f32| body.left() + body.width() * f;
    let y = |f: f32| body.top() + body.height() * f;
    // the left: nearly solid where the words are, gone by two thirds of the width
    p.rect_filled(Rect::from_min_max(body.min, pos2(x(0.18), body.bottom())), 0.0, bg_a(236));
    hgrad(p, Rect::from_min_max(pos2(x(0.18), body.top()), pos2(x(0.46), body.bottom())), bg_a(236), bg_a(120));
    hgrad(p, Rect::from_min_max(pos2(x(0.46), body.top()), pos2(x(0.68), body.bottom())), bg_a(120), bg_a(0));
    // the bottom and a touch of the top
    vgrad(p, Rect::from_min_max(pos2(body.left(), y(0.52)), body.max), bg_a(0), bg_a(250));
    vgrad(p, Rect::from_min_max(body.min, pos2(body.right(), y(0.14))), bg_a(150), bg_a(0));
}

/// A title in the arena's gold: deep gold with its top half lit, on a soft shadow (egui has no gradient text).
pub fn gold_title(p: &Painter, pos: Pos2, align: egui::Align2, text: &str, font: FontId) -> Rect {
    p.text(pos + vec2(0.0, 2.5), align, text, font.clone(), rgba(0, 0, 0, 170));
    let r = p.text(pos, align, text, font.clone(), GOLD_DEEP);
    let lit = Rect::from_min_max(r.min - vec2(4.0, 6.0), pos2(r.right() + 4.0, r.top() + r.height() * 0.56));
    p.with_clip_rect(lit).text(pos, align, text, font, GOLD_HI);
    r
}

/// Text with a dark shadow under it: words standing on a painting.
pub fn shadow_text(p: &Painter, pos: Pos2, align: egui::Align2, text: impl ToString, font: FontId, color: Color32) -> Rect {
    let t = text.to_string();
    p.text(pos + vec2(0.0, 1.5), align, &t, font.clone(), rgba(0, 0, 0, 190));
    p.text(pos, align, t, font, color)
}

// ---------------------------------------------------------------------------------------------------------------
// Icons: small vector shapes painted in code (no picture files, nothing of AA Classic's art)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    Play,
    Lock,
    Star,
    Clock,
    Refresh,
    /// a film frame: the empty Library
    Film,
}

/// Paints `kind` centred on `c`, `s` pixels across.
pub fn icon(p: &Painter, c: Pos2, s: f32, kind: Icon, color: Color32) {
    let k = s / 20.0;
    let w = (1.6 * k).max(1.2);
    let line = Stroke::new(w, color);
    let at = |x: f32, y: f32| c + vec2(x * k, y * k);
    match kind {
        Icon::Play => {
            p.add(Shape::convex_polygon(vec![at(-5.0, -8.0), at(8.5, 0.0), at(-5.0, 8.0)], color, Stroke::NONE));
        }
        Icon::Lock => {
            p.add(Shape::rect_filled(Rect::from_min_max(at(-7.0, -1.0), at(7.0, 9.0)), 1.5 * k, color));
            let arc: Vec<Pos2> = (0..=12).map(|i| {
                let a = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
                at(4.5 * a.cos(), -1.0 + 6.0 * a.sin())
            }).collect();
            p.add(Shape::line(arc, line));
        }
        Icon::Star => {
            let pts: Vec<Pos2> = (0..10).map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                let r = if i % 2 == 0 { 9.0 } else { 3.8 };
                at(r * a.cos(), r * a.sin())
            }).collect();
            // a star is not convex: a fan of triangles from the centre
            for i in 0..10 {
                p.add(Shape::convex_polygon(vec![c, pts[i], pts[(i + 1) % 10]], color, Stroke::NONE));
            }
        }
        Icon::Clock => {
            p.circle_stroke(c, 8.0 * k, line);
            p.line_segment([c, at(0.0, -5.0)], line);
            p.line_segment([c, at(3.5, 2.0)], line);
        }
        Icon::Refresh => {
            let arc: Vec<Pos2> = (0..=20).map(|i| {
                let a = 0.35 + i as f32 / 20.0 * 5.2;
                at(7.0 * a.cos(), 7.0 * a.sin())
            }).collect();
            let end = *arc.last().unwrap();
            p.add(Shape::line(arc, line));
            p.add(Shape::convex_polygon(vec![end + vec2(-4.0 * k, -3.0 * k), end + vec2(3.5 * k, -3.5 * k), end + vec2(0.5 * k, 3.5 * k)], color, Stroke::NONE));
        }
        Icon::Film => {
            p.add(Shape::rect_stroke(Rect::from_center_size(c, vec2(18.0 * k, 14.0 * k)), 1.5 * k, line, egui::StrokeKind::Middle));
            for x in [-6.5, -2.2, 2.2, 6.5] {
                for y in [-5.0, 5.0] {
                    p.add(Shape::rect_filled(Rect::from_center_size(at(x, y), vec2(2.0 * k, 1.6 * k)), 0.0, color));
                }
            }
            p.add(Shape::convex_polygon(vec![at(-2.0, -2.5), at(3.0, 0.0), at(-2.0, 2.5)], color, Stroke::NONE));
        }
    }
}

/// A round icon button: the icon in a ring that lights on hover. `danger` turns it red on hover.
pub fn icon_button(ui: &mut egui::Ui, kind: Icon, size: f32, tip: &str, danger: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    let hov = resp.hovered();
    let col = if hov { if danger { RED } else { GOLD } } else { MUTED };
    let p = ui.painter();
    p.circle_filled(rect.center(), size / 2.0, if hov { rgba(255, 220, 150, 22) } else { rgba(0, 0, 0, 120) });
    p.circle_stroke(rect.center(), size / 2.0 - 0.5, Stroke::new(1.0, if hov { col } else { gold_a(70) }));
    icon(p, rect.center(), size * 0.5, kind, col);
    resp.on_hover_text(tip)
}
