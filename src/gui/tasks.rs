//! The work the window hands to background threads: install, sign-in and play, sign-up, build packs, the server
//! check, launcher updates and the recorder. Each one starts a thread that answers with an `Event` (events.rs).
//! The transfer screen's own work (scan, DaruGear look-up, transfer, undo) is in screens/transfer.rs.

use super::events::Event;
use super::App;
use crate::core::*;
use crate::net::{self, Server};
use crate::{addons, install, recorder, signin, update, vault};
use eframe::egui;
use std::time::{Duration, SystemTime};

impl App {
    /// Install game: everything in one go on a worker thread; Stop sets `cancel`.
    pub(in crate::gui) fn start_install(&mut self) {
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
                let _ = t2.lock().unwrap().send(Event::Line(l));
            }, &move |f| {
                let _ = t3.lock().unwrap().send(Event::Progress(f));
            });
            let _ = tx.send(Event::Installed(r.map(|d| (d.version, d.build))));
        });
    }

    pub(in crate::gui) fn import_pack(&mut self) {
        let Some(file) = rfd::FileDialog::new().set_title("A build pack from a friend").pick_file() else { return };
        self.busy = true;
        let (paths, tx, server) = (self.paths.clone(), self.tx.clone(), Server::parse(&self.cfg.login_ip));
        std::thread::spawn(move || {
            let accepted = match &server {
                Ok(s @ Server::Api { .. }) => net::info(s).map(|i| i.build_ids).unwrap_or_default(),
                _ => vec![],
            };
            let r = vault::import(&paths, &file, &accepted).map(|b| format!("client build {} is in the vault ({} files)", &b.id[..12], b.files.len()));
            let _ = tx.send(Event::Pack(r));
        });
    }

    pub(in crate::gui) fn export_pack(&mut self) {
        let Some(file) = rfd::FileDialog::new().set_title("Save the build pack as").set_file_name("aac-client.aabuild").save_file() else { return };
        self.busy = true;
        let (paths, tx) = (self.paths.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let r = vault::add(&paths, &paths.bin()).and_then(|b| vault::export(&paths, &b.id, &file)).map(|b| format!("client build {} written to {}", &b.id[..12], file.display()));
            let _ = tx.send(Event::Pack(r));
        });
    }

    /// Asks the server whether it is up, off the window's thread.
    pub(in crate::gui) fn check_server(&mut self) {
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
            let _ = tx.send(Event::ServerUp(r));
        });
    }

    pub(in crate::gui) fn start_setup(&mut self) {
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
                let _ = t2.send(Event::Line(l));
            });
            let _ = tx.send(Event::SetupDone(r));
        });
    }

    /// The one button: makes the account (first start with an invite), makes or refreshes our copy of the game,
    /// signs in and starts the game, each step going on to the next.
    pub(in crate::gui) fn press_play(&mut self) {
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

    pub(in crate::gui) fn start_play(&mut self) {
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
                        let _ = tx.send(Event::Line(line));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = tx.send(Event::Ticket(Err(format!("server addons: {e}"))));
                        return;
                    }
                }
                match crate::art::before_play(&paths, &server) {
                    Ok(Some(line)) => { let _ = tx.send(Event::Line(line)); }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = tx.send(Event::Ticket(Err(e)));
                        return;
                    }
                }
                let _ = tx.send(Event::Ticket(net::login(&server, &username, secret.cred(), remember).map(|t| (t.username, t.ticket, t.session))));
            });
            return;
        }
        let token = token_from_password(&self.password);
        self.launch(aac, username, token);
    }

    pub(in crate::gui) fn sign_out(&mut self) {
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
        // the next account must not see this one's replays
        crate::library::forget_cached(&self.paths);
        self.lib = Default::default();
        self.say("Signed out on this PC");
    }

    pub(in crate::gui) fn start_signup(&mut self) {
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
            let _ = tx.send(Event::SignedUp(net::signup(&server, &username, &password, &invite)));
        });
    }

    pub(in crate::gui) fn launch(&mut self, aac: Option<AacInstall>, username: String, token: String) {
        let address = match Server::parse(&self.cfg.login_ip).and_then(|s| s.client_address()) {
            Ok(a) => a,
            Err(e) => return self.say(e),
        };
        let l = Launch { username, token, login_ip: address.clone(), game_ip: address, lang: self.cfg.lang.clone(), login_port: self.cfg.login_port };
        // a Watch from the Library: the recording for the game's replay player, written after the server's addons
        // went in (they replace its folder); a plain Play leaves none from before
        let watch = self.lib.take_pending();
        match crate::library::set_pending_watch(&self.paths, watch) {
            Ok(()) if watch.is_some() => self.say(format!("the game opens replay {} once it is in the world", watch.unwrap())),
            Ok(()) => {}
            Err(e) if watch.is_some() => {
                self.lib.note = Some(Err(format!("The game starts, but the replay cannot be opened by itself: {e}. Open Replays in the game.")));
                self.say(format!("replay: {e}"));
            }
            Err(e) => self.say(format!("replay: an old request could not be removed: {e}")),
        }
        match play(&self.paths, aac.as_ref(), self.cfg.mode, &l) {
            Ok(c) => {
                self.say(format!("client started (pid {})", c.id()));
                self.child = Some((c, SystemTime::now()));
            }
            Err(e) => self.warn(e),
        }
    }

    /// Downloads the launcher the manifest offers; Event::Updated says when it is in place.
    pub(in crate::gui) fn update_launcher(&mut self) {
        if let Some(r) = self.update.clone() {
            self.busy = true;
            self.say(format!("downloading launcher {} ...", r.version));
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Event::Updated(update::apply(&r)));
            });
        }
    }

    /// Recording AA Classic: set up (download, start, start with Windows) or turn on again.
    pub(in crate::gui) fn rec_on(&mut self) {
        self.rec_busy = Some("Getting the recorder…".into());
        self.rec_note = None;
        let (paths, tx) = (self.paths.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let say = |l: String| {
                let _ = tx.send(Event::RecSay(l));
            };
            let r = recorder::turn_on(&paths, &say).map(|()| "Recording is on, and starts with Windows from now on.".to_string());
            let ok = r.is_ok();
            // the one admin prompt, right after the first setup (never again after a No: the card's button)
            let admin = if ok { crate::admin::after_setup(&paths, false, &say) } else { None };
            let _ = tx.send(Event::RecDone(r));
            if let Some(a) = admin {
                let _ = tx.send(Event::RecAdmin(a));
            }
        });
    }

    /// The one admin prompt, again after a No (admin.rs).
    pub(in crate::gui) fn rec_admin(&mut self) {
        self.rec_busy = Some(crate::admin::WHY.into());
        self.rec_note = None;
        let (paths, tx) = (self.paths.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let say = |l: String| {
                let _ = tx.send(Event::RecSay(l));
            };
            let a = crate::admin::after_setup(&paths, true, &say).unwrap_or_else(|| Ok("Nothing to set up: Windows does not ask when AA Classic starts.".into()));
            let _ = tx.send(Event::RecAdmin(a));
        });
    }

    /// Reads the recorder's status every two seconds, and sends new fights when there is a sign-in.
    pub(in crate::gui) fn poll_recorder(&mut self, ctx: &egui::Context) {
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
                    let _ = tx.send(Event::RecSent(r));
                });
            }
        }
    }
}
