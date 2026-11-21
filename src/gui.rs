//! The launcher window: a game-launcher style screen with a backdrop painted in code.
//! It deliberately uses no AA Classic logo or artwork, so it never passes for the official launcher.

use crate::cli::default_aac_dir;
use crate::core::*;
use crate::net::{self, Server};
use crate::{addons, install, vault};
use eframe::egui::{
    self, Align, Align2, Color32, FontData, FontFamily, FontId, Layout, Margin, Painter, Pos2, Rect, RichText, Sense,
    Shape, Stroke, TextEdit, UiBuilder, Vec2, ViewportCommand, pos2, vec2,
};
use std::process::Child;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, SystemTime};

// ---------------------------------------------------------------------------------------------------------------
// Look
// ---------------------------------------------------------------------------------------------------------------

const GOLD: Color32 = Color32::from_rgb(214, 177, 94);
const INK: Color32 = Color32::from_rgb(234, 228, 214);
const MUTED: Color32 = Color32::from_rgb(146, 156, 170);
const DARK_TEXT: Color32 = Color32::from_rgb(34, 24, 8);
const GREEN: Color32 = Color32::from_rgb(118, 204, 140);
const RED: Color32 = Color32::from_rgb(232, 112, 100);
const BLUE: Color32 = Color32::from_rgb(110, 170, 240);

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

fn gold_a(a: u8) -> Color32 {
    rgba(214, 177, 94, a)
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
    v.selection.bg_fill = Color32::from_rgb(112, 88, 40);
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
        p.circle_filled(moon, 36.0 + i as f32 * 16.0, rgba(240, 200, 130, 3 + (15 - i) as u8 / 2));
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

/// The big gold button (play / install / update).
fn big_button(ui: &mut egui::Ui, size: Vec2, label: &str, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
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
        (Color32::from_rgb(252, 226, 156), Color32::from_rgb(208, 162, 72), Color32::from_rgb(255, 236, 180), DARK_TEXT)
    } else {
        (Color32::from_rgb(240, 208, 132), Color32::from_rgb(184, 140, 58), Color32::from_rgb(250, 222, 150), DARK_TEXT)
    };
    let r = if enabled && resp.is_pointer_button_down_on() { rect.shrink(1.0) } else { rect };
    grad_shape(p, r, 10.0, top, bottom, Stroke::new(1.0, edge));
    let inner = r.shrink(4.0);
    p.add(Shape::closed_line(chamfer(inner, 7.0), Stroke::new(1.0, rgba(255, 245, 210, if enabled { 70 } else { 20 }))));
    p.text(r.center(), Align2::CENTER_CENTER, label, title_font(24.0), text);
    resp
}

/// Text field with a small caption above it.
fn field(ui: &mut egui::Ui, caption: &str, value: &mut String, password: bool, hint: &str) -> egui::Response {
    ui.label(RichText::new(caption.to_uppercase()).size(11.5).color(MUTED).extra_letter_spacing(1.2));
    let r = ui.add(
        TextEdit::singleline(value)
            .password(password)
            .hint_text(hint)
            .desired_width(f32::INFINITY)
            .margin(Margin::symmetric(10, 8))
            .font(FontId::proportional(15.5)),
    );
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
    let p = ui.painter();
    let edge = if selected {
        Stroke::new(1.5, GOLD)
    } else if resp.hovered() && enabled {
        Stroke::new(1.0, gold_a(140))
    } else {
        Stroke::new(1.0, Color32::from_rgb(52, 60, 76))
    };
    let (t, b) = if selected { (rgba(52, 44, 24, 230), rgba(24, 22, 16, 235)) } else { (rgba(24, 30, 40, 220), rgba(14, 18, 25, 230)) };
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
    /// the account API's answer to a login: account name and launch ticket
    Ticket(Result<(String, String), String>),
    SignedUp(Result<String, String>),
    Progress(f32),
    /// Install game ended: game data version and client build
    Installed(Result<(u32, String), String>),
    /// a build pack was imported or exported
    Pack(Result<String, String>),
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Play,
    Settings,
    Log,
}

enum Action {
    Setup,
    /// Install game: the launcher's own download
    Install,
    Stop,
    ImportPack,
    ExportPack,
    Play,
    SignUp,
    ChooseFolder,
    SetMode(Mode),
    RemoveClient,
    RemoveAll,
    OpenFolder,
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
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    child: Option<(Child, SystemTime)>,
    state: Result<State, String>,
    tab: Tab,
    confirm: Option<(Action, SystemTime)>,
}

