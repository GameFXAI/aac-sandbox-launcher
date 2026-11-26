//! The app window: the AA Replay companion app (docs/companion-app.md), drawn edge to edge in code. It uses no AA
//! Classic logo or artwork, so it never passes for the official launcher.
//!
//! How it fits together:
//! - `App` (here) holds everything between frames; `Page` says which page is up.
//! - shell.rs draws the frame every page sits in: title bar, left rail, status bar, the window's edges.
//! - Pages (screens/*.rs) draw into the centre and the inspector and hand back what the player pressed as an `Action`.
//! - `App::run` (here) carries out an `Action`; work that takes time goes to a thread (tasks.rs, or the page's own
//!   file), which answers with an `Event` over the channel.
//! - `App::poll` (events.rs) takes the `Event`s in at the start of each frame.
//! - theme.rs is the look (colours, fonts, painting), icons.rs the line icons, widgets.rs the app's own widgets.

use crate::core::*;
use crate::net;
use crate::{find, library, recorder, settings, signin, transfer, update};

mod events;
mod icons;
mod pictures;
mod report;
mod screens;
mod shell;
mod status_bar;
mod tasks;
mod theme;
mod widgets;

use eframe::egui::{self, Rect, Stroke, pos2, vec2};
use events::Event;
use report::ReportForm;
use screens::library::Lib;
use screens::shares::{Do, Hub};
use screens::transfer::Xfer;
use std::process::Child;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::SystemTime;
use theme::*;

/// Is there an AA Classic profile on this PC to bring settings from?
fn transfer_docs_present() -> bool {
    transfer::real_docs().is_some()
}

/// The title bar's sections; each has its own pages in the left rail. Account is the account menu's pages.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(in crate::gui) enum Section {
    Library,
    Community,
    Guild,
    Account,
}

/// The page in the centre.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(in crate::gui) enum Page {
    /// the account's own recordings (screens/library.rs)
    Captures,
    SharedWithMe,
    Saved,
    Discover,
    Following,
    CommunityShared,
    GuildHome,
    GuildShares,
    Review,
    Members,
    Settings,
    Transfer,
    Log,
}

impl Page {
    pub fn section(self) -> Section {
        match self {
            Page::Captures | Page::SharedWithMe | Page::Saved => Section::Library,
            Page::Discover | Page::Following | Page::CommunityShared => Section::Community,
            Page::GuildHome | Page::GuildShares | Page::Review | Page::Members => Section::Guild,
            Page::Settings | Page::Transfer | Page::Log => Section::Account,
        }
    }

    /// The rail's pages for a section. Library's last one is a shortcut into Guild.
    pub fn rail(s: Section) -> &'static [Page] {
        match s {
            Section::Library => &[Page::Captures, Page::SharedWithMe, Page::Saved, Page::Review],
            Section::Community => &[Page::Discover, Page::Following, Page::CommunityShared],
            Section::Guild => &[Page::GuildHome, Page::GuildShares, Page::Review, Page::Members],
            Section::Account => &[Page::Settings, Page::Transfer, Page::Log],
        }
    }

    /// The page's name in the rail of section `s`.
    pub fn label(self, s: Section) -> &'static str {
        match self {
            Page::Captures => "My captures",
            Page::SharedWithMe | Page::CommunityShared => "Shared with me",
            Page::Saved => "Saved",
            Page::Discover => "Discover",
            Page::Following => "Following",
            Page::GuildHome => "Overview",
            Page::GuildShares => "Guild shares",
            Page::Review if s == Section::Library => "Guild review",
            Page::Review => "Review queue",
            Page::Members => "Members",
            Page::Settings => "Settings",
            Page::Transfer => "From AA Classic",
            Page::Log => "Log",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Page::Captures => Icon::Monitor,
            Page::SharedWithMe | Page::CommunityShared => Icon::People,
            Page::Saved => Icon::Star,
            Page::Discover => Icon::Compass,
            Page::Following => Icon::Rss,
            Page::GuildHome => Icon::Home,
            Page::GuildShares => Icon::Layers,
            Page::Review => Icon::Shield,
            Page::Members => Icon::People,
            Page::Settings => Icon::Gear,
            Page::Transfer => Icon::Download,
            Page::Log => Icon::List,
        }
    }

    /// The first page of a section (a title-bar tab's click).
    pub fn home(s: Section) -> Page {
        Page::rail(s)[0]
    }
}

