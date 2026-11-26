//! Shared with me, Saved, Community (Discover, Following) and Guild shares: other people's moments as the server
//! lists them for this account, and the inspector of the picked one (Jump into ArcheAge, Save, its discussion). The
//! server decides what each feed holds; the page only searches and sorts what it was given.

use super::library::{display_title, length_words};
use super::shares::{Do, Op, Watching, scope_of};
use crate::gui::theme::*;
use crate::gui::widgets::{EDGE, chip, empty_state, input_box, link_button, outline_button, primary_button, search_box};
use crate::gui::{Action, App, Page};
use crate::library;
use crate::net::Server;
use crate::shares::Share;
use eframe::egui::{self, Align, Align2, Color32, Layout, Rect, RichText, Sense, Stroke, UiBuilder, pos2, vec2};

/// Community's kind tabs, and the kind of moment each one shows.
pub const KINDS: [(&str, &str); 6] = [("All", ""), ("Duels", "duel"), ("Tournaments", "tournament"), ("Sieges", "siege"), ("Naval", "naval"), ("Trade runs", "trade")];

/// "duel" -> "Duel".
pub fn kind_label(kind: &str) -> String {
    match kind {
        "duel" | "" => "Duel".into(),
        "tournament" => "Tournament".into(),
        "siege" => "Siege".into(),
        "naval" => "Naval".into(),
        "trade" => "Trade run".into(),
        other => {
            let mut c = other.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

pub fn share_title(s: &Share) -> String {
    display_title(&s.title)
}

/// The day it was shared ("Today", "3 Nov 2026"), from the server's time.
pub fn shared_day(s: &Share) -> String {
    library::parse_iso(&s.created).map(|t| library::day(t * 1000, library::now_unix())).unwrap_or_default()
}

/// The shares a page lists: its search over the title and who was in it, then Community's kind tab.
pub fn filtered(list: &[Share], query: &str, kind: &str) -> Vec<Share> {
    let q = query.trim().to_lowercase();
    list.iter()
        .filter(|s| q.is_empty() || [&s.title, &s.by, &s.me, &s.other, &s.note].iter().any(|t| t.to_lowercase().contains(&q)))
        .filter(|s| kind.is_empty() || s.kind == kind || (kind == "duel" && s.kind.is_empty()))
        .cloned()
        .collect()
}

/// A feed page's heading, its search's hint, and what it says while it has nothing.
struct Words {
    title: &'static str,
    count: &'static str,
    hint: &'static str,
    icon: Icon,
    empty: &'static str,
    sub: &'static str,
}

fn words(page: Page) -> Words {
    match page {
        Page::Saved => Words {
            title: "Saved replays",
            count: "saved",
            hint: "Search by title or player",
            icon: Icon::Bookmark,
            empty: "No saved moments",
            sub: "Save a moment from Community or from a share, and it stays here for you.",
        },
        Page::Discover => Words {
            title: "Community",
            count: "posts",
            hint: "Search community",
            icon: Icon::Compass,
            empty: "No community posts yet",
            sub: "Moments players share with everyone on this server show here, newest first. Share one of yours from My captures.",
        },
        Page::Following => Words {
            title: "Following",
            count: "posts",
            hint: "Search following",
            icon: Icon::Rss,
            empty: "Nothing from players you follow",
            sub: "Follow a player from one of their shared moments, and what they share next shows here.",
        },
        Page::GuildShares => Words {
            title: "Guild shares",
            count: "shared",
            hint: "Search guild shares",
            icon: Icon::Layers,
            empty: "No guild shares yet",
            sub: "Moments your guild's members share with the guild show here.",
        },
        _ => Words {
            title: "Shared with me",
            count: "shared with you",
            hint: "Search by title or player",
            icon: Icon::People,
            empty: "Nothing shared with you yet",
            sub: "When a friend or your guild shares a moment, it shows here. Jump into ArcheAge plays it in the game.",
        },
    }
}

impl App {
    fn pick(&mut self, page: Page, id: i64) {
        if self.hub.picked.insert(page, id) != Some(id) {
            self.hub.reply.clear();
        }
    }

    /// A feed page: heading, search (and Community's kinds), then the shares as cards.
    pub(in crate::gui) fn feed_page(&mut self, ui: &mut egui::Ui, centre: Rect, act: &mut Option<Action>) {
        let page = self.page;
        let Some(scope) = scope_of(page) else { return };
        let wd = words(page);
        let p = ui.painter().clone();
        let (x0, x1) = (centre.left() + 24.0, centre.right() - 24.0);

        let t = p.text(pos2(x0 + 4.0, centre.top() + 22.0), Align2::LEFT_TOP, wd.title, title_font(27.0), INK);
        if let Some(f) = self.hub.feed(scope) {
            let n = f.shares.len();
            p.text(pos2(t.right() + 14.0, t.center().y + 2.0), Align2::LEFT_CENTER, format!("{n}{} {}", if f.next.is_some() { "+" } else { "" }, wd.count), text_font(14.5), MUTED);
        }
        let loading = self.hub.feeds.get(&scope).is_some_and(|f| f.loading);
        let refresh = Rect::from_center_size(pos2(x1 - 16.0, t.center().y + 1.0), vec2(32.0, 32.0));
        ui.scope_builder(UiBuilder::new().max_rect(refresh), |ui| {
            if loading {
                ui.add(egui::Spinner::new().size(18.0).color(GOLD));
            } else if icon_button(ui, Icon::Refresh, 32.0, "Ask the server again", false).clicked() {
                *act = Some(Action::Shares(Do::Refresh));
            }
        });
        if page == Page::Discover {
            let b = Rect::from_min_max(pos2(refresh.left() - 176.0, t.center().y - 21.0), pos2(refresh.left() - 12.0, t.center().y + 21.0));
            ui.scope_builder(UiBuilder::new().max_rect(b), |ui| {
                if outline_button(ui, "Share clip", Some(Icon::Share), false, b.width(), 42.0, false).on_hover_text("Share one of your captures (My captures)").clicked() {
                    *act = Some(Action::OpenShare(self.lib.selected));
                }
            });
        }

        // search, and Community's kinds
        let mut y = t.bottom() + 18.0;
        let bar = Rect::from_min_max(pos2(x0, y), pos2(x1, y + 40.0));
        ui.scope_builder(UiBuilder::new().max_rect(bar), |ui| {
            search_box(ui, self.hub.queries.entry(page).or_default(), wd.hint, bar.width(), 40.0);
        });
        y = bar.bottom() + 16.0;
        let tabs = matches!(page, Page::Discover | Page::Following);
        if tabs {
            let row = Rect::from_min_max(pos2(x0, y), pos2(x1, y + 40.0));
            ui.scope_builder(UiBuilder::new().max_rect(row).layout(Layout::left_to_right(Align::Center)), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (i, (label, _)) in KINDS.iter().enumerate() {
                    if chip(ui, label, self.hub.kind == i, false, 38.0).clicked() {
                        self.hub.kind = i;
                    }
                }
            });
            rule(&p, x0, x1, row.bottom() + 10.0);
            y = row.bottom() + 26.0;
        }

        let area = Rect::from_min_max(pos2(x0, y), pos2(x1, centre.bottom()));
        let empty = Rect::from_min_max(area.min, area.max - vec2(0.0, 24.0));
        if !matches!(Server::parse(&self.cfg.login_ip), Ok(Server::Api { .. })) {
            return empty_state(ui, empty, wd.icon, "No sharing on this server", "Shared moments live on servers with an account API (a server link).");
        }
        let Some(feed) = self.hub.feed(scope).cloned() else {
            if let Some(e) = self.hub.feeds.get(&scope).and_then(|f| f.error.clone()) {
                empty_state(ui, empty, Icon::Refresh, "This page could not be fetched", &e);
                return self.empty_button(ui, empty, "Try again", Action::Shares(Do::Refresh), act);
            }
            ui.scope_builder(UiBuilder::new().max_rect(Rect::from_center_size(empty.center() - vec2(0.0, 40.0), vec2(300.0, 60.0))).layout(Layout::top_down(Align::Center)), |ui| {
                ui.add(egui::Spinner::new().size(26.0).color(GOLD));
                ui.label(RichText::new("Asking the server…").font(text_font(14.0)).color(MUTED));
            });
            return;
        };
        if feed.shares.is_empty() {
            return empty_state(ui, empty, wd.icon, wd.empty, wd.sub);
        }
        let (kind_name, kind) = if tabs { KINDS[self.hub.kind] } else { KINDS[0] };
        let query = self.hub.queries.get(&page).cloned().unwrap_or_default();
        let list = filtered(&feed.shares, &query, kind);
        if list.is_empty() {
            if !query.trim().is_empty() {
                return empty_state(ui, empty, Icon::Search, "Nothing matches", "Try other words, or clear the search.");
            }
            return empty_state(ui, empty, wd.icon, &format!("No {} here yet", kind_name.to_lowercase()), &format!("No one has shared {} here yet. They show under {kind_name} as soon as someone does.", kind_name.to_lowercase()));
        }
        if !self.hub.picked.get(&page).is_some_and(|id| list.iter().any(|s| s.id == *id)) {
            self.pick(page, list[0].id);
        }

        ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
            egui::ScrollArea::vertical().id_salt(("feed", page as u8)).auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(6.0);
                let gap = 16.0;
                let avail = ui.available_width() - 10.0;
                ui.spacing_mut().item_spacing = vec2(gap, gap);
                let mut rest: &[Share] = &list;
                if page == Page::Discover {
                    let h = (avail * 0.29).clamp(220.0, 300.0);
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        self.focus_post(ui, vec2(avail, h), &list[0], act);
                    });
                    rest = &list[1..];
                }
                let cols = if page == Page::Discover { 2 } else if avail > 900.0 { 4 } else if avail > 520.0 { 3 } else { 2 };
                let cw = ((avail - gap * (cols - 1) as f32) / cols as f32).floor();
                for row in rest.chunks(cols) {
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        for s in row {
                            if page == Page::Discover {
                                self.post_card(ui, vec2(cw, (cw * 0.40).round() + 104.0), s, act);
                            } else {
                                self.share_card(ui, vec2(cw, (cw * 0.56).round() + 62.0), s, act);
                            }
                        }
                    });
                }
                if feed.next.is_some() {
                    ui.vertical_centered(|ui| {
                        let label = if page == Page::Discover { "More community posts" } else { "More" };
                        if loading {
                            ui.add(egui::Spinner::new().size(18.0).color(GOLD));
                        } else if outline_button(ui, label, Some(Icon::ChevronRight), false, 0.0, 38.0, false).clicked() {
                            *act = Some(Action::Shares(Do::More(scope)));
                        }
                    });
                }
                ui.add_space(16.0);
            });
        });
    }

    /// Clicks on a card: a click picks it, a double click jumps into ArcheAge.
    fn card_clicks(&mut self, resp: egui::Response, s: &Share, act: &mut Option<Action>) {
        let resp = resp.on_hover_text(if self.child.is_some() { "Double-click: ArcheAge is open and plays it there" } else { "Double-click to jump into ArcheAge" });
        if resp.clicked() {
            self.pick(self.page, s.id);
        }
        if resp.double_clicked() {
            *act = Some(Action::Shares(Do::Watch(s.id)));
        }
    }

    fn card_frame(&self, p: &egui::Painter, rect: Rect, picked: bool, hov: bool) {
        if picked {
            glow(p, rect, 11.0, 1.1);
        }
        p.rect_filled(rect, 11.0, if hov || picked { Color32::from_rgb(22, 22, 26) } else { PANEL });
    }

    fn card_rim(&self, p: &egui::Painter, rect: Rect, picked: bool, hov: bool) {
        if picked {
            p.rect_stroke(rect, 11.0, Stroke::new(1.5, GOLD), egui::StrokeKind::Inside);
        } else {
            p.rect_stroke(rect, 11.0, Stroke::new(1.0, if hov { Color32::from_rgb(70, 70, 80) } else { LINE }), egui::StrokeKind::Inside);
        }
    }

    /// A grid card (Shared with me, Saved, Guild shares): the still, the name, who shared it, its length.
    fn share_card(&mut self, ui: &mut egui::Ui, size: egui::Vec2, s: &Share, act: &mut Option<Action>) {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let picked = self.hub.picked.get(&self.page) == Some(&s.id);
        let hov = resp.hovered();
        let p = ui.painter().clone();
        self.card_frame(&p, rect, picked, hov);
        let thumb = Rect::from_min_size(rect.min, vec2(rect.width(), (rect.width() * 0.56).round()));
        crate::gui::pictures::draw(&p, &self.share_pic(s), thumb, egui::CornerRadius { nw: 11, ne: 11, sw: 0, se: 0 }, if hov { Color32::WHITE } else { Color32::from_gray(228) });
        if s.saved {
            let b = Rect::from_min_size(pos2(thumb.right() - 38.0, thumb.top() + 10.0), vec2(28.0, 28.0));
            p.rect_filled(b, 7.0, gold_a(200));
            icon(&p, b.center(), 15.0, Icon::BookmarkFill, BG);
        }
        if hov {
            let c = thumb.center();
            p.circle_filled(c, 22.0, rgba(10, 10, 12, 170));
            p.circle_stroke(c, 22.0, Stroke::new(1.2, GOLD_HI));
            icon(&p, c + vec2(1.5, 0.0), 18.0, Icon::Play, GOLD_HI);
        }
        let tx = rect.left() + 13.0;
        let title = crate::gui::status_bar::fit(&p, &share_title(s), rect.width() - 26.0, bold_font(15.0));
        p.text(pos2(tx, thumb.bottom() + 11.0), Align2::LEFT_TOP, title, bold_font(15.0), INK);
        avatar(&p, pos2(tx + 9.0, thumb.bottom() + 43.0), 9.0, &s.by);
        p.text(pos2(tx + 25.0, thumb.bottom() + 43.0), Align2::LEFT_CENTER, &s.by, text_font(13.0), MUTED);
        if let Some(l) = s.length {
            p.text(pos2(rect.right() - 13.0, thumb.bottom() + 43.0), Align2::RIGHT_CENTER, length_words(l), text_font(13.0), rgba(214, 212, 208, 255));
        }
        self.card_rim(&p, rect, picked, hov);
        self.card_clicks(resp, s, act);
    }

    /// A Community card: the still, the name, who shared it and the kind, the first line of the note.
    fn post_card(&mut self, ui: &mut egui::Ui, size: egui::Vec2, s: &Share, act: &mut Option<Action>) {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let picked = self.hub.picked.get(&self.page) == Some(&s.id);
        let hov = resp.hovered();
        let p = ui.painter().clone();
        self.card_frame(&p, rect, picked, hov);
        let thumb = Rect::from_min_size(rect.min, vec2(rect.width(), (rect.width() * 0.40).round()));
        crate::gui::pictures::draw(&p, &self.share_pic(s), thumb, egui::CornerRadius { nw: 11, ne: 11, sw: 0, se: 0 }, if hov { Color32::WHITE } else { Color32::from_gray(228) });
        let tx = rect.left() + 18.0;
        let title = crate::gui::status_bar::fit(&p, &share_title(s), rect.width() - 36.0, bold_font(17.0));
        p.text(pos2(tx, thumb.bottom() + 12.0), Align2::LEFT_TOP, title, bold_font(17.0), INK);
        avatar(&p, pos2(tx + 10.0, thumb.bottom() + 52.0), 10.0, &s.by);
        p.text(pos2(tx + 28.0, thumb.bottom() + 52.0), Align2::LEFT_CENTER, format!("{}  ·  {}", s.by, kind_label(&s.kind)), text_font(13.5), MUTED);
        let line = if s.note.trim().is_empty() { shared_day(s) } else { s.note.lines().next().unwrap_or("").to_string() };
        let line = crate::gui::status_bar::fit(&p, &line, rect.width() - 36.0, text_font(14.0));
        p.text(pos2(tx, thumb.bottom() + 74.0), Align2::LEFT_TOP, line, text_font(14.0), rgba(214, 212, 208, 255));
        self.card_rim(&p, rect, picked, hov);
        self.card_clicks(resp, s, act);
    }

    /// Community's newest post, wide: the still on the left, who, the name, the note, replies and Save on the right.
    fn focus_post(&mut self, ui: &mut egui::Ui, size: egui::Vec2, s: &Share, act: &mut Option<Action>) {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let picked = self.hub.picked.get(&self.page) == Some(&s.id);
        let hov = resp.hovered();
        let p = ui.painter().clone();
        self.card_frame(&p, rect, picked, hov);
        let thumb = Rect::from_min_size(rect.min, vec2((rect.width() * 0.57).round(), rect.height()));
        crate::gui::pictures::draw(&p, &self.share_pic(s), thumb, egui::CornerRadius { nw: 11, ne: 0, sw: 11, se: 0 }, if hov { Color32::WHITE } else { Color32::from_gray(232) });
        if !s.other.trim().is_empty() {
            let b = Rect::from_min_size(thumb.min + vec2(14.0, 14.0), vec2(124.0, 28.0));
            p.rect_filled(b, 7.0, rgba(10, 10, 12, 200));
            icon(&p, pos2(b.left() + 18.0, b.center().y), 15.0, Icon::People, GREEN);
            p.text(pos2(b.left() + 34.0, b.center().y), Align2::LEFT_CENTER, "2 in replay", medium_font(13.0), INK);
        }
        let x = thumb.right() + 30.0;
        let w = rect.right() - 26.0 - x;
        let mut y = rect.top() + 30.0;
        avatar(&p, pos2(x + 15.0, y + 15.0), 15.0, &s.by);
        p.text(pos2(x + 40.0, y + 15.0), Align2::LEFT_CENTER, format!("{}  ·  {}", s.by, kind_label(&s.kind)), medium_font(14.5), rgba(214, 212, 208, 255));
        y += 44.0;
        let tg = p.layout(share_title(s), title_font(25.0), INK, w);
        let th = tg.size().y;
        p.galley(pos2(x, y), tg, INK);
        y += th + 12.0;
        if !s.note.trim().is_empty() {
            let mut job = egui::text::LayoutJob::simple(s.note.trim().to_string(), text_font(15.0), rgba(214, 212, 208, 255), w);
            job.wrap.max_rows = 3;
            let g = p.fonts_mut(|f| f.layout_job(job));
            p.galley(pos2(x, y), g, INK);
        }
        let by = rect.bottom() - 46.0;
        icon(&p, pos2(x + 10.0, by), 17.0, Icon::Chat, INK);
        let rr = p.text(pos2(x + 30.0, by), Align2::LEFT_CENTER, format!("{} repl{}", s.replies, if s.replies == 1 { "y" } else { "ies" }), text_font(14.5), INK);
        p.line_segment([pos2(rr.right() + 18.0, by - 12.0), pos2(rr.right() + 18.0, by + 12.0)], Stroke::new(1.0, EDGE));
        let save = Rect::from_min_size(pos2(rr.right() + 30.0, by - 16.0), vec2(96.0, 32.0));
        let busy = self.hub.busy_with(|o| matches!(o, Op::Save(i, _) if *i == s.id));
        ui.scope_builder(UiBuilder::new().max_rect(save).layout(Layout::left_to_right(Align::Center)), |ui| {
            if link_button(ui, if s.saved { Icon::BookmarkFill } else { Icon::Bookmark }, if s.saved { "Saved" } else { "Save" }, false).clicked() && !busy {
                *act = Some(Action::Shares(Do::Save(s.id, !s.saved)));
            }
        });
        self.card_rim(&p, rect, picked, hov);
        self.card_clicks(resp, s, act);
    }

    /// The inspector of a feed page (and of the guild's pages, whose part below the buttons is guild.rs's).
    pub(in crate::gui) fn share_inspector(&mut self, ui: &mut egui::Ui, r: Rect, act: &mut Option<Action>) {
        let page = self.page;
        let p = ui.painter().clone();
        let x = r.left() + 28.0;
        let w = r.width() - 56.0;
        let Some(s) = self.hub.picked.get(&page).and_then(|id| self.hub.find(*id)) else {
            caps(&p, pos2(x, r.top() + 32.0), "Nothing selected", MUTED);
            let what = match page {
                Page::Discover | Page::Following => "Pick a post to see who shared it and its discussion. Jump into ArcheAge plays the moment in the game.",
                Page::GuildHome | Page::GuildShares | Page::Review | Page::Members => "Pick a guild share to read the guild's note and mark it reviewed.",
                _ => "Pick a moment to see who shared it. Jump into ArcheAge plays it in the game, in the world itself.",
            };
            let g = p.layout(what.into(), text_font(13.5), MUTED, w);
            p.galley(pos2(x, r.top() + 60.0), g, MUTED);
            return;
        };

        // the name, who, when and how long
        let mut y = r.top() + 30.0;
        let tg = p.layout(share_title(&s), title_font(25.0), INK, w);
        let th = tg.size().y;
        p.galley(pos2(x, y), tg, INK);
        y += th + 12.0;
        avatar(&p, pos2(x + 13.0, y + 13.0), 13.0, &s.by);
        let mut who = s.by.clone();
        if !s.other.trim().is_empty() {
            let other = if s.other.trim() == s.by { s.me.trim() } else { s.other.trim() };
            if !other.is_empty() && other != s.by {
                who = format!("{who}  vs  {other}");
            }
        }
        p.text(pos2(x + 34.0, y + 13.0), Align2::LEFT_CENTER, crate::gui::status_bar::fit(&p, &who, w - 34.0, bold_font(14.5)), bold_font(14.5), INK);
        y += 38.0;
        let mut facts = vec![kind_label(&s.kind), library::day(s.when, library::now_unix())];
        if let Some(l) = s.length {
            facts.push(length_words(l));
        }
        p.text(pos2(x, y), Align2::LEFT_TOP, facts.join("  ·  "), text_font(14.5), rgba(214, 212, 208, 255));
        y += 22.0;
        let shared = match (s.audience.as_str(), &s.guild) {
            ("guild", Some(g)) => format!("Shared with {g}"),
            ("guild", None) => "Shared with the guild".into(),
            ("friends", _) => "Shared with friends".into(),
            _ => "Shared with the community".into(),
        };
        p.text(pos2(x, y), Align2::LEFT_TOP, format!("{shared}  ·  {}", shared_day(&s)), text_font(13.0), MUTED);
        y += 36.0;

        // Jump into ArcheAge, and Save
        let watch = self.hub.watch.as_ref().filter(|(id, _)| *id == s.id).map(|(_, w)| w.clone());
        let asking = matches!(watch, Some(Watching::Asking));
        let b = Rect::from_min_size(pos2(x, y), vec2(w - 58.0, 46.0));
        let resp = ui.scope_builder(UiBuilder::new().max_rect(b), |ui| primary_button(ui, "Jump into ArcheAge", None, true, b.width(), 46.0, !asking && !self.busy)).inner;
        let tip = if self.child.is_some() { "ArcheAge is open: the moment plays there in a second or two" } else { "Starts ArcheAge and plays this moment once you're in the world" };
        if resp.on_hover_text(tip).clicked() {
            *act = Some(Action::Shares(Do::Watch(s.id)));
        }
        let sb = Rect::from_min_size(pos2(b.right() + 12.0, y), vec2(46.0, 46.0));
        let busy = self.hub.busy_with(|o| matches!(o, Op::Save(i, _) if *i == s.id));
        let sr = ui.interact(sb, ui.scope_id().with(("save", s.id)), Sense::click());
        p.rect_filled(sb, 9.0, if sr.hovered() { RAISED } else { Color32::from_rgb(17, 17, 20) });
        p.rect_stroke(sb, 9.0, Stroke::new(1.0, if s.saved { gold_a(150) } else { EDGE }), egui::StrokeKind::Inside);
        icon(&p, sb.center(), 18.0, if s.saved { Icon::BookmarkFill } else { Icon::Bookmark }, if s.saved { GOLD_HI } else { INK });
        if sr.on_hover_text(if s.saved { "Remove from saved" } else { "Save to watch again" }).clicked() && !busy {
            *act = Some(Action::Shares(Do::Save(s.id, !s.saved)));
        }
        y = b.bottom() + 14.0;
        if let Some(wst) = watch {
            y = watch_box(ui, &p, x, y, w, &wst, self.child.is_some());
        }

        match page {
            Page::Review | Page::GuildHome => self.review_part(ui, r, x, w, y, &s, act),
            Page::Saved => {
                rule(&p, x, x + w, y + 6.0);
                y += 26.0;
                caps(&p, pos2(x, y), "Saved details", MUTED);
                y += 34.0;
                icon(&p, pos2(x + 10.0, y + 10.0), 18.0, Icon::CalendarCheck, INK);
                p.text(pos2(x + 36.0, y + 10.0), Align2::LEFT_CENTER, format!("Shared {}", shared_day(&s).to_lowercase()), text_font(14.5), INK);
                y += 36.0;
                icon(&p, pos2(x + 10.0, y + 10.0), 18.0, Icon::People, INK);
                let lr = p.text(pos2(x + 36.0, y + 10.0), Align2::LEFT_CENTER, "Shared by ", text_font(14.5), INK);
                p.text(pos2(lr.right(), y + 10.0), Align2::LEFT_CENTER, &s.by, bold_font(14.5), INK);
                y += 34.0;
                rule(&p, x, x + w, y + 6.0);
                y += 18.0;
                ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(pos2(x - 2.0, y), vec2(w, 32.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
                    if link_button(ui, Icon::Bookmark, "Remove from saved", false).clicked() && !busy {
                        *act = Some(Action::Shares(Do::Save(s.id, false)));
                    }
                });
            }
            _ => self.discussion(ui, r, x, w, y, &s, act),
        }
    }

    /// The sharer's note, follow or unshare, and the discussion with a reply box at the bottom.
    fn discussion(&mut self, ui: &mut egui::Ui, r: Rect, x: f32, w: f32, mut y: f32, s: &Share, act: &mut Option<Action>) {
        let p = ui.painter().clone();
        if !s.note.trim().is_empty() {
            let g = p.layout(s.note.trim().to_string(), text_font(14.0), rgba(214, 212, 208, 255), w - 18.0);
            let gh = g.size().y;
            p.rect_filled(Rect::from_min_size(pos2(x, y + 2.0), vec2(2.0, gh - 4.0)), 1.0, gold_a(140));
            p.galley(pos2(x + 14.0, y), g, INK);
            y += gh + 12.0;
        }
        ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(pos2(x - 2.0, y), vec2(w, 30.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
            if s.mine {
                if link_button(ui, Icon::Close, "Stop sharing", true).on_hover_text("No one else sees it any more; replies go with it").clicked() {
                    *act = Some(Action::Shares(Do::Unshare(s.id)));
                }
            } else {
                let busy = self.hub.busy_with(|o| matches!(o, Op::Follow(i, _) if *i == s.id));
                let label = if s.following { format!("Following {}", s.by) } else { format!("Follow {}", s.by) };
                if link_button(ui, if s.following { Icon::Check } else { Icon::UserPlus }, &label, false).on_hover_text(if s.following { "Stop following" } else { "Their next shared moments show in Following" }).clicked() && !busy {
                    *act = Some(Action::Shares(Do::Follow(s.id, !s.following)));
                }
            }
        });
        y += 38.0;
        rule(&p, x, x + w, y);
        y += 20.0;
        caps(&p, pos2(x, y), "Discussion", MUTED);
        y += 32.0;

        // the reply box at the bottom; the newest replies that fit above it
        let reply_top = r.bottom() - 70.0;
        let detail = self.hub.details.get(&s.id);
        match detail.and_then(|d| d.data.as_ref()) {
            None if detail.and_then(|d| d.error.as_ref()).is_some() => {
                let g = p.layout(detail.and_then(|d| d.error.clone()).unwrap_or_default(), text_font(13.0), AMBER, w);
                p.galley(pos2(x, y), g, AMBER);
            }
            None => {
                ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(pos2(x, y), vec2(w, 24.0))), |ui| {
                    ui.add(egui::Spinner::new().size(16.0).color(GOLD));
                });
            }
            Some(d) if d.replies.is_empty() => {
                p.text(pos2(x, y), Align2::LEFT_TOP, "No replies yet. Say what you saw.", text_font(13.5), MUTED);
            }
            Some(d) => {
                let laid: Vec<_> = d.replies.iter().map(|rp| (rp, p.layout(rp.text.clone(), text_font(14.0), INK, w - 44.0))).collect();
                let room = reply_top - 16.0 - y;
                let mut used = 0.0;
                let mut from = laid.len();
                while from > 0 {
                    let h = laid[from - 1].1.size().y + 34.0;
                    if used + h > room {
                        break;
                    }
                    used += h;
                    from -= 1;
                }
                if from > 0 {
                    p.text(pos2(x, y - 4.0), Align2::LEFT_TOP, format!("{} earlier repl{}", from, if from == 1 { "y" } else { "ies" }), text_font(12.0), FAINT);
                    y += 16.0;
                }
                for (rp, g) in laid.into_iter().skip(from) {
                    avatar(&p, pos2(x + 15.0, y + 15.0), 15.0, &rp.name);
                    p.text(pos2(x + 44.0, y), Align2::LEFT_TOP, &rp.name, bold_font(14.0), INK);
                    let gh = g.size().y;
                    p.galley(pos2(x + 44.0, y + 20.0), g, INK);
                    y += gh + 34.0;
                }
            }
        }
        rule(&p, x, x + w, reply_top - 8.0);
        let busy = self.hub.busy_with(|o| matches!(o, Op::Reply(i) if *i == s.id));
        let row = Rect::from_min_size(pos2(x, reply_top + 6.0), vec2(w, 44.0));
        ui.scope_builder(UiBuilder::new().max_rect(row).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let field = input_box(ui, &mut self.hub.reply, Icon::Chat, "Write a reply", w - 96.0, 42.0);
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let can = !busy && !self.hub.reply.trim().is_empty();
            if (primary_button(ui, "Reply", None, false, 86.0, 42.0, can).clicked() || enter) && can {
                *act = Some(Action::Shares(Do::Reply(s.id, self.hub.reply.clone())));
            }
        });
    }
}