impl App {
    fn new(cc: &eframe::CreationContext) -> App {
        install_fonts(&cc.egui_ctx);
        install_style(&cc.egui_ctx);
        let paths = Paths::new();
        let mut cfg = Config::load(&paths);
        if cfg.aac_dir.is_none() {
            cfg.aac_dir = default_aac_dir();
            // a PC without AA Classic: the launcher downloads the game itself
            if cfg.aac_dir.is_none() && !paths.exe().is_file() {
                cfg.mode = Mode::Own;
            }
        }
        let (tx, rx) = channel();
        let mut a = App {
            paths,
            cfg,
            password: String::new(),
            invite: String::new(),
            signup: std::env::var("AAEMU_SANDBOX_SIGNUP").is_ok(), // for screenshots
            asking: false,
            log: vec![],
            busy: false,
            progress: None,
            cancel: Default::default(),
            installing: false,
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
        };
        a.refresh();
        a
    }

    fn aac(&self) -> Result<AacInstall, String> {
        let d = self.cfg.aac_dir.clone().ok_or("choose your AA Classic folder in Settings")?;
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

    fn start_play(&mut self) {
        let aac = match self.aac_if_needed() {
            Ok(a) => a,
            Err(e) => return self.say(e),
        };
        if self.cfg.username.trim().is_empty() || self.password.is_empty() {
            return self.say("enter your account and password for this server");
        }
        let server = match Server::parse(&self.cfg.login_ip) {
            Ok(s) => s,
            Err(e) => return self.say(e),
        };
        let _ = self.cfg.save(&self.paths);
        let username = self.cfg.username.trim().to_string();
        if let Server::Api { .. } = server {
            // the password is checked by the server's account API; the client only ever gets a one-time ticket
            self.asking = true;
            self.say("signing in...");
            let (tx, password, paths) = (self.tx.clone(), self.password.clone(), self.paths.clone());
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
                let _ = tx.send(Msg::Ticket(net::login(&server, &username, &password).map(|t| (t.username, t.ticket))));
            });
            return;
        }
        let token = token_from_password(&self.password);
        self.launch(aac, username, token);
    }

    fn start_signup(&mut self) {
        let server = match Server::parse(&self.cfg.login_ip) {
            Ok(s) => s,
            Err(e) => return self.say(e),
        };
        if self.cfg.username.trim().is_empty() || self.password.is_empty() {
            return self.say("choose an account name and a password");
        }
        self.asking = true;
        self.say("creating the account...");
        let (tx, username, password, invite) = (self.tx.clone(), self.cfg.username.trim().to_string(), self.password.clone(), self.invite.clone());
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
            Err(e) => self.say(e),
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
        match a {
            Action::Setup => self.start_setup(),
            Action::Install => self.start_install(),
            Action::Stop => {
                self.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                self.say("stopping after the part that is being downloaded...");
            }
            Action::ImportPack => self.import_pack(),
            Action::ExportPack => self.export_pack(),
            Action::Play => self.start_play(),
            Action::SignUp => self.start_signup(),
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
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
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
                        (Ok((name, ticket)), Ok(aac)) => {
                            self.cfg.username = name.clone();
                            let _ = self.cfg.save(&self.paths);
                            self.launch(aac, name, ticket);
                        }
                        (Err(e), _) | (_, Err(e)) => self.say(format!("sign-in failed: {e}")),
                    }
                }
                Msg::SignedUp(r) => {
                    self.asking = false;
                    match r {
                        Ok(name) => {
                            self.cfg.username = name.clone();
                            let _ = self.cfg.save(&self.paths);
                            self.signup = false;
                            self.invite.clear();
                            self.say(format!("account {name} created: press Play"));
                        }
                        Err(e) => self.say(format!("sign-up failed: {e}")),
                    }
                }
                Msg::Progress(f) => self.progress = Some(f),
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
                        Err(e) => self.say(format!("install: {e}")),
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
                        Ok(()) => self.say("set up: ready to play"),
                        Err(e) => self.say(format!("set up failed: {e}")),
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
        if self.busy || self.asking || self.child.is_some() || self.confirm.is_some() {
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
        p.add(Shape::convex_polygon(dia(6.0), GOLD, Stroke::NONE));
        let logo = p.text(pos2(rect.left() + 58.0, rect.center().y - 1.0), Align2::LEFT_CENTER, "AAEMU", title_font(22.0), GOLD);
        let sb = p.text(pos2(logo.right() + 8.0, logo.bottom() - 5.0), Align2::LEFT_BOTTOM, "SANDBOX", bold_font(11.5), MUTED);

        let tabs = Rect::from_min_max(pos2(sb.right() + 40.0, rect.top()), rect.max);
        ui.scope_builder(UiBuilder::new().max_rect(tabs).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (t, name) in [(Tab::Play, "Play"), (Tab::Settings, "Settings"), (Tab::Log, "Log")] {
                if nav_tab(ui, name, self.tab == t).clicked() {
                    self.tab = t;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if win_button(ui, WinBtn::Close).clicked() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                }
                if win_button(ui, WinBtn::Minimize).clicked() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
                }
            });
        });
        let _ = act;
    }

    fn play_tab(&mut self, ui: &mut egui::Ui, body: Rect, act: &mut Option<Action>) {
        // left: the pitch
        let hero = Rect::from_min_size(body.min + vec2(56.0, 64.0), vec2(520.0, body.height() - 80.0));
        ui.scope_builder(UiBuilder::new().max_rect(hero).layout(Layout::top_down(Align::Min)), |ui| {
            ui.label(RichText::new("PRIVATE TEST SERVER").font(bold_font(12.5)).color(GOLD).extra_letter_spacing(3.0));
            ui.add_space(2.0);
            ui.label(RichText::new("AAEmu Sandbox").font(title_font(52.0)).color(INK));
            ui.add_space(2.0);
            ui.label(RichText::new("Play on your own AAEmu server with your own copy of the client, kept apart from AA Classic.").size(16.0).color(MUTED));
            ui.add_space(28.0);
            tick_row(ui, "Own copy of the client", "Your AA Classic folder is only read, never changed.");
            tick_row(ui, "Own user folder", "Settings, addons and logs never mix with AA Classic's.");
            tick_row(ui, "No reports sent", "Crash and performance uploads are switched off.");
            tick_row(ui, "Separate account", "Your AA Classic login is never asked for.");
        });

        // right: sign in
        let w = 360.0;
        let cardr = Rect::from_min_size(pos2(body.right() - w - 48.0, body.top() + 36.0), vec2(w, if self.signup { 484.0 } else { 424.0 }));
        card(ui.painter(), cardr);
        let mut enter = false;
        ui.scope_builder(UiBuilder::new().max_rect(cardr.shrink2(vec2(28.0, 24.0))).layout(Layout::top_down(Align::Min)), |ui| {
            ui.label(RichText::new(if self.signup { "Create account" } else { "Sign in" }).font(title_font(26.0)).color(INK));
            ui.label(RichText::new("Your account on this server, not your AA Classic account.").size(13.0).color(MUTED));
            ui.add_space(14.0);
            if field(ui, "Server", &mut self.cfg.login_ip, false, "https://... or 127.0.0.1").changed() {
                self.cfg.game_ip = self.cfg.login_ip.clone();
            }
            field(ui, "Account", &mut self.cfg.username, false, "account name");
            let pw = field(ui, "Password", &mut self.password, true, "password for this server");
            if pw.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                enter = true;
            }
            if self.signup {
                let inv = field(ui, "Invite code", &mut self.invite, false, "from the server's owner");
                if inv.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    enter = true;
                }
            }
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                ui.painter().circle_stroke(r.center(), 6.0, Stroke::new(1.2, GOLD));
                ui.painter().text(r.center(), Align2::CENTER_CENTER, "!", bold_font(10.0), GOLD);
                ui.label(RichText::new("Never type your AA Classic password here.").size(12.5).color(GOLD));
            });
            ui.add_space(4.0);
            // accounts are made on a server with an account API (a link), not on a bare address
            if matches!(Server::parse(&self.cfg.login_ip), Ok(Server::Api { .. })) {
                let label = if self.signup { "I already have an account" } else { "No account yet? Create one" };
                if ui.add(egui::Label::new(RichText::new(label).size(13.0).color(BLUE).underline()).sense(Sense::click())).clicked() {
                    self.signup = !self.signup;
                }
            } else {
                self.signup = false;
            }
        });
        if enter && self.signup && !self.asking {
            *act = Some(Action::SignUp);
        } else if enter && matches!(self.state, Ok(State::Ready)) && !self.busy && !self.asking && self.child.is_none() {
            *act = Some(Action::Play);
        }
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui, body: Rect, act: &mut Option<Action>) {
        let cardr = Rect::from_center_size(body.center() + vec2(0.0, 4.0), vec2(760.0, body.height() - 56.0));
        card(ui.painter(), cardr);
        let inner = cardr.shrink2(vec2(32.0, 26.0));
        let locked = self.busy || self.child.is_some();
        ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)), |ui| {
            ui.label(RichText::new("Settings").font(title_font(26.0)).color(INK));
            ui.add_space(8.0);

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
            ui.label(RichText::new("Only binNext and game_pak are read. Nothing in this folder is ever written.").size(12.5).color(MUTED));
            ui.add_space(12.0);

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
            ui.add_space(10.0);

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

            ui.label(RichText::new("SANDBOX FOLDER").size(11.5).color(MUTED).extra_letter_spacing(1.2));
            ui.horizontal(|ui| {
                ui.label(RichText::new(self.paths.root.display().to_string()).monospace().color(INK));
                if ui.button("Open").clicked() {
                    *act = Some(Action::OpenFolder);
                }
                let l1 = if self.confirming(&Action::RemoveClient) { "Click again to remove" } else { "Remove client copy" };
                if ui.add_enabled(!locked, egui::Button::new(RichText::new(l1).color(RED))).clicked() {
                    *act = Some(Action::RemoveClient);
                }
                let l2 = if self.confirming(&Action::RemoveAll) { "Click again to remove" } else { "Remove copy and profile" };
                if ui.add_enabled(!locked, egui::Button::new(RichText::new(l2).color(RED))).clicked() {
                    *act = Some(Action::RemoveAll);
                }
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
        } else if self.signup && self.tab == Tab::Play {
            let sub = self.log.last().filter(|l| l.starts_with("sign-up failed")).cloned().unwrap_or_else(|| "Pick a name and a password for this server, and enter your invite code.".into());
            (BLUE, "New account".into(), sub, "CREATE ACCOUNT", Some(Action::SignUp))
        } else if self.installing {
            (GOLD, "Installing the game".into(), self.log.last().cloned().unwrap_or_default(), "STOP", Some(Action::Stop))
        } else if self.busy {
            (GOLD, "Installing your copy".into(), self.log.last().cloned().unwrap_or_default(), "INSTALLING", None)
        } else if let Some((c, _)) = &self.child {
            (BLUE, "Playing".into(), format!("Client running (pid {}). Close the game to play again.", c.id()), "PLAYING", None)
        } else {
            match &self.state {
                Err(e) => (RED, "Choose your AA Classic folder".into(), e.clone(), "SETTINGS", Some(Action::ChooseFolder)),
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
                Ok(State::NotSetUp) => (
                    MUTED,
                    "Not installed yet".into(),
                    format!("{mode}: makes your own copy of the client. {}", if self.cfg.mode == Mode::Full { "Needs ~45 GB." } else { "Needs ~65 MB." }),
                    "INSTALL",
                    Some(Action::Setup),
                ),
                Ok(State::BinOutdated) | Ok(State::PakOutdated) => {
                    (GOLD, "AA Classic was updated".into(), "Update your copy before playing.".into(), "UPDATE", Some(Action::Setup))
                }
                Ok(State::Ready) => {
                    let who = if self.cfg.username.trim().is_empty() { String::new() } else { format!(" as {}", self.cfg.username.trim()) };
                    let sub = match self.log.last().filter(|l| l.starts_with("sign-in failed")) {
                        Some(l) => l.clone(),
                        None => format!("{mode} · server {}{who}", self.cfg.login_ip.trim()),
                    };
                    (GREEN, "Ready to play".into(), sub, "PLAY", Some(Action::Play))
                }
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
            *act = match action {
                Some(Action::ChooseFolder) => {
                    self.tab = Tab::Settings;
                    Some(Action::ChooseFolder)
                }
                a => a,
            };
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
            Tab::Settings => self.settings_tab(root, body, &mut act),
            Tab::Log => self.log_tab(root, body),
        }
        self.bottom_bar(root, bottom, &mut act);
        // a thin frame around the borderless window
        p.rect_stroke(full.shrink(0.5), 0.0, Stroke::new(1.0, gold_a(50)), egui::StrokeKind::Inside);

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
            .with_title("AAEmu Sandbox"),
        ..Default::default()
    };
    let _ = eframe::run_native("AAEmu Sandbox", options, Box::new(|cc| Ok(Box::new(App::new(cc)))));
}
