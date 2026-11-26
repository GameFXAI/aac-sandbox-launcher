//! Guild > Overview, Review queue and Members, the guild part of the share inspector (Mark reviewed, the guild's note
//! with its marks), and the Friends block at the bottom of the rail. The guild, its members, who is online and what
//! is reviewed are the server's (it reads them from the game); the app only shows them and sends what was pressed.

use super::feeds::{kind_label, share_title};
use super::library::length_words;
use super::shares::{Do, Op};
use crate::gui::theme::*;
use crate::gui::widgets::{EDGE, chip, empty_state, link_button, outline_button, pill, primary_button, search_box};
use crate::gui::{Action, App, Page};
use crate::library;
use crate::net::Server;
use crate::shares::{self as api, Person, ReviewItem, Scope, Share};
use eframe::egui::{self, Align, Align2, Color32, Layout, Rect, RichText, Sense, Stroke, TextEdit, UiBuilder, pos2, vec2};

const FILTERS: [&str; 3] = ["All", "Needs review", "Reviewed"];

/// "In ArcheAge", "Last seen today", "Last seen on 3 Nov 2026".
pub fn seen_words(p: &Person) -> String {
    if p.online {
        return "In ArcheAge".into();
    }
    match p.last_seen.as_deref().and_then(library::parse_iso) {
        Some(t) => match library::day(t * 1000, library::now_unix()) {
            d if d == "Today" || d == "Yesterday" => format!("Last seen {}", d.to_lowercase()),
            d => format!("Last seen on {d}"),
        },
        None => "Not in ArcheAge".into(),
    }
}

/// The review queue as the page shows it: the search, then the filter (all, needs review, reviewed).
pub fn review_shown(items: &[ReviewItem], query: &str, filter: usize) -> Vec<ReviewItem> {
    let q = query.trim().to_lowercase();
    items
        .iter()
        .filter(|i| q.is_empty() || [&i.share.title, &i.share.by, &i.share.note].iter().any(|t| t.to_lowercase().contains(&q)))
        .filter(|i| match filter {
            1 => i.reviewed.is_none(),
            2 => i.reviewed.is_some(),
            _ => true,
        })
        .cloned()
        .collect()
}

/// A guild note's text as the editor holds it: the note, then one "00:42 what" line per mark.
pub fn note_text(text: &str, marks: &[api::Mark]) -> String {
    let mut s = text.trim().to_string();
    for m in marks {
        if !s.is_empty() {
            s.push('\n');
        }
        s.push_str(&format!("{} {}", api::mark_words(m.at), m.text));
    }
    s
}

impl App {
    /// The page when there is no guild to show: the server's reason, a spinner, or "not in a guild".
    fn no_guild(&mut self, ui: &mut egui::Ui, area: Rect, act: &mut Option<Action>) -> bool {
        if !matches!(Server::parse(&self.cfg.login_ip), Ok(Server::Api { .. })) {
            empty_state(ui, area, Icon::Shield, "No guild on this server", "Guilds show on servers with an account API (a server link).");
            return true;
        }
        match (&self.hub.social.data, &self.hub.social.error) {
            (Some(s), _) if s.guild.is_some() => false,
            (Some(_), _) => {
                empty_state(ui, area, Icon::Shield, "You're not in a guild", "Join or found a guild in ArcheAge, and its shared moments, review queue and members show here.");
                true
            }
            (None, Some(e)) => {
                let e = e.clone();
                empty_state(ui, area, Icon::Shield, "Your guild could not be fetched", &e);
                self.empty_button(ui, area, "Try again", Action::Shares(Do::Refresh), act);
                true
            }
            (None, None) => {
                ui.scope_builder(UiBuilder::new().max_rect(Rect::from_center_size(area.center() - vec2(0.0, 40.0), vec2(300.0, 60.0))).layout(Layout::top_down(Align::Center)), |ui| {
                    ui.add(egui::Spinner::new().size(26.0).color(GOLD));
                    ui.label(RichText::new("Asking the server…").font(text_font(14.0)).color(MUTED));
                });
                true
            }
        }
    }

