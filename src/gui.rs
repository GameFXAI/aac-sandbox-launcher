//! The launcher window.

use crate::cli::default_aac_dir;
use crate::core::*;
use eframe::egui;
use std::process::Child;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, SystemTime};

enum Msg {
    Line(String),
    SetupDone(Result<(), String>),
}

struct App {
    paths: Paths,
    cfg: Config,
    password: String,
    log: Vec<String>,
    busy: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    child: Option<(Child, SystemTime)>,
    state: String,
}

impl App {
    fn new() -> App {
        let paths = Paths::new();
        let mut cfg = Config::load(&paths);
        if cfg.aac_dir.is_none() {
            cfg.aac_dir = default_aac_dir();
        }
        let (tx, rx) = channel();
        let mut a = App { paths, cfg, password: String::new(), log: vec![], busy: false, tx, rx, child: None, state: String::new() };
        a.refresh();
        a
    }

    fn aac(&self) -> Result<AacInstall, String> {
        let d = self.cfg.aac_dir.clone().ok_or("choose your AA Classic folder")?;
        AacInstall::open(&d, &self.paths)
    }

    fn refresh(&mut self) {
        self.state = match self.aac() {
            Ok(a) => status(&self.paths, &a, self.cfg.mode).describe().to_string(),
            Err(e) => e,
        };
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
        let aac = match self.aac() {
            Ok(a) => a,
            Err(e) => return self.say(e),
        };
        if self.password.is_empty() {
            return self.say("enter your password for this server");
        }
        let l = Launch {
            username: self.cfg.username.trim().to_string(),
            token: token_from_password(&self.password),
            login_ip: self.cfg.login_ip.trim().to_string(),
            game_ip: self.cfg.game_ip.trim().to_string(),
            lang: self.cfg.lang.clone(),
        };
        let _ = self.cfg.save(&self.paths);
        match play(&self.paths, &aac, self.cfg.mode, &l) {
            Ok(c) => {
                self.say(format!("client started (pid {})", c.id()));
                self.child = Some((c, SystemTime::now()));
            }
            Err(e) => self.say(e),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Line(l) => self.say(l),
                Msg::SetupDone(r) => {
                    self.busy = false;
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
        if self.busy || self.child.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        egui::CentralPanel::default().show(root, |ui| {
            ui.heading("AAEmu sandbox launcher");
            ui.label("Starts your own AA Classic client against an AAEmu server, separated from AA Classic: own copy, own user folder, no reports to AA Classic, your AA Classic login never asked for.");
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("AA Classic folder:");
                let shown = self.cfg.aac_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(not chosen)".into());
                ui.monospace(shown);
                if ui.add_enabled(!self.busy, egui::Button::new("Choose...")).clicked() {
                    if let Some(d) = rfd::FileDialog::new().set_title("Your AA Classic folder (the one with binNext and game_pak)").pick_folder() {
                        self.cfg.aac_dir = Some(d);
                        self.refresh();
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Install mode:");
                let before = self.cfg.mode;
                ui.add_enabled_ui(!self.busy, |ui| {
                    ui.radio_value(&mut self.cfg.mode, Mode::Standard, "Standard (65 MB, reads AA Classic's game data)");
                    ui.radio_value(&mut self.cfg.mode, Mode::Full, "Full separation (copies ~45 GB)");
                });
                if before != self.cfg.mode {
                    self.refresh();
                }
            });
            ui.horizontal(|ui| {
                ui.label(format!("Copy: {}", self.state));
                let label = if self.state == State::Ready.describe() { "Refresh copy" } else { "Set up" };
                if ui.add_enabled(!self.busy && self.child.is_none(), egui::Button::new(label)).clicked() {
                    self.start_setup();
                }
                if self.busy {
                    ui.spinner();
                }
            });
            ui.separator();

            egui::Grid::new("login").num_columns(2).show(ui, |ui| {
                ui.label("Server:");
                if ui.text_edit_singleline(&mut self.cfg.login_ip).changed() {
                    self.cfg.game_ip = self.cfg.login_ip.clone();
                }
                ui.end_row();
                ui.label("Account (this server):");
                ui.text_edit_singleline(&mut self.cfg.username);
                ui.end_row();
                ui.label("Password (this server):");
                ui.add(egui::TextEdit::singleline(&mut self.password).password(true));
                ui.end_row();
            });
            ui.small("Never your AA Classic password. This server's accounts are separate.");
            let can_play = !self.busy && self.child.is_none() && self.state == State::Ready.describe();
            if ui.add_enabled(can_play, egui::Button::new("Play").min_size(egui::vec2(120.0, 32.0))).clicked() {
                self.start_play();
            }
            ui.separator();

            egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                for l in &self.log {
                    ui.monospace(l);
                }
            });
        });
    }
}

pub fn run() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([720.0, 520.0]).with_title("AAEmu sandbox launcher"),
        ..Default::default()
    };
    let _ = eframe::run_native("AAEmu sandbox launcher", options, Box::new(|_cc| Ok(Box::new(App::new()))));
}
