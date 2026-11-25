//! The Library: the account's own recordings, kept on the server. Each is a card with Watch and Delete; a strip at
//! the top says the plan and how many recordings it keeps. Everything shown is what the server last said (net.rs
//! `list_replays`); nothing about the plan or the cap is worked out here.
//!
//! Watch starts the game as Play does and leaves the recording for the game's own replay player (library.rs
//! `set_pending_watch`), which opens it once the game is in the world. The launcher never handles a fight's packets.

use crate::gui::events::Event;
use crate::gui::theme::*;
use crate::gui::widgets::{ghost_button, gold_button};
use crate::gui::{Action, App, Tab};
use crate::library;
use crate::net::{self, Library, Replay, Server};
use eframe::egui::{self, Align, Align2, Color32, FontId, Layout, Rect, RichText, Sense, Stroke, UiBuilder, pos2, vec2};
use std::time::{Duration, Instant};

/// How long a Watch waits for the game to start before it is dropped (setup or sign-in failed meanwhile).
const WATCH_FOR: Duration = Duration::from_secs(300);

/// The Library's state between frames.
#[derive(Default)]
pub struct Lib {
    /// the last list: the server's answer, or the cached one until it comes
    pub data: Option<Library>,
    /// a fetch was made since the app started (or since something changed the list)
    pub fresh: bool,
    pub loading: bool,
    /// the last fetch failed: why
    pub error: Option<String>,
    /// the card asking "Delete this replay?"
    pub confirm: Option<i32>,
    /// the delete on its way to the server
    pub deleting: Option<i32>,
    /// the recording Watch asked for, until the game starts
    pub pending: Option<(i32, Instant)>,
    /// the last thing pressed came to this: Ok(what to say) or Err(what went wrong)
    pub note: Option<Result<String, String>>,
}

impl Lib {
    /// The recording to leave for the game, taken once (a Watch older than `WATCH_FOR` is dropped).
    pub fn take_pending(&mut self) -> Option<i32> {
        self.pending.take().filter(|(_, at)| at.elapsed() < WATCH_FOR).map(|(n, _)| n)
    }
}

/// What a fight's title says it came to: won, lost, or neither (left, 10 min).
fn outcome(title: &str) -> (Color32, &'static str) {
    if title.starts_with("beat ") {
        (GOLD_HI, "WON")
    } else if title.starts_with("lost to ") {
        (RED, "LOST")
    } else {
        (INK, "FIGHT")
    }
}