    /// Guild > Overview: the guild, who is online, what needs a review, the newest guild shares.
    pub(in crate::gui) fn guild_home(&mut self, ui: &mut egui::Ui, centre: Rect, act: &mut Option<Action>) {
        let area = centre.shrink2(vec2(24.0, 24.0));
        if self.no_guild(ui, area, act) {
            return;
        }
        let Some(guild) = self.hub.social.data.as_ref().and_then(|s| s.guild.clone()) else { return };
        if !self.hub.picked.contains_key(&Page::GuildHome) {
            let first = self.hub.review.data.as_ref().and_then(|r| r.shares.iter().find(|i| i.reviewed.is_none()).map(|i| i.share.id));
            if let Some(id) = first.or_else(|| self.hub.feed(Scope::Guild).and_then(|f| f.shares.first()).map(|s| s.id)) {
                self.hub.picked.insert(Page::GuildHome, id);
            }
        }
        let p = ui.painter().clone();
        let (x0, x1) = (centre.left() + 28.0, centre.right() - 28.0);

        // the crest, the name, who is online
        let crest = Rect::from_min_size(pos2(x0, centre.top() + 26.0), vec2(96.0, 108.0));
        crate::gui::pictures::draw(&p, &self.pics.crest, crest, 12, Color32::WHITE);
        let tx = crest.right() + 30.0;
        let t = p.text(pos2(tx, crest.top() + 14.0), Align2::LEFT_TOP, &guild.name, title_font(34.0), INK);
        let mut ax = tx + 18.0;
        let online: Vec<&Person> = guild.members.iter().filter(|m| m.online).collect();
        for m in online.iter().take(3) {
            avatar(&p, pos2(ax, t.bottom() + 30.0), 17.0, &m.name);
            ax += 40.0;
        }
        let dx = if online.is_empty() { tx + 6.0 } else { ax + 2.0 };
        p.circle_filled(pos2(dx, t.bottom() + 30.0), 5.0, if guild.online > 0 { GREEN } else { FAINT });
        p.text(pos2(dx + 14.0, t.bottom() + 30.0), Align2::LEFT_CENTER, format!("{} member{} online", guild.online, if guild.online == 1 { "" } else { "s" }), text_font(15.0), rgba(214, 212, 208, 255));
        let mut y = crest.bottom() + 22.0;
        rule(&p, x0, x1, y);
        y += 24.0;

        // needs review
        let h = p.text(pos2(x0, y), Align2::LEFT_TOP, "Needs review", bold_font(22.0), INK);
        let review = self.hub.review.data.clone();
        if let Some(r) = &review {
            p.text(pos2(h.right() + 22.0, h.center().y), Align2::LEFT_CENTER, format!("{} of {} reviewed", r.done(), r.shares.len()), text_font(15.0), rgba(214, 212, 208, 255));
        }
        let link = Rect::from_min_max(pos2(x1 - 220.0, y - 4.0), pos2(x1, y + 30.0));
        ui.scope_builder(UiBuilder::new().max_rect(link).layout(Layout::right_to_left(Align::Center)), |ui| {
            let r = ui.add(egui::Button::new(RichText::new("View review queue  ›").font(medium_font(15.0)).color(INK)).frame(false));
            if r.clicked() {
                self.page = Page::Review;
            }
        });
        y = h.bottom() + 16.0;
        if let Some(r) = &review {
            progress(&p, Rect::from_min_size(pos2(x0, y), vec2(x1 - x0, 6.0)), r.done(), r.shares.len());
        }
        y += 22.0;
        match &review {
            None => {
                let line = self.hub.review.error.clone().unwrap_or_else(|| "Asking the server…".into());
                p.text(pos2(x0, y), Align2::LEFT_TOP, line, text_font(14.0), MUTED);
                y += 30.0;
            }
            Some(r) => {
                let open: Vec<ReviewItem> = r.shares.iter().filter(|i| i.reviewed.is_none()).take(2).cloned().collect();
                if open.is_empty() {
                    let line = if r.shares.is_empty() { "Nothing is shared with the guild yet." } else { "Everything shared with the guild is reviewed." };
                    p.text(pos2(x0, y + 4.0), Align2::LEFT_TOP, line, text_font(14.5), MUTED);
                    y += 34.0;
                } else {
                    for i in &open {
                        let row = Rect::from_min_size(pos2(x0, y), vec2(x1 - x0, 94.0));
                        ui.scope_builder(UiBuilder::new().max_rect(row), |ui| self.review_row(ui, row.width(), i, false, act));
                        y += 104.0;
                    }
                }
            }
        }
        y += 8.0;
        rule(&p, x0, x1, y);
        y += 26.0;

        // the newest guild shares
        p.text(pos2(x0, y), Align2::LEFT_TOP, "Recent guild shares", bold_font(22.0), INK);
        y += 44.0;
        let recent: Vec<Share> = self.hub.feed(Scope::Guild).map(|f| f.shares.iter().take(2).cloned().collect()).unwrap_or_default();
        if recent.is_empty() {
            p.text(pos2(x0, y), Align2::LEFT_TOP, "Moments your guild's members share with the guild show here.", text_font(14.0), MUTED);
            return;
        }
        let gap = 16.0;
        let cw = ((x1 - x0 - gap) / 2.0).floor();
        let ch = ((cw * 0.40).round() + 74.0).min(centre.bottom() - y - 12.0);
        if ch < 80.0 {
            return;
        }
        for (k, s) in recent.iter().enumerate() {
            let card = Rect::from_min_size(pos2(x0 + k as f32 * (cw + gap), y), vec2(cw, ch));
            ui.scope_builder(UiBuilder::new().max_rect(card), |ui| self.small_card(ui, card.size(), s, act));
        }
    }

