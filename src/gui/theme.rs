//! The app's look (Obsidian Ember, docs/companion-app.md): near-black graphite, one amber accent on the picked item and
//! the primary action, warm white text, cool grey microcopy, Plus Jakarta Sans. Colours, fonts, the egui style and the
//! painting every page draws with. Pages take it with `use crate::gui::theme::*`.

use eframe::egui::{self, Color32, FontData, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use std::sync::Arc;

pub use super::icons::{Icon, icon, icon_button};

// ---------------------------------------------------------------------------------------------------------------
// Colours
// ---------------------------------------------------------------------------------------------------------------

/// The window: near black.
pub const BG: Color32 = Color32::from_rgb(11, 11, 13);
/// Panels (the inspector, cards).
pub const PANEL: Color32 = Color32::from_rgb(19, 19, 22);
/// Raised controls (fields, buttons).
pub const RAISED: Color32 = Color32::from_rgb(26, 26, 30);
/// Hairlines.
pub const LINE: Color32 = Color32::from_rgb(40, 40, 46);
/// The accent: amber. Only on the picked item and the primary action.
pub const GOLD: Color32 = Color32::from_rgb(242, 161, 74);
pub const GOLD_HI: Color32 = Color32::from_rgb(255, 196, 120);
/// Warnings (a plan in its last days, the AA Classic password line).
pub const AMBER: Color32 = Color32::from_rgb(240, 180, 90);
/// Warm white text.
pub const INK: Color32 = Color32::from_rgb(243, 238, 230);
/// Cool grey microcopy.
pub const MUTED: Color32 = Color32::from_rgb(152, 152, 162);
pub const FAINT: Color32 = Color32::from_rgb(104, 104, 114);
/// Text on amber.
pub const DARK_TEXT: Color32 = Color32::from_rgb(28, 16, 4);
/// Presence and "running".
pub const GREEN: Color32 = Color32::from_rgb(59, 208, 127);
/// Alerts: terracotta.
pub const RED: Color32 = Color32::from_rgb(224, 118, 90);
/// Links.
pub const BLUE: Color32 = Color32::from_rgb(236, 190, 128);

pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// The accent at an alpha: bronze fills and glows.
pub fn gold_a(a: u8) -> Color32 {
    rgba(242, 161, 74, a)
}

pub fn bg_a(a: u8) -> Color32 {
    rgba(11, 11, 13, a)
}

// ---------------------------------------------------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------------------------------------------------

/// Headings and names: Plus Jakarta Sans SemiBold.
pub fn title_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// Emphasis: SemiBold.
pub fn bold_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// Tabs, buttons, labels: Medium.
pub fn medium_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

pub fn text_font(size: f32) -> FontId {
    FontId::proportional(size)
}

/// The bundled fonts (assets/fonts, SIL Open Font License): Plus Jakarta Sans Regular, Medium and SemiBold, static
/// instances of the variable font. Nothing is read from Windows' font folder.
static UI_FONT: &[u8] = include_bytes!("../../assets/fonts/PlusJakartaSans-Regular.ttf");
static MEDIUM_FONT: &[u8] = include_bytes!("../../assets/fonts/PlusJakartaSans-Medium.ttf");
static BOLD_FONT: &[u8] = include_bytes!("../../assets/fonts/PlusJakartaSans-SemiBold.ttf");

pub fn install_fonts(ctx: &egui::Context) {
    let mut f = egui::FontDefinitions::default();
    for (key, bytes) in [("ui", UI_FONT), ("medium", MEDIUM_FONT), ("bold", BOLD_FONT)] {
        f.font_data.insert(key.into(), Arc::new(FontData::from_static(bytes)));
    }
    let base = f.families[&FontFamily::Proportional].clone();
    f.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "ui".into());
    let with = |key: &str| {
        let mut v: Vec<String> = vec![key.into()];
        v.extend(base.iter().cloned());
        v
    };
    f.families.insert(FontFamily::Name("bold".into()), with("bold"));
    f.families.insert(FontFamily::Name("medium".into()), with("medium"));
    ctx.set_fonts(f);
}

