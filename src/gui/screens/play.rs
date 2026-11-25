//! The Play screen: the pitch on the left, the sign-in card (or the first start's offer) on the right, and the
//! recording card under the pitch.

use crate::gui::theme::*;
use crate::gui::widgets::{field, tick_row};
use crate::gui::{Action, App};
use crate::net::Server;
use crate::{recorder, transfer};
use eframe::egui::{self, Align, Align2, Color32, Layout, Rect, RichText, Sense, Stroke, UiBuilder, pos2, vec2};

impl App {
    pub(in crate::gui) fn play_tab(&mut self, ui: &mut egui::Ui, body: Rect, act: &mut Option<Action>) {
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
    pub(in crate::gui) fn recording_card(&mut self, ui: &mut egui::Ui, body: Rect, act: &mut Option<Action>) {
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
    pub(in crate::gui) fn offer_card(&mut self, ui: &mut egui::Ui, body: Rect, act: &mut Option<Action>) {
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
}
