//! The "Report a problem" dialog: the form over everything else, and sending it (or saving it as a file).

use super::events::Event;
use super::theme::*;
use super::{Action, App};
use crate::net::{self, Server};
use crate::update;
use eframe::egui::{self, Align, Align2, Layout, Rect, RichText, Sense, TextEdit, UiBuilder, vec2};

/// The "Report a problem" dialog.
#[derive(Default)]
pub struct ReportForm {
    pub text: String,
    pub logs: bool,
    pub sending: bool,
    /// the outcome: Ok(what to tell the player) or Err(what went wrong)
    pub done: Option<Result<String, String>>,
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

impl App {
    pub(in crate::gui) fn send_report(&mut self) {
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
            let _ = tx.send(Event::Reported(r));
        });
    }

    /// The "Report a problem" dialog, over everything else.
    pub(in crate::gui) fn report_dialog(&mut self, ctx: &egui::Context, act: &mut Option<Action>) {
        let Some(form) = self.report.as_mut() else { return };
        let mut close = false;
        egui::Area::new(egui::Id::NULL.with("report")).order(egui::Order::Foreground).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            let r = Rect::from_center_size(ctx.content_rect().center(), vec2(520.0, 360.0));
            ui.painter().rect_filled(ctx.content_rect(), 0.0, rgba(0, 0, 0, 150));
            // the dimmed backdrop takes the clicks, so nothing behind the dialog can be pressed
            let _ = ui.interact(ctx.content_rect(), ui.scope_id().with("backdrop"), Sense::click_and_drag());
            card(ui.painter(), r);
            ui.scope_builder(UiBuilder::new().max_rect(r.shrink2(vec2(28.0, 22.0))).layout(Layout::top_down(Align::Min)), |ui| {
                ui.label(RichText::new("Report a problem").font(title_font(26.0)).color(INK));
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
                    ui.spacing_mut().item_spacing.x = 10.0;
                    if super::widgets::primary_button(ui, "Send", None, false, 96.0, 36.0, can).clicked() {
                        *act = Some(Action::SendReport);
                    }
                    if !form.sending && super::widgets::outline_button(ui, if sent { "Close" } else { "Cancel" }, None, false, 96.0, 36.0, false).clicked() {
                        close = true;
                    }
                });
            });
        });
        if close {
            self.report = None;
        }
    }
}
