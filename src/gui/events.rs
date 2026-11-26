//! What comes back to the window: every background thread sends an `Event` over the app's channel, and `poll`
//! takes them in at the start of each frame. (The other direction is `Action`: what a screen asks for, run by
//! `App::run`, which starts the background work in tasks.rs or a screen's own file.)

use super::screens::transfer::TransferEvent;
use super::{App, Page};
use crate::core::*;
use crate::{net, signin, update};
use eframe::egui::{self, ViewportCommand};
use std::time::Duration;

/// Background thread -> window.
pub enum Event {
    Line(String),
    SetupDone(Result<(), String>),
    /// the account API's answer to a login: account name, launch ticket and (with "remember me") the sign-in
    Ticket(Result<(String, String, Option<String>), String>),
    /// "Report a problem" was sent (its id) or not
    Reported(Result<String, String>),
    SignedUp(Result<String, String>),
    /// the account API's answer to a sign-in without a game: account name and (with "keep me signed in") the sign-in
    SignedIn(Result<(String, Option<String>), String>),
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
    Transfer(TransferEvent),
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
    /// the Library's list from the server
    Library(Result<net::Library, String>),
    /// a Library delete: the recording's number, and what the server said
    ReplayDeleted(i32, Result<(), String>),
    /// sharing, friends and the guild (screens/shares.rs)
    Shares(super::screens::shares::ShareEvent),
    /// a recording's real picture came (None: there is none, or it could not be read)
    Picture(super::pictures::Thumb, Option<egui::ColorImage>),
}

impl App {
    /// Keeps the server's sign-in sealed on this PC (with "keep me signed in"); the password is forgotten then.
    fn keep_sign_in(&mut self, name: &str, session: Option<String>) {
        let Some(session) = session else { return };
        let s = signin::Saved { server: self.cfg.login_ip.trim().to_string(), username: name.to_string(), session };
        match signin::save(&self.paths, &s) {
            Ok(()) => {
                self.say(format!("Signed in as {name}: remembered on this PC (the password is not kept)"));
                self.saved = Some(s);
                self.password.clear();
            }
            Err(e) => self.say(e),
        }
    }

