//! Signed out: the welcome page (a moment's picture with what the app is, and the sign-in card). Also the record
//! button's menu (recording AA Classic) and the first start's offer to bring AA Classic's settings.

use crate::gui::theme::*;
use crate::gui::widgets::{EDGE, field, outline_button, primary_button};
use crate::gui::{Action, App};
use crate::net::Server;
use crate::{recorder, transfer};
use eframe::egui::{self, Align, Align2, Color32, Layout, Rect, RichText, Sense, Stroke, UiBuilder, pos2, vec2};

impl App {
    pub(in crate::gui) fn welcome(&mut self, ui: &mut egui::Ui, body: Rect, act: &mut Option<Action>) {
        let p = ui.painter().clone();
        let pad = 24.0;
        let card_w = 400.0;
        let pic = Rect::from_min_max(body.min + vec2(pad, pad), pos2(body.right() - card_w - pad * 2.0, body.bottom() - pad));

        // the picture, with what the app is over its lower part
        crate::gui::pictures::draw(&p, self.pics.welcome(), pic, 16, Color32::from_gray(200));
        let fade = Rect::from_min_max(pos2(pic.left(), pic.top() + pic.height() * 0.3), pic.max);
        vgrad(&p, fade, bg_a(0), bg_a(242));
        p.rect_stroke(pic, 16.0, Stroke::new(1.0, rgba(255, 255, 255, 22)), egui::StrokeKind::Inside);
        // laid out first, then set on the picture's lower edge, however many lines they take
        let x = pic.left() + 40.0;
        let wrap = (pic.width() - 80.0).min(600.0);
        let head = p.layout("Relive every moment in the world itself".into(), title_font(34.0), INK, wrap);
        let sub = p.layout(
            "Your fights are recorded as you play. Share them with friends and your guild, talk them over, and jump back into any moment in ArcheAge to see everything unfold again.".into(),
            text_font(15.0),
            rgba(222, 218, 210, 255),
            wrap,
        );
        let mut y = pic.bottom() - 40.0 - sub.size().y - 14.0 - head.size().y - 28.0;
        caps(&p, pos2(x, y), "AA Replay", GOLD);
        y += 28.0;
        let hh = head.size().y;
        p.galley(pos2(x, y), head, INK);
        y += hh + 14.0;
        p.galley(pos2(x, y), sub, INK);

        // the server, top left of the picture
        let (dot, text) = match &self.server_up {
            None => (MUTED, "Checking the server…".to_string()),
            Some(Ok(name)) => (GREEN, format!("{name} is online")),
            Some(Err(_)) => (RED, "The server does not answer".to_string()),
        };
        let chip_g = p.layout_no_wrap(text, medium_font(13.0), INK);
        let chip = Rect::from_min_size(pic.min + vec2(20.0, 20.0), vec2(chip_g.size().x + 38.0, 30.0));
        p.rect_filled(chip, 15.0, rgba(10, 10, 12, 200));
        p.rect_stroke(chip, 15.0, Stroke::new(1.0, rgba(255, 255, 255, 30)), egui::StrokeKind::Inside);
        p.circle_filled(pos2(chip.left() + 17.0, chip.center().y), 4.0, dot);
        p.galley(pos2(chip.left() + 28.0, chip.center().y - chip_g.size().y / 2.0), chip_g, INK);
        if let Some(Err(e)) = &self.server_up {
            ui.interact(chip, ui.scope_id().with("server-why"), Sense::hover()).on_hover_text(e.as_str());
        }

        // the card
        let tall = if self.signup && self.baked_invite.is_none() { 590.0 } else { 512.0 };
        let cardr = Rect::from_min_size(pos2(body.right() - card_w - pad, body.center().y - tall / 2.0), vec2(card_w, tall));
        p.rect_filled(cardr, 16.0, Color32::from_rgb(17, 17, 20));
        p.rect_stroke(cardr, 16.0, Stroke::new(1.0, LINE), egui::StrokeKind::Inside);
        let mut enter = false;
        let mut press = false;
        ui.scope_builder(UiBuilder::new().max_rect(cardr.shrink2(vec2(32.0, 30.0))).layout(Layout::top_down(Align::Min)), |ui| {
            let (title, sub) = if self.signup && self.baked_invite.is_some() {
                ("Create your account", "Your invite is already in. Pick a name and a password.")
            } else if self.signup {
                ("Create your account", "Pick a name and a password, and enter the invite code you were given.")
            } else {
                ("Welcome back", "Sign in with your AA Replay account, not your AA Classic one.")
            };
            ui.label(RichText::new(title).font(title_font(26.0)).color(INK));
            ui.add(egui::Label::new(RichText::new(sub).font(text_font(13.5)).color(MUTED)).wrap());
            ui.add_space(18.0);
            field(ui, "Name", &mut self.cfg.username, false, "Your account name");
            let pw = field(ui, "Password", &mut self.password, true, "Your password for this server");
            if pw.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                enter = true;
            }
            if self.signup && self.baked_invite.is_none() {
                let inv = field(ui, "Invite code", &mut self.invite, false, "From the server's owner");
                if inv.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    enter = true;
                }
            }
            ui.checkbox(&mut self.remember, RichText::new("Keep me signed in on this PC").font(text_font(13.5)).color(INK))
                .on_hover_text("Keeps a sign-in from the server, sealed for your Windows user. Your password is never stored.");
            ui.add_space(12.0);
            let label = if self.signup { "Create account" } else { "Sign in" };
            if primary_button(ui, label, None, false, ui.available_width(), 42.0, !self.asking).clicked() {
                press = true;
            }
            ui.add_space(8.0);
            if self.asking {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0).color(GOLD));
                    ui.label(RichText::new("Asking the server…").font(text_font(13.0)).color(MUTED));
                });
            } else if let Some(h) = &self.hint {
                ui.add(egui::Label::new(RichText::new(h.as_str()).font(text_font(13.0)).color(RED)).wrap());
            } else {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    icon(ui.painter(), r.center(), 15.0, Icon::Warning, AMBER);
                    ui.label(RichText::new("Never type your AA Classic password here.").font(text_font(13.0)).color(AMBER));
                });
            }
            ui.add_space(6.0);
            // accounts are made on a server with an account API (a link), not on a bare address
            if matches!(Server::parse(&self.cfg.login_ip), Ok(Server::Api { .. })) {
                let label = if self.signup { "I already have an account" } else { "Have an invite code? Create an account" };
                if ui.add(egui::Label::new(RichText::new(label).font(medium_font(13.5)).color(GOLD_HI)).sense(Sense::click())).clicked() {
                    self.signup = !self.signup;
                    self.hint = None;
                }
            } else {
                self.signup = false;
            }
        });
        if (enter || press) && !self.asking {
            *act = Some(if self.signup { Action::SignUp } else { Action::SignIn });
        }
    }

    /// The record button's state: lit while recording, and the word in its chip.
    pub(in crate::gui) fn record_chip(&self) -> (bool, &'static str, Color32) {
        if self.rec_busy.is_some() {
            return (false, "Setting up", AMBER);
        }
        if !self.rec.set_up {
            return (false, "Set up", MUTED);
        }
        match self.rec.status.state.as_str() {
            "recording" => (true, "Live", RED),
            "ready" => (false, "On", GREEN),
            "starting" => (false, "Starting", AMBER),
            "problem" => (false, "Problem", AMBER),
            _ => (false, "Off", MUTED),
        }
    }

    /// Recording AA Classic, in the record button's menu: what it is doing, what is wrong with its fix, and its
    /// controls. Never a silent half-state.
    pub(in crate::gui) fn record_menu(&mut self, ui: &mut egui::Ui, act: &mut Option<Action>) {
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
            lines.push(("Play AA Classic as usual: your fights become captures here, hit by hit.".into(), INK));
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
        if let Some(n) = &self.rec_note {
            let at = lines.len().min(1);
            match n {
                Ok(t) => lines.insert(at, (t.clone(), GREEN)),
                Err(t) => lines.insert(at, (t.clone(), RED)),
            }
        }
        egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal(|ui| {
                let (d, _) = ui.allocate_exact_size(vec2(12.0, 22.0), Sense::hover());
                ui.painter().circle_filled(d.center(), 5.0, dot);
                ui.label(RichText::new(head).font(bold_font(16.0)).color(INK));
            });
            for (t, c) in lines.iter().take(4) {
                ui.add(egui::Label::new(RichText::new(t.as_str()).font(text_font(13.0)).color(*c)).wrap());
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if self.rec_busy.is_some() {
                    ui.add(egui::Spinner::new().color(GOLD));
                } else if !rec.set_up {
                    if primary_button(ui, "Set up recording", None, false, 0.0, 34.0, true)
                        .on_hover_text("Downloads the recorder, starts it, and starts it with Windows. Windows then asks once for admin rights, because AA Classic runs as admin: answer Yes, and it never asks again.")
                        .clicked()
                    {
                        *act = Some(Action::RecOn);
                    }
                } else if st.running() {
                    if matches!(rec.admin, crate::admin::State::NotSetUp | crate::admin::State::Declined)
                        && primary_button(ui, "Set up admin once", None, false, 0.0, 34.0, true).on_hover_text(crate::admin::WHY).clicked()
                    {
                        *act = Some(Action::RecAdmin);
                    }
                    if outline_button(ui, "Turn off", None, false, 0.0, 34.0, false).on_hover_text("Stops recording and no longer starts it with Windows. What was recorded is kept.").clicked() {
                        *act = Some(Action::RecOff);
                    }
                } else if primary_button(ui, "Turn on", None, false, 0.0, 34.0, true).clicked() {
                    *act = Some(Action::RecOn);
                }
                if rec.set_up && outline_button(ui, "Recordings folder", Some(Icon::Folder), false, 0.0, 34.0, false).clicked() {
                    let _ = std::process::Command::new("explorer").arg(recorder::data_dir(&self.paths)).spawn();
                }
            });
        });
    }

    /// The first start's one-time offer, over everything: AA Classic's settings into this client, then the game.
    pub(in crate::gui) fn offer_dialog(&mut self, ctx: &egui::Context, act: &mut Option<Action>) {
        let ready: Vec<transfer::Item> = self.xfer.items().into_iter().filter(|i| i.ready).collect();
        egui::Area::new(egui::Id::NULL.with("offer")).order(egui::Order::Foreground).fixed_pos(ctx.content_rect().min).show(ctx, |ui| {
            let screen = ctx.content_rect();
            ui.painter().rect_filled(screen, 0.0, rgba(0, 0, 0, 160));
            let _ = ui.interact(screen, ui.scope_id().with("backdrop"), Sense::click_and_drag());
            let r = Rect::from_center_size(screen.center(), vec2(460.0, 214.0 + ready.len() as f32 * 50.0));
            ui.painter().rect_filled(r, 16.0, Color32::from_rgb(19, 19, 23));
            ui.painter().rect_stroke(r, 16.0, Stroke::new(1.0, EDGE), egui::StrokeKind::Inside);
            ui.scope_builder(UiBuilder::new().max_rect(r.shrink2(vec2(30.0, 26.0))).layout(Layout::top_down(Align::Min)), |ui| {
                ui.label(RichText::new("Bring your AA Classic settings?").font(title_font(23.0)).color(INK));
                ui.add(egui::Label::new(RichText::new("Once, before your first game. AA Classic's files are only read, and Settings can undo it.").font(text_font(13.5)).color(MUTED)).wrap());
                ui.add_space(10.0);
                for it in &ready {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                        icon(ui.painter(), rect.center(), 18.0, Icon::CheckCircle, GREEN);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(RichText::new(it.title).font(bold_font(14.5)).color(INK));
                            ui.add(egui::Label::new(RichText::new(&it.summary).font(text_font(12.5)).color(MUTED)).truncate());
                        });
                    });
                    ui.add_space(4.0);
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    if primary_button(ui, "Bring them and play", None, false, 0.0, 38.0, true).clicked() {
                        *act = Some(Action::BringSettings);
                    }
                    if outline_button(ui, "Not now, just play", None, false, 0.0, 38.0, false).clicked() {
                        *act = Some(Action::SkipOffer);
                    }
                });
                ui.add_space(6.0);
                if ui.add(egui::Label::new(RichText::new("Choose what to bring…").font(medium_font(13.0)).color(GOLD_HI)).sense(Sense::click())).clicked() {
                    *act = Some(Action::OpenTransfer);
                }
            });
        });
        let _ = Align2::CENTER_CENTER;
    }
}
