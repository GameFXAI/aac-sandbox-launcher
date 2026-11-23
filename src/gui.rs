//! The launcher window: a game-launcher style screen with a backdrop painted in code.
//! It deliberately uses no AA Classic logo or artwork, so it never passes for the official launcher.

use crate::core::*;
use crate::net::{self, Server};
use crate::{addons, find, install, recorder, settings, signin, transfer, update, vault};

/// Is there an AA Classic profile on this PC to bring settings from?
fn transfer_docs_present() -> bool {
    transfer::real_docs().is_some()
}
use eframe::egui::{
    self, Align, Align2, Color32, FontData, FontFamily, FontId, Layout, Margin, Painter, Pos2, Rect, RichText, Sense,
    Shape, Stroke, TextEdit, UiBuilder, Vec2, ViewportCommand, pos2, vec2,
};
use std::process::Child;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, SystemTime};

mod transfer_ui;
use transfer_ui::{XMsg, Xfer};

// ---------------------------------------------------------------------------------------------------------------
// Look
// ---------------------------------------------------------------------------------------------------------------

/// The accent: AA REPLAY teal (the name stays from the first look). VIOLET is the second brand colour.
const GOLD: Color32 = Color32::from_rgb(72, 212, 194);
const VIOLET: Color32 = Color32::from_rgb(150, 112, 240);
/// Warnings (the AA Classic password line).
const AMBER: Color32 = Color32::from_rgb(236, 186, 92);
const INK: Color32 = Color32::from_rgb(234, 228, 214);
const MUTED: Color32 = Color32::from_rgb(146, 156, 170);
const DARK_TEXT: Color32 = Color32::from_rgb(6, 30, 34);
const GREEN: Color32 = Color32::from_rgb(118, 204, 140);
const RED: Color32 = Color32::from_rgb(232, 112, 100);
const BLUE: Color32 = Color32::from_rgb(110, 170, 240);

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

fn gold_a(a: u8) -> Color32 {
    rgba(72, 212, 194, a)
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), m(a.a(), b.a()))
}

fn title_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("title".into()))
}

fn bold_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// Segoe UI for text and Palatino for titles, read from Windows' own font folder (nothing bundled).
fn install_fonts(ctx: &egui::Context) {
    let mut f = egui::FontDefinitions::default();
    let dir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let mut load = |key: &str, files: &[&str]| -> bool {
        for n in files {
            if let Ok(b) = std::fs::read(format!("{dir}\\Fonts\\{n}")) {
                f.font_data.insert(key.into(), Arc::new(FontData::from_owned(b)));
                return true;
            }
        }
        false
    };
    let ui = load("ui", &["segoeui.ttf"]);
    let bold = load("bold", &["seguisb.ttf", "segoeuib.ttf"]);
    let title = load("title", &["palab.ttf", "georgiab.ttf"]);
    let base = f.families[&FontFamily::Proportional].clone();
    if ui {
        f.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "ui".into());
    }
    let with = |key: &str, have: bool| {
        let mut v: Vec<String> = if have { vec![key.into()] } else { vec![] };
        v.extend(base.iter().cloned());
        v
    };
    f.families.insert(FontFamily::Name("bold".into()), with("bold", bold));
    f.families.insert(FontFamily::Name("title".into()), with("title", title));
    ctx.set_fonts(f);
}

