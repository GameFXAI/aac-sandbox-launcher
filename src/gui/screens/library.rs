//! Library > My captures: the account's own recordings, kept on the server, as a grid or a list with search, a
//! visibility filter and a sort; the inspector shows the picked one with Jump into ArcheAge, Delete, its sharing and
//! the plan. Everything shown is what the server last said (net.rs `list_replays`); nothing about the plan or the
//! cap is worked out here.
//!
//! Jump into ArcheAge starts the game as Play does and leaves the recording for the game's own replay player
//! (library.rs `set_pending_watch`), which opens it once the game is in the world. The app never handles a fight's
//! packets.

use crate::gui::events::Event;
use crate::gui::theme::*;
use crate::gui::widgets::{EDGE, dropdown, empty_state, link_button, outline_button, primary_button, search_box, thumb_badge, view_toggle};
use crate::gui::{Action, App};
use crate::library;
use crate::net::{self, Library, Replay, Server};
use eframe::egui::{self, Align, Align2, Color32, Layout, Rect, RichText, Sense, Stroke, UiBuilder, pos2, vec2};
use std::time::{Duration, Instant};

/// How long a Jump into ArcheAge waits for the game to start before it is dropped (setup or sign-in failed meanwhile).
const WATCH_FOR: Duration = Duration::from_secs(300);

const VISIBILITY: [&str; 3] = ["All visibility", "Private", "Shared"];
const SORTS: [&str; 3] = ["Newest first", "Oldest first", "Longest first"];

/// My captures' state between frames.
#[derive(Default)]
pub struct Lib {
    /// the last list: the server's answer, or the cached one until it comes
    pub data: Option<Library>,
    /// a fetch was made since the app started (or since something changed the list)
    pub fresh: bool,
    pub loading: bool,
    /// the last fetch failed: why
    pub error: Option<String>,
    /// the inspector asking "Delete this capture?"
    pub confirm: Option<i32>,
    /// the delete on its way to the server
    pub deleting: Option<i32>,
    /// the recording Jump into ArcheAge asked for, until the game starts
    pub pending: Option<(i32, Instant)>,
    /// the recording the game was started for (the inspector says it opens once in the world)
    pub launched: Option<i32>,
    /// a Jump into ArcheAge that did not get the game started: which recording, and why
    pub failed: Option<(i32, String)>,
    /// the last thing pressed came to this: Ok(what to say) or Err(what went wrong)
    pub note: Option<Result<String, String>>,
    /// the picked recording
    pub selected: Option<i32>,
    pub query: String,
    pub visibility: usize,
    pub sort: usize,
    /// list instead of grid
    pub list: bool,
}

impl Lib {
    /// The recording to leave for the game, taken once (a watch older than `WATCH_FOR` is dropped).
    pub fn take_pending(&mut self) -> Option<i32> {
        self.pending.take().filter(|(_, at)| at.elapsed() < WATCH_FOR).map(|(n, _)| n)
    }
}

/// What a fight's title says it came to: won, lost, or neither (left).
fn outcome(title: &str) -> &'static str {
    if title.starts_with("beat ") {
        "Won"
    } else if title.starts_with("lost to ") {
        "Lost"
    } else {
        "Fight"
    }
}

/// "beat Opponent 0:42" -> ("beat Opponent", Some("0:42")).
fn split_length(title: &str) -> (&str, Option<&str>) {
    match title.rsplit_once(' ') {
        Some((head, tail)) if tail.contains(':') && tail.chars().all(|c| c.is_ascii_digit() || c == ':') => (head, Some(tail)),
        _ => (title, None),
    }
}

/// "0:42" -> 42, "2:18" -> 138, "1:02:03" -> 3723.
fn seconds(len: &str) -> Option<u32> {
    len.split(':').try_fold(0u32, |acc, part| part.parse::<u32>().ok().map(|v| acc * 60 + v))
}

/// 138 -> "2m 18s", 42 -> "42s".
fn length_words(secs: u32) -> String {
    match (secs / 3600, secs % 3600 / 60, secs % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, s) => format!("{m}m {s:02}s"),
        (h, m, _) => format!("{h}h {m:02}m"),
    }
}

/// The capture's name as shown: the title without its length, starting with a capital.
fn display_title(title: &str) -> String {
    let head = split_length(title).0.trim();
    let mut c = head.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => "Untitled capture".into(),
    }
}

fn length_of(r: &Replay) -> Option<u32> {
    split_length(&r.title).1.and_then(seconds)
}