/// "beat Opponent 0:42" -> ("beat Opponent", Some("0:42")).
fn split_length(title: &str) -> (&str, Option<&str>) {
    match title.rsplit_once(' ') {
        Some((head, tail)) if tail.contains(':') && tail.chars().all(|c| c.is_ascii_digit() || c == ':') => (head, Some(tail)),
        _ => (title, None),
    }
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

/// Days left on a sub, when it ends within five days (the badge turns amber).
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

    /// Watch: the game starts as with Play, and opens the recording once it is in the world.
    pub(in crate::gui) fn watch_replay(&mut self, n: i32) {
        if self.child.is_some() {
            return self.lib.note = Some(Err("The game is already running: open Replays in the game to watch it.".into()));
        }
        self.lib.pending = Some((n, Instant::now()));
        self.lib.note = Some(Ok("Starting the game: the replay opens once you are in the world.".into()));
        self.press_play();
        if self.hint.is_some() {
            // Play could not even start (no sign-in, no game folder): nothing waits for the game
            self.lib.pending = None;
            self.lib.note = self.hint.clone().map(Err);
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
                self.lib.note = Some(Ok("Replay deleted.".into()));
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

    pub(in crate::gui) fn library_tab(&mut self, ui: &mut egui::Ui, body: Rect, act: &mut Option<Action>) {
        if !self.lib.fresh && !self.lib.loading && self.secret().is_some() {
            self.fetch_library();
        }
        let p = ui.painter().clone();
        let inner = body.shrink2(vec2(48.0, 0.0));

        // header: the title on the left, the plan on the right
        let head = Rect::from_min_size(pos2(inner.left(), body.top() + 24.0), vec2(inner.width(), 64.0));
        shadow_text(&p, head.min, Align2::LEFT_TOP, "REPLAYS  ·  EVERY FIGHT, AGAIN", title_font(12.5), INK);
        gold_title(&p, head.min + vec2(-2.0, 16.0), Align2::LEFT_TOP, "Your replays", title_font(38.0));
        shadow_text(&p, head.min + vec2(0.0, 62.0), Align2::LEFT_TOP, "Your own AA Classic fights, recorded on this PC and kept on the server.", FontId::proportional(13.5), MUTED);
        let strip = Rect::from_min_size(pos2(head.right() - 470.0, head.top()), vec2(470.0, 62.0));
        self.plan_strip(ui, strip, act);

        // the last press, or a failed refresh, in one line under the header
        let line_y = head.bottom() + 22.0;
        let line = match (&self.lib.note, &self.lib.error) {
            (Some(Ok(t)), _) => Some((t.clone(), GREEN)),
            (Some(Err(t)), _) => Some((t.clone(), RED)),
            (None, Some(e)) if self.lib.data.is_some() => Some((format!("Could not refresh: {e}"), AMBER)),
            _ => None,
        };
        if let Some((t, c)) = line {
            p.text(pos2(inner.left(), line_y), Align2::LEFT_TOP, t, FontId::proportional(13.0), c);
        }

        let list = Rect::from_min_max(pos2(inner.left(), line_y + 26.0), pos2(inner.right(), body.bottom() - 14.0));
        if self.secret().is_none() {
            return self.library_message(ui, list, Icon::Lock, "Sign in to see your replays", "Your recordings are kept with your account on the server.", Some(("Go to sign-in", Act::Play)), act);
        }
        if !matches!(Server::parse(&self.cfg.login_ip), Ok(Server::Api { .. })) {
            return self.library_message(ui, list, Icon::Film, "No replay library on this server", "Replays are kept by servers with an account API (a server link).", None, act);
        }
        let Some(data) = self.lib.data.clone() else {
            if let Some(e) = self.lib.error.clone() {
                return self.library_message(ui, list, Icon::Refresh, "Your replays could not be fetched", &e, Some(("Try again", Act::Refresh)), act);
            }
            ui.scope_builder(UiBuilder::new().max_rect(Rect::from_center_size(list.center() - vec2(0.0, 30.0), vec2(300.0, 60.0))).layout(Layout::top_down(Align::Center)), |ui| {
                ui.add(egui::Spinner::new().size(26.0).color(GOLD));
                ui.label(RichText::new("Getting your replays…").size(14.0).color(MUTED));
            });
            return;
        };
        if data.replays.is_empty() {
            let set_up = self.rec.set_up;
            let (sub, button) = if set_up {
                ("Recordings appear here automatically while you play.", None)
            } else {
                ("Recordings appear here automatically while you play. Turn recording on first.", Some(("Set up recording", Act::RecOn)))
            };
            return self.library_message(ui, list, Icon::Film, "No replays yet", sub, button, act);
        }

        let gap = 16.0;
        let cw = ((list.width() - gap - 6.0) / 2.0).floor();
        let ch = 120.0;
        let playing = self.child.is_some();
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(gap, gap);
                for pair in data.replays.chunks(2) {
                    ui.horizontal(|ui| {
                        for r in pair {
                            self.replay_card(ui, vec2(cw, ch), r, &data, playing, act);
                        }
                    });
                }
                ui.add_space(4.0);
            });
        });
    }

    fn plan_strip(&mut self, ui: &mut egui::Ui, r: Rect, act: &mut Option<Action>) {
        let p = ui.painter().clone();
        let Some(l) = self.lib.data.clone() else {
            // no plan known yet: just the refresh button
            ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(pos2(r.right() - 34.0, r.center().y - 17.0), vec2(34.0, 34.0))), |ui| {
                if !self.lib.loading && icon_button(ui, Icon::Refresh, 34.0, "Refresh", false).clicked() {
                    *act = Some(Action::RefreshLibrary);
                }
            });
            return;
        };
        let sub = l.is_sub();
        card(&p, r);
        let badge_c = pos2(r.left() + 32.0, r.center().y);
        p.circle_filled(badge_c, 18.0, rgba(0, 0, 0, 140));
        p.circle_stroke(badge_c, 18.0, Stroke::new(1.2, if sub { GOLD } else { gold_a(90) }));
        if sub {
            icon(&p, badge_c, 22.0, Icon::Star, ending_soon(&l).map(|_| AMBER).unwrap_or(GOLD_HI));
        } else {
            icon(&p, badge_c, 18.0, Icon::Film, GOLD);
        }
        let (name, line) = plan_words(&l);
        let tx = r.left() + 62.0;
        p.text(pos2(tx, r.top() + 10.0), Align2::LEFT_TOP, &name, title_font(19.0), if sub { GOLD_HI } else { INK });
        let line_col = if ending_soon(&l).is_some() { AMBER } else { MUTED };
        let line = match ending_soon(&l) {
            Some(0) => "Replay+ has ended: the server will say Free on the next refresh".to_string(),
            Some(d) => format!("{line} · {d} day{} left", if d == 1 { "" } else { "s" }),
            None => line,
        };
        let buttons = if !sub && !l.upgrade_url.is_empty() { 140.0 } else { 52.0 };
        let g = p.layout(line, FontId::proportional(12.0), line_col, r.width() - 62.0 - buttons);
        p.galley(pos2(tx, r.top() + 35.0), g, line_col);
        ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_max(pos2(r.right() - 150.0, r.top()), r.max - vec2(12.0, 0.0))).layout(Layout::right_to_left(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if self.lib.loading {
                ui.add(egui::Spinner::new().size(18.0).color(GOLD));
            } else if icon_button(ui, Icon::Refresh, 30.0, "Refresh", false).clicked() {
                *act = Some(Action::RefreshLibrary);
            }
            if !sub && !l.upgrade_url.is_empty()
                && gold_button(ui, "Upgrade", 92.0, 30.0, true).on_hover_text("Replay+ keeps far more of your recordings. Opens the server's page.").clicked()
            {
                *act = Some(Action::Upgrade);
            }
        });
    }

    /// One recording, as the Arena window's setup cards: its picture (in colour, or grey past the plan's cap), the
    /// fight's name in the title serif, who fought in gold, when; Watch in gold and Delete beside it.
    fn replay_card(&mut self, ui: &mut egui::Ui, size: egui::Vec2, r: &Replay, l: &Library, playing: bool, act: &mut Option<Action>) {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let hovered = ui.rect_contains_pointer(rect);
        let p = ui.painter().clone();
        let dim = r.over_cap;
        let fade = |c: Color32| if dim { c.gamma_multiply(0.6) } else { c };
        card(&p, rect);
        if hovered {
            p.rect_stroke(rect, 2.0, Stroke::new(1.0, gold_a(120)), egui::StrokeKind::Inside);
        }

        // the picture: by how the fight ended; clicking it watches
        let (accent, word) = outcome(&r.title);
        let pics = &self.pics;
        let tex = match (word, dim) {
            ("WON", false) => &pics.won,
            ("WON", true) => &pics.won_grey,
            ("LOST", false) => &pics.lost,
            ("LOST", true) => &pics.lost_grey,
            (_, false) => &pics.fight,
            (_, true) => &pics.fight_grey,
        };
        let th = size.y - 22.0;
        let thumb = Rect::from_min_size(rect.min + vec2(11.0, 11.0), vec2((th * 1.6).round(), th));
        let tile = ui.interact(thumb, ui.scope_id().with(("watch", r.n)), if playing { Sense::hover() } else { Sense::click() });
        let lit = tile.hovered() && !playing;
        crate::gui::pictures::draw(&p, tex, thumb, if lit { Color32::WHITE } else { Color32::from_gray(if dim { 150 } else { 225 }) });
        vgrad(&p, Rect::from_min_max(thumb.min, pos2(thumb.right(), thumb.top() + 24.0)), rgba(0, 0, 0, 170), rgba(0, 0, 0, 0));
        p.rect_stroke(thumb, 0.0, Stroke::new(1.0, if lit { GOLD } else { gold_a(110) }), egui::StrokeKind::Inside);
        let label = match word { "WON" => "WON", "LOST" => "LOST", _ => "LEFT THE RING" };
        p.text(thumb.left_top() + vec2(8.0, 6.0), Align2::LEFT_TOP, label, title_font(11.5), fade(accent));
        if dim {
            p.circle_filled(thumb.center() + vec2(0.0, 4.0), 18.0, rgba(0, 0, 0, 170));
            p.circle_stroke(thumb.center() + vec2(0.0, 4.0), 18.0, Stroke::new(1.0, GOLD));
            icon(&p, thumb.center() + vec2(0.0, 4.0), 18.0, Icon::Lock, GOLD_HI);
        } else if lit {
            p.circle_filled(thumb.center() + vec2(0.0, 4.0), 20.0, rgba(0, 0, 0, 150));
            p.circle_stroke(thumb.center() + vec2(0.0, 4.0), 20.0, Stroke::new(1.2, GOLD_HI));
            icon(&p, thumb.center() + vec2(1.5, 4.0), 18.0, Icon::Play, GOLD_HI);
        }
        let tip = if playing { "The game is running: open Replays in the game to watch this one" } else { "Watch: the game starts and opens this replay" };
        if tile.on_hover_text(tip).clicked() {
            *act = Some(Action::WatchReplay(r.n));
        }

        // the words
        let x = thumb.right() + 16.0;
        let (head, length) = split_length(&r.title);
        let mut title = head.to_string();
        if let Some(c) = title.get(..1) {
            title = c.to_uppercase() + &title[1..];
        }
        let tg = p.layout_no_wrap(title, title_font(19.0), fade(INK));
        let tw = tg.size().x;
        p.galley(pos2(x, rect.top() + 11.0), tg, fade(INK));
        if let Some(len) = length {
            let chip = Rect::from_min_size(pos2(x + tw + 10.0, rect.top() + 14.0), vec2(42.0, 18.0));
            p.rect_filled(chip, 2.0, rgba(0, 0, 0, 140));
            p.rect_stroke(chip, 2.0, Stroke::new(1.0, gold_a(60)), egui::StrokeKind::Inside);
            p.text(chip.center() + vec2(0.0, 0.5), Align2::CENTER_CENTER, len, title_font(12.0), fade(GOLD));
        }
        let me = if r.me.trim().is_empty() { "You" } else { r.me.as_str() };
        let vs = if r.other.trim().is_empty() { me.to_string() } else { format!("{me}  ·  {}", r.other) };
        p.text(pos2(x, rect.top() + 39.0), Align2::LEFT_TOP, vs, FontId::proportional(13.0), fade(GOLD));
        let low = pos2(x, rect.top() + 59.0);
        if dim {
            icon(&p, low + vec2(6.0, 8.0), 12.0, Icon::Lock, AMBER);
            let t = if l.upgrade_url.is_empty() { "Past your plan's limit: goes with your next fight" } else { "Past your plan's limit: Upgrade to keep it" };
            p.text(low + vec2(18.0, 1.0), Align2::LEFT_TOP, t, FontId::proportional(12.0), AMBER);
        } else {
            icon(&p, low + vec2(6.0, 8.0), 12.0, Icon::Clock, MUTED);
            p.text(low + vec2(18.0, 1.0), Align2::LEFT_TOP, library::when(r.when, library::now_unix()), FontId::proportional(12.5), MUTED);
        }

        // Watch and Delete along the bottom; Delete asks on the card itself
        let row = Rect::from_min_max(pos2(x, rect.bottom() - 40.0), pos2(rect.right() - 12.0, rect.bottom() - 11.0));
        if self.lib.deleting == Some(r.n) {
            ui.scope_builder(UiBuilder::new().max_rect(row).layout(Layout::right_to_left(Align::Center)), |ui| {
                ui.add(egui::Spinner::new().size(18.0).color(RED));
            });
        } else if self.lib.confirm == Some(r.n) {
            let ask = Rect::from_min_max(pos2(x - 6.0, rect.top() + 8.0), rect.max - vec2(8.0, 8.0));
            p.rect_filled(ask, 3.0, rgba(36, 12, 9, 245));
            p.rect_stroke(ask, 3.0, Stroke::new(1.0, RED.gamma_multiply(0.8)), egui::StrokeKind::Inside);
            p.text(pos2(ask.left() + 14.0, ask.top() + 12.0), Align2::LEFT_TOP, "Delete this replay?", title_font(18.0), INK);
            p.text(pos2(ask.left() + 14.0, ask.top() + 38.0), Align2::LEFT_TOP, "This can't be undone.", FontId::proportional(12.5), MUTED);
            ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_max(pos2(ask.left() + 14.0, ask.bottom() - 40.0), ask.max - vec2(12.0, 10.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if ui.add_sized(vec2(84.0, 28.0), egui::Button::new(RichText::new("Delete").color(Color32::WHITE)).fill(Color32::from_rgb(150, 44, 34)).stroke(Stroke::new(1.0, RED))).clicked() {
                    *act = Some(Action::DeleteReplay(r.n));
                }
                if ghost_button(ui, "Keep", 76.0, 28.0, false).clicked() {
                    self.lib.confirm = None;
                }
            });
        } else {
            ui.scope_builder(UiBuilder::new().max_rect(row).layout(Layout::left_to_right(Align::Center)), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if gold_button(ui, "Watch", 86.0, 28.0, !playing).on_hover_text(tip).clicked() {
                    *act = Some(Action::WatchReplay(r.n));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ghost_button(ui, "Delete", 76.0, 28.0, true).on_hover_text("Delete this replay").clicked() {
                        self.lib.confirm = Some(r.n);
                        self.lib.note = None;
                    }
                });
            });
        }
    }

    fn library_message(&mut self, ui: &mut egui::Ui, area: Rect, ic: Icon, title: &str, sub: &str, button: Option<(&str, Act)>, act: &mut Option<Action>) {
        let c = area.center() - vec2(0.0, 40.0);
        let p = ui.painter();
        p.circle_filled(c, 40.0, gold_a(18));
        p.circle_stroke(c, 40.0, Stroke::new(1.0, gold_a(70)));
        icon(p, c, 38.0, ic, GOLD);
        ui.scope_builder(UiBuilder::new().max_rect(Rect::from_center_size(c + vec2(0.0, 96.0), vec2(560.0, 110.0))).layout(Layout::top_down(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(RichText::new(title).font(title_font(22.0)).color(INK));
            ui.add(egui::Label::new(RichText::new(sub).size(13.5).color(MUTED)).wrap());
            if let Some((label, what)) = button {
                ui.add_space(8.0);
                if gold_button(ui, label, 0.0, 34.0, true).clicked() {
                    match what {
                        Act::Play => self.tab = Tab::Play,
                        Act::Refresh => *act = Some(Action::RefreshLibrary),
                        Act::RecOn => *act = Some(Action::RecOn),
                    }
                }
            }
        });
    }
}