    /// A short card (the overview's recent shares): the still, the name, who and the kind.
    fn small_card(&mut self, ui: &mut egui::Ui, size: egui::Vec2, s: &Share, act: &mut Option<Action>) {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let picked = self.hub.picked.get(&self.page) == Some(&s.id);
        let hov = resp.hovered();
        let p = ui.painter().clone();
        if picked {
            glow(&p, rect, 11.0, 1.1);
        }
        p.rect_filled(rect, 11.0, if hov || picked { Color32::from_rgb(22, 22, 26) } else { PANEL });
        let thumb = Rect::from_min_size(rect.min, vec2(rect.width(), (rect.height() - 74.0).max(40.0)));
        crate::gui::pictures::draw(&p, self.pics.for_share(&s.kind, s.id), thumb, egui::CornerRadius { nw: 11, ne: 11, sw: 0, se: 0 }, if hov { Color32::WHITE } else { Color32::from_gray(228) });
        let tx = rect.left() + 20.0;
        p.text(pos2(tx, thumb.bottom() + 12.0), Align2::LEFT_TOP, crate::gui::status_bar::fit(&p, &share_title(s), rect.width() - 40.0, bold_font(17.0)), bold_font(17.0), INK);
        avatar(&p, pos2(tx + 10.0, thumb.bottom() + 52.0), 10.0, &s.by);
        p.text(pos2(tx + 28.0, thumb.bottom() + 52.0), Align2::LEFT_CENTER, format!("{}  ·  {}", s.by, kind_label(&s.kind)), text_font(13.5), MUTED);
        p.rect_stroke(rect, 11.0, Stroke::new(if picked { 1.5 } else { 1.0 }, if picked { GOLD } else { LINE }), egui::StrokeKind::Inside);
        if resp.clicked() {
            self.hub.picked.insert(self.page, s.id);
        }
        if resp.double_clicked() {
            *act = Some(Action::Shares(Do::Watch(s.id)));
        }
    }