/// The captures the page shows: the search over title and names, the visibility filter (every capture is private
/// until sharing exists on the server), then the sort.
fn shown(l: &Library, query: &str, visibility: usize, sort: usize) -> Vec<Replay> {
    let q = query.trim().to_lowercase();
    let mut v: Vec<Replay> = l
        .replays
        .iter()
        .filter(|r| q.is_empty() || r.title.to_lowercase().contains(&q) || r.other.to_lowercase().contains(&q) || r.me.to_lowercase().contains(&q))
        .filter(|_| visibility != 2)
        .cloned()
        .collect();
    match sort {
        1 => v.sort_by_key(|r| r.when),
        2 => v.sort_by_key(|r| std::cmp::Reverse(length_of(r).unwrap_or(0))),
        _ => v.sort_by_key(|r| std::cmp::Reverse(r.when)),
    }
    v
}

/// "Replay+" / "Free" and the line under it, from the server's plan.
pub fn plan_words(l: &Library) -> (String, String) {
    if l.is_sub() {
        let until = l.plan_expires_at.as_deref().and_then(library::parse_iso);
        let line = match until {
            Some(t) => format!("Keeping up to {} recordings · until {}", l.cap, library::date(t)),
            None => format!("Keeping up to {} recordings", l.cap),
        };
        ("Replay+".into(), line)
    } else {
        ("Free".into(), format!("Keeping your newest {} recordings", l.cap))
    }
}

/// Days left on a sub, when it ends within five days.
pub fn ending_soon(l: &Library) -> Option<i64> {
    let until = l.plan_expires_at.as_deref().and_then(library::parse_iso)?;
    let left = library::days_left(until, library::now_unix());
    (l.is_sub() && left <= 5).then_some(left)
}

