//! The "Transfer from AA Classic" screen: a checklist of what can move, a preview of the selected line, the two
//! sources only the player can give (a recording and a DaruGear link), and Undo.

use crate::gui::events::Event;
use crate::gui::theme::*;
use crate::gui::{Action, App, Secret, Tab};
use crate::net::{self, Server};
use crate::{recording, transfer};
use eframe::egui::{self, Align, Align2, Color32, FontId, Layout, Margin, Painter, Pos2, Rect, RichText, Sense, Stroke, TextEdit, UiBuilder, pos2, vec2};
use std::path::PathBuf;
use std::time::SystemTime;

/// What the screen keeps between frames.
#[derive(Default)]
pub struct Xfer {
    pub scan: Option<transfer::Scan>,
    pub scanning: bool,
    /// ticked ids
    pub ticks: Vec<&'static str>,
    /// ids the player changed by hand (a rescan keeps their choice)
    pub touched: Vec<&'static str>,
    pub selected: usize,
    pub link: String,
    pub build: Option<net::Build>,
    pub build_error: Option<String>,
    /// the code being looked up at DaruGear
    pub fetching: Option<String>,
    pub recording: Option<PathBuf>,
    pub working: bool,
    /// the last result: success and its lines
    pub result: Option<(bool, String, Vec<String>)>,
}

/// The transfer screen's background threads -> window (inside `Event::Transfer`).
pub enum TransferEvent {
    Scanned(Box<transfer::Scan>),
    Build(String, Result<net::Build, String>),
    Done(Result<Vec<String>, String>),
    Undone(Result<Vec<String>, String>),
}

impl Xfer {
    pub fn items(&self) -> Vec<transfer::Item> {
        self.scan.as_ref().map(|s| s.items()).unwrap_or_default()
    }

    pub fn ticked(&self) -> Vec<&'static str> {
        self.ticks.clone()
    }
}

/// A square check box drawn in the launcher's gold.
fn check_box(p: &Painter, r: Rect, on: bool, enabled: bool, hover: bool) {
    let edge = if !enabled { rgba(90, 96, 108, 160) } else if on || hover { GOLD } else { rgba(110, 150, 150, 200) };
    let fill = if on && enabled { GOLD } else { rgba(10, 13, 19, 220) };
    p.rect(r, 3.0, fill, Stroke::new(1.3, edge), egui::StrokeKind::Inside);
    if on && enabled {
        let c = r.center();
        let s = Stroke::new(2.0, DARK_TEXT);
        p.line_segment([c + vec2(-4.5, 0.5), c + vec2(-1.2, 3.8)], s);
        p.line_segment([c + vec2(-1.2, 3.8), c + vec2(5.0, -3.6)], s);
    }
}

fn chip(p: &Painter, right_center: Pos2, text: &str, color: Color32) -> Rect {
    let g = p.layout_no_wrap(text.to_string(), bold_font(10.5), color);
    let r = Rect::from_min_size(right_center - vec2(g.size().x + 16.0, 10.0), vec2(g.size().x + 16.0, 20.0));
    p.rect(r, 10.0, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 22), Stroke::new(1.0, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 110)), egui::StrokeKind::Inside);
    p.galley(r.center() - g.size() / 2.0, g, color);
    r
}

fn elide(p: &Painter, text: &str, font: FontId, width: f32) -> String {
    if p.layout_no_wrap(text.to_string(), font.clone(), INK).size().x <= width {
        return text.to_string();
    }
    let mut t: String = text.to_string();
    while !t.is_empty() && p.layout_no_wrap(format!("{t}…"), font.clone(), INK).size().x > width {
        t.pop();
    }
    format!("{}…", t.trim_end())
}