    pub(in crate::gui) fn poll(&mut self, ctx: &egui::Context) {
        self.poll_recorder(ctx);
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Event::Line(l) => {
                    if let Some(v) = l.trim().strip_suffix('%').and_then(|n| n.trim().parse::<f32>().ok()) {
                        self.progress = Some(v / 100.0);
                    }
                    self.say(l)
                }
                Event::Ticket(r) => {
                    self.asking = false;
                    match (r, self.aac_if_needed()) {
                        (Ok((name, ticket, session)), Ok(aac)) => {
                            self.cfg.username = name.clone();
                            let _ = self.cfg.save(&self.paths);
                            self.keep_sign_in(&name, session);
                            self.signed = true;
                            // the first start: offer AA Classic's settings before the game starts (the ticket is
                            // left to run out; the next press asks for a fresh one)
                            if !self.cfg.transfer_offered && self.offer_ready() && self.secret().is_some() {
                                self.offer = true;
                            } else {
                                self.launch(aac, name, ticket);
                            }
                        }
                        (Err(e), _) | (_, Err(e)) => {
                            // a Jump into ArcheAge waits for no game now
                            if let Some((n, _)) = self.lib.pending.take() {
                                self.lib.failed = Some((n, format!("Sign-in failed: {e}")));
                            }
                            if e == net::explain("BadSession") {
                                // the remembered sign-in ran out or was ended: the password once more
                                self.saved = None;
                                signin::forget(&self.paths);
                            }
                            self.warn(format!("Sign-in failed: {e}"))
                        }
                    }
                }
                Event::SignedIn(r) => {
                    self.asking = false;
                    match r {
                        Ok((name, session)) => {
                            self.cfg.username = name.clone();
                            let _ = self.cfg.save(&self.paths);
                            self.keep_sign_in(&name, session);
                            self.signed = true;
                            self.hint = None;
                            self.say(format!("Signed in as {name}"));
                            self.lib = Default::default();
                            self.lib.data = crate::library::load_cached(&self.paths, &self.cfg.login_ip, &name);
                            self.fetch_library();
                            if self.page.section() == crate::gui::Section::Account {
                                self.page = Page::Captures;
                            }
                        }
                        Err(e) => {
                            if e == net::explain("BadSession") {
                                self.saved = None;
                                signin::forget(&self.paths);
                            }
                            self.warn(format!("Sign-in failed: {e}"))
                        }
                    }
                }
                Event::Reported(r) => {
                    if let Some(f) = self.report.as_mut() {
                        f.sending = false;
                        f.done = Some(r.clone());
                    }
                    self.say(match r {
                        Ok(l) | Err(l) => format!("report: {l}"),
                    });
                }
                Event::SignedUp(r) => {
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
                            // one press: the account, then signed in to it
                            self.sign_in();
                        }
                        // the invite of a second install was used up by the first: the account is there, sign in
                        Err(e) if self.baked_invite.is_some() && (e == net::explain("BadInvite") || e == net::explain("UsernameTaken")) => {
                            self.baked_invite = None;
                            signin::clear_invite(&self.paths);
                            self.signup = false;
                            self.say(format!("{e}: signing in to the account instead"));
                            self.sign_in();
                        }
                        Err(e) => self.warn(format!("Could not create the account: {e}")),
                    }
                }
                Event::Update(r) => {
                    self.say(format!("launcher {} is available (this is {})", r.version, update::VERSION));
                    self.update = Some(r);
                }
                Event::Updated(r) => {
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
                Event::Progress(f) => self.progress = Some(f),
                Event::Transfer(m) => self.transfer_event(m),
                Event::ServerUp(r) => self.server_up = Some(r),
                Event::RecSay(l) => {
                    self.say(format!("recorder: {l}"));
                    self.rec_busy = Some(l);
                }
                Event::RecDone(r) => {
                    self.rec_busy = None;
                    self.rec_read = None;
                    match &r {
                        Ok(l) => self.say(format!("recorder: {l}")),
                        Err(e) => self.say(format!("recorder: {e}")),
                    }
                    self.rec_note = Some(r.map_err(|e| format!("Recording could not be set up: {e}")));
                }
                Event::RecAdmin(r) => {
                    self.rec_busy = None;
                    self.rec_read = None;
                    match &r {
                        Ok(l) | Err(l) => self.say(format!("recorder: {l}")),
                    }
                    // a No is not an error: recording works, Windows asks at each AA Classic start
                    self.rec_note = Some(r);
                }
                Event::Library(r) => self.library_came(r),
                Event::ReplayDeleted(n, r) => self.replay_deleted(n, r),
                Event::Shares(e) => self.shares_came(e),
                Event::Picture(key, img) => self.thumbs.arrived(ctx, key, img),
                Event::RecSent(r) => {
                    self.rec_sending = false;
                    self.rec_read = None;
                    match r {
                        Ok(0) => {}
                        Ok(n) => {
                            // the Library's list is old now: again at once on its screen, else on the next visit
                            self.lib.fresh = false;
                            if self.page == Page::Captures {
                                self.fetch_library();
                            }
                            let l = format!("{n} new fight{} sent to your replays: watch {} in Replays.", if n == 1 { "" } else { "s" }, if n == 1 { "it" } else { "them" });
                            self.say(format!("recorder: {l}"));
                            self.rec_note = Some(Ok(l));
                        }
                        Err(e) => {
                            self.say(format!("recorder: sending fights: {e}"));
                            self.rec_note = Some(Err(format!("Your fights could not be sent yet: {e}. They are tried again in a minute.")));
                        }
                    }
                }
                Event::Installed(r) => {
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
                Event::Pack(r) => {
                    self.busy = false;
                    self.say(match r {
                        Ok(l) => l,
                        Err(e) => format!("build pack: {e}"),
                    });
                    self.refresh();
                }
                Event::SetupDone(r) => {
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
}
