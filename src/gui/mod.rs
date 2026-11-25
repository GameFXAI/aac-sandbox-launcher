//! The launcher window: a game-launcher style screen with a backdrop painted in code.
//! It deliberately uses no AA Classic logo or artwork, so it never passes for the official launcher.
//!
//! How it fits together:
//! - `App` (here) holds everything between frames; `Tab` says which screen is up.
//! - Screens (screens/*.rs) draw into the body and hand back what the player pressed as an `Action`.
//! - `App::run` (here) carries out an `Action`; work that takes time goes to a thread (tasks.rs, or the screen's
//!   own file), which answers with an `Event` over the channel.
//! - `App::poll` (events.rs) takes the `Event`s in at the start of each frame.
//! - theme.rs is the look (colours, fonts, painting), widgets.rs the launcher's own widgets.

use crate::core::*;
use crate::net;
use crate::{find, library, recorder, settings, signin, transfer, update};

mod bottom_bar;
mod events;
mod pictures;
mod report;
mod screens;
mod tasks;
mod theme;
mod widgets;

use eframe::egui::{self, Align, Align2, Layout, Rect, RichText, Sense, Shape, Stroke, UiBuilder, ViewportCommand, pos2, vec2};
use events::Event;
use report::ReportForm;
use screens::library::Lib;
use screens::transfer::Xfer;
use std::process::Child;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::SystemTime;
use theme::*;
use widgets::{WinBtn, gear_button, nav_tab, win_button};

/// Is there an AA Classic profile on this PC to bring settings from?
fn transfer_docs_present() -> bool {
    transfer::real_docs().is_some()
}

/// The screen in the body. Play and Replays are in the top bar; the others are behind the gear (Play returns home).
#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Play,
    /// the account's recordings (screens/library.rs)
    Library,
    Transfer,
    Settings,
    Log,
}

/// Window -> work: what the player pressed, returned by a screen and carried out by `App::run` once per frame.
/// Work that takes time starts a thread there, which answers with an `Event` (events.rs).
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
    /// the Library: the list again from the server
    RefreshLibrary,
    /// the Library: delete this recording (its stable number), after the card's "Delete this replay?"
    DeleteReplay(i32),
    /// the Library: start the game and open this recording in its replay player
    WatchReplay(i32),
    /// the server's upgrade page (the only thing a real checkout will change is where it points)
    Upgrade,
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
    /// background threads send their `Event`s here; `poll` reads `rx` each frame
    tx: Sender<Event>,
    rx: Receiver<Event>,
    child: Option<(Child, SystemTime)>,
    state: Result<State, String>,
    tab: Tab,
    confirm: Option<(Action, SystemTime)>,
    xfer: Xfer,
    /// what went wrong with the last thing the player pressed: shown in the bottom bar until the next press
    hint: Option<String>,
    /// integrity.rs: this exe is not the build that was installed here (kept for the whole run)
    modified: Option<&'static str>,
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
    /// the replay Library (screens/library.rs)
    lib: Lib,
    /// the paintings behind the screens and on the cards (pictures.rs)
    pics: pictures::Pictures,
}