pub fn install_style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(INK);
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.window_stroke = Stroke::new(1.0, LINE);
    v.window_corner_radius = 12.into();
    v.extreme_bg_color = Color32::from_rgb(15, 15, 18);
    v.selection.bg_fill = gold_a(90);
    v.selection.stroke = Stroke::new(1.0, GOLD);
    v.hyperlink_color = BLUE;
    v.text_cursor.stroke = Stroke::new(1.5, GOLD);
    for (w, fill, stroke) in [
        (&mut v.widgets.inactive, RAISED, Color32::from_rgb(50, 50, 57)),
        (&mut v.widgets.hovered, Color32::from_rgb(33, 33, 38), Color32::from_rgb(86, 86, 96)),
        (&mut v.widgets.active, Color32::from_rgb(38, 38, 44), GOLD),
        (&mut v.widgets.open, Color32::from_rgb(33, 33, 38), GOLD),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, stroke);
        w.corner_radius = 8.into();
        w.expansion = 0.0;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.noninteractive.corner_radius = 8.into();
    ctx.set_visuals_of(egui::Theme::Dark, v.clone());
    ctx.set_visuals_of(egui::Theme::Light, v);
    ctx.all_styles_mut(|s| {
        use egui::TextStyle::*;
        s.spacing.item_spacing = vec2(10.0, 8.0);
        s.spacing.button_padding = vec2(14.0, 7.0);
        s.spacing.interact_size.y = 32.0;
        s.text_styles.insert(Body, text_font(14.0));
        s.text_styles.insert(Button, medium_font(14.0));
        s.text_styles.insert(Small, text_font(12.0));
        s.text_styles.insert(Monospace, FontId::monospace(12.5));
        s.text_styles.insert(Heading, title_font(22.0));
    });
}

// ---------------------------------------------------------------------------------------------------------------
// Painting
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

/// A panel: graphite with a hairline edge.
pub fn card(p: &Painter, r: Rect) {
    p.rect_filled(r, 10.0, PANEL);
    p.rect_stroke(r, 10.0, Stroke::new(1.0, LINE), egui::StrokeKind::Inside);
}

/// A soft amber glow around `r` (the picked item, the primary action): rings that fade outwards.
pub fn glow(p: &Painter, r: Rect, radius: f32, strength: f32) {
    for i in 1..=7 {
        let a = ((8 - i) as f32 * 4.2 * strength) as u8;
        p.rect_stroke(r.expand(i as f32 * 1.6), radius + i as f32 * 1.6, Stroke::new(1.8, gold_a(a)), egui::StrokeKind::Outside);
    }
}

/// The picked item's rim: a glow and a fine amber edge.
pub fn picked_rim(p: &Painter, r: Rect, radius: f32) {
    glow(p, r, radius, 1.0);
    p.rect_stroke(r, radius, Stroke::new(1.5, GOLD), egui::StrokeKind::Inside);
}

/// A bronze fill: amber at low alpha, a little lighter at the top.
pub fn bronze(p: &Painter, r: Rect, radius: f32, strength: f32) {
    let a = |x: f32| (x * strength).clamp(0.0, 255.0) as u8;
    p.rect_filled(r, radius, gold_a(a(34.0)));
    let top = Rect::from_min_max(r.min, pos2(r.right(), r.center().y));
    p.rect_filled(top.shrink2(vec2(radius * 0.5, 0.0)), 0.0, gold_a(a(10.0)));
}

/// A person, shown by the first letter of their name in a circle of a colour of their own (no stock faces).
pub fn avatar(p: &Painter, c: Pos2, r: f32, name: &str) {
    const TONES: [Color32; 8] = [
        Color32::from_rgb(120, 86, 60),
        Color32::from_rgb(70, 96, 120),
        Color32::from_rgb(96, 110, 74),
        Color32::from_rgb(118, 72, 82),
        Color32::from_rgb(84, 80, 120),
        Color32::from_rgb(128, 104, 58),
        Color32::from_rgb(64, 110, 104),
        Color32::from_rgb(110, 84, 110),
    ];
    let h = name.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b.to_ascii_lowercase() as u32));
    let tone = TONES[(h % TONES.len() as u32) as usize];
    p.circle_filled(c, r, tone);
    p.circle_stroke(c, r, Stroke::new(1.0, rgba(255, 255, 255, 30)));
    let letter = name.trim().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
    p.text(c + vec2(0.0, 0.5), egui::Align2::CENTER_CENTER, letter, bold_font(r * 0.95), INK);
}

/// The window's background.
pub fn backdrop(p: &Painter, r: Rect) {
    p.rect_filled(r, 0.0, BG);
}

/// A section heading in small spaced capitals ("DISCUSSION", "SHARING").
pub fn caps(p: &Painter, pos: Pos2, text: &str, color: Color32) -> Rect {
    let spaced: String = text.to_uppercase().chars().flat_map(|c| [c, '\u{200A}']).collect();
    p.text(pos, egui::Align2::LEFT_TOP, spaced, medium_font(11.5), color)
}

/// A thin horizontal rule.
pub fn rule(p: &Painter, x0: f32, x1: f32, y: f32) {
    p.line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(1.0, LINE));
}