/// Opens the server's upgrade page: only a web address, never anything else the answer might hold.
pub fn open_upgrade(url: &str) -> Result<(), String> {
    let u = url.trim();
    if !(u.starts_with("https://") || u.starts_with("http://")) || u.contains(['"', ' ', '\n']) {
        return Err("the server's upgrade link is not a web address".into());
    }
    std::process::Command::new("explorer").arg(u).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Where a Jump into ArcheAge stands for one capture.
enum Launch {
    None,
    Opening,
    Running,
    Failed(String),
}

impl App {
    /// Asks the server for the list, off the window's thread. Quiet when there is no sign-in or no account API.
    pub(in crate::gui) fn fetch_library(&mut self) {
        if self.lib.loading {
            return;
        }
        let Ok(server @ Server::Api { .. }) = Server::parse(&self.cfg.login_ip) else { return };
        let Some((user, secret)) = self.secret() else { return };
        self.lib.loading = true;
        self.lib.fresh = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::Library(net::list_replays(&server, &user, secret.cred())));
        });
    }

    pub(in crate::gui) fn delete_replay(&mut self, n: i32) {
        self.lib.confirm = None;
        let Ok(server @ Server::Api { .. }) = Server::parse(&self.cfg.login_ip) else { return };
        let Some((user, secret)) = self.secret() else {
            return self.lib.note = Some(Err("Sign in first.".into()));
        };
        self.lib.deleting = Some(n);
        self.lib.note = None;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::ReplayDeleted(n, net::delete_replay(&server, &user, secret.cred(), n)));
        });
    }

    /// Jump into ArcheAge: the game starts as with Play, and opens the recording once it is in the world.
    pub(in crate::gui) fn watch_replay(&mut self, n: i32) {
        if self.child.is_some() {
            return self.lib.note = Some(Err("ArcheAge is already running: open Replays in the game to watch it.".into()));
        }
        self.lib.failed = None;
        self.lib.launched = None;
        self.lib.pending = Some((n, Instant::now()));
        self.lib.note = None;
        self.press_play();
        if let Some(h) = self.hint.clone() {
            // Play could not even start (no game folder, AA Classic open): nothing waits for the game
            self.lib.pending = None;
            self.lib.failed = Some((n, h));
        }
    }

    /// The server's answer to a list.
    pub(in crate::gui) fn library_came(&mut self, r: Result<Library, String>) {
        self.lib.loading = false;
        match r {
            Ok(l) => {
                if let Some(s) = self.secret().map(|(u, _)| u) {
                    library::save_cached(&self.paths, &self.cfg.login_ip, &s, &l);
                }
                if self.lib.confirm.is_some_and(|n| !l.replays.iter().any(|r| r.n == n)) {
                    self.lib.confirm = None;
                }
                self.lib.data = Some(l);
                self.lib.error = None;
            }
            Err(e) => self.lib.error = Some(e),
        }
    }

    pub(in crate::gui) fn replay_deleted(&mut self, n: i32, r: Result<(), String>) {
        self.lib.deleting = None;
        match r {
            Ok(()) => {
                if let Some(l) = &mut self.lib.data {
                    l.replays.retain(|r| r.n != n);
                }
                if self.lib.selected == Some(n) {
                    self.lib.selected = None;
                }
                self.lib.note = Some(Ok("Capture deleted.".into()));
                // the server's list again: which ones are over the cap may have changed
                self.fetch_library();
            }
            Err(e) => {
                if e.contains("not on the server") {
                    self.fetch_library();
                }
                self.lib.note = Some(Err(e));
            }
        }
    }

    fn launch_state(&self, n: i32) -> Launch {
        match (&self.lib.failed, &self.lib.pending, self.lib.launched) {
            (Some((f, why)), _, _) if *f == n => Launch::Failed(why.clone()),
            (_, Some((p, _)), _) if *p == n => Launch::Opening,
            (_, _, Some(l)) if l == n && self.child.is_some() => Launch::Running,
            _ => Launch::None,
        }
    }

    pub(in crate::gui) fn captures_page(&mut self, ui: &mut egui::Ui, centre: Rect, act: &mut Option<Action>) {
        if !self.lib.fresh && !self.lib.loading && self.secret().is_some() {
            self.fetch_library();
        }
        let p = ui.painter().clone();
        let (x0, x1) = (centre.left() + 24.0, centre.right() - 24.0);

        // heading: the name, how many, and refresh
        let t = p.text(pos2(x0 + 4.0, centre.top() + 22.0), Align2::LEFT_TOP, "My captures", title_font(27.0), INK);
        if let Some(l) = &self.lib.data {
            let n = l.replays.len();
            p.text(pos2(t.right() + 14.0, t.center().y + 2.0), Align2::LEFT_CENTER, format!("{n} capture{}", if n == 1 { "" } else { "s" }), text_font(14.5), MUTED);
        }
        let refresh = Rect::from_center_size(pos2(x1 - 16.0, t.center().y + 1.0), vec2(32.0, 32.0));
        ui.scope_builder(UiBuilder::new().max_rect(refresh), |ui| {
            if self.lib.loading {
                ui.add(egui::Spinner::new().size(18.0).color(GOLD));
            } else if icon_button(ui, Icon::Refresh, 32.0, "Ask the server for your captures again", false).clicked() {
                *act = Some(Action::RefreshLibrary);
            }
        });

        // the toolbar: search, visibility, grid or list, sort
        let bar = Rect::from_min_max(pos2(x0, t.bottom() + 18.0), pos2(x1, t.bottom() + 58.0));
        ui.scope_builder(UiBuilder::new().max_rect(bar).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let fixed = 170.0 + 88.0 + 156.0 + 12.0 * 3.0 + 14.0;
            let sw = (bar.width() - fixed).max(160.0);
            search_box(ui, &mut self.lib.query, "Search captures", sw, 40.0);
            dropdown(ui, "visibility", Icon::Filter, &VISIBILITY, &mut self.lib.visibility, 170.0, 40.0, true);
            view_toggle(ui, &mut self.lib.list, 40.0);
            let (sep, _) = ui.allocate_exact_size(vec2(2.0, 24.0), Sense::hover());
            ui.painter().line_segment([sep.center_top(), sep.center_bottom()], Stroke::new(1.0, EDGE));
            dropdown(ui, "sort", Icon::Sort, &SORTS, &mut self.lib.sort, 156.0, 40.0, false);
        });

        let area = Rect::from_min_max(pos2(x0, bar.bottom() + 18.0), pos2(x1, centre.bottom()));
        let empty = Rect::from_min_max(area.min, area.max - vec2(0.0, 24.0));
        if !matches!(Server::parse(&self.cfg.login_ip), Ok(Server::Api { .. })) {
            return empty_state(ui, empty, Icon::Film, "No capture library on this server", "Captures are kept by servers with an account API (a server link).");
        }
        let Some(data) = self.lib.data.clone() else {
            if let Some(e) = self.lib.error.clone() {
                empty_state(ui, empty, Icon::Refresh, "Your captures could not be fetched", &e);
                return self.empty_button(ui, empty, "Try again", Action::RefreshLibrary, act);
            }
            ui.scope_builder(UiBuilder::new().max_rect(Rect::from_center_size(empty.center() - vec2(0.0, 40.0), vec2(300.0, 60.0))).layout(Layout::top_down(Align::Center)), |ui| {
                ui.add(egui::Spinner::new().size(26.0).color(GOLD));
                ui.label(RichText::new("Getting your captures…").font(text_font(14.0)).color(MUTED));
            });
            return;
        };
        if data.replays.is_empty() {
            if self.rec.set_up {
                return empty_state(ui, empty, Icon::Film, "No captures yet", "Your fights appear here by themselves while you play AA Classic with recording on.");
            }
            empty_state(ui, empty, Icon::Film, "No captures yet", "Your fights appear here by themselves while you play AA Classic. Turn recording on first.");
            return self.empty_button(ui, empty, "Set up recording", Action::RecOn, act);
        }
        let list = shown(&data, &self.lib.query, self.lib.visibility, self.lib.sort);
        if list.is_empty() {
            let why = if self.lib.visibility == 2 { "None of your captures is shared yet." } else { "Try other words, or clear the search." };
            return empty_state(ui, empty, Icon::Search, "No captures match", why);
        }
        if !self.lib.selected.is_some_and(|n| list.iter().any(|r| r.n == n)) {
            self.lib.selected = list.first().map(|r| r.n);
        }

        let playing = self.child.is_some();
        ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                // room for the picked card's glow
                ui.add_space(6.0);
                if self.lib.list {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    let w = ui.available_width() - 8.0;
                    for r in &list {
                        ui.horizontal(|ui| {
                            ui.add_space(4.0);
                            self.capture_row(ui, w, r, playing, act);
                        });
                    }
                } else {
                    let gap = 16.0;
                    let avail = ui.available_width() - 10.0;
                    let cols = if avail > 900.0 { 4 } else if avail > 520.0 { 3 } else { 2 };
                    let cw = ((avail - gap * (cols - 1) as f32) / cols as f32).floor();
                    let ch = (cw * 0.56).round() + 62.0;
                    ui.spacing_mut().item_spacing = vec2(gap, gap);
                    for row in list.chunks(cols) {
                        ui.horizontal(|ui| {
                            ui.add_space(4.0);
                            for r in row {
                                self.capture_card(ui, vec2(cw, ch), r, playing, act);
                            }
                        });
                    }
                }
                ui.add_space(16.0);
            });
        });
    }

    fn empty_button(&mut self, ui: &mut egui::Ui, area: Rect, label: &str, a: Action, act: &mut Option<Action>) {
        let c = pos2(area.center().x, area.top() + area.height() * 0.36 + 160.0);
        ui.scope_builder(UiBuilder::new().max_rect(Rect::from_center_size(c, vec2(240.0, 40.0))).layout(Layout::top_down(Align::Center)), |ui| {
            if primary_button(ui, label, None, false, 0.0, 38.0, true).clicked() {
                *act = Some(a);
            }
        });
    }

    /// One capture in the grid: its still (private badge, a lock past the plan's limit), its name, its day and its
    /// length. A click picks it; a double click jumps into ArcheAge.
    fn capture_card(&mut self, ui: &mut egui::Ui, size: egui::Vec2, r: &Replay, playing: bool, act: &mut Option<Action>) {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let picked = self.lib.selected == Some(r.n);
        let hov = resp.hovered();
        let p = ui.painter().clone();
        let dim = r.over_cap;
        if picked {
            glow(&p, rect, 11.0, 1.1);
        }
        p.rect_filled(rect, 11.0, if hov || picked { Color32::from_rgb(22, 22, 26) } else { PANEL });
        let thumb = Rect::from_min_size(rect.min, vec2(rect.width(), (rect.width() * 0.56).round()));
        let tint = if dim { Color32::from_gray(105) } else if hov { Color32::WHITE } else { Color32::from_gray(228) };
        crate::gui::pictures::draw(&p, self.pics.for_recording(r.n), thumb, egui::CornerRadius { nw: 11, ne: 11, sw: 0, se: 0 }, tint);
        vgrad(&p, Rect::from_min_max(thumb.min, pos2(thumb.right(), thumb.top() + 44.0)), rgba(0, 0, 0, 120), rgba(0, 0, 0, 0));
        thumb_badge(&p, thumb.min + vec2(10.0, 10.0), Icon::Lock);
        let c = thumb.center();
        if dim {
            p.circle_filled(c, 20.0, rgba(0, 0, 0, 170));
            p.circle_stroke(c, 20.0, Stroke::new(1.0, AMBER));
            icon(&p, c, 18.0, Icon::Lock, AMBER);
        } else if hov {
            p.circle_filled(c, 22.0, rgba(10, 10, 12, 170));
            p.circle_stroke(c, 22.0, Stroke::new(1.2, GOLD_HI));
            icon(&p, c + vec2(1.5, 0.0), 18.0, Icon::Play, GOLD_HI);
        }
        let tx = rect.left() + 13.0;
        let title = crate::gui::status_bar::elide(&p, &display_title(&r.title), rect.width() - 26.0);
        p.text(pos2(tx, thumb.bottom() + 11.0), Align2::LEFT_TOP, title, bold_font(15.0), if dim { MUTED } else { INK });
        let day = library::day(r.when, library::now_unix());
        p.text(pos2(tx, thumb.bottom() + 34.0), Align2::LEFT_TOP, if dim { "Past your plan's limit".to_string() } else { day }, text_font(13.0), if dim { AMBER } else { MUTED });
        if let Some(s) = length_of(r) {
            p.text(pos2(rect.right() - 13.0, thumb.bottom() + 34.0), Align2::RIGHT_TOP, length_words(s), text_font(13.0), rgba(214, 212, 208, 255));
        }
        if picked {
            p.rect_stroke(rect, 11.0, Stroke::new(1.5, GOLD), egui::StrokeKind::Inside);
        } else {
            p.rect_stroke(rect, 11.0, Stroke::new(1.0, if hov { Color32::from_rgb(70, 70, 80) } else { LINE }), egui::StrokeKind::Inside);
        }
        let tip = if playing { "ArcheAge is running: open Replays in the game to watch this one" } else { "Double-click to jump into ArcheAge" };
        let resp = resp.on_hover_text(tip);
        if resp.clicked() {
            self.lib.selected = Some(r.n);
            self.lib.confirm = None;
        }
        if resp.double_clicked() && !playing {
            *act = Some(Action::WatchReplay(r.n));
        }
    }

    /// One capture in the list: a small still, its name, the day and who else, its length and its visibility.
    fn capture_row(&mut self, ui: &mut egui::Ui, width: f32, r: &Replay, playing: bool, act: &mut Option<Action>) {
        let (rect, resp) = ui.allocate_exact_size(vec2(width, 76.0), Sense::click());
        let picked = self.lib.selected == Some(r.n);
        let hov = resp.hovered();
        let p = ui.painter().clone();
        if picked {
            bronze(&p, rect, 10.0, 1.0);
            p.rect_stroke(rect, 10.0, Stroke::new(1.5, GOLD), egui::StrokeKind::Inside);
        } else {
            p.rect_filled(rect, 10.0, if hov { Color32::from_rgb(22, 22, 26) } else { PANEL });
            p.rect_stroke(rect, 10.0, Stroke::new(1.0, LINE), egui::StrokeKind::Inside);
        }
        let thumb = Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(107.0, 60.0));
        crate::gui::pictures::draw(&p, self.pics.for_recording(r.n), thumb, 7, if r.over_cap { Color32::from_gray(105) } else { Color32::from_gray(230) });
        let x = thumb.right() + 16.0;
        p.text(pos2(x, rect.top() + 16.0), Align2::LEFT_TOP, display_title(&r.title), bold_font(15.0), if r.over_cap { MUTED } else { INK });
        let mut line = library::day(r.when, library::now_unix());
        if !r.other.trim().is_empty() {
            line = format!("{line}  ·  with {}", r.other.trim());
        }
        if r.over_cap {
            line = "Past your plan's limit".into();
        }
        p.text(pos2(x, rect.top() + 40.0), Align2::LEFT_TOP, line, text_font(13.0), if r.over_cap { AMBER } else { MUTED });
        icon(&p, pos2(rect.right() - 26.0, rect.center().y), 15.0, Icon::Lock, MUTED);
        if let Some(s) = length_of(r) {
            p.text(pos2(rect.right() - 48.0, rect.center().y), Align2::RIGHT_CENTER, length_words(s), text_font(13.5), rgba(214, 212, 208, 255));
        }
        let resp = resp.on_hover_text(if playing { "ArcheAge is running: open Replays in the game to watch this one" } else { "Double-click to jump into ArcheAge" });
        if resp.clicked() {
            self.lib.selected = Some(r.n);
            self.lib.confirm = None;
        }
        if resp.double_clicked() && !playing {
            *act = Some(Action::WatchReplay(r.n));
        }
    }

    /// The inspector: the picked capture with Jump into ArcheAge and its launch state, Delete, its sharing, the plan.
    pub(in crate::gui) fn captures_inspector(&mut self, ui: &mut egui::Ui, r: Rect, act: &mut Option<Action>) {
        let p = ui.painter().clone();
        let x = r.left() + 28.0;
        let w = r.width() - 56.0;
        let data = self.lib.data.clone();
        let pick = data.as_ref().and_then(|l| l.replays.iter().find(|c| Some(c.n) == self.lib.selected).cloned());
        let Some(c) = pick else {
            caps(&p, pos2(x, r.top() + 32.0), "Nothing selected", MUTED);
            let g = p.layout("Pick a capture to see it here. Jump into ArcheAge opens it in the game, in the world itself.".into(), text_font(13.5), MUTED, w);
            p.galley(pos2(x, r.top() + 60.0), g, MUTED);
            if let Some(l) = &data {
                self.plan_box(ui, Rect::from_min_max(pos2(x, r.bottom() - 132.0), pos2(x + w, r.bottom() - 24.0)), l, act);
            }
            return;
        };

        // who, where, how long
        let mut y = r.top() + 30.0;
        let tg = p.layout(display_title(&c.title), title_font(25.0), INK, w);
        let th = tg.size().y;
        p.galley(pos2(x, y), tg, INK);
        y += th + 12.0;
        let me = if c.me.trim().is_empty() { self.account_name() } else { c.me.trim().to_string() };
        avatar(&p, pos2(x + 13.0, y + 13.0), 13.0, &me);
        let mr = p.text(pos2(x + 34.0, y + 13.0), Align2::LEFT_CENTER, &me, bold_font(14.5), INK);
        if !c.other.trim().is_empty() {
            let vr = p.text(pos2(mr.right() + 10.0, y + 13.0), Align2::LEFT_CENTER, "vs", text_font(13.5), MUTED);
            avatar(&p, pos2(vr.right() + 20.0, y + 13.0), 13.0, c.other.trim());
            p.text(pos2(vr.right() + 41.0, y + 13.0), Align2::LEFT_CENTER, c.other.trim(), bold_font(14.5), INK);
        }
        y += 38.0;
        let day = library::day(c.when, library::now_unix());
        p.text(pos2(x, y), Align2::LEFT_TOP, format!("Captured {}", if day == "Today" || day == "Yesterday" { day.to_lowercase() } else { format!("on {day}") }), text_font(14.5), rgba(214, 212, 208, 255));
        y += 24.0;
        let mut facts = vec![outcome(&c.title).to_string(), "2 players".to_string()];
        if c.other.trim().is_empty() {
            facts.pop();
        }
        if let Some(s) = length_of(&c) {
            facts.push(length_words(s));
        }
        p.text(pos2(x, y), Align2::LEFT_TOP, facts.join("  ·  "), text_font(14.5), rgba(214, 212, 208, 255));
        y += 40.0;

        // Jump into ArcheAge, or where that stands
        let state = self.launch_state(c.n);
        let playing = self.child.is_some();
        match state {
            Launch::Opening => {
                let b = Rect::from_min_size(pos2(x, y), vec2(w, 92.0));
                p.rect_filled(b, 10.0, gold_a(14));
                p.rect_stroke(b, 10.0, Stroke::new(1.0, gold_a(90)), egui::StrokeKind::Inside);
                ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(b.min + vec2(16.0, 15.0), vec2(22.0, 22.0))), |ui| {
                    ui.add(egui::Spinner::new().size(18.0).color(GOLD));
                });
                p.text(b.min + vec2(46.0, 16.0), Align2::LEFT_TOP, "Opening ArcheAge…", bold_font(15.5), INK);
                let g = p.layout("The moment opens once you're in the world. You can keep browsing.".into(), text_font(13.0), MUTED, w - 62.0);
                p.galley(b.min + vec2(46.0, 42.0), g, MUTED);
                y = b.bottom() + 14.0;
            }
            Launch::Running => {
                let b = Rect::from_min_size(pos2(x, y), vec2(w, 74.0));
                p.rect_filled(b, 10.0, rgba(59, 208, 127, 14));
                p.rect_stroke(b, 10.0, Stroke::new(1.0, rgba(59, 208, 127, 90)), egui::StrokeKind::Inside);
                p.circle_filled(b.min + vec2(24.0, 25.0), 5.0, GREEN);
                p.text(b.min + vec2(42.0, 15.0), Align2::LEFT_TOP, "ArcheAge is running", bold_font(15.5), INK);
                p.text(b.min + vec2(42.0, 41.0), Align2::LEFT_TOP, "The moment opens once you're in the world.", text_font(13.0), MUTED);
                y = b.bottom() + 14.0;
            }
            Launch::Failed(why) => {
                let g = p.layout(why, text_font(13.0), MUTED, w - 62.0);
                let b = Rect::from_min_size(pos2(x, y), vec2(w, 52.0 + g.size().y + 54.0));
                p.rect_filled(b, 10.0, rgba(224, 118, 90, 16));
                p.rect_stroke(b, 10.0, Stroke::new(1.0, rgba(224, 118, 90, 110)), egui::StrokeKind::Inside);
                icon(&p, b.min + vec2(26.0, 26.0), 19.0, Icon::Warning, RED);
                p.text(b.min + vec2(46.0, 16.0), Align2::LEFT_TOP, "ArcheAge didn't start", bold_font(15.5), INK);
                let gh = g.size().y;
                p.galley(b.min + vec2(46.0, 42.0), g, MUTED);
                ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(b.min + vec2(46.0, 50.0 + gh), vec2(w - 62.0, 38.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
                    if primary_button(ui, "Retry launch", Some(Icon::Refresh), false, 0.0, 36.0, !playing).clicked() {
                        *act = Some(Action::WatchReplay(c.n));
                    }
                });
                y = b.bottom() + 14.0;
            }
            Launch::None => {
                let can = !playing && !c.over_cap && !self.asking && !self.busy;
                let b = Rect::from_min_size(pos2(x, y), vec2(w, 46.0));
                let resp = ui.scope_builder(UiBuilder::new().max_rect(b), |ui| primary_button(ui, "Jump into ArcheAge", None, true, w, 46.0, can)).inner;
                let tip = if playing {
                    "ArcheAge is running: open Replays in the game to watch this one"
                } else if c.over_cap {
                    "Past your plan's limit: this capture can't be opened"
                } else {
                    "Starts ArcheAge and opens this moment once you're in the world"
                };
                if resp.on_hover_text(tip).clicked() {
                    *act = Some(Action::WatchReplay(c.n));
                }
                y = b.bottom() + 14.0;
                if playing {
                    let g = p.layout("ArcheAge is running: open Replays in the game to watch this one.".into(), text_font(13.0), MUTED, w);
                    let gh = g.size().y;
                    p.galley(pos2(x, y), g, MUTED);
                    y += gh + 10.0;
                }
            }
        }

        // past the plan's limit
        if c.over_cap {
            let line = if data.as_ref().is_some_and(|l| !l.upgrade_url.is_empty()) { "Your plan keeps fewer captures than you have: this one goes with your next recording. Upgrade to keep it." } else { "Your plan keeps fewer captures than you have: this one goes with your next recording." };
            let g = p.layout(line.into(), text_font(13.0), AMBER, w - 28.0);
            let gh = g.size().y;
            icon(&p, pos2(x + 8.0, y + 9.0), 15.0, Icon::Lock, AMBER);
            p.galley(pos2(x + 28.0, y), g, AMBER);
            y += gh + 12.0;
        }

        // Delete, asked on the spot
        if self.lib.deleting == Some(c.n) {
            ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(pos2(x, y), vec2(w, 32.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
                ui.add(egui::Spinner::new().size(16.0).color(RED));
                ui.label(RichText::new("Deleting…").font(text_font(13.5)).color(MUTED));
            });
            y += 40.0;
        } else if self.lib.confirm == Some(c.n) {
            let b = Rect::from_min_size(pos2(x, y), vec2(w, 112.0));
            p.rect_filled(b, 10.0, rgba(224, 118, 90, 14));
            p.rect_stroke(b, 10.0, Stroke::new(1.0, rgba(224, 118, 90, 110)), egui::StrokeKind::Inside);
            p.text(b.min + vec2(16.0, 14.0), Align2::LEFT_TOP, "Delete this capture?", bold_font(15.5), INK);
            p.text(b.min + vec2(16.0, 38.0), Align2::LEFT_TOP, "It's removed from the server. This can't be undone.", text_font(13.0), MUTED);
            ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_max(b.min + vec2(16.0, 64.0), b.max - vec2(16.0, 12.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if outline_button(ui, "Delete", Some(Icon::Trash), false, 110.0, 34.0, true).clicked() {
                    *act = Some(Action::DeleteReplay(c.n));
                }
                if outline_button(ui, "Keep", None, false, 84.0, 34.0, false).clicked() {
                    self.lib.confirm = None;
                }
            });
            y = b.bottom() + 12.0;
        } else {
            ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(pos2(x - 2.0, y), vec2(w, 32.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
                if link_button(ui, Icon::Trash, "Delete", true).on_hover_text("Delete this capture from the server").clicked() {
                    self.lib.confirm = Some(c.n);
                    self.lib.note = None;
                }
            });
            y += 40.0;
        }

        // sharing
        rule(&p, x, x + w, y + 6.0);
        y += 26.0;
        caps(&p, pos2(x, y), "Sharing", MUTED);
        y += 30.0;
        icon(&p, pos2(x + 10.0, y + 14.0), 20.0, Icon::Lock, INK);
        p.text(pos2(x + 34.0, y), Align2::LEFT_TOP, "Private", bold_font(15.0), INK);
        p.text(pos2(x + 34.0, y + 23.0), Align2::LEFT_TOP, "Only you can see this capture.", text_font(13.5), rgba(214, 212, 208, 255));
        y += 58.0;
        rule(&p, x, x + w, y);

        if let Some(l) = &data {
            let plan = Rect::from_min_max(pos2(x, (y + 20.0).max(r.bottom() - 132.0)), pos2(x + w, r.bottom() - 24.0));
            if plan.height() > 80.0 {
                self.plan_box(ui, plan, l, act);
            }
        }
    }

    /// The plan, as the server last said it, with Upgrade when the server gives a page for it.
    fn plan_box(&mut self, ui: &mut egui::Ui, r: Rect, l: &Library, act: &mut Option<Action>) {
        let p = ui.painter().clone();
        let soon = ending_soon(l);
        caps(&p, r.min, "Your plan", MUTED);
        let (name, line) = plan_words(l);
        let line = match soon {
            Some(0) => "Replay+ has ended: the server will say Free on the next refresh".to_string(),
            Some(d) => format!("{line} · {d} day{} left", if d == 1 { "" } else { "s" }),
            None => line,
        };
        let nr = p.text(r.min + vec2(0.0, 26.0), Align2::LEFT_TOP, &name, bold_font(16.0), if l.is_sub() { GOLD_HI } else { INK });
        if l.is_sub() {
            icon(&p, pos2(nr.right() + 14.0, nr.center().y), 14.0, Icon::Star, if soon.is_some() { AMBER } else { GOLD });
        }
        let g = p.layout(line, text_font(13.0), if soon.is_some() { AMBER } else { MUTED }, r.width());
        p.galley(r.min + vec2(0.0, 50.0), g, MUTED);
        if !l.is_sub() && !l.upgrade_url.is_empty() {
            ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(pos2(r.right() - 120.0, r.top() + 18.0), vec2(120.0, 34.0))).layout(Layout::right_to_left(Align::Center)), |ui| {
                if outline_button(ui, "Upgrade", None, true, 0.0, 32.0, false).on_hover_text("Replay+ keeps far more of your captures. Opens the server's page.").clicked() {
                    *act = Some(Action::Upgrade);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_split_into_outcome_and_length() {
        assert_eq!(split_length("beat Opponent 0:42"), ("beat Opponent", Some("0:42")));
        assert_eq!(split_length("fought 10 min"), ("fought 10 min", None));
        assert_eq!(outcome("beat Opponent 0:42"), "Won");
        assert_eq!(outcome("lost to Opponent 1-2 1:14"), "Lost");
        assert_eq!(outcome("left the ring 0:12"), "Fight");
        assert_eq!(display_title("beat Opponent 0:42"), "Beat Opponent");
        assert_eq!(display_title(""), "Untitled capture");
    }

    #[test]
    fn lengths_read_as_people_say_them() {
        assert_eq!(seconds("0:42"), Some(42));
        assert_eq!(seconds("2:18"), Some(138));
        assert_eq!(seconds("1:02:03"), Some(3723));
        assert_eq!(seconds("x"), None);
        assert_eq!(length_words(42), "42s");
        assert_eq!(length_words(138), "2m 18s");
        assert_eq!(length_words(3723), "1h 02m");
    }

    fn rep(n: i32, title: &str, when: i64, other: &str) -> Replay {
        Replay { n, title: title.into(), when, me: "Me".into(), other: other.into(), over_cap: false }
    }

    #[test]
    fn the_page_searches_filters_and_sorts() {
        let l = Library { replays: vec![rep(1, "beat Ren 0:42", 100, "Ren"), rep(2, "lost to Ivy 2:18", 300, "Ivy"), rep(3, "beat Ivy 1:05", 200, "Ivy")], ..Default::default() };
        let ns = |v: Vec<Replay>| v.iter().map(|r| r.n).collect::<Vec<_>>();
        assert_eq!(ns(shown(&l, "", 0, 0)), vec![2, 3, 1]);
        assert_eq!(ns(shown(&l, "", 0, 1)), vec![1, 3, 2]);
        assert_eq!(ns(shown(&l, "", 0, 2)), vec![2, 3, 1]);
        assert_eq!(ns(shown(&l, "ivy", 0, 0)), vec![2, 3]);
        assert_eq!(ns(shown(&l, "", 1, 0)), vec![2, 3, 1]);
        // nothing is shared until sharing exists on the server
        assert!(shown(&l, "", 2, 0).is_empty());
    }

    #[test]
    fn the_plan_is_said_as_the_server_gave_it() {
        let free = Library { plan: "free".into(), cap: 3, ..Default::default() };
        assert_eq!(plan_words(&free).0, "Free");
        assert!(plan_words(&free).1.contains("newest 3"));
        let sub = Library { plan: "sub".into(), cap: 50, plan_expires_at: Some("2027-01-01T00:00:00Z".into()), ..Default::default() };
        assert_eq!(plan_words(&sub), ("Replay+".into(), "Keeping up to 50 recordings · until 1 Jan 2027".into()));
        let soon = Library { plan: "sub".into(), plan_expires_at: Some(iso(library::now_unix() + 2 * 86_400)), ..Default::default() };
        assert_eq!(ending_soon(&soon), Some(2));
        assert_eq!(ending_soon(&sub), None);
        assert_eq!(ending_soon(&free), None);
    }

    fn iso(t: i64) -> String {
        let d = t.div_euclid(86_400);
        let s = t.rem_euclid(86_400);
        // the inverse of library::date for the test only, through the same day counting
        let day = library::date(d * 86_400);
        let mut it = day.split(' ');
        let (dd, mon, y) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap());
        let m = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"].iter().position(|x| *x == mon).unwrap() + 1;
        format!("{y}-{m:02}-{:02}T{:02}:{:02}:{:02}Z", dd.parse::<u32>().unwrap(), s / 3600, s % 3600 / 60, s % 60)
    }

    #[test]
    fn only_a_web_address_is_opened() {
        assert!(open_upgrade("file:///C:/Windows/System32/calc.exe").is_err());
        assert!(open_upgrade("C:\\evil.exe").is_err());
        assert!(open_upgrade("https://x.invalid/a b").is_err());
    }

    #[test]
    fn a_watch_is_taken_once() {
        let mut l = Lib { pending: Some((5, Instant::now())), ..Default::default() };
        assert_eq!(l.take_pending(), Some(5));
        assert_eq!(l.take_pending(), None);
    }
}