fn install_style(ctx: &egui::Context) {
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

fn vgrad(p: &Painter, r: Rect, top: Color32, bottom: Color32) {
    let mut m = egui::Mesh::default();
    m.colored_vertex(r.left_top(), top);
    m.colored_vertex(r.right_top(), top);
    m.colored_vertex(r.left_bottom(), bottom);
    m.colored_vertex(r.right_bottom(), bottom);
    m.add_triangle(0, 1, 2);
    m.add_triangle(1, 2, 3);
    p.add(Shape::mesh(m));
}

fn hgrad(p: &Painter, r: Rect, left: Color32, right: Color32) {
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
fn chamfer(r: Rect, c: f32) -> Vec<Pos2> {
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
fn grad_shape(p: &Painter, r: Rect, c: f32, top: Color32, bottom: Color32, stroke: Stroke) {
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
fn card(p: &Painter, r: Rect) {
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
fn backdrop(p: &Painter, r: Rect) {
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

fn ridge(p: &Painter, r: Rect, base: f32, amp: f32, freq: f32, seed: f32, top: Color32, bottom: Color32) {
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

// ---------------------------------------------------------------------------------------------------------------
// Widgets
// ---------------------------------------------------------------------------------------------------------------

fn nav_tab(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
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
enum WinBtn {
    Close,
    Minimize,
}

fn win_button(ui: &mut egui::Ui, kind: WinBtn) -> egui::Response {
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
fn gear_button(ui: &mut egui::Ui, open: bool) -> egui::Response {
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
fn big_button(ui: &mut egui::Ui, size: Vec2, label: &str, enabled: bool) -> egui::Response {
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
fn field(ui: &mut egui::Ui, caption: &str, value: &mut String, password: bool, hint: &str) -> egui::Response {
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

fn tick_row(ui: &mut egui::Ui, title: &str, detail: &str) {
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
fn mode_card(ui: &mut egui::Ui, width: f32, selected: bool, enabled: bool, title: &str, size: &str, detail: &str) -> bool {
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

// ---------------------------------------------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------------------------------------------

enum Msg {
    Line(String),
    SetupDone(Result<(), String>),
    /// the account API's answer to a login: account name, launch ticket and (with "remember me") the sign-in
    Ticket(Result<(String, String, Option<String>), String>),
    /// "Report a problem" was sent (its id) or not
    Reported(Result<String, String>),
    SignedUp(Result<String, String>),
    Progress(f32),
    /// Install game ended: game data version and client build
    Installed(Result<(u32, String), String>),
    /// a build pack was imported or exported
    Pack(Result<String, String>),
    /// a newer launcher is on offer
    Update(update::Release),
    /// the new launcher is in place: start it
    Updated(Result<std::path::PathBuf, String>),
    /// the transfer screen's workers
    Xfer(XMsg),
    /// the server's /api/info answer
    ServerUp(Result<String, String>),
    /// recording AA Classic: a step of setting it up
    RecSay(String),
    /// recording was set up and started
    RecDone(Result<String, String>),
    /// the one admin prompt: what it came to
    RecAdmin(Result<String, String>),
    /// fights sent to the account's replays
    RecSent(Result<usize, String>),
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Play,
    Transfer,
    Settings,
    Log,
}

enum Action {
    /// Install game: the launcher's own download
    Install,
    Stop,
    ImportPack,
    ExportPack,
    UpdateLauncher,
    CopySettings,
    Play,
    SignUp,
    ChooseFolder,
    SetMode(Mode),
    RemoveClient,
    RemoveAll,
    OpenFolder,
    OpenTransfer,
    Transfer,
    UndoTransfer,
    /// the "Report a problem" dialog opens
    Report,
    SendReport,
    SignOut,
    /// the first start's offer: bring AA Classic's settings, then play
    BringSettings,
    /// the first start's offer: play without them
    SkipOffer,
    /// recording AA Classic: set up (download, start, start with Windows) or turn on again
    RecOn,
    RecOff,
    /// the one admin prompt, again after a No (admin.rs)
    RecAdmin,
}

/// What a sign-in proves the account with, kept only in memory (the password) or sealed on disk (the token).
#[derive(Clone)]
pub enum Secret {
    Password(String),
    Session(String),
}

impl Secret {
    pub fn cred(&self) -> net::Cred<'_> {
        match self {
            Secret::Password(p) => net::Cred::Password(p),
            Secret::Session(s) => net::Cred::Session(s),
        }
    }
}

/// The "Report a problem" dialog.
#[derive(Default)]
struct ReportForm {
    text: String,
    logs: bool,
    sending: bool,
    /// the outcome: Ok(what to tell the player) or Err(what went wrong)
    done: Option<Result<String, String>>,
}

/// The last `max` bytes of a text file, with anything that looks like a ticket or a sign-in (64 hex) blanked.
fn log_tail(path: &std::path::Path, max: usize) -> String {
    let Ok(b) = std::fs::read(path) else { return String::new() };
    let from = b.len().saturating_sub(max);
    let text = String::from_utf8_lossy(&b[from..]).to_string();
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    for c in text.chars().chain(Some('\n')) {
        if c.is_ascii_hexdigit() {
            run.push(c);
            continue;
        }
        if run.len() >= 64 {
            out.push_str("<hidden>");
        } else {
            out.push_str(&run);
        }
        run.clear();
        out.push(c);
    }
    out.pop();
    out
}

struct App {
    paths: Paths,
    cfg: Config,
    password: String,
    invite: String,
    /// the sign-in card shows the sign-up form
    signup: bool,
    /// waiting for the account API
    asking: bool,
    log: Vec<String>,
    busy: bool,
    progress: Option<f32>,
    /// set to stop a running Install game
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    installing: bool,
    /// a newer launcher the manifest offers
    update: Option<update::Release>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    child: Option<(Child, SystemTime)>,
    state: Result<State, String>,
    tab: Tab,
    confirm: Option<(Action, SystemTime)>,
    xfer: Xfer,
    /// what went wrong with the last thing the player pressed: shown in the bottom bar until the next press
    hint: Option<String>,
    /// the server's answer to /api/info: Ok(name) when it is up
    server_up: Option<Result<String, String>>,
    /// the sign-in card shows the server address field
    /// keep the sign-in on this PC (the server's token, never the password)
    remember: bool,
    /// the remembered sign-in for this server, if any
    saved: Option<signin::Saved>,
    /// the invite the installer left (read from the setup file's name): the first PLAY makes the account with it
    baked_invite: Option<String>,
    /// the first start's "bring your AA Classic settings" card is up
    offer: bool,
    /// the game starts when the transfer that is running ends
    play_after_transfer: bool,
    report: Option<ReportForm>,
    /// how the AA Classic folder was found, for Settings
    aac_how: Option<&'static str>,
    /// recording AA Classic, as last read (every two seconds)
    rec: recorder::Readiness,
    rec_read: Option<std::time::Instant>,
    /// setting up: what it is doing
    rec_busy: Option<String>,
    /// the last setup or send outcome: Ok(what to say) or Err(what went wrong)
    rec_note: Option<Result<String, String>>,
    rec_sending: bool,
    rec_sent_at: Option<std::time::Instant>,
}

impl App {
    fn new(cc: &eframe::CreationContext) -> App {
        install_fonts(&cc.egui_ctx);
        install_style(&cc.egui_ctx);
        let paths = Paths::new();
        let mut cfg = Config::load(&paths);
        let mut aac_how = None;
        if cfg.aac_dir.as_ref().is_none_or(|d| !find::is_aac(d)) {
            // the registry first, then the usual folders on every drive (find.rs)
            if let Some(h) = find::aac() {
                cfg.aac_dir = Some(h.dir);
                aac_how = Some(h.how);
                let _ = cfg.save(&paths);
            }
        }
        let (tx, rx) = channel();
        let saved = signin::load(&paths).filter(|s| s.server == cfg.login_ip.trim());
        let baked_invite = signin::pending_invite(&paths);
        let mut a = App {
            paths,
            cfg,
            password: String::new(),
            invite: String::new(),
            // with the invite from the download and no sign-in yet, the first PLAY makes the account
            signup: std::env::var("AAEMU_SANDBOX_SIGNUP").is_ok() || (baked_invite.is_some() && saved.is_none()),
            asking: false,
            log: vec![],
            busy: false,
            progress: None,
            cancel: Default::default(),
            installing: false,
            update: None,
            tx,
            rx,
            child: None,
            state: Err(String::new()),
            // start tab, for screenshots: AAEMU_SANDBOX_TAB=settings|log
            tab: match std::env::var("AAEMU_SANDBOX_TAB").as_deref() {
                Ok("settings") => Tab::Settings,
                Ok("log") => Tab::Log,
                _ => Tab::Play,
            },
            confirm: None,
            xfer: Xfer::default(),
            hint: None,
            server_up: None,
            remember: true,
            saved,
            baked_invite,
            offer: false,
            play_after_transfer: false,
            report: None,
            aac_how,
            rec: Default::default(),
            rec_read: None,
            rec_busy: None,
            rec_note: None,
            rec_sending: false,
            rec_sent_at: None,
        };
        if let Some(s) = &a.saved {
            a.cfg.username = s.username.clone();
        }
        a.check_server();
        a.refresh();
        // the first start offers to bring AA Classic's settings: look at them now, off the window's thread
        if !a.cfg.transfer_offered && transfer_docs_present() {
            a.rescan();
        }
        if std::env::var("AAEMU_SANDBOX_TAB").as_deref() == Ok("transfer") {
            a.open_transfer();
        }
        // ask the signed manifest for a newer launcher, off the window's thread; silent when there is none
        let (url, tx) = (a.cfg.update_manifest().to_string(), a.tx.clone());
        if !url.is_empty() {
            std::thread::spawn(move || {
                if let Ok(Some(r)) = update::check(&url) {
                    let _ = tx.send(Msg::Update(r));
                }
            });
        }
        a
    }

    fn aac(&self) -> Result<AacInstall, String> {
        let d = self.cfg.aac_dir.clone().ok_or("The launcher looked in Windows' list of programs and on every drive. Show it the folder AA Classic is installed in (the one with binNext and game_pak).")?;
        AacInstall::open(&d, &self.paths)
    }

    /// The AA Classic folder, when the mode uses one.
    fn aac_if_needed(&self) -> Result<Option<AacInstall>, String> {
        if self.cfg.mode == Mode::Own { Ok(None) } else { self.aac().map(Some) }
    }

    fn refresh(&mut self) {
        self.state = self.aac_if_needed().map(|a| status(&self.paths, a.as_ref(), self.cfg.mode));
    }

    /// Install game: everything in one go on a worker thread; Stop sets `cancel`.
    fn start_install(&mut self) {
        let _ = self.cfg.save(&self.paths);
        self.busy = true;
        self.installing = true;
        self.progress = None;
        self.cancel.store(false, std::sync::atomic::Ordering::SeqCst);
        let (paths, tx, cancel, server) = (self.paths.clone(), self.tx.clone(), self.cancel.clone(), Server::parse(&self.cfg.login_ip));
        std::thread::spawn(move || {
            // with a server link, the server says which client builds and which game version it takes
            let info = match &server {
                Ok(s @ Server::Api { .. }) => net::info(s).ok(),
                _ => None,
            };
            let plan = install::Plan {
                to: info.as_ref().and_then(|i| i.builds.iter().filter_map(|b| b.parse::<u32>().ok()).max()),
                accepted: info.map(|i| i.build_ids).unwrap_or_default(),
                ..Default::default()
            };
            let (t2, t3) = (std::sync::Mutex::new(tx.clone()), std::sync::Mutex::new(tx.clone()));
            let r = install::run(&paths, &install::Source::default(), &plan, &cancel, &move |l| {
                let _ = t2.lock().unwrap().send(Msg::Line(l));
            }, &move |f| {
                let _ = t3.lock().unwrap().send(Msg::Progress(f));
            });
            let _ = tx.send(Msg::Installed(r.map(|d| (d.version, d.build))));
        });
    }

    fn import_pack(&mut self) {
        let Some(file) = rfd::FileDialog::new().set_title("A build pack from a friend").pick_file() else { return };
        self.busy = true;
        let (paths, tx, server) = (self.paths.clone(), self.tx.clone(), Server::parse(&self.cfg.login_ip));
        std::thread::spawn(move || {
            let accepted = match &server {
                Ok(s @ Server::Api { .. }) => net::info(s).map(|i| i.build_ids).unwrap_or_default(),
                _ => vec![],
            };
            let r = vault::import(&paths, &file, &accepted).map(|b| format!("client build {} is in the vault ({} files)", &b.id[..12], b.files.len()));
            let _ = tx.send(Msg::Pack(r));
        });
    }

    fn export_pack(&mut self) {
        let Some(file) = rfd::FileDialog::new().set_title("Save the build pack as").set_file_name("aac-client.aabuild").save_file() else { return };
        self.busy = true;
        let (paths, tx) = (self.paths.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let r = vault::add(&paths, &paths.bin()).and_then(|b| vault::export(&paths, &b.id, &file)).map(|b| format!("client build {} written to {}", &b.id[..12], file.display()));
            let _ = tx.send(Msg::Pack(r));
        });
    }

    /// Asks the server whether it is up, off the window's thread.
    fn check_server(&mut self) {
        self.server_up = None;
        let tx = self.tx.clone();
        let server = Server::parse(&self.cfg.login_ip);
        std::thread::spawn(move || {
            let r = match server {
                Ok(s @ Server::Api { .. }) => net::info(&s).map(|i| {
                    // the default AAEmu name says nothing to a player
                    if i.name.trim().is_empty() || i.name.starts_with("AAEmu") { "The arena server".to_string() } else { i.name }
                }),
                Ok(_) => Ok("Your own server".to_string()),
                Err(e) => Err(e),
            };
            let _ = tx.send(Msg::ServerUp(r));
        });
    }

    /// A problem with what the player just pressed: logged, and shown in the bottom bar.
    fn warn(&mut self, s: impl Into<String>) {
        let s = s.into();
        self.hint = Some(s.clone());
        self.say(s);
    }

    fn say(&mut self, s: impl Into<String>) {
        let s = s.into();
        log_line(&self.paths, &s);
        self.log.push(s);
    }

    fn start_setup(&mut self) {
        let aac = match self.aac() {
            Ok(a) => a,
            Err(e) => return self.say(e),
        };
        let _ = self.cfg.save(&self.paths);
        // AA Classic on another drive: our copy goes to that drive, so its game data can still be linked
        match place_client(&mut self.paths, &aac, self.cfg.mode) {
            Ok(Some(line)) => self.say(line),
            Ok(None) => {}
            Err(e) => return self.warn(e),
        }
        self.busy = true;
        self.progress = None;
        let (paths, mode, tx) = (self.paths.clone(), self.cfg.mode, self.tx.clone());
        std::thread::spawn(move || {
            let t2 = tx.clone();
            let r = setup(&paths, &aac, mode, &move |l| {
                let _ = t2.send(Msg::Line(l));
            });
            let _ = tx.send(Msg::SetupDone(r));
        });
    }

    /// The account name and what proves it: the remembered sign-in, else the password typed now.
    pub(super) fn secret(&self) -> Option<(String, Secret)> {
        if let Some(s) = &self.saved {
            return Some((s.username.clone(), Secret::Session(s.session.clone())));
        }
        let u = self.cfg.username.trim();
        (!u.is_empty() && !self.password.is_empty()).then(|| (u.to_string(), Secret::Password(self.password.clone())))
    }

    /// The one button: makes the account (first start with an invite), makes or refreshes our copy of the game,
    /// signs in and starts the game, each step going on to the next.
    pub(super) fn press_play(&mut self) {
        if self.signup {
            return self.start_signup();
        }
        if self.secret().is_none() {
            return self.warn("Enter your account name and password on the right.");
        }
        match &self.state {
            Ok(State::Ready) => self.start_play(),
            Ok(_) if self.cfg.mode == Mode::Own => self.warn("Install the game first (Settings)."),
            Ok(_) => self.start_setup(),
            Err(e) => {
                let e = e.clone();
                self.warn(e)
            }
        }
    }

    fn start_play(&mut self) {
        // before signing in: a ticket is only good for two minutes
        let running = crate::core::blocking_processes();
        if !running.is_empty() {
            return self.warn(format!("Close AA Classic first (its game or launcher is still open: {}). Both can't run at once.", running.join(", ")));
        }
        let aac = match self.aac_if_needed() {
            Ok(a) => a,
            Err(e) => return self.warn(e),
        };
        let Some((username, secret)) = self.secret() else {
            return self.warn("Enter your account name and password on the right.");
        };
        let server = match Server::parse(&self.cfg.login_ip) {
            Ok(s) => s,
            Err(e) => return self.warn(e),
        };
        let _ = self.cfg.save(&self.paths);
        if let Server::Api { .. } = server {
            // the password is checked by the server's account API; the client only ever gets a one-time ticket
            self.asking = true;
            self.say("Signing in…");
            let (tx, paths, remember) = (self.tx.clone(), self.paths.clone(), self.remember && self.saved.is_none());
            std::thread::spawn(move || {
                // the server's addons first: a ticket is only good for two minutes
                match addons::ensure(&paths, &server) {
                    Ok(Some(line)) => {
                        let _ = tx.send(Msg::Line(line));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = tx.send(Msg::Ticket(Err(format!("server addons: {e}"))));
                        return;
                    }
                }
                // the AA REPLAY loading screens, splash and icon (never stops a play)
                if let Some(line) = crate::art::ensure(&paths) {
                    let _ = tx.send(Msg::Line(line));
                }
                let _ = tx.send(Msg::Ticket(net::login(&server, &username, secret.cred(), remember).map(|t| (t.username, t.ticket, t.session))));
            });
            return;
        }
        let token = token_from_password(&self.password);
        self.launch(aac, username, token);
    }

    /// Is there an AA Classic profile on this PC to bring settings from? (Only then is the first start's offer made.)
    fn offer_ready(&self) -> bool {
        self.xfer.items().iter().any(|i| i.ready)
    }

    fn sign_out(&mut self) {
        if let Some(s) = self.saved.take() {
            // ends the token on the server too; the file goes either way
            if let Ok(server @ Server::Api { .. }) = Server::parse(&s.server) {
                std::thread::spawn(move || {
                    let _ = net::logout(&server, &s.session);
                });
            }
        }
        signin::forget(&self.paths);
        self.password.clear();
        self.say("Signed out on this PC");
    }

    fn send_report(&mut self) {
        let Some(form) = self.report.as_mut() else { return };
        if form.sending {
            return;
        }
        form.sending = true;
        form.done = None;
        let (text, logs) = (form.text.trim().to_string(), form.logs);
        let launcher_log = if logs { log_tail(&self.paths.log_file(), 200 * 1024) } else { String::new() };
        let client_log = if logs { log_tail(&self.paths.docs().join("ArcheAge.log"), 200 * 1024) } else { String::new() };
        let user = self.secret().map(|(u, _)| u).unwrap_or_else(|| self.cfg.username.trim().to_string());
        let session = self.saved.as_ref().map(|s| s.session.clone());
        let (tx, paths, server) = (self.tx.clone(), self.paths.clone(), Server::parse(&self.cfg.login_ip));
        std::thread::spawn(move || {
            let sent = match &server {
                Ok(s @ Server::Api { .. }) => net::report(s, &user, session.as_deref(), &text, &launcher_log, &client_log),
                _ => Err("this server takes no reports".into()),
            };
            // not sent: the same report as a file the player can hand on
            let r = sent.map(|id| format!("Sent. Thank you! (report {id})")).or_else(|e| {
                let dir = paths.root.join("reports");
                let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let file = dir.join(format!("report-{t}.txt"));
                let body = format!("AA Replay launcher {}\r\naccount: {user}\r\n\r\n{text}\r\n\r\n--- launcher log ---\r\n{launcher_log}\r\n\r\n--- game log ---\r\n{client_log}\r\n", update::VERSION);
                match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, body)) {
                    Ok(()) => {
                        let _ = std::process::Command::new("explorer").arg("/select,").arg(&file).spawn();
                        Err(format!("Could not send it ({e}). It is saved as {}: send that file to whoever invited you.", file.display()))
                    }
                    Err(w) => Err(format!("Could not send it ({e}) or save it ({w})")),
                }
            });
            let _ = tx.send(Msg::Reported(r));
        });
    }

    fn start_signup(&mut self) {
        let server = match Server::parse(&self.cfg.login_ip) {
            Ok(s) => s,
            Err(e) => return self.warn(e),
        };
        if self.cfg.username.trim().is_empty() || self.password.is_empty() {
            return self.warn("Choose an account name and a password.");
        }
        let invite = self.baked_invite.clone().unwrap_or_else(|| self.invite.trim().to_string());
        if invite.is_empty() {
            return self.warn("Enter the invite code you were given.");
        }
        self.asking = true;
        self.say("Creating your account…");
        let (tx, username, password) = (self.tx.clone(), self.cfg.username.trim().to_string(), self.password.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::SignedUp(net::signup(&server, &username, &password, &invite)));
        });
    }

    fn launch(&mut self, aac: Option<AacInstall>, username: String, token: String) {
        let address = match Server::parse(&self.cfg.login_ip).and_then(|s| s.client_address()) {
            Ok(a) => a,
            Err(e) => return self.say(e),
        };
        let l = Launch { username, token, login_ip: address.clone(), game_ip: address, lang: self.cfg.lang.clone(), login_port: self.cfg.login_port };
        match play(&self.paths, aac.as_ref(), self.cfg.mode, &l) {
            Ok(c) => {
                self.say(format!("client started (pid {})", c.id()));
                self.child = Some((c, SystemTime::now()));
            }
            Err(e) => self.warn(e),
        }
    }

    /// Asks for a second click within 4 seconds before a removal.
    fn confirmed(&mut self, a: Action) -> bool {
        let same = matches!(
            (&self.confirm, &a),
            (Some((Action::RemoveClient, _)), Action::RemoveClient) | (Some((Action::RemoveAll, _)), Action::RemoveAll)
        );
        let fresh = self.confirm.as_ref().is_some_and(|(_, t)| t.elapsed().map(|d| d.as_secs() < 4).unwrap_or(false));
        if same && fresh {
            self.confirm = None;
            true
        } else {
            self.confirm = Some((a, SystemTime::now()));
            false
        }
    }

    fn confirming(&self, a: &Action) -> bool {
        let fresh = self.confirm.as_ref().is_some_and(|(_, t)| t.elapsed().map(|d| d.as_secs() < 4).unwrap_or(false));
        fresh
            && matches!(
                (&self.confirm, a),
                (Some((Action::RemoveClient, _)), Action::RemoveClient) | (Some((Action::RemoveAll, _)), Action::RemoveAll)
            )
    }

    fn run(&mut self, a: Action) {
        self.hint = None;
        match a {
            Action::Install => self.start_install(),
            Action::Stop => {
                self.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                self.say("stopping after the part that is being downloaded...");
            }
            Action::UpdateLauncher => {
                if let Some(r) = self.update.clone() {
                    self.busy = true;
                    self.say(format!("downloading launcher {} ...", r.version));
                    let tx = self.tx.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(Msg::Updated(update::apply(&r)));
                    });
                }
            }
            Action::CopySettings => {
                let r = settings::real_file().ok_or("no AA Classic settings file was found".to_string()).and_then(|f| settings::copy_from(&self.paths, &f));
                self.say(match r {
                    Ok(n) => format!("{n} display and sound settings copied from AA Classic (its file was only read)"),
                    Err(e) => format!("settings: {e}"),
                });
            }
            Action::ImportPack => self.import_pack(),
            Action::ExportPack => self.export_pack(),
            Action::Play => self.press_play(),
            Action::SignUp => self.start_signup(),
            Action::Report => {
                self.report = Some(ReportForm { logs: true, ..Default::default() });
            }
            Action::SendReport => self.send_report(),
            Action::SignOut => self.sign_out(),
            Action::BringSettings => {
                self.offer = false;
                self.cfg.transfer_offered = true;
                let _ = self.cfg.save(&self.paths);
                self.retick_all();
                self.play_after_transfer = true;
                self.start_transfer();
                if !self.xfer.working {
                    // nothing could be moved after all: play
                    self.play_after_transfer = false;
                    self.press_play();
                }
            }
            Action::SkipOffer => {
                self.offer = false;
                self.cfg.transfer_offered = true;
                let _ = self.cfg.save(&self.paths);
                self.press_play();
            }
            Action::ChooseFolder => {
                if let Some(d) = rfd::FileDialog::new().set_title("Your AA Classic folder (the one with binNext and game_pak)").pick_folder() {
                    self.cfg.aac_dir = Some(d);
                    let _ = self.cfg.save(&self.paths);
                    self.refresh();
                }
            }
            Action::SetMode(m) => {
                self.cfg.mode = m;
                let _ = self.cfg.save(&self.paths);
                self.refresh();
            }
            Action::RemoveClient => {
                if self.confirmed(Action::RemoveClient) {
                    let r = remove_client(&self.paths);
                    self.say(match r {
                        Ok(()) => "client copy removed (AA Classic untouched)".to_string(),
                        Err(e) => format!("remove failed: {e}"),
                    });
                    self.refresh();
                }
            }
            Action::RemoveAll => {
                if self.confirmed(Action::RemoveAll) {
                    let r = remove_client(&self.paths).and_then(|_| remove_profile(&self.paths));
                    self.say(match r {
                        Ok(()) => "client copy and sandbox profile removed (AA Classic untouched)".to_string(),
                        Err(e) => format!("remove failed: {e}"),
                    });
                    self.refresh();
                }
            }
            Action::OpenFolder => {
                let _ = std::process::Command::new("explorer").arg(&self.paths.root).spawn();
            }
            Action::OpenTransfer => {
                if self.offer {
                    self.offer = false;
                    self.cfg.transfer_offered = true;
                    let _ = self.cfg.save(&self.paths);
                }
                self.open_transfer()
            }
            Action::Transfer => self.start_transfer(),
            Action::UndoTransfer => self.start_undo(),
            Action::RecOn => {
                self.rec_busy = Some("Getting the recorder…".into());
                self.rec_note = None;
                let (paths, tx) = (self.paths.clone(), self.tx.clone());
                std::thread::spawn(move || {
                    let say = |l: String| {
                        let _ = tx.send(Msg::RecSay(l));
                    };
                    let r = recorder::turn_on(&paths, &say).map(|()| "Recording is on, and starts with Windows from now on.".to_string());
                    let ok = r.is_ok();
                    // the one admin prompt, right after the first setup (never again after a No: the card's button)
                    let admin = if ok { crate::admin::after_setup(&paths, false, &say) } else { None };
                    let _ = tx.send(Msg::RecDone(r));
                    if let Some(a) = admin {
                        let _ = tx.send(Msg::RecAdmin(a));
                    }
                });
            }
            Action::RecAdmin => {
                self.rec_busy = Some(crate::admin::WHY.into());
                self.rec_note = None;
                let (paths, tx) = (self.paths.clone(), self.tx.clone());
                std::thread::spawn(move || {
                    let say = |l: String| {
                        let _ = tx.send(Msg::RecSay(l));
                    };
                    let a = crate::admin::after_setup(&paths, true, &say).unwrap_or_else(|| Ok("Nothing to set up: Windows does not ask when AA Classic starts.".into()));
                    let _ = tx.send(Msg::RecAdmin(a));
                });
            }
            Action::RecOff => {
                self.rec_note = Some(recorder::turn_off(&self.paths).map(|()| "Recording is off. What it recorded is kept.".to_string()));
                self.rec_read = None;
            }
        }
    }

    /// Reads the recorder's status every two seconds, and sends new fights when there is a sign-in.
    fn poll_recorder(&mut self, ctx: &egui::Context) {
        if self.rec_read.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
            self.rec = recorder::readiness(&self.paths);
            self.rec_read = Some(std::time::Instant::now());
        }
        if self.rec.set_up || self.rec_busy.is_some() {
            ctx.request_repaint_after(Duration::from_secs(2));
        }
        let due = self.rec_sent_at.is_none_or(|t| t.elapsed() > Duration::from_secs(60));
        if self.rec.waiting > 0 && !self.rec_sending && due && matches!(Server::parse(&self.cfg.login_ip), Ok(Server::Api { .. })) {
            if let Some((user, secret)) = self.secret() {
                self.rec_sending = true;
                self.rec_sent_at = Some(std::time::Instant::now());
                let (paths, tx, server) = (self.paths.clone(), self.tx.clone(), Server::parse(&self.cfg.login_ip));
                std::thread::spawn(move || {
                    let r = server.and_then(|s| recorder::send(&paths, &s, &user, secret.cred()));
                    let _ = tx.send(Msg::RecSent(r));
                });
            }
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        self.poll_recorder(ctx);
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Line(l) => {
                    if let Some(v) = l.trim().strip_suffix('%').and_then(|n| n.trim().parse::<f32>().ok()) {
                        self.progress = Some(v / 100.0);
                    }
                    self.say(l)
                }
                Msg::Ticket(r) => {
                    self.asking = false;
                    match (r, self.aac_if_needed()) {
                        (Ok((name, ticket, session)), Ok(aac)) => {
                            self.cfg.username = name.clone();
                            let _ = self.cfg.save(&self.paths);
                            if let Some(session) = session {
                                let s = signin::Saved { server: self.cfg.login_ip.trim().to_string(), username: name.clone(), session };
                                match signin::save(&self.paths, &s) {
                                    Ok(()) => {
                                        self.say(format!("Signed in as {name}: remembered on this PC (the password is not kept)"));
                                        self.saved = Some(s);
                                        self.password.clear();
                                    }
                                    Err(e) => self.say(e),
                                }
                            }
                            // the first start: offer AA Classic's settings before the game starts (the ticket is
                            // left to run out; the next press asks for a fresh one)
                            if !self.cfg.transfer_offered && self.offer_ready() && self.secret().is_some() {
                                self.offer = true;
                                self.tab = Tab::Play;
                            } else {
                                self.launch(aac, name, ticket);
                            }
                        }
                        (Err(e), _) | (_, Err(e)) => {
                            if e == net::explain("BadSession") {
                                // the remembered sign-in ran out or was ended: the password once more
                                self.saved = None;
                                signin::forget(&self.paths);
                            }
                            self.warn(format!("Sign-in failed: {e}"))
                        }
                    }
                }
                Msg::Reported(r) => {
                    if let Some(f) = self.report.as_mut() {
                        f.sending = false;
                        f.done = Some(r.clone());
                    }
                    self.say(match r {
                        Ok(l) | Err(l) => format!("report: {l}"),
                    });
                }
                Msg::SignedUp(r) => {
                    self.asking = false;
                    match r {
                        Ok(name) => {
                            self.cfg.username = name.clone();
                            let _ = self.cfg.save(&self.paths);
                            self.signup = false;
                            self.invite.clear();
                            self.baked_invite = None;
                            signin::clear_invite(&self.paths);
                            self.say(format!("Account {name} created"));
                            // one press: the account, then the game
                            self.press_play();
                        }
                        // the invite of a second install was used up by the first: the account is there, sign in
                        Err(e) if self.baked_invite.is_some() && (e == net::explain("BadInvite") || e == net::explain("UsernameTaken")) => {
                            self.baked_invite = None;
                            signin::clear_invite(&self.paths);
                            self.signup = false;
                            self.say(format!("{e}: signing in to the account instead"));
                            self.press_play();
                        }
                        Err(e) => self.warn(format!("Could not create the account: {e}")),
                    }
                }
                Msg::Update(r) => {
                    self.say(format!("launcher {} is available (this is {})", r.version, update::VERSION));
                    self.update = Some(r);
                }
                Msg::Updated(r) => {
                    self.busy = false;
                    match r {
                        Ok(exe) => {
                            // the new launcher takes over
                            self.say("launcher updated: restarting");
                            if std::process::Command::new(&exe).spawn().is_ok() {
                                ctx.send_viewport_cmd(ViewportCommand::Close);
                            }
                        }
                        Err(e) => self.say(format!("launcher update: {e}")),
                    }
                }
                Msg::Progress(f) => self.progress = Some(f),
                Msg::Xfer(m) => self.transfer_msg(m),
                Msg::ServerUp(r) => self.server_up = Some(r),
                Msg::RecSay(l) => {
                    self.say(format!("recorder: {l}"));
                    self.rec_busy = Some(l);
                }
                Msg::RecDone(r) => {
                    self.rec_busy = None;
                    self.rec_read = None;
                    match &r {
                        Ok(l) => self.say(format!("recorder: {l}")),
                        Err(e) => self.say(format!("recorder: {e}")),
                    }
                    self.rec_note = Some(r.map_err(|e| format!("Recording could not be set up: {e}")));
                }
                Msg::RecAdmin(r) => {
                    self.rec_busy = None;
                    self.rec_read = None;
                    match &r {
                        Ok(l) | Err(l) => self.say(format!("recorder: {l}")),
                    }
                    // a No is not an error: recording works, Windows asks at each AA Classic start
                    self.rec_note = Some(r);
                }
                Msg::RecSent(r) => {
                    self.rec_sending = false;
                    self.rec_read = None;
                    match r {
                        Ok(0) => {}
                        Ok(n) => {
                            let l = format!("{n} new fight{} sent to your replays: watch {} in the arena's replay player.", if n == 1 { "" } else { "s" }, if n == 1 { "it" } else { "them" });
                            self.say(format!("recorder: {l}"));
                            self.rec_note = Some(Ok(l));
                        }
                        Err(e) => {
                            self.say(format!("recorder: sending fights: {e}"));
                            self.rec_note = Some(Err(format!("Your fights could not be sent yet: {e}. They are tried again in a minute.")));
                        }
                    }
                }
                Msg::Installed(r) => {
                    self.busy = false;
                    self.installing = false;
                    self.progress = None;
                    match r {
                        Ok((version, build)) => {
                            self.cfg.mode = Mode::Own;
                            let _ = self.cfg.save(&self.paths);
                            self.say(format!("installed: game data version {version}, client build {}", &build[..12.min(build.len())]));
                        }
                        Err(e) => self.warn(format!("Install stopped: {e}")),
                    }
                    self.refresh();
                }
                Msg::Pack(r) => {
                    self.busy = false;
                    self.say(match r {
                        Ok(l) => l,
                        Err(e) => format!("build pack: {e}"),
                    });
                    self.refresh();
                }
                Msg::SetupDone(r) => {
                    self.busy = false;
                    self.progress = None;
                    match r {
                        Ok(()) => {
                            self.say("Your copy of the game is ready");
                            // one press: with an account filled in, Install goes on to Play
                            if !self.signup && self.secret().is_some() {
                                self.refresh();
                                if matches!(self.state, Ok(State::Ready)) {
                                    self.start_play();
                                }
                            }
                        }
                        Err(e) => self.warn(format!("Install failed: {e}")),
                    }
                    self.refresh();
                }
            }
        }
        if let Some((c, since)) = &mut self.child {
            if let Ok(Some(st)) = c.try_wait() {
                let since = *since;
                self.child = None;
                self.say(format!("client closed ({st})"));
                for f in crash_files(&self.paths, since) {
                    self.say(format!("crash file kept in the sandbox (nothing was sent): {}", f.display()));
                }
            }
        }
        let reporting = self.report.as_ref().is_some_and(|r| r.sending);
        if self.busy || self.asking || reporting || self.child.is_some() || self.confirm.is_some() || self.xfer.working || self.xfer.scanning || self.xfer.fetching.is_some() {
            ctx.request_repaint_after(Duration::from_millis(400));
        }
    }

    // --- screen parts -------------------------------------------------------------------------------------------

    fn top_bar(&mut self, ui: &mut egui::Ui, rect: Rect, act: &mut Option<Action>) {
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, rgba(4, 6, 10, 190));
        p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, gold_a(60)));
        let drag = ui.interact(rect, ui.scope_id().with("titlebar"), Sense::click_and_drag());
        if drag.drag_started_by(egui::PointerButton::Primary) {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }

        // emblem: a gold diamond with a smaller one inside
        let e = pos2(rect.left() + 34.0, rect.center().y);
        let dia = |r: f32| vec![e + vec2(0.0, -r), e + vec2(r, 0.0), e + vec2(0.0, r), e + vec2(-r, 0.0)];
        p.add(Shape::convex_polygon(dia(14.0), gold_a(30), Stroke::new(1.5, GOLD)));
        p.add(Shape::convex_polygon(dia(6.0), VIOLET, Stroke::NONE));
        let aa = p.text(pos2(rect.left() + 58.0, rect.center().y - 1.0), Align2::LEFT_CENTER, "AA", title_font(22.0), INK);
        let sb = p.text(pos2(aa.right() + 6.0, rect.center().y - 1.0), Align2::LEFT_CENTER, "REPLAY", title_font(22.0), GOLD);

        let tabs = Rect::from_min_max(pos2(sb.right() + 40.0, rect.top()), rect.max);
        ui.scope_builder(UiBuilder::new().max_rect(tabs).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            // one screen: everything else is behind the gear, and the gear (or Back) brings you back
            if self.tab != Tab::Play && nav_tab(ui, "‹  Back", false).clicked() {
                self.tab = Tab::Play;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if win_button(ui, WinBtn::Close).clicked() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                }
                if win_button(ui, WinBtn::Minimize).clicked() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
                }
                let open = self.tab != Tab::Play;
                if gear_button(ui, open).on_hover_text(if open { "Back to Play" } else { "Settings" }).clicked() {
                    self.tab = if open { Tab::Play } else { Tab::Settings };
                }
                ui.add_space(8.0);
                if ui.add(egui::Button::new(RichText::new("Report a problem").size(13.0).color(INK))).on_hover_text("Tell us what went wrong. The launcher's and the game's logs go with it (no password is in them).").clicked() {
                    *act = Some(Action::Report);
                }
                if let Some(r) = &self.update {
                    ui.add_space(10.0);
                    let free = !self.busy && self.child.is_none();
                    if ui.add_enabled(free, egui::Button::new(RichText::new(format!("Update launcher to {}", r.version)).color(GOLD))).clicked() {
                        *act = Some(Action::UpdateLauncher);
                    }
                }
            });
        });
        let _ = act;
    }

    fn play_tab(&mut self, ui: &mut egui::Ui, body: Rect, act: &mut Option<Action>) {
        // left: the pitch
        let hero = Rect::from_min_size(body.min + vec2(56.0, 64.0), vec2(610.0, body.height() - 80.0));
        ui.scope_builder(UiBuilder::new().max_rect(hero).layout(Layout::top_down(Align::Min)), |ui| {
            ui.label(RichText::new("ARENA  ·  REPLAYS  ·  PRACTICE").font(bold_font(12.5)).color(GOLD).extra_letter_spacing(3.0));
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                ui.label(RichText::new("AA").font(title_font(56.0)).color(INK));
                ui.label(RichText::new("Replay").font(title_font(56.0)).color(GOLD));
            });
            ui.add_space(2.0);
            ui.label(RichText::new("Fight in the arena with your friends, then watch every fight back and get better.").size(16.0).color(MUTED));
            ui.add_space(14.0);
            tick_row(ui, "Any build, in seconds", "Wear a DaruGear build or a class setup. Dying costs nothing.");
            tick_row(ui, "Every fight recorded", "Watch it back in the game: pause, slow it down, see every hit.");
            tick_row(ui, "Kept apart from AA Classic", "Own copy, own folder, own account. AA Classic is never changed.");
        });
        self.recording_card(ui, body, act);
        if self.offer {
            return self.offer_card(ui, body, act);
        }

        // right: sign in
        let w = 360.0;
        let tall = if self.saved.is_some() {
            250.0
        } else {
            if self.signup && self.baked_invite.is_none() { 482.0 } else { 416.0 }
        };
        let cardr = Rect::from_min_size(pos2(body.right() - w - 48.0, body.top() + 36.0), vec2(w, tall));
        card(ui.painter(), cardr);
        let mut enter = false;
        ui.scope_builder(UiBuilder::new().max_rect(cardr.shrink2(vec2(28.0, 24.0))).layout(Layout::top_down(Align::Min)), |ui| {
            let (title, sub) = if self.saved.is_some() {
                ("Welcome back", "You stay signed in on this PC. Press Play.")
            } else if self.signup && self.baked_invite.is_some() {
                ("Create your account", "Your invite is already in. Pick a name and a password for this test.")
            } else if self.signup {
                ("Create account", "Pick a name and a password, and enter the invite code you were given.")
            } else {
                ("Sign in", "Your AA Replay account, not your AA Classic one.")
            };
            ui.label(RichText::new(title).font(title_font(26.0)).color(INK));
            ui.label(RichText::new(sub).size(13.0).color(MUTED));
            ui.add_space(8.0);
            // the server: a status line only (always the arena box; core::server_address)
            ui.horizontal(|ui| {
                let (dot, text) = match &self.server_up {
                    None => (MUTED, "Checking the arena server…".to_string()),
                    Some(Ok(name)) => (GREEN, format!("{name} is online")),
                    Some(Err(_)) => (RED, "The arena server does not answer".to_string()),
                };
                let (r, _) = ui.allocate_exact_size(vec2(10.0, 18.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 4.0, dot);
                let l = ui.label(RichText::new(text).size(13.0).color(INK));
                if let Some(Err(e)) = &self.server_up {
                    l.on_hover_text(e.as_str());
                }
            });
            ui.add_space(6.0);
            if let Some(s) = &self.saved {
                ui.add_space(6.0);
                ui.label(RichText::new("SIGNED IN AS").size(11.5).color(MUTED).extra_letter_spacing(1.2));
                ui.label(RichText::new(s.username.as_str()).font(bold_font(20.0)).color(INK));
                ui.add_space(8.0);
                if ui.add(egui::Label::new(RichText::new("Not you? Sign out").size(13.0).color(BLUE).underline()).sense(Sense::click())).clicked() {
                    *act = Some(Action::SignOut);
                }
                return;
            }
            field(ui, "Name", &mut self.cfg.username, false, "account name");
            let pw = field(ui, "Password", &mut self.password, true, "password for this server");
            if pw.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                enter = true;
            }
            ui.checkbox(&mut self.remember, RichText::new("Remember me on this PC").size(13.5).color(INK))
                .on_hover_text("Keeps a sign-in from the server, sealed for your Windows user. Your password is never stored.");
            if self.signup && self.baked_invite.is_none() {
                let inv = field(ui, "Invite code", &mut self.invite, false, "from the server's owner");
                if inv.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    enter = true;
                }
            }
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                ui.painter().circle_stroke(r.center(), 6.0, Stroke::new(1.2, AMBER));
                ui.painter().text(r.center(), Align2::CENTER_CENTER, "!", bold_font(10.0), AMBER);
                ui.label(RichText::new("Never type your AA Classic password here.").size(12.5).color(AMBER));
            });
            ui.add_space(4.0);
            // accounts are made on a server with an account API (a link), not on a bare address
            if matches!(Server::parse(&self.cfg.login_ip), Ok(Server::Api { .. })) {
                let label = if self.signup { "I already have an account" } else { "Have an invite code? Create an account" };
                if ui.add(egui::Label::new(RichText::new(label).size(13.0).color(BLUE).underline()).sense(Sense::click())).clicked() {
                    self.signup = !self.signup;
                }
            } else {
                self.signup = false;
            }
        });
        if enter && !self.busy && !self.asking && self.child.is_none() {
            *act = Some(Action::Play);
        }
    }

    /// Recording AA Classic: one line of what it is doing, then what is wrong with its fix. Never a silent half-state.
    fn recording_card(&mut self, ui: &mut egui::Ui, body: Rect, act: &mut Option<Action>) {
        let r = Rect::from_min_size(pos2(body.left() + 48.0, body.bottom() - 148.0), vec2(610.0, 140.0));
        card(ui.painter(), r);
        let rec = self.rec.clone();
        let st = &rec.status;
        let (dot, head) = if self.rec_busy.is_some() {
            (AMBER, "Setting up recording")
        } else if !rec.set_up {
            (MUTED, "Record your AA Classic fights")
        } else {
            match st.state.as_str() {
                "recording" => (RED, "Recording"),
                "ready" => (GREEN, "Ready to record"),
                "starting" => (AMBER, "Starting"),
                "problem" => (AMBER, "Not recording"),
                _ => (MUTED, "Recording is off"),
            }
        };
        let mut lines: Vec<(String, Color32)> = Vec::new();
        if let Some(b) = &self.rec_busy {
            lines.push((b.clone(), INK));
            lines.push(("The recorder is about 47 MB. Nothing is installed outside the launcher's folder.".into(), MUTED));
        } else if !rec.set_up {
            lines.push(("Play AA Classic as usual: your fights become replays you watch here, hit by hit.".into(), INK));
            lines.push(("Only your own fights are kept, and other players' names are taken out on this PC.".into(), MUTED));
            lines.push((crate::admin::WHY.into(), MUTED));
        } else {
            let problem = st.problem.is_some();
            for (i, w) in recorder::summary(&rec).into_iter().enumerate() {
                // with a problem the heading says "Not recording": the problem and its fix come first
                if i == 0 && problem {
                    continue;
                }
                let c = if i == 1 && problem { AMBER } else if i == 0 { INK } else { MUTED };
                lines.push((w, c));
            }
        }
        // three lines fit: what it is doing or what is wrong, and the last thing pressed or sent
        if let Some(n) = &self.rec_note {
            let at = lines.len().min(1);
            match n {
                Ok(t) => lines.insert(at, (t.clone(), GREEN)),
                Err(t) => lines.insert(at, (t.clone(), RED)),
            }
        }
        ui.scope_builder(UiBuilder::new().max_rect(r.shrink2(vec2(24.0, 16.0))).layout(Layout::top_down(Align::Min)), |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.horizontal(|ui| {
                let (d, _) = ui.allocate_exact_size(vec2(12.0, 22.0), Sense::hover());
                ui.painter().circle_filled(d.center(), 5.0, dot);
                ui.label(RichText::new(head).font(bold_font(16.0)).color(INK));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.rec_busy.is_some() {
                        ui.add(egui::Spinner::new());
                    } else if !rec.set_up {
                        if ui.add(egui::Button::new(RichText::new("Set up recording").font(bold_font(14.0)).color(DARK_TEXT)).fill(GOLD))
                            .on_hover_text("Downloads the recorder, starts it, and starts it with Windows. Windows then asks once for admin rights, because AA Classic runs as admin: answer Yes, and it never asks again.")
                            .clicked()
                        {
                            *act = Some(Action::RecOn);
                        }
                    } else if st.running() {
                        if matches!(rec.admin, crate::admin::State::NotSetUp | crate::admin::State::Declined)
                            && ui.button("Set up admin once").on_hover_text(crate::admin::WHY).clicked()
                        {
                            *act = Some(Action::RecAdmin);
                        }
                        if ui.button("Turn off").on_hover_text("Stops recording and no longer starts it with Windows. What was recorded is kept.").clicked() {
                            *act = Some(Action::RecOff);
                        }
                    } else if ui.add(egui::Button::new(RichText::new("Turn on").font(bold_font(14.0)).color(DARK_TEXT)).fill(GOLD)).clicked() {
                        *act = Some(Action::RecOn);
                    }
                    if rec.set_up && ui.add(egui::Label::new(RichText::new("Recordings folder").size(12.5).color(BLUE).underline()).sense(Sense::click())).clicked() {
                        let _ = std::process::Command::new("explorer").arg(recorder::data_dir(&self.paths)).spawn();
                    }
                });
            });
            ui.add_space(2.0);
            for (t, c) in lines.iter().take(3) {
                ui.add(egui::Label::new(RichText::new(t.as_str()).size(13.0).color(*c)).wrap());
            }
        });
    }

    /// The first start's one-time offer: AA Classic's settings into this client, then the game.
    fn offer_card(&mut self, ui: &mut egui::Ui, body: Rect, act: &mut Option<Action>) {
        let w = 400.0;
        let ready: Vec<transfer::Item> = self.xfer.items().into_iter().filter(|i| i.ready).collect();
        let cardr = Rect::from_min_size(pos2(body.right() - w - 48.0, body.top() + 36.0), vec2(w, 200.0 + ready.len() as f32 * 48.0));
        card(ui.painter(), cardr);
        ui.scope_builder(UiBuilder::new().max_rect(cardr.shrink2(vec2(28.0, 24.0))).layout(Layout::top_down(Align::Min)), |ui| {
            ui.label(RichText::new("Bring your AA Classic settings?").font(title_font(24.0)).color(INK));
            ui.label(RichText::new("Once, before your first game. AA Classic's files are only read, and Settings can undo it.").size(13.0).color(MUTED));
            ui.add_space(8.0);
            for it in &ready {
                // a painted tick (the fonts have no check mark), the title, and its summary on one cut-off line
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                    let (p, c) = (ui.painter(), rect.center());
                    p.line_segment([c + vec2(-5.0, 0.0), c + vec2(-1.5, 3.5)], Stroke::new(1.8, GREEN));
                    p.line_segment([c + vec2(-1.5, 3.5), c + vec2(5.5, -4.0)], Stroke::new(1.8, GREEN));
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(RichText::new(it.title).font(bold_font(14.5)).color(INK));
                        ui.add(egui::Label::new(RichText::new(&it.summary).size(12.5).color(MUTED)).truncate());
                    });
                });
                ui.add_space(4.0);
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new("Bring them and play").font(bold_font(15.0)).color(DARK_TEXT)).fill(GOLD)).clicked() {
                    *act = Some(Action::BringSettings);
                }
                if ui.button("Not now, just play").clicked() {
                    *act = Some(Action::SkipOffer);
                }
            });
            ui.add_space(4.0);
            if ui.add(egui::Label::new(RichText::new("Choose what to bring…").size(12.5).color(BLUE).underline()).sense(Sense::click())).clicked() {
                *act = Some(Action::OpenTransfer);
            }
        });
    }

    /// The "Report a problem" dialog, over everything else.
    fn report_dialog(&mut self, ctx: &egui::Context, act: &mut Option<Action>) {
        let Some(form) = self.report.as_mut() else { return };
        let mut close = false;
        egui::Area::new(egui::Id::NULL.with("report")).order(egui::Order::Foreground).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            let r = Rect::from_center_size(ctx.content_rect().center(), vec2(520.0, 360.0));
            ui.painter().rect_filled(ctx.content_rect(), 0.0, rgba(0, 0, 0, 150));
            // the dimmed backdrop takes the clicks, so nothing behind the dialog can be pressed
            let _ = ui.interact(ctx.content_rect(), ui.scope_id().with("backdrop"), Sense::click_and_drag());
            card(ui.painter(), r);
            ui.scope_builder(UiBuilder::new().max_rect(r.shrink2(vec2(28.0, 22.0))).layout(Layout::top_down(Align::Min)), |ui| {
                ui.label(RichText::new("Report a problem").font(title_font(24.0)).color(INK));
                let ask = ui.label(RichText::new("What happened, and what were you doing? It goes to the people running the test.").size(13.0).color(MUTED));
                ui.add_space(6.0);
                ui.add(TextEdit::multiline(&mut form.text).desired_rows(6).desired_width(f32::INFINITY).hint_text("e.g. the game closed when I entered the arena"))
                    .labelled_by(ask.id);
                ui.checkbox(&mut form.logs, RichText::new("Include the launcher's log and the game's log").size(13.0).color(INK))
                    .on_hover_text("launcher.log and ArcheAge.log from this launcher's own folder. Sign-in tokens in them are blanked; no password is ever in them.");
                ui.add_space(6.0);
                match &form.done {
                    Some(Ok(t)) => {
                        ui.label(RichText::new(t.as_str()).color(GREEN));
                    }
                    Some(Err(e)) => {
                        ui.add(egui::Label::new(RichText::new(e.as_str()).size(12.5).color(AMBER)).wrap());
                    }
                    None if form.sending => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Sending…").color(MUTED));
                        });
                    }
                    None => {}
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let sent = matches!(form.done, Some(Ok(_)));
                    let can = !form.sending && !sent && (!form.text.trim().is_empty() || form.logs);
                    if ui.add_enabled(can, egui::Button::new(RichText::new("Send").font(bold_font(15.0)).color(DARK_TEXT)).fill(GOLD)).clicked() {
                        *act = Some(Action::SendReport);
                    }
                    if ui.add_enabled(!form.sending, egui::Button::new(if sent { "Close" } else { "Cancel" })).clicked() {
                        close = true;
                    }
                });
            });
        });
        if close {
            self.report = None;
        }
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui, body: Rect, act: &mut Option<Action>) {
        let cardr = Rect::from_center_size(body.center() + vec2(0.0, 4.0), vec2(760.0, body.height() - 56.0));
        card(ui.painter(), cardr);
        let inner = cardr.shrink2(vec2(32.0, 26.0));
        let locked = self.busy || self.child.is_some();
        ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Settings").font(title_font(26.0)).color(INK));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(format!("Launcher {}", update::VERSION)).size(12.0).color(MUTED));
                });
            });
            ui.add_space(4.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(inner.width() - 12.0);

            ui.label(RichText::new("ACCOUNT").size(11.5).color(MUTED).extra_letter_spacing(1.2));
            ui.horizontal(|ui| {
                match &self.saved {
                    Some(s) => {
                        ui.label(RichText::new(format!("Signed in as {} (remembered on this PC; the password is not kept)", s.username)).color(INK));
                        if ui.button("Sign out").clicked() {
                            *act = Some(Action::SignOut);
                        }
                    }
                    None => {
                        ui.label(RichText::new("Not remembered on this PC: Play asks for the password.").color(INK));
                    }
                }
            });
            ui.add_space(6.0);

            ui.label(RichText::new("FROM AA CLASSIC").size(11.5).color(MUTED).extra_letter_spacing(1.2));
            ui.horizontal(|ui| {
                if ui.add_enabled(!locked, egui::Button::new("Bring my AA Classic settings…")).on_hover_text("Display, interface, keybinds, bars and gear: tick what comes over. Undo is on the same screen.").clicked() {
                    *act = Some(Action::OpenTransfer);
                }
                ui.label(RichText::new("Settings, keybinds, interface, bars and gear. AA Classic's files are only read.").size(12.5).color(MUTED));
            });
            ui.add_space(6.0);

            ui.label(RichText::new("AA CLASSIC FOLDER").size(11.5).color(MUTED).extra_letter_spacing(1.2));
            ui.horizontal(|ui| {
                let shown = self.cfg.aac_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(not chosen)".into());
                egui::Frame::new().fill(Color32::from_rgb(12, 16, 23)).stroke(Stroke::new(1.0, Color32::from_rgb(54, 62, 78))).corner_radius(3).inner_margin(Margin::symmetric(10, 7)).show(ui, |ui| {
                    ui.set_width(inner.width() - 150.0);
                    ui.label(RichText::new(shown).monospace().color(INK));
                });
                if ui.add_enabled(!locked, egui::Button::new("Choose...")).clicked() {
                    *act = Some(Action::ChooseFolder);
                }
            });
            if let Some(d) = &self.cfg.aac_dir {
                if let Some(how) = self.aac_how {
                    ui.label(RichText::new(format!("Found through {how}.")).size(12.5).color(MUTED));
                }
                if self.cfg.mode == Mode::Standard {
                    let note = find::place_note(&self.paths.root, &self.paths.client(), d);
                    ui.add(egui::Label::new(RichText::new(note).size(12.5).color(INK)).wrap());
                }
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Only binNext and game_pak are read. Nothing in this folder is ever written.").size(12.5).color(MUTED));
                let hint = "Takes your display, quality and sound settings from AA Classic's system.cfg into this client's own. That file is only read. No server address, language or account data is taken.";
                if ui.add_enabled(!locked && settings::real_file().is_some(), egui::Button::new("Copy my graphics settings")).on_hover_text(hint).clicked() {
                    *act = Some(Action::CopySettings);
                }
            });
            ui.add_space(4.0);

            ui.label(RichText::new("INSTALL MODE").size(11.5).color(MUTED).extra_letter_spacing(1.2));
            let w = (inner.width() - 24.0) / 3.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if mode_card(ui, w, self.cfg.mode == Mode::Standard, !locked, "Standard", "~65 MB", "Copies the program files, shares AA Classic's game data. Same drive only.") {
                    *act = Some(Action::SetMode(Mode::Standard));
                }
                if mode_card(ui, w, self.cfg.mode == Mode::Full, !locked, "Full separation", "~45 GB", "Copies everything from your AA Classic folder. Nothing is shared.") {
                    *act = Some(Action::SetMode(Mode::Full));
                }
                if mode_card(ui, w, self.cfg.mode == Mode::Own, !locked, "Download", "~46 GB", "No AA Classic needed. Program files come from a friend's build pack.") {
                    *act = Some(Action::SetMode(Mode::Own));
                }
            });
            ui.add_space(6.0);

            ui.label(RichText::new("CLIENT BUILD").size(11.5).color(MUTED).extra_letter_spacing(1.2));
            ui.horizontal(|ui| {
                let n = vault::builds(&self.paths).len();
                ui.label(RichText::new(match n {
                    0 => "none yet".to_string(),
                    1 => "1 build kept".to_string(),
                    n => format!("{n} builds kept"),
                }).color(INK));
                if ui.add_enabled(!locked, egui::Button::new("Import build pack...")).on_hover_text("A pack is a program: only import one from someone you trust.").clicked() {
                    *act = Some(Action::ImportPack);
                }
                if ui.add_enabled(!locked && self.paths.exe().is_file(), egui::Button::new("Export build pack...")).clicked() {
                    *act = Some(Action::ExportPack);
                }
                ui.label(RichText::new("Only import a pack from someone you trust.").size(12.5).color(MUTED));
            });
            ui.add_space(10.0);

            ui.label(RichText::new("THIS LAUNCHER").size(11.5).color(MUTED).extra_letter_spacing(1.2));
            ui.horizontal(|ui| {
                if ui.button("Open its folder").on_hover_text(self.paths.root.display().to_string()).clicked() {
                    *act = Some(Action::OpenFolder);
                }
                if ui.button("Show log").clicked() {
                    self.tab = Tab::Log;
                }
                if ui.button("Copy log").on_hover_text("Copies what the launcher did, to send to whoever helps you. It holds no password.").clicked() {
                    ui.ctx().copy_text(self.log.join("
"));
                }
                let l1 = if self.confirming(&Action::RemoveClient) { "Click again to remove" } else { "Remove client copy" };
                if ui.add_enabled(!locked, egui::Button::new(RichText::new(l1).color(RED))).on_hover_text("Deletes this launcher's copy of the game. AA Classic is not touched.").clicked() {
                    *act = Some(Action::RemoveClient);
                }
                let l2 = if self.confirming(&Action::RemoveAll) { "Click again to remove" } else { "Remove everything" };
                if ui.add_enabled(!locked, egui::Button::new(RichText::new(l2).color(RED))).on_hover_text("Deletes the copy and this launcher's settings, addons and logs. AA Classic is not touched.").clicked() {
                    *act = Some(Action::RemoveAll);
                }
            });
            });
        });
    }

    fn log_tab(&mut self, ui: &mut egui::Ui, body: Rect) {
        let cardr = Rect::from_center_size(body.center() + vec2(0.0, 4.0), vec2(body.width() - 96.0, body.height() - 56.0));
        card(ui.painter(), cardr);
        let inner = cardr.shrink2(vec2(28.0, 22.0));
        ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Log").font(title_font(26.0)).color(INK));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Back").clicked() {
                        self.tab = Tab::Settings;
                    }
                    if ui.button("Copy").clicked() {
                        ui.ctx().copy_text(self.log.join("\n"));
                    }
                });
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                if self.log.is_empty() {
                    ui.label(RichText::new("Nothing yet.").color(MUTED));
                }
                for l in &self.log {
                    ui.label(RichText::new(l).monospace().color(INK));
                }
            });
        });
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui, rect: Rect, act: &mut Option<Action>) {
        let p = ui.painter().clone();
        vgrad(&p, rect, rgba(6, 8, 13, 225), rgba(3, 4, 7, 245));
        p.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, gold_a(80)));

        let mode = match self.cfg.mode {
            Mode::Standard => "Standard install",
            Mode::Full => "Full separation",
            Mode::Own => "Download",
        };
        let (dot, title, sub, button, action): (Color32, String, String, &str, Option<Action>) = if self.asking {
            (GOLD, "Asking the server".into(), self.log.last().cloned().unwrap_or_default(), "WAIT", None)
        } else if self.offer && self.tab == Tab::Play {
            (GOLD, "Before your first game".into(), "Bring your AA Classic settings over, or skip it: Settings can do it any time.".into(), "JUST PLAY", Some(Action::SkipOffer))
        } else if self.xfer.working && self.play_after_transfer {
            (GOLD, "Bringing your settings".into(), "The game starts right after.".into(), "WAIT", None)
        } else if self.signup && self.tab == Tab::Play {
            let sub = if self.baked_invite.is_some() { "PLAY makes your account, sets up your copy of the game and starts it." } else { "Have an account already? Use the link on the right." };
            match &self.hint {
                Some(h) => (RED, "New account".into(), h.clone(), "PLAY", Some(Action::SignUp)),
                None => (BLUE, "New account".into(), sub.into(), "PLAY", Some(Action::SignUp)),
            }
        } else if self.tab == Tab::Transfer && !self.installing && !self.busy {
            self.transfer_status()
        } else if self.installing {
            (GOLD, "Installing the game".into(), self.log.last().cloned().unwrap_or_default(), "STOP", Some(Action::Stop))
        } else if self.busy {
            (GOLD, "Installing your copy".into(), self.log.last().cloned().unwrap_or_default(), "INSTALLING", None)
        } else if let Some((c, _)) = &self.child {
            let _ = c;
            (BLUE, "Playing".into(), "The game is running. Have fun! Close it to play again.".into(), "PLAYING", None)
        } else {
            match &self.state {
                Err(e) => (RED, "AA Classic was not found".into(), e.clone(), "FIND IT", Some(Action::ChooseFolder)),
                Ok(State::NotSetUp) if self.cfg.mode == Mode::Own => {
                    let last = self.log.last().filter(|l| l.starts_with("install:") || l.starts_with("build pack:")).cloned();
                    if vault::builds(&self.paths).is_empty() {
                        (MUTED, "First: a build pack".into(), last.unwrap_or_else(|| "The program files (65 MB) come from a friend who has the game: ask for a build pack and import it.".into()), "IMPORT PACK", Some(Action::ImportPack))
                    } else if install::load_state(&self.paths).is_some() {
                        (GOLD, "Install not finished".into(), last.unwrap_or_else(|| "Goes on where it stopped.".into()), "CONTINUE", Some(Action::Install))
                    } else {
                        (MUTED, "Not installed yet".into(), last.unwrap_or_else(|| "Downloads the game data from AA Classic's file host: about 24 GB to download, 46 GB on disk. No account needed.".into()), "INSTALL GAME", Some(Action::Install))
                    }
                }
                Ok(State::NotSetUp) => {
                    // AA Classic on another drive: say where the copy goes and why
                    let other = match (&self.cfg.aac_dir, self.cfg.mode) {
                        (Some(d), Mode::Standard) if find::client_place(&self.paths.client(), d).is_some() => Some(find::place_note(&self.paths.root, &self.paths.client(), d)),
                        _ => None,
                    };
                    (
                        if self.hint.is_some() { RED } else { MUTED },
                        "One step before you play".into(),
                        self.hint.clone().or(other).unwrap_or_else(|| format!(
                            "The first press makes your own copy of the game ({}), then it starts. AA Classic stays exactly as it is.",
                            if self.cfg.mode == Mode::Full { "about 45 GB" } else { "about 65 MB, a minute or two" }
                        )),
                        "PLAY",
                        Some(Action::Play),
                    )
                }
                Ok(State::BinOutdated) | Ok(State::PakOutdated) => {
                    (GOLD, "AA Classic was updated".into(), "PLAY updates your copy to match it first (a minute or two).".into(), "PLAY", Some(Action::Play))
                }
                Ok(State::Ready) => match &self.hint {
                    Some(h) => (RED, "Ready to play".into(), h.clone(), "PLAY", Some(Action::Play)),
                    None => {
                        let sub = match &self.saved {
                            Some(s) => format!("Signed in as {} · your copy of the game is up to date", s.username),
                            None if self.cfg.username.trim().is_empty() => "Enter your name and password on the right, then press Play.".to_string(),
                            None => "Enter your password on the right, then press Play.".to_string(),
                        };
                        let _ = mode;
                        (GREEN, "Ready to play".into(), sub, "PLAY", Some(Action::Play))
                    }
                },
            }
        };

        // status, left
        let x = rect.left() + 40.0;
        let y = rect.top() + 30.0;
        for i in 1..4 {
            p.circle_filled(pos2(x, y), 4.0 + i as f32 * 2.5, Color32::from_rgba_unmultiplied(dot.r(), dot.g(), dot.b(), 18));
        }
        p.circle_filled(pos2(x, y), 4.5, dot);
        p.text(pos2(x + 18.0, y), Align2::LEFT_CENTER, &title, bold_font(17.0), INK);
        let sub_g = p.layout(sub, FontId::proportional(13.0), MUTED, rect.width() - 420.0);
        p.galley(pos2(x + 18.0, y + 14.0), sub_g, MUTED);
        if self.busy {
            let bar = Rect::from_min_size(pos2(x + 18.0, rect.bottom() - 22.0), vec2(rect.width() - 420.0, 6.0));
            p.rect_filled(bar, 3.0, rgba(255, 255, 255, 18));
            let f = match self.progress {
                Some(v) => v,
                None => ((ui.input(|i| i.time) as f32 * 0.6).sin() * 0.5 + 0.5) * 0.9 + 0.05,
            };
            let fill = Rect::from_min_size(bar.min, vec2(bar.width() * f.clamp(0.02, 1.0), bar.height()));
            hgrad(&p, fill, Color32::from_rgb(170, 128, 50), Color32::from_rgb(246, 214, 140));
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }

        // big button, right
        let b = Rect::from_min_size(pos2(rect.right() - 300.0, rect.center().y - 30.0), vec2(256.0, 60.0));
        let enabled = action.is_some();
        let resp = ui.scope_builder(UiBuilder::new().max_rect(b), |ui| big_button(ui, b.size(), button, enabled)).inner;
        if resp.clicked() {
            *act = action;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.poll(&ctx);

        let full = root.max_rect();
        let p = root.painter().clone();
        backdrop(&p, full);

        let top = Rect::from_min_size(full.min, vec2(full.width(), 52.0));
        let bottom = Rect::from_min_max(pos2(full.left(), full.bottom() - 100.0), full.max);
        let body = Rect::from_min_max(pos2(full.left(), top.bottom()), pos2(full.right(), bottom.top()));

        let mut act = None;
        self.top_bar(root, top, &mut act);
        match self.tab {
            Tab::Play => self.play_tab(root, body, &mut act),
            Tab::Transfer => self.transfer_tab(root, body, &mut act),
            Tab::Settings => self.settings_tab(root, body, &mut act),
            Tab::Log => self.log_tab(root, body),
        }
        self.bottom_bar(root, bottom, &mut act);
        // a thin frame around the borderless window
        p.rect_stroke(full.shrink(0.5), 0.0, Stroke::new(1.0, gold_a(50)), egui::StrokeKind::Inside);
        self.report_dialog(&ctx, &mut act);

        if let Some(a) = act {
            self.run(a);
        }
    }
}

pub fn run() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 680.0])
            .with_resizable(false)
            .with_decorations(false)
            .with_title("AA Replay"),
        ..Default::default()
    };
    let _ = eframe::run_native("AA Replay", options, Box::new(|cc| Ok(Box::new(App::new(cc)))));
}