/// Window -> work: what the player pressed, returned by a page and carried out by `App::run` once per frame.
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
    /// the welcome page: sign in to the account (no game is started)
    SignIn,
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
    /// My captures: the list again from the server
    RefreshLibrary,
    /// My captures: delete this recording (its stable number), after the inspector's "Delete this capture?"
    DeleteReplay(i32),
    /// My captures: start the game and open this recording in its replay player
    WatchReplay(i32),
    /// the server's upgrade page (the only thing a real checkout will change is where it points)
    Upgrade,
    /// Share clip: My captures with the dialog open for this capture (none: just My captures)
    OpenShare(Option<i32>),
    /// sharing, friends and the guild (screens/shares.rs)
    Shares(Do),
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
    /// signed in this run (with the password, not remembered): the shell shows instead of the welcome page
    signed: bool,
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
    page: Page,
    confirm: Option<(Action, SystemTime)>,
    xfer: Xfer,
    /// what went wrong with the last thing the player pressed: shown in the status bar until the next press
    hint: Option<String>,
    /// integrity.rs: this exe is not the build that was installed here (kept for the whole run)
    modified: Option<&'static str>,
    /// the server's answer to /api/info: Ok(name) when it is up
    server_up: Option<Result<String, String>>,
    /// keep the sign-in on this PC (the server's token, never the password)
    remember: bool,
    /// the remembered sign-in for this server, if any
    saved: Option<signin::Saved>,
    /// the invite the installer left (read from the setup file's name): the first sign-in makes the account with it
    baked_invite: Option<String>,
    /// the first start's "bring your AA Classic settings" dialog is up
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
    /// My captures (screens/library.rs)
    lib: Lib,
    /// shares, friends and the guild (screens/shares.rs)
    hub: Hub,
    /// the moment pictures (pictures.rs)
    pics: pictures::Pictures,
    /// the recordings' real pictures, as they come from the server
    thumbs: pictures::Thumbs,
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
            // with the invite from the download and no sign-in yet, the welcome page makes the account
            signup: std::env::var("AAEMU_SANDBOX_SIGNUP").is_ok() || (baked_invite.is_some() && saved.is_none()),
            signed: false,
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
            // start page, for screenshots: AAEMU_SANDBOX_TAB=library|shared|saved|community|following|guild|
            // guildshares|review|members|settings|log|share (My captures with Share clip open)
            page: match std::env::var("AAEMU_SANDBOX_TAB").as_deref() {
                Ok("settings") => Page::Settings,
                Ok("log") => Page::Log,
                Ok("shared") => Page::SharedWithMe,
                Ok("saved") => Page::Saved,
                Ok("community") => Page::Discover,
                Ok("following") => Page::Following,
                Ok("guild") => Page::GuildHome,
                Ok("guildshares") => Page::GuildShares,
                Ok("review") => Page::Review,
                Ok("members") => Page::Members,
                _ => Page::Captures,
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
            lib: Lib { share_on_load: std::env::var("AAEMU_SANDBOX_TAB").as_deref() == Ok("share"), ..Default::default() },
            hub: Hub::default(),
            pics,
            thumbs: Default::default(),
        };
        if let Some(s) = &a.saved {
            a.cfg.username = s.username.clone();
            // the last list at once (the plan too), and a fresh one off the window's thread
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

    /// A problem with what the player just pressed: logged, and shown in the status bar.
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
    /// A recording's picture: the real one from the game when the server has it, else its drawn still.
    pub(in crate::gui) fn rec_pic(&self, r: &crate::net::Replay) -> eframe::egui::TextureHandle {
        self.thumbs.get(pictures::Thumb::Own(r.n), r.thumb).unwrap_or_else(|| self.pics.for_recording(r.n).clone())
    }

    /// A shared moment's picture: the real one when the server has it, else its kind's drawn still.
    pub(in crate::gui) fn share_pic(&self, s: &crate::shares::Share) -> eframe::egui::TextureHandle {
        self.thumbs.get(pictures::Thumb::Share(s.id), s.thumb).unwrap_or_else(|| self.pics.for_share(&s.kind, s.id).clone())
    }

    /// The real pictures the cards asked for this frame, fetched one after another on a thread.
    fn fetch_pictures(&mut self) {
        let want = self.thumbs.take_wanted();
        if want.is_empty() {
            return;
        }
        let server = crate::net::Server::parse(&self.cfg.login_ip).ok();
        let secret = self.secret();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            for key in want {
                let img = match (&server, &secret) {
                    (Some(s), Some((user, secret))) => {
                        let (path, fields) = match key {
                            pictures::Thumb::Own(n) => ("/replays/thumb/get", serde_json::json!({ "n": n })),
                            pictures::Thumb::Share(id) => ("/shares/thumb", serde_json::json!({ "share": id })),
                        };
                        crate::net::picture(s, path, user, secret.cred(), fields).ok().flatten().and_then(|b| pictures::decode(&b))
                    }
                    _ => None,
                };
                let _ = tx.send(Event::Picture(key, img));
            }
        });
    }

    pub(in crate::gui) fn secret(&self) -> Option<(String, Secret)> {
        if let Some(s) = &self.saved {
            return Some((s.username.clone(), Secret::Session(s.session.clone())));
        }
        let u = self.cfg.username.trim();
        (!u.is_empty() && !self.password.is_empty()).then(|| (u.to_string(), Secret::Password(self.password.clone())))
    }

    /// Signed in: remembered on this PC, or the server took the password this run.
    pub(in crate::gui) fn signed_in(&self) -> bool {
        self.saved.is_some() || (self.signed && self.secret().is_some())
    }

    /// The account's name as the app shows it.
    pub(in crate::gui) fn account_name(&self) -> String {
        self.saved.as_ref().map(|s| s.username.clone()).unwrap_or_else(|| self.cfg.username.trim().to_string())
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
            Action::SignIn => self.sign_in(),
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
            Action::OpenShare(n) => {
                self.page = Page::Captures;
                if let Some(n) = n {
                    self.lib.selected = Some(n);
                    self.open_share(n);
                }
            }
            Action::Shares(d) => self.shares_do(d),
        }
    }
}