    /// One guild share in the review queue (`table`: with the Shared by column) or on the overview.
    fn review_row(&mut self, ui: &mut egui::Ui, width: f32, i: &ReviewItem, table: bool, act: &mut Option<Action>) {
        let s = &i.share;
        let (rect, resp) = ui.allocate_exact_size(vec2(width, if table { 100.0 } else { 94.0 }), Sense::click());
        let picked = self.hub.picked.get(&self.page) == Some(&s.id);
        let hov = resp.hovered();
        let p = ui.painter().clone();
        if picked {
            glow(&p, rect, 10.0, 1.0);
            bronze(&p, rect, 10.0, 1.0);
            p.rect_stroke(rect, 10.0, Stroke::new(1.5, GOLD), egui::StrokeKind::Inside);
        } else {
            p.rect_filled(rect, 10.0, if hov { Color32::from_rgb(22, 22, 26) } else { PANEL });
            p.rect_stroke(rect, 10.0, Stroke::new(1.0, LINE), egui::StrokeKind::Inside);
        }
        let thumb = Rect::from_min_size(rect.min + vec2(12.0, 10.0), vec2(if table { 160.0 } else { 254.0 }, rect.height() - 20.0));
        crate::gui::pictures::draw(&p, self.pics.for_share(&s.kind, s.id), thumb, 7, Color32::from_gray(232));
        let x = thumb.right() + 18.0;
        let state_x = rect.right() - 24.0;
        let by_x = rect.left() + width * 0.55;
        let title_w = if table { by_x - x - 24.0 } else { state_x - 170.0 - x };
        p.text(pos2(x, rect.center().y - 12.0), Align2::LEFT_BOTTOM, crate::gui::status_bar::fit(&p, &share_title(s), title_w, bold_font(18.0)), bold_font(18.0), INK);
        if table {
            let mut facts = vec![kind_label(&s.kind)];
            if let Some(l) = s.length {
                facts.push(length_words(l));
            }
            p.text(pos2(x, rect.center().y + 6.0), Align2::LEFT_TOP, facts.join("  ·  "), text_font(14.0), rgba(214, 212, 208, 255));
            p.line_segment([pos2(by_x - 12.0, rect.top() + 22.0), pos2(by_x - 12.0, rect.bottom() - 22.0)], Stroke::new(1.0, EDGE));
            avatar(&p, pos2(by_x + 22.0, rect.center().y), 22.0, &s.by);
            p.text(pos2(by_x + 56.0, rect.center().y), Align2::LEFT_CENTER, crate::gui::status_bar::fit(&p, &s.by, state_x - 162.0 - by_x - 56.0, bold_font(16.0)), bold_font(16.0), INK);
            p.line_segment([pos2(state_x - 150.0, rect.top() + 22.0), pos2(state_x - 150.0, rect.bottom() - 22.0)], Stroke::new(1.0, EDGE));
        } else {
            avatar(&p, pos2(x + 15.0, rect.center().y + 14.0), 14.0, &s.by);
            p.text(pos2(x + 38.0, rect.center().y + 14.0), Align2::LEFT_CENTER, format!("{}  ·  {}", s.by, kind_label(&s.kind)), text_font(15.0), rgba(214, 212, 208, 255));
        }
        let done = i.reviewed.is_some();
        let pr = pill(&p, pos2(state_x, rect.center().y), if done { "Reviewed" } else { "Needs review" }, done);
        let resp = match &i.reviewed {
            Some(r) => resp.on_hover_text(format!("Reviewed by {}", r.name)),
            None => resp,
        };
        let _ = pr;
        if resp.clicked() {
            self.hub.picked.insert(self.page, s.id);
        }
        if resp.double_clicked() {
            *act = Some(Action::Shares(Do::Watch(s.id)));
        }
    }