/// What a message's button does.
#[derive(Clone, Copy)]
enum Act {
    Play,
    Refresh,
    RecOn,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_split_into_outcome_and_length() {
        assert_eq!(split_length("beat Opponent 0:42"), ("beat Opponent", Some("0:42")));
        assert_eq!(split_length("fought 10 min"), ("fought 10 min", None));
        assert_eq!(outcome("beat Opponent 0:42").1, "WON");
        assert_eq!(outcome("lost to Opponent 1-2 1:14").1, "LOST");
        assert_eq!(outcome("left the ring 0:12").1, "FIGHT");
    }

    #[test]
    fn the_plan_is_said_as_the_server_gave_it() {
        let free = Library { plan: "free".into(), cap: 3, ..Default::default() };
        assert_eq!(plan_words(&free).0, "Free");
        assert!(plan_words(&free).1.contains("newest 3"));
        let sub = Library { plan: "sub".into(), cap: 50, plan_expires_at: Some("2027-01-01T00:00:00Z".into()), ..Default::default() };
        assert_eq!(plan_words(&sub), ("Replay+".into(), "Keeping up to 50 recordings · until 1 Jan 2027".into()));
        let soon = Library { plan: "sub".into(), plan_expires_at: Some(format!("{}", iso(library::now_unix() + 2 * 86_400))), ..Default::default() };
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