/// The window's parts, in points.
pub(in crate::gui) const TITLE_H: f32 = 52.0;
pub(in crate::gui) const STATUS_H: f32 = 42.0;
pub(in crate::gui) const RAIL_W: f32 = 226.0;
pub(in crate::gui) const INSPECTOR_W: f32 = 328.0;

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.poll(&ctx);
        self.keep_fresh();
        self.fetch_pictures();
        // the first seconds: the window settles to its size and scale, so draw it again until it has
        if ctx.input(|i| i.time) < 3.0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }

        let full = root.max_rect();
        let p = root.painter().clone();
        backdrop(&p, full);
        let top = Rect::from_min_size(full.min, vec2(full.width(), TITLE_H));
        let bottom = Rect::from_min_max(pos2(full.left(), full.bottom() - STATUS_H), full.max);
        let body = Rect::from_min_max(pos2(full.left(), top.bottom()), pos2(full.right(), bottom.top()));

        let mut act = None;
        // the account's pages work signed out too (Settings, the transfer, the log); the rest wants a sign-in
        let shell = self.signed_in() || self.page.section() == Section::Account;
        self.title_bar(root, top, shell, &mut act);
        if shell {
            let rail = Rect::from_min_max(body.min, pos2(body.left() + RAIL_W, body.bottom()));
            self.rail(root, rail, &mut act);
            let rest = Rect::from_min_max(pos2(rail.right(), body.top()), body.max);
            if self.page.section() == Section::Account {
                let page = rest.shrink2(vec2(8.0, 0.0));
                match self.page {
                    Page::Settings => self.settings_tab(root, page, &mut act),
                    Page::Transfer => self.transfer_tab(root, page, &mut act),
                    _ => self.log_tab(root, page),
                }
            } else {
                let insp = Rect::from_min_max(pos2(rest.right() - INSPECTOR_W, rest.top()), rest.max);
                let centre = Rect::from_min_max(rest.min, pos2(insp.left(), rest.bottom()));
                shell::inspector_panel(&p, insp);
                match self.page {
                    Page::Captures => {
                        self.captures_page(root, centre, &mut act);
                        self.captures_inspector(root, insp, &mut act);
                    }
                    Page::GuildHome => {
                        self.guild_home(root, centre, &mut act);
                        self.share_inspector(root, insp, &mut act);
                    }
                    Page::Review => {
                        self.review_page(root, centre, &mut act);
                        self.share_inspector(root, insp, &mut act);
                    }
                    Page::Members => {
                        self.members_page(root, centre, &mut act);
                        self.members_inspector(root, insp);
                    }
                    _ => {
                        self.feed_page(root, centre, &mut act);
                        self.share_inspector(root, insp, &mut act);
                    }
                }
            }
        } else {
            self.welcome(root, body, &mut act);
        }
        self.status_bar(root, bottom, &mut act);
        if self.offer {
            self.offer_dialog(&ctx, &mut act);
        }
        self.report_dialog(&ctx, &mut act);
        self.share_dialog(&ctx, &mut act);
        shell::resize_edges(root, full);
        // a hairline around the borderless window
        p.rect_stroke(full.shrink(0.5), 0.0, Stroke::new(1.0, rgba(255, 255, 255, 20)), egui::StrokeKind::Inside);

        if let Some(a) = act {
            self.run(a);
        }
    }
}

pub fn run() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1320.0, 820.0])
            .with_min_inner_size([1100.0, 700.0])
            .with_resizable(true)
            .with_decorations(false)
            .with_title("AA Replay"),
        ..Default::default()
    };
    let _ = eframe::run_native("AA Replay", options, Box::new(|cc| Ok(Box::new(App::new(cc)))));
}