    /// Guild > Review queue: how much is reviewed, a search and a filter, then the guild's shares as a table.
    pub(in crate::gui) fn review_page(&mut self, ui: &mut egui::Ui, centre: Rect, act: &mut Option<Action>) {
        let p = ui.painter().clone();
        let (x0, x1) = (centre.left() + 24.0, centre.right() - 24.0);
        let t = p.text(pos2(x0 + 4.0, centre.top() + 22.0), Align2::LEFT_TOP, "Guild review", title_font(27.0), INK);
        let area = Rect::from_min_max(pos2(x0, t.bottom() + 24.0), pos2(x1, centre.bottom() - 24.0));
        if self.no_guild(ui, area, act) {
            return;
        }
        let Some(review) = self.hub.review.data.clone() else {
            match self.hub.review.error.clone() {
                Some(e) => {
                    empty_state(ui, area, Icon::Refresh, "The review queue could not be fetched", &e);
                    self.empty_button(ui, area, "Try again", Action::Shares(Do::Refresh), act);
                }
                None => {
                    ui.scope_builder(UiBuilder::new().max_rect(Rect::from_center_size(area.center() - vec2(0.0, 40.0), vec2(300.0, 60.0))).layout(Layout::top_down(Align::Center)), |ui| {
                        ui.add(egui::Spinner::new().size(26.0).color(GOLD));
                    });
                }
            }
            return;
        };
        let mut y = t.bottom() + 16.0;
        let lr = p.text(pos2(x0 + 8.0, y + 10.0), Align2::LEFT_CENTER, format!("{} of {} reviewed", review.done(), review.shares.len()), text_font(17.0), INK);
        progress(&p, Rect::from_min_max(pos2(lr.right() + 26.0, y + 6.0), pos2(x1, y + 14.0)), review.done(), review.shares.len());
        y += 40.0;
        let bar = Rect::from_min_max(pos2(x0, y), pos2(x1, y + 42.0));
        ui.scope_builder(UiBuilder::new().max_rect(bar).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            search_box(ui, self.hub.queries.entry(Page::Review).or_default(), "Search replays", (bar.width() * 0.4).max(200.0), 42.0);
            for (k, f) in FILTERS.iter().enumerate() {
                if chip(ui, f, self.hub.filter == k, true, 42.0).clicked() {
                    self.hub.filter = k;
                }
            }
        });
        y = bar.bottom() + 22.0;
        if review.shares.is_empty() {
            return empty_state(ui, Rect::from_min_max(pos2(x0, y), area.max), Icon::CalendarCheck, "Nothing to review", "Moments your guild's members share with the guild line up here for a look.");
        }
        let query = self.hub.queries.get(&Page::Review).cloned().unwrap_or_default();
        let list = review_shown(&review.shares, &query, self.hub.filter);
        let by_x = x0 + 4.0 + (x1 - x0 - 16.0) * 0.55;
        caps(&p, pos2(x0 + 18.0, y), "Replay", MUTED);
        caps(&p, pos2(by_x, y), "Shared by", MUTED);
        caps(&p, pos2(x1 - 168.0, y), "State", MUTED);
        y += 26.0;
        rule(&p, x0 + 6.0, x1 - 6.0, y);
        y += 16.0;
        if list.is_empty() {
            let why = match self.hub.filter {
                1 => "Everything here is reviewed.",
                2 => "Nothing is reviewed yet.",
                _ => "Try other words, or clear the search.",
            };
            return empty_state(ui, Rect::from_min_max(pos2(x0, y), area.max), Icon::Search, "Nothing matches", why);
        }
        if !self.hub.picked.get(&Page::Review).is_some_and(|id| list.iter().any(|i| i.share.id == *id)) {
            self.hub.picked.insert(Page::Review, list[0].share.id);
        }
        ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_max(pos2(x0, y), pos2(x1, centre.bottom()))), |ui| {
            egui::ScrollArea::vertical().id_salt("review").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                ui.add_space(4.0);
                let w = ui.available_width() - 12.0;
                for i in &list {
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        self.review_row(ui, w, i, true, act);
                    });
                }
                ui.add_space(16.0);
            });
        });
    }

    /// Guild > Members: everyone in the guild, online first, as the game has them.
    pub(in crate::gui) fn members_page(&mut self, ui: &mut egui::Ui, centre: Rect, act: &mut Option<Action>) {
        let p = ui.painter().clone();
        let (x0, x1) = (centre.left() + 24.0, centre.right() - 24.0);
        let t = p.text(pos2(x0 + 4.0, centre.top() + 22.0), Align2::LEFT_TOP, "Members", title_font(27.0), INK);
        let area = Rect::from_min_max(pos2(x0, t.bottom() + 24.0), pos2(x1, centre.bottom() - 24.0));
        if self.no_guild(ui, area, act) {
            return;
        }
        let Some(guild) = self.hub.social.data.as_ref().and_then(|s| s.guild.clone()) else { return };
        p.text(pos2(t.right() + 14.0, t.center().y + 2.0), Align2::LEFT_CENTER, format!("{} of {} online", guild.online, guild.members.len()), text_font(14.5), MUTED);
        let mut members = guild.members.clone();
        members.sort_by(|a, b| b.online.cmp(&a.online).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_max(area.min, pos2(x1, centre.bottom()))), |ui| {
            egui::ScrollArea::vertical().id_salt("members").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                let w = ui.available_width() - 12.0;
                for m in &members {
                    let (r, _) = ui.allocate_exact_size(vec2(w, 64.0), Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(r, 10.0, PANEL);
                    p.rect_stroke(r, 10.0, Stroke::new(1.0, LINE), egui::StrokeKind::Inside);
                    person(p, pos2(r.left() + 34.0, r.center().y), 20.0, m);
                    p.text(pos2(r.left() + 68.0, r.center().y - 10.0), Align2::LEFT_CENTER, &m.name, bold_font(16.0), INK);
                    let lvl = if m.level > 0 { format!("Level {}", m.level) } else { String::new() };
                    p.text(pos2(r.left() + 68.0, r.center().y + 12.0), Align2::LEFT_CENTER, lvl, text_font(13.0), MUTED);
                    p.text(pos2(r.right() - 22.0, r.center().y), Align2::RIGHT_CENTER, seen_words(m), text_font(14.0), if m.online { GREEN } else { MUTED });
                }
            });
        });
    }

    /// The Members page's inspector: the guild in a few words.
    pub(in crate::gui) fn members_inspector(&mut self, ui: &mut egui::Ui, r: Rect) {
        let p = ui.painter().clone();
        let x = r.left() + 28.0;
        let w = r.width() - 56.0;
        let Some(g) = self.hub.social.data.as_ref().and_then(|s| s.guild.clone()) else {
            caps(&p, pos2(x, r.top() + 32.0), "Your guild", MUTED);
            return;
        };
        caps(&p, pos2(x, r.top() + 32.0), "Your guild", MUTED);
        let tg = p.layout(g.name.clone(), title_font(25.0), INK, w);
        let th = tg.size().y;
        p.galley(pos2(x, r.top() + 58.0), tg, INK);
        let y = r.top() + 70.0 + th;
        p.text(pos2(x, y), Align2::LEFT_TOP, format!("{} members  ·  {} online", g.members.len(), g.online), text_font(14.5), rgba(214, 212, 208, 255));
        let line = p.layout("Members come from your guild in ArcheAge: invite, promote and remove them there.".into(), text_font(13.0), MUTED, w);
        p.galley(pos2(x, y + 34.0), line, MUTED);
    }

    /// The inspector's guild part: Mark reviewed, and the guild's note with its marks (any member may write it).
    pub(in crate::gui) fn review_part(&mut self, ui: &mut egui::Ui, r: Rect, x: f32, w: f32, mut y: f32, s: &Share, act: &mut Option<Action>) {
        let p = ui.painter().clone();
        if s.audience != "guild" {
            return;
        }
        let detail = self.hub.details.get(&s.id).and_then(|d| d.data.clone());
        let reviewed = self
            .hub
            .review
            .data
            .as_ref()
            .and_then(|rv| rv.shares.iter().find(|i| i.share.id == s.id).map(|i| i.reviewed.clone()))
            .unwrap_or_else(|| detail.as_ref().and_then(|d| d.review.clone()));
        let busy = self.hub.busy_with(|o| matches!(o, Op::Reviewed(i, _) if *i == s.id));
        let b = Rect::from_min_size(pos2(x, y), vec2(w, 46.0));
        ui.scope_builder(UiBuilder::new().max_rect(b), |ui| {
            let label = if reviewed.is_some() { "Back to needs review" } else { "Mark reviewed" };
            if outline_button(ui, label, Some(if reviewed.is_some() { Icon::Refresh } else { Icon::Check }), false, w, 46.0, false).clicked() && !busy {
                *act = Some(Action::Shares(Do::Reviewed(s.id, reviewed.is_none())));
            }
        });
        y = b.bottom() + 10.0;
        if let Some(rv) = &reviewed {
            let when = library::parse_iso(&rv.at).map(|t| library::day(t * 1000, library::now_unix())).unwrap_or_default();
            p.text(pos2(x, y), Align2::LEFT_TOP, format!("Reviewed by {}  ·  {}", rv.name, when), text_font(13.0), MUTED);
            y += 22.0;
        }
        rule(&p, x, x + w, y + 8.0);
        y += 28.0;
        caps(&p, pos2(x, y), "Guild notes", MUTED);
        y += 30.0;

        // writing the note
        let saving = self.hub.busy_with(|o| matches!(o, Op::Note(i) if *i == s.id));
        if let Some((id, text)) = self.hub.note_edit.as_mut().filter(|(id, _)| *id == s.id) {
            let id = *id;
            let ed = Rect::from_min_size(pos2(x, y), vec2(w, (r.bottom() - 90.0 - y).clamp(90.0, 220.0)));
            ui.scope_builder(UiBuilder::new().max_rect(ed), |ui| {
                ui.add_sized(ed.size(), TextEdit::multiline(text).font(text_font(14.0)).hint_text("Watch the gangplank.\n00:42 Hold the gangplank\n00:47 Wait for the pull").desired_width(w));
            });
            y = ed.bottom() + 6.0;
            p.text(pos2(x, y), Align2::LEFT_TOP, "A line starting with a time (00:42) becomes a mark.", text_font(12.0), FAINT);
            y += 22.0;
            let text = text.clone();
            ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(pos2(x, y), vec2(w, 38.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if primary_button(ui, "Save note", None, false, 110.0, 36.0, !saving).clicked() {
                    *act = Some(Action::Shares(Do::Note(id, text.clone())));
                }
                if outline_button(ui, "Cancel", None, false, 90.0, 36.0, false).clicked() {
                    self.hub.note_edit = None;
                }
            });
            return;
        }

        // the note as the guild wrote it
        let note = detail.as_ref().and_then(|d| d.note.clone());
        match &note {
            Some(n) => {
                if !n.text.trim().is_empty() {
                    let g = p.layout(n.text.trim().to_string(), text_font(14.5), INK, w - 32.0);
                    let bx = Rect::from_min_size(pos2(x, y), vec2(w, g.size().y + 28.0));
                    p.rect_filled(bx, 9.0, Color32::from_rgb(17, 17, 20));
                    p.rect_stroke(bx, 9.0, Stroke::new(1.0, EDGE), egui::StrokeKind::Inside);
                    p.galley(bx.min + vec2(16.0, 14.0), g, INK);
                    y = bx.bottom() + 16.0;
                }
                for m in &n.marks {
                    icon(&p, pos2(x + 10.0, y + 10.0), 16.0, Icon::Bookmark, INK);
                    p.text(pos2(x + 32.0, y + 10.0), Align2::LEFT_CENTER, api::mark_words(m.at), text_font(14.5), MUTED);
                    p.text(pos2(x + 96.0, y + 10.0), Align2::LEFT_CENTER, crate::gui::status_bar::fit(&p, &m.text, w - 96.0, text_font(14.5)), text_font(14.5), INK);
                    y += 32.0;
                }
                let when = library::parse_iso(&n.at).map(|t| library::day(t * 1000, library::now_unix())).unwrap_or_default();
                p.text(pos2(x, y + 2.0), Align2::LEFT_TOP, format!("By {}  ·  {}", n.name, when), text_font(12.5), FAINT);
                y += 28.0;
            }
            None if detail.is_none() => {
                ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(pos2(x, y), vec2(w, 24.0))), |ui| {
                    ui.add(egui::Spinner::new().size(16.0).color(GOLD));
                });
                y += 30.0;
            }
            None => {
                let g = p.layout("No note yet. Write what the guild should watch for, with the moments it happens.".into(), text_font(13.5), MUTED, w);
                let gh = g.size().y;
                p.galley(pos2(x, y), g, MUTED);
                y += gh + 12.0;
            }
        }
        if detail.is_some() {
            ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(pos2(x - 2.0, y), vec2(w, 30.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
                if link_button(ui, Icon::Pencil, if note.is_some() { "Edit the note" } else { "Write a note" }, false).clicked() {
                    let text = note.as_ref().map(|n| note_text(&n.text, &n.marks)).unwrap_or_default();
                    self.hub.note_edit = Some((s.id, text));
                }
            });
        }
    }

    /// How tall the rail's Friends block is: its heading and up to three friends.
    pub(in crate::gui) fn friends_height(&self) -> f32 {
        match self.hub.social.data.as_ref() {
            Some(s) if !s.friends.is_empty() => 58.0 + 62.0 * s.friends.len().min(3) as f32,
            _ => 132.0,
        }
    }

    /// The rail's Friends: how many are online, and up to three of them, online first.
    pub(in crate::gui) fn friends_block(&self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter();
        let x = rect.left() + 8.0;
        let top = rect.top();
        let hr = p.text(pos2(x, top + 18.0), Align2::LEFT_TOP, "Friends", bold_font(15.0), INK);
        let tip = "Add friends in ArcheAge: they show here";
        let add = Rect::from_center_size(pos2(rect.right() - 16.0, hr.center().y), vec2(26.0, 26.0));
        icon(p, add.center(), 17.0, Icon::UserPlus, MUTED);
        ui.interact(add, ui.scope_id().with("add-friend"), Sense::hover()).on_hover_text(tip);
        let say = |text: &str| {
            let g = p.layout(text.to_string(), text_font(12.5), MUTED, rect.width() - 16.0);
            p.galley(pos2(x, top + 46.0), g, MUTED);
        };
        let Some(s) = self.hub.social.data.as_ref() else {
            return say(match &self.hub.social.error {
                Some(e) if e.contains("character") => "Make a character in ArcheAge and your friends show here.",
                Some(_) => "Your friends could not be fetched just now.",
                None => "Asking the server…",
            });
        };
        let online = s.online();
        if !s.friends.is_empty() {
            p.text(pos2(hr.right() + 12.0, hr.center().y), Align2::LEFT_CENTER, format!("{online} online"), text_font(13.0), if online > 0 { GREEN } else { MUTED });
        }
        if s.friends.is_empty() {
            return say("No friends yet. Add friends in ArcheAge and they show here.");
        }
        let mut list: Vec<&Person> = s.friends.iter().collect();
        list.sort_by(|a, b| b.online.cmp(&a.online).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        let mut y = top + 58.0;
        for f in list.into_iter().take(3) {
            person(p, pos2(x + 16.0, y + 24.0), 19.0, f);
            p.text(pos2(x + 46.0, y + 13.0), Align2::LEFT_CENTER, crate::gui::status_bar::fit(p, &f.name, rect.width() - 60.0, bold_font(14.5)), bold_font(14.5), INK);
            p.text(pos2(x + 46.0, y + 35.0), Align2::LEFT_CENTER, seen_words(f), text_font(12.5), rgba(214, 212, 208, 255));
            y += 62.0;
        }
    }
}