/// Where a Jump into ArcheAge on a share stands, under its button.
fn watch_box(ui: &mut egui::Ui, p: &egui::Painter, x: f32, y: f32, w: f32, wst: &Watching, running: bool) -> f32 {
    let (title, line, col) = match wst {
        Watching::Asking => ("Asking the server…", "It checks you may watch this moment.".to_string(), GOLD),
        Watching::Opening if running => ("ArcheAge is running", "The moment plays once you're in the world.".to_string(), GREEN),
        Watching::Opening => ("Opening ArcheAge…", "The moment plays once you're in the world. You can keep browsing.".to_string(), GOLD),
        Watching::Playing => ("Playing in ArcheAge", "Your replay player starts it in a second or two. Leave the arena ring first.".to_string(), GREEN),
        Watching::Failed(e) => ("It didn't open", e.clone(), RED),
    };
    let g = p.layout(line, text_font(13.0), MUTED, w - 62.0);
    let b = Rect::from_min_size(pos2(x, y), vec2(w, 50.0 + g.size().y));
    p.rect_filled(b, 10.0, Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 16));
    p.rect_stroke(b, 10.0, Stroke::new(1.0, Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 100)), egui::StrokeKind::Inside);
    match wst {
        Watching::Asking | Watching::Opening if col == GOLD => {
            ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(b.min + vec2(16.0, 15.0), vec2(22.0, 22.0))), |ui| {
                ui.add(egui::Spinner::new().size(18.0).color(GOLD));
            });
        }
        Watching::Failed(_) => icon(p, b.min + vec2(26.0, 26.0), 19.0, Icon::Warning, RED),
        _ => {
            p.circle_filled(b.min + vec2(24.0, 25.0), 5.0, GREEN);
        }
    }
    p.text(b.min + vec2(46.0, 16.0), Align2::LEFT_TOP, title, bold_font(15.0), INK);
    p.galley(b.min + vec2(46.0, 40.0), g, MUTED);
    b.bottom() + 14.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(id: i64, title: &str, by: &str, kind: &str) -> Share {
        Share { id, title: title.into(), by: by.into(), kind: kind.into(), ..Default::default() }
    }

    #[test]
    fn pages_search_and_show_one_kind() {
        let l = vec![s(1, "beat Ren 0:42", "Ayla", "duel"), s(2, "West Gate fell", "Ivy", "siege"), s(3, "lost to Ivy", "Corvin", "")];
        let ids = |v: Vec<Share>| v.iter().map(|x| x.id).collect::<Vec<_>>();
        assert_eq!(ids(filtered(&l, "", "")), vec![1, 2, 3]);
        assert_eq!(ids(filtered(&l, "ivy", "")), vec![2, 3]);
        assert_eq!(ids(filtered(&l, "", "duel")), vec![1, 3]);
        assert_eq!(ids(filtered(&l, "", "naval")), Vec::<i64>::new());
        assert_eq!(kind_label("trade"), "Trade run");
        assert_eq!(kind_label("raid"), "Raid");
        assert_eq!(share_title(&l[0]), "Beat Ren");
    }
}