/// One checklist line. Returns (box clicked, line clicked).
#[allow(clippy::too_many_arguments)]
fn line(ui: &mut egui::Ui, width: f32, item: &transfer::Item, on: bool, enabled: bool, selected: bool, tag: (&str, Color32)) -> (bool, bool) {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 50.0), Sense::click());
    let p = ui.painter();
    let hover = resp.hovered();
    if selected {
        p.rect_filled(rect, 4.0, gold_a(26));
        p.line_segment([rect.left_top() + vec2(0.0, 6.0), rect.left_bottom() - vec2(0.0, 6.0)], Stroke::new(2.0, GOLD));
    } else if hover {
        p.rect_filled(rect, 4.0, rgba(255, 255, 255, 10));
    }
    let boxr = Rect::from_center_size(pos2(rect.left() + 26.0, rect.center().y), vec2(18.0, 18.0));
    let box_hit = ui.interact(boxr.expand(8.0), ui.scope_id().with(("xbox", item.id)), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    check_box(p, boxr, on, enabled, box_hit.hovered());
    let tag_r = chip(p, pos2(rect.right() - 12.0, rect.center().y), tag.0, tag.1);
    let text_w = tag_r.left() - rect.left() - 60.0;
    let title_c = if enabled { INK } else { MUTED };
    p.text(pos2(rect.left() + 48.0, rect.top() + 9.0), Align2::LEFT_TOP, item.title, bold_font(15.0), title_c);
    let sum = elide(p, &item.summary, FontId::proportional(12.5), text_w);
    p.text(pos2(rect.left() + 48.0, rect.top() + 28.0), Align2::LEFT_TOP, sum, FontId::proportional(12.5), if enabled { MUTED } else { rgba(120, 128, 140, 200) });
    (box_hit.clicked(), resp.clicked() && !box_hit.clicked())
}

fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).size(11.5).color(MUTED).extra_letter_spacing(1.2));
}

impl App {
    /// Opens the screen: looks at AA Classic's files and the newest recording, and at the clipboard for a link.
    pub(in crate::gui) fn open_transfer(&mut self) {
        self.tab = Tab::Transfer;
        if self.xfer.link.trim().is_empty() {
            // only a DaruGear link is taken from the clipboard, never other text
            if let Some(code) = transfer::clipboard_text().filter(|t| t.contains("darugear.com/")).and_then(|t| net::daru_code(&t)) {
                self.xfer.link = format!("https://darugear.com/?s={code}");
            }
        }
        if self.xfer.recording.is_none() {
            self.xfer.recording = recording::find(&self.paths).into_iter().next();
        }
        self.rescan();
        self.check_link();
    }

    pub(in crate::gui) fn rescan(&mut self) {
        if self.xfer.scanning {
            return;
        }
        self.xfer.scanning = true;
        let (paths, tx, rec) = (self.paths.clone(), self.tx.clone(), self.xfer.recording.clone());
        std::thread::spawn(move || {
            let s = transfer::scan(&paths, transfer::real_docs().as_deref(), rec.as_deref());
            let _ = tx.send(Event::Transfer(TransferEvent::Scanned(Box::new(s))));
        });
    }