/// A person's round picture with a dot for in the game (green) or not (grey).
fn person(p: &egui::Painter, c: egui::Pos2, r: f32, m: &Person) {
    avatar(p, c, r, &m.name);
    let dot = c + vec2(r * 0.72, r * 0.72);
    p.circle_filled(dot, 5.5, BG);
    p.circle_filled(dot, 4.0, if m.online { GREEN } else { Color32::from_gray(110) });
}

/// A thin gold bar: how much of the queue is reviewed.
fn progress(p: &egui::Painter, r: Rect, done: usize, of: usize) {
    p.rect_filled(r, r.height() / 2.0, rgba(255, 255, 255, 22));
    if of > 0 && done > 0 {
        let f = done as f32 / of as f32;
        let fill = Rect::from_min_size(r.min, vec2(r.width() * f, r.height()));
        p.rect_filled(fill.expand(2.0), r.height(), gold_a(30));
        p.rect_filled(fill, r.height() / 2.0, GOLD);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shares::{Mark, Reviewed};

    #[test]
    fn the_queue_searches_and_filters() {
        let item = |id: i64, by: &str, done: bool| ReviewItem { share: Share { id, by: by.into(), title: format!("moment {id}"), ..Default::default() }, reviewed: done.then(|| Reviewed { name: "Corvin".into(), at: String::new() }) };
        let l = vec![item(1, "Ayla", false), item(2, "Brannoc", true), item(3, "Ayla", true)];
        let ids = |v: Vec<ReviewItem>| v.iter().map(|i| i.share.id).collect::<Vec<_>>();
        assert_eq!(ids(review_shown(&l, "", 0)), vec![1, 2, 3]);
        assert_eq!(ids(review_shown(&l, "", 1)), vec![1]);
        assert_eq!(ids(review_shown(&l, "", 2)), vec![2, 3]);
        assert_eq!(ids(review_shown(&l, "ayla", 2)), vec![3]);
    }

    #[test]
    fn a_note_round_trips_through_the_editor() {
        let marks = vec![Mark { at: 42, text: "Hold the gangplank".into() }, Mark { at: 47, text: "Wait for the pull".into() }];
        let t = note_text("Watch the gangplank.", &marks);
        assert_eq!(t, "Watch the gangplank.\n00:42 Hold the gangplank\n00:47 Wait for the pull");
        assert_eq!(api::parse_marks(&t), marks);
    }

    #[test]
    fn people_are_said_as_in_the_game_or_last_seen() {
        let on = Person { name: "Ayla".into(), online: true, ..Default::default() };
        assert_eq!(seen_words(&on), "In ArcheAge");
        let off = Person { name: "Brannoc".into(), last_seen: Some("2025-01-03T10:00:00Z".into()), ..Default::default() };
        assert_eq!(seen_words(&off), "Last seen on 3 Jan 2025");
        assert_eq!(seen_words(&Person::default()), "Not in ArcheAge");
    }
}