impl App {
    fn new(cc: &eframe::CreationContext) -> App {
        install_fonts(&cc.egui_ctx);
        install_style(&cc.egui_ctx);
        let pics = pictures::Pictures::load(&cc.egui_ctx);
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
        let modified = crate::integrity::check(&paths);
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
                Ok("library") => Tab::Library,
                _ => Tab::Play,
            },
            confirm: None,
            xfer: Xfer::default(),
            hint: None,
            modified,
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
            lib: Lib::default(),
            pics,
        };
        if let Some(s) = &a.saved {
            a.cfg.username = s.username.clone();
            // the last list at once (the plan badge too), and a fresh one off the window's thread
            a.lib.data = library::load_cached(&a.paths, &a.cfg.login_ip, &s.username);
            a.fetch_library();
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
                    let _ = tx.send(Event::Update(r));
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

    /// The account name and what proves it: the remembered sign-in, else the password typed now.
    pub(in crate::gui) fn secret(&self) -> Option<(String, Secret)> {
        if let Some(s) = &self.saved {
            return Some((s.username.clone(), Secret::Session(s.session.clone())));
        }
        let u = self.cfg.username.trim();
        (!u.is_empty() && !self.password.is_empty()).then(|| (u.to_string(), Secret::Password(self.password.clone())))
    }

    /// Is there an AA Classic profile on this PC to bring settings from? (Only then is the first start's offer made.)
    fn offer_ready(&self) -> bool {
        self.xfer.items().iter().any(|i| i.ready)
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
            Action::UpdateLauncher => self.update_launcher(),
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
            Action::RecOn => self.rec_on(),
            Action::RecAdmin => self.rec_admin(),
            Action::RecOff => {
                self.rec_note = Some(recorder::turn_off(&self.paths).map(|()| "Recording is off. What it recorded is kept.".to_string()));
                self.rec_read = None;
            }
            Action::RefreshLibrary => {
                self.lib.note = None;
                self.fetch_library();
            }
            Action::DeleteReplay(n) => self.delete_replay(n),
            Action::WatchReplay(n) => self.watch_replay(n),
            Action::Upgrade => {
                let url = self.lib.data.as_ref().map(|l| l.upgrade_url.clone()).unwrap_or_default();
                if let Err(e) = screens::library::open_upgrade(&url) {
                    self.warn(e);
                }
            }
        }
    }

    // --- screen parts -------------------------------------------------------------------------------------------

    fn top_bar(&mut self, ui: &mut egui::Ui, rect: Rect, act: &mut Option<Action>) {
        let p = ui.painter().clone();
        vgrad(&p, rect, rgba(20, 15, 11, 255), rgba(10, 8, 6, 255));
        hgrad(&p, Rect::from_min_max(pos2(rect.left(), rect.bottom() - 1.0), pos2(rect.center().x, rect.bottom())), gold_a(10), gold_a(110));
        hgrad(&p, Rect::from_min_max(pos2(rect.center().x, rect.bottom() - 1.0), rect.right_bottom()), gold_a(110), gold_a(10));
        let drag = ui.interact(rect, ui.scope_id().with("titlebar"), Sense::click_and_drag());
        if drag.drag_started_by(egui::PointerButton::Primary) {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }

        // emblem: a gold diamond with a smaller one inside
        let e = pos2(rect.left() + 34.0, rect.center().y);
        let dia = |r: f32| vec![e + vec2(0.0, -r), e + vec2(r, 0.0), e + vec2(0.0, r), e + vec2(-r, 0.0)];
        p.add(Shape::convex_polygon(dia(14.0), gold_a(24), Stroke::new(1.5, GOLD)));
        p.add(Shape::convex_polygon(dia(6.5), GOLD_DEEP, Stroke::new(1.0, GOLD_HI)));
        let aa = p.text(pos2(rect.left() + 58.0, rect.center().y), Align2::LEFT_CENTER, "AA", title_font(23.0), INK);
        let sb = gold_title(&p, pos2(aa.right() + 7.0, rect.center().y), Align2::LEFT_CENTER, "Replay", title_font(23.0));

        let tabs = Rect::from_min_max(pos2(sb.right() + 40.0, rect.top()), rect.max);
        ui.scope_builder(UiBuilder::new().max_rect(tabs).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            // Play and Replays; everything else is behind the gear, and Play brings you back
            if nav_tab(ui, "Play", self.tab == Tab::Play).clicked() {
                self.tab = Tab::Play;
            }
            if nav_tab(ui, "Replays", self.tab == Tab::Library).clicked() {
                self.tab = Tab::Library;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if win_button(ui, WinBtn::Close).clicked() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                }
                if win_button(ui, WinBtn::Minimize).clicked() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
                }
                let open = !matches!(self.tab, Tab::Play | Tab::Library);
                if gear_button(ui, open).on_hover_text(if open { "Back to Play" } else { "Settings" }).clicked() {
                    self.tab = if open { Tab::Play } else { Tab::Settings };
                }
                ui.add_space(8.0);
                if let Some(l) = &self.lib.data {
                    if plan_badge(ui, l).clicked() {
                        self.tab = Tab::Library;
                    }
                    ui.add_space(8.0);
                }
                if ui.add(egui::Button::new(RichText::new("Report a problem").size(14.0).color(INK))).on_hover_text("Tell us what went wrong. The launcher's and the game's logs go with it (no password is in them).").clicked() {
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

        // the screen's painting under everything in the body
        let (art, light) = match self.tab {
            Tab::Play => (&self.pics.hero_play, 1.0),
            Tab::Library => (&self.pics.hero_replays, 0.62),
            _ => (&self.pics.hero_more, 0.4),
        };
        scene(&p, art, body, light);

        let mut act = None;
        self.top_bar(root, top, &mut act);
        match self.tab {
            Tab::Play => self.play_tab(root, body, &mut act),
            Tab::Library => self.library_tab(root, body, &mut act),
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

/// The plan in the top bar, as the server last said it: "FREE", or a star and "REPLAY+" (amber in its last days).
fn plan_badge(ui: &mut egui::Ui, l: &crate::net::Library) -> egui::Response {
    let soon = screens::library::ending_soon(l);
    let (text, col) = match (l.is_sub(), soon) {
        (true, Some(d)) => (format!("REPLAY+  {d}D"), AMBER),
        (true, None) => ("REPLAY+".to_string(), GOLD),
        _ => ("FREE".to_string(), MUTED),
    };
    let font = bold_font(11.5);
    let g = ui.painter().layout_no_wrap(text.clone(), font.clone(), col);
    let star = if l.is_sub() { 16.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 22.0 + star, 24.0), Sense::click());
    let p = ui.painter();
    let fill = if l.is_sub() { col.gamma_multiply(0.16) } else { rgba(255, 255, 255, 8) };
    p.rect_filled(rect, 12.0, if resp.hovered() { col.gamma_multiply(0.26) } else { fill });
    p.rect_stroke(rect, 12.0, Stroke::new(1.0, col.gamma_multiply(0.8)), egui::StrokeKind::Inside);
    if l.is_sub() {
        icon(p, pos2(rect.left() + 15.0, rect.center().y), 12.0, Icon::Star, col);
    }
    p.text(pos2(rect.left() + 11.0 + star, rect.center().y), Align2::LEFT_CENTER, text, font, col);
    let (name, line) = screens::library::plan_words(l);
    resp.on_hover_text(format!("{name}: {line}. Open your replays."))
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