    /// Looks the link's build up at DaruGear when the code changed.
    fn check_link(&mut self) {
        let code = net::daru_code(&self.xfer.link);
        let Some(code) = code else {
            self.xfer.build = None;
            self.xfer.build_error = (!self.xfer.link.trim().is_empty()).then(|| "That is not a DaruGear link: it looks like https://darugear.com/?s=abc123".into());
            return;
        };
        if self.xfer.build.as_ref().is_some_and(|b| b.code == code) || self.xfer.fetching.as_deref() == Some(code.as_str()) {
            return;
        }
        self.xfer.fetching = Some(code.clone());
        self.xfer.build = None;
        self.xfer.build_error = None;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = net::daru(&code);
            let _ = tx.send(Event::Transfer(TransferEvent::Build(code, r)));
        });
    }

    /// Server parts can only go out with the account's name and its password or remembered sign-in, on a server link.
    fn account_ready(&self) -> Result<(Server, String, Secret), &'static str> {
        match Server::parse(&self.cfg.login_ip) {
            Ok(s @ Server::Api { .. }) => match self.secret() {
                Some((u, c)) => Ok((s, u, c)),
                None => Err("Sign in on the Play screen first"),
            },
            _ => Err("Needs a server link (https://...) in Settings"),
        }
    }

    pub(in crate::gui) fn transfer_event(&mut self, m: TransferEvent) {
        match m {
            TransferEvent::Scanned(s) => {
                self.xfer.scanning = false;
                let mut s = *s;
                s.build = self.xfer.build.clone();
                s.build_error = self.xfer.build_error.clone();
                self.xfer.scan = Some(s);
                self.retick();
            }
            TransferEvent::Build(code, r) => {
                if self.xfer.fetching.as_deref() == Some(code.as_str()) {
                    self.xfer.fetching = None;
                }
                if net::daru_code(&self.xfer.link).as_deref() != Some(code.as_str()) {
                    return; // the link changed meanwhile
                }
                match r {
                    Ok(b) => {
                        self.xfer.build = Some(b);
                        self.xfer.build_error = None;
                    }
                    Err(e) => {
                        self.xfer.build = None;
                        self.xfer.build_error = Some(e);
                    }
                }
                if let Some(s) = self.xfer.scan.as_mut() {
                    s.build = self.xfer.build.clone();
                    s.build_error = self.xfer.build_error.clone();
                }
                self.retick();
            }
            TransferEvent::Done(r) => {
                self.xfer.working = false;
                // the first start's offer: the game starts once the setup is in place, moved or not
                if std::mem::take(&mut self.play_after_transfer) {
                    if let Err(e) = &r {
                        self.say(format!("transfer: {e}"));
                    }
                    self.press_play();
                }
                match r {
                    Ok(lines) => {
                        self.say(format!("transfer from AA Classic: {}", lines.join("; ")));
                        self.xfer.result = Some((true, "Moved".into(), lines));
                    }
                    Err(e) => {
                        self.say(format!("transfer: {e}"));
                        self.xfer.result = Some((false, "Not moved".into(), vec![e]));
                    }
                }
                self.rescan();
            }
            TransferEvent::Undone(r) => {
                self.xfer.working = false;
                match r {
                    Ok(lines) => {
                        self.say(format!("transfer undone: {}", lines.join("; ")));
                        self.xfer.result = Some((true, "Put back".into(), lines));
                    }
                    Err(e) => {
                        self.say(format!("undo: {e}"));
                        self.xfer.result = Some((false, "Undo failed".into(), vec![e]));
                    }
                }
                self.rescan();
            }
        }
    }

    /// Ticks every line that can move (the first start's offer: everything there is).
    pub(in crate::gui) fn retick_all(&mut self) {
        self.xfer.touched.clear();
        self.retick();
    }

    /// Ticks every line that can move, except the ones the player unticked.
    fn retick(&mut self) {
        let server_ok = self.account_ready().is_ok();
        let items = self.xfer.items();
        let can: Vec<&'static str> = items.iter().filter(|i| i.ready && (!i.server || server_ok)).map(|i| i.id).collect();
        let keep: Vec<&'static str> = self.xfer.ticks.iter().copied().filter(|id| can.contains(id)).collect();
        let mut t: Vec<&'static str> = can.iter().copied().filter(|id| !self.xfer.touched.contains(id) || keep.contains(id)).collect();
        t.sort_by_key(|id| transfer::IDS.iter().position(|x| x == id));
        self.xfer.ticks = t;
    }

    pub(in crate::gui) fn start_transfer(&mut self) {
        let Some(scan) = self.xfer.scan.clone() else { return };
        let ticks = self.xfer.ticked();
        if ticks.is_empty() {
            return;
        }
        let server_part = scan.payload(&ticks);
        let account = if server_part.is_some() {
            match self.account_ready() {
                Ok(a) => Some(a),
                Err(e) => return self.say(format!("transfer: {e}")),
            }
        } else {
            None
        };
        self.xfer.working = true;
        self.xfer.result = None;
        let (paths, tx, link) = (self.paths.clone(), self.tx.clone(), self.cfg.login_ip.trim().to_string());
        std::thread::spawn(move || {
            let r = (|| {
                let applied = transfer::apply_local(&paths, &scan, &ticks)?;
                let mut lines = applied.lines;
                let mut server = String::new();
                let mut user = String::new();
                if let (Some(p), Some((s, u, pw))) = (server_part, account) {
                    match net::transfer(&s, &u, pw.cred(), &p) {
                        Ok(a) => {
                            lines.push(format!("on your account {u}: {} (at your next login)", a.accepted.join(", ")));
                            server = link;
                            user = u;
                        }
                        Err(e) => {
                            // what moved on this PC stays undoable
                            let _ = transfer::record(&paths, &ticks, applied.backup.clone(), "", "");
                            return Err(if lines.is_empty() { format!("the server did not take it: {e}") } else { format!("{} moved; the server did not take the rest: {e}", lines.join(", ")) });
                        }
                    }
                }
                transfer::record(&paths, &ticks, applied.backup, &server, &user)?;
                Ok(lines)
            })();
            let _ = tx.send(Event::Transfer(TransferEvent::Done(r)));
        });
    }

    pub(in crate::gui) fn start_undo(&mut self) {
        let Some(last) = transfer::last(&self.paths) else { return };
        let account = if !last.server.is_empty() {
            match (Server::parse(&last.server), self.secret()) {
                (Ok(s), Some((u, c))) if u.eq_ignore_ascii_case(&last.account) => Some((s, last.account.clone(), c)),
                _ => return self.say("undo: sign in to the account the transfer went to first"),
            }
        } else {
            None
        };
        self.xfer.working = true;
        self.xfer.result = None;
        let (paths, tx) = (self.paths.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let r = (|| {
                if let Some((s, u, pw)) = &account {
                    net::transfer(s, u, pw.cred(), &serde_json::json!({ "undo": true }))?;
                }
                let mut lines = transfer::undo_local(&paths)?;
                if let Some((_, u, _)) = account {
                    lines.push(format!("keybinds, options and bars of {u} at the next login"));
                }
                Ok(lines)
            })();
            let _ = tx.send(Event::Transfer(TransferEvent::Undone(r)));
        });
    }

    // --- the screen ----------------------------------------------------------------------------------------------

    pub(in crate::gui) fn transfer_tab(&mut self, ui: &mut egui::Ui, body: Rect, act: &mut Option<Action>) {
        let cardr = Rect::from_center_size(body.center() + vec2(0.0, 4.0), vec2(body.width() - 80.0, body.height() - 36.0));
        card(ui.painter(), cardr);
        let inner = cardr.shrink2(vec2(30.0, 20.0));
        let p = ui.painter().clone();

        // header
        p.text(inner.left_top(), Align2::LEFT_TOP, "Bring your setup from AA Classic", title_font(26.0), INK);
        p.text(inner.left_top() + vec2(0.0, 34.0), Align2::LEFT_TOP, "Tick what should come over and press Transfer. AA Classic's own files are only read, and Undo puts everything back.", FontId::proportional(13.0), MUTED);

        let top = inner.top() + 62.0;
        let left_w = 548.0;
        let left = Rect::from_min_max(pos2(inner.left(), top), pos2(inner.left() + left_w, inner.bottom()));
        let right = Rect::from_min_max(pos2(left.right() + 26.0, top), pos2(inner.right(), inner.bottom()));
        p.line_segment([pos2(left.right() + 13.0, top + 4.0), pos2(left.right() + 13.0, inner.bottom() - 4.0)], Stroke::new(1.0, rgba(255, 255, 255, 16)));

        let items = self.xfer.items();
        let server = self.account_ready();
        let host = match Server::parse(&self.cfg.login_ip) {
            Ok(Server::Api { host, .. }) => host,
            _ => "the server".into(),
        };
        let locked = self.xfer.working || self.child.is_some() || self.busy;

        // the checklist
        ui.scope_builder(UiBuilder::new().max_rect(left).layout(Layout::top_down(Align::Min)), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if items.is_empty() {
                ui.add_space(30.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Looking at your AA Classic files…").color(MUTED));
                });
                return;
            }
            caption(ui, "On this PC");
            let no_recording = self.xfer.scan.as_ref().is_some_and(|s| s.recording.is_none());
            let mut folded = false;
            for (i, it) in items.iter().enumerate() {
                if i == 3 {
                    ui.add_space(8.0);
                    let who = if self.cfg.username.trim().is_empty() { String::new() } else { format!(" {}", self.cfg.username.trim()) };
                    caption(ui, &format!("On your account{who}"));
                    let _ = &host;
                }
                if no_recording && !it.ready && matches!(it.id, "keybinds" | "options" | "bars") {
                    if !folded {
                        folded = true;
                        egui::Frame::new().inner_margin(Margin { left: 14, right: 8, top: 2, bottom: 2 }).show(ui, |ui| {
                            ui.set_max_width(left_w - 30.0);
                            ui.add(egui::Label::new(RichText::new("Keybinds, game options and action bars come from a recording of an AA Classic session. Have one? Choose it on the right.").size(12.5).color(MUTED)).wrap());
                        });
                        ui.add_space(6.0);
                    }
                    continue;
                }
                let enabled = it.ready && (!it.server || server.is_ok()) && !locked;
                let on = self.xfer.ticks.contains(&it.id) && it.ready && (!it.server || server.is_ok());
                let grey = rgba(150, 156, 168, 255);
                let tag = if !it.ready {
                    match it.id {
                        "keybinds" | "options" | "bars" if self.xfer.scan.as_ref().is_some_and(|s| s.recording.is_none()) => ("NEEDS RECORDING", grey),
                        "gear" => ("PASTE LINK", grey),
                        "layout" if self.xfer.scan.as_ref().is_some_and(|s| s.source().is_some()) => ("PLAY ONCE FIRST", grey),
                        _ => ("NOT FOUND", grey),
                    }
                } else if it.server && server.is_err() {
                    ("SIGN IN FIRST", RED)
                } else if it.server {
                    ("ACCOUNT", BLUE)
                } else {
                    ("THIS PC", GREEN)
                };
                let (boxed, picked) = line(ui, left_w, it, on, enabled, self.xfer.selected == i, tag);
                if boxed {
                    if let Some(pos) = self.xfer.ticks.iter().position(|x| *x == it.id) {
                        self.xfer.ticks.remove(pos);
                    } else {
                        self.xfer.ticks.push(it.id);
                    }
                    if !self.xfer.touched.contains(&it.id) {
                        self.xfer.touched.push(it.id);
                    }
                    self.xfer.selected = i;
                }
                if picked {
                    self.xfer.selected = i;
                }
            }
        });

        // the preview, then the sources
        ui.scope_builder(UiBuilder::new().max_rect(right).layout(Layout::top_down(Align::Min)), |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let frame = egui::Frame::new().fill(rgba(6, 9, 14, 200)).stroke(Stroke::new(1.0, rgba(255, 255, 255, 20))).corner_radius(4).inner_margin(Margin::symmetric(14, 12));
            frame.show(ui, |ui| {
                ui.set_width(right.width() - 28.0);
                ui.set_min_height(150.0);
                if let Some((ok, head, lines)) = &self.xfer.result {
                    ui.label(RichText::new(head).font(bold_font(16.0)).color(if *ok { GREEN } else { RED }));
                    for l in lines {
                        ui.label(RichText::new(format!("•  {l}")).size(13.0).color(INK));
                    }
                    if *ok {
                        ui.label(RichText::new("Start the game to see it. Undo puts everything back.").size(12.5).color(MUTED));
                    }
                } else if self.xfer.working {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Moving…").color(MUTED));
                    });
                } else if let Some(it) = items.get(self.xfer.selected) {
                    ui.label(RichText::new(it.title).font(bold_font(16.0)).color(GOLD));
                    ui.label(RichText::new(&it.summary).size(13.5).color(INK));
                    for l in &it.lines {
                        ui.label(RichText::new(l).size(12.5).color(MUTED));
                    }
                    if it.server {
                        if let Err(e) = &server {
                            ui.label(RichText::new(*e).size(12.5).color(RED));
                        }
                    }
                }
            });
            ui.add_space(6.0);

            caption(ui, "DaruGear build");
            ui.horizontal(|ui| {
                let r = ui.add_enabled(!locked, TextEdit::singleline(&mut self.xfer.link).hint_text("https://darugear.com/?s=…").desired_width(right.width() - 92.0).margin(Margin::symmetric(8, 6)));
                if r.changed() {
                    self.check_link();
                }
                if ui.add_enabled(!locked, egui::Button::new("Paste")).on_hover_text("Takes a DaruGear link from the clipboard").clicked() {
                    if let Some(t) = transfer::clipboard_text() {
                        self.xfer.link = t.trim().to_string();
                        self.check_link();
                    }
                }
            });
            if self.xfer.fetching.is_some() {
                ui.label(RichText::new("Asking DaruGear…").size(12.0).color(MUTED));
            }

            caption(ui, "AA Classic recording (optional)");
            ui.horizontal(|ui| {
                let name = self.xfer.recording.as_ref().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "none found".into());
                let shown = elide(ui.painter(), &name, FontId::monospace(12.0), right.width() - 120.0);
                ui.label(RichText::new(shown).monospace().size(12.0).color(if self.xfer.recording.is_some() { INK } else { MUTED }));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add_enabled(!locked, egui::Button::new("Choose…")).on_hover_text("A recording made by the capture tool. Only keybinds, options and action bars are read from it.").clicked() {
                        if let Some(f) = rfd::FileDialog::new().set_title("A recording of an AA Classic session").add_filter("Recording", &["jsonl"]).pick_file() {
                            self.xfer.recording = Some(f);
                            self.rescan();
                        }
                    }
                });
            });

            if let Some(scan) = self.xfer.scan.as_mut() {
                if scan.sources.len() > 1 {
                    caption(ui, "Window layout from");
                    let labels: Vec<String> = scan.sources.iter().map(|s| s.label.clone()).collect();
                    let mut sel = scan.source;
                    egui::ComboBox::from_id_salt("xfer-source").width(right.width() - 4.0).selected_text(elide(ui.painter(), labels[sel].split(" (").next().unwrap_or(""), FontId::proportional(15.0), right.width() - 60.0)).show_ui(ui, |ui| {
                        for (i, l) in labels.iter().enumerate() {
                            ui.selectable_value(&mut sel, i, l);
                        }
                    });
                    scan.source = sel;
                }
            }

            if let Some(last) = transfer::last(&self.paths) {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let when = last.when;
                    let ago = SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs().saturating_sub(when)).unwrap_or(0);
                    let ago = if ago < 90 { "just now".to_string() } else if ago < 5400 { format!("{} min ago", ago / 60) } else if ago < 129600 { format!("{} h ago", ago / 3600) } else { format!("{} days ago", ago / 86400) };
                    ui.label(RichText::new(format!("Last transfer {ago}: {}", last.items.join(", "))).size(12.0).color(MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_enabled(!locked, egui::Button::new(RichText::new("Undo").color(GOLD))).on_hover_text("Puts back what the last transfer replaced, here and on the server").clicked() {
                            *act = Some(Action::UndoTransfer);
                        }
                    });
                });
            }
        });
    }

    /// The bottom bar's line while the screen is open: (dot, title, sub, button, action).
    pub(in crate::gui) fn transfer_status(&self) -> (Color32, String, String, &'static str, Option<Action>) {
        if self.xfer.working {
            return (GOLD, "Moving your setup".into(), "Writing into this client's profile and sending your account's part…".into(), "WAIT", None);
        }
        if self.child.is_some() {
            return (BLUE, "Close the game first".into(), "The client keeps its interface files open while it runs.".into(), "TRANSFER", None);
        }
        let items = self.xfer.items();
        let ticked: Vec<&transfer::Item> = items.iter().filter(|i| self.xfer.ticks.contains(&i.id)).collect();
        if self.xfer.scanning && items.is_empty() {
            return (GOLD, "Looking at AA Classic".into(), "Reading its settings, interface store and your newest recording…".into(), "TRANSFER", None);
        }
        if ticked.is_empty() {
            return (MUTED, "Nothing ticked".into(), "Tick what should move.".into(), "TRANSFER", None);
        }
        let here = ticked.iter().filter(|i| !i.server).count();
        let there = ticked.len() - here;
        let mut sub = vec![];
        if here > 0 {
            sub.push(format!("{here} into this client"));
        }
        if there > 0 {
            sub.push(format!("{there} onto your account, applied at your next login"));
        }
        (GREEN, format!("Ready to move {} thing{}", ticked.len(), if ticked.len() == 1 { "" } else { "s" }), sub.join(" · "), "TRANSFER", Some(Action::Transfer))
    }
}
