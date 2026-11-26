//! What the app keeps of other people's moments between frames: the feeds (Shared with me, Saved, Community,
//! Following, Guild shares), the picked share and its discussion, the account's friends and guild, the guild's review
//! queue, and the Share clip dialog. All of it is what the server last said (crate::shares); every press goes to the
//! server and the app shows its answer, never a number of its own.
//!
//! Pages ask for what they show with `keep_fresh` each frame: a feed is fetched when first shown and again once it is
//! a minute old, or right after something the account did changed it. Each call runs on its own thread and answers
//! with an `Event::Shares`.

use crate::gui::events::Event;
use crate::gui::{App, Page, Section};
use crate::net::{Cred, Server};
use crate::shares::{self as api, Audience, Detail, Feed, Review, Scope, Share, Social};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// How old a feed, the friends list or the review queue may get before a page asks again.
const STALE: Duration = Duration::from_secs(60);

/// Something the server sends, as the app last had it.
pub struct Loaded<T> {
    pub data: Option<T>,
    pub loading: bool,
    pub error: Option<String>,
    /// when the last answer came (or failed)
    pub at: Option<Instant>,
}

impl<T> Default for Loaded<T> {
    fn default() -> Self {
        Loaded { data: None, loading: false, error: None, at: None }
    }
}

impl<T> Loaded<T> {
    fn due(&self) -> bool {
        !self.loading && self.at.is_none_or(|t| t.elapsed() > STALE)
    }

    fn came(&mut self, r: Result<T, String>) {
        self.loading = false;
        self.at = Some(Instant::now());
        match r {
            Ok(d) => {
                self.data = Some(d);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Ask again on the next frame that shows it.
    fn stale(&mut self) {
        self.at = None;
    }
}

/// The Share clip dialog over My captures.
pub struct ShareDialog {
    pub n: i32,
    pub audience: Audience,
    pub note: String,
    pub sending: bool,
    pub error: Option<String>,
}

/// A request on its way to the server (the page greys out what it is about meanwhile).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    Rename(i32),
    Share(i32),
    Unshare(i64),
    Reply(i64),
    Save(i64, bool),
    Follow(i64, bool),
    Reviewed(i64, bool),
    Note(i64),
}

/// What the player pressed on these pages (carried by `Action::Shares`).
pub enum Do {
    Rename(i32, String),
    Share(i32, Audience, String),
    Unshare(i64),
    Reply(i64, String),
    Save(i64, bool),
    Follow(i64, bool),
    Reviewed(i64, bool),
    Note(i64, String),
    /// Jump into ArcheAge on someone else's share
    Watch(i64),
    /// the page's refresh button: everything it shows, again
    Refresh,
    /// the next page of a feed
    More(Scope),
}

/// The server's answers (carried by `Event::Shares`).
pub enum ShareEvent {
    /// a feed: which, whether it is a further page, the answer
    Feed(Scope, bool, Result<Feed, String>),
    Detail(i64, Result<Detail, String>),
    Social(Result<Social, String>),
    Review(Result<Review, String>),
    /// a request ended: Ok(what to say)
    Done(Op, Result<String, String>),
    /// a watch grant: how many seconds it lasts
    Watch(i64, Result<u32, String>),
}

/// Where a Jump into ArcheAge on a share stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Watching {
    Asking,
    /// the server granted it and the game is starting: it plays once the game is in the world
    Opening,
    /// the game was already open: its replay player starts it within a second or two
    Playing,
    Failed(String),
}

#[derive(Default)]
pub struct Hub {
    pub feeds: HashMap<Scope, Loaded<Feed>>,
    /// the picked share on each page
    pub picked: HashMap<Page, i64>,
    pub details: HashMap<i64, Loaded<Detail>>,
    pub reply: String,
    /// each page's search
    pub queries: HashMap<Page, String>,
    /// Community's kind tab
    pub kind: usize,
    /// the review queue's filter: all, needs review, reviewed
    pub filter: usize,
    pub social: Loaded<Social>,
    pub review: Loaded<Review>,
    pub dialog: Option<ShareDialog>,
    /// My captures: the capture being renamed, and the name typed so far
    pub rename: Option<(i32, String)>,
    /// the guild note being written: which share, and its text (marks as "00:42 what" lines)
    pub note_edit: Option<(i64, String)>,
    pub busy: HashSet<Op>,
    /// what the last request came to, for the status bar
    pub said: Option<Result<String, String>>,
    pub watch: Option<(i64, Watching)>,
}

impl Hub {
    pub fn feed(&self, scope: Scope) -> Option<&Feed> {
        self.feeds.get(&scope).and_then(|f| f.data.as_ref())
    }

    /// A share the app has from any feed, the review queue or a detail.
    pub fn find(&self, id: i64) -> Option<Share> {
        self.feeds
            .values()
            .filter_map(|f| f.data.as_ref())
            .flat_map(|f| f.shares.iter())
            .chain(self.review.data.iter().flat_map(|r| r.shares.iter().map(|i| &i.share)))
            .chain(self.details.get(&id).and_then(|d| d.data.as_ref()).map(|d| &d.share))
            .find(|s| s.id == id)
            .cloned()
    }

    pub fn busy_with(&self, f: impl Fn(&Op) -> bool) -> bool {
        self.busy.iter().any(f)
    }

    /// Saved or following changed on the server: say so on every copy the app holds.
    fn mark(&mut self, id: i64, f: impl Fn(&mut Share)) {
        for s in self.feeds.values_mut().filter_map(|l| l.data.as_mut()).flat_map(|d| d.shares.iter_mut()).filter(|s| s.id == id) {
            f(s);
        }
        if let Some(r) = self.review.data.as_mut() {
            for i in r.shares.iter_mut().filter(|i| i.share.id == id) {
                f(&mut i.share);
            }
        }
        if let Some(d) = self.details.get_mut(&id).and_then(|d| d.data.as_mut()) {
            f(&mut d.share);
        }
    }

    fn all_stale(&mut self) {
        self.feeds.values_mut().for_each(Loaded::stale);
        self.review.stale();
    }
}

/// The feed a page shows.
pub fn scope_of(page: Page) -> Option<Scope> {
    match page {
        Page::SharedWithMe | Page::CommunityShared => Some(Scope::WithMe),
        Page::Saved => Some(Scope::Saved),
        Page::Discover => Some(Scope::Community),
        Page::Following => Some(Scope::Following),
        Page::GuildShares | Page::GuildHome => Some(Scope::Guild),
        _ => None,
    }
}

impl App {
    /// Runs `work` against the account API on its own thread and sends its answer back as `wrap(answer)`. False when
    /// there is no account API or no sign-in (nothing is sent).
    fn ask<T: Send + 'static>(&self, work: impl FnOnce(&Server, &str, Cred) -> Result<T, String> + Send + 'static, wrap: impl FnOnce(Result<T, String>) -> ShareEvent + Send + 'static) -> bool {
        let Ok(server @ Server::Api { .. }) = Server::parse(&self.cfg.login_ip) else { return false };
        let Some((user, secret)) = self.secret() else { return false };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = work(&server, &user, secret.cred());
            let _ = tx.send(Event::Shares(wrap(r)));
        });
        true
    }

    fn fetch_feed(&mut self, scope: Scope, more: bool) {
        let f = self.hub.feeds.entry(scope).or_default();
        if f.loading {
            return;
        }
        let after = if more { f.data.as_ref().and_then(|d| d.next) } else { None };
        if more && after.is_none() {
            return;
        }
        if self.ask(move |s, u, c| api::feed(s, u, c, scope, after), move |r| ShareEvent::Feed(scope, more, r)) {
            self.hub.feeds.entry(scope).or_default().loading = true;
        }
    }

    fn fetch_detail(&mut self, id: i64) {
        if self.hub.details.get(&id).is_some_and(|d| d.loading) {
            return;
        }
        if self.ask(move |s, u, c| api::get(s, u, c, id), move |r| ShareEvent::Detail(id, r)) {
            self.hub.details.entry(id).or_default().loading = true;
        }
    }

    fn fetch_social(&mut self) {
        if !self.hub.social.loading && self.ask(api::social, ShareEvent::Social) {
            self.hub.social.loading = true;
        }
    }

    fn fetch_review(&mut self) {
        if !self.hub.review.loading && self.ask(api::review, ShareEvent::Review) {
            self.hub.review.loading = true;
        }
    }

    /// Each frame: what the page on screen shows, asked for when it is missing or a minute old.
    pub(in crate::gui) fn keep_fresh(&mut self) {
        if self.secret().is_none() || !matches!(Server::parse(&self.cfg.login_ip), Ok(Server::Api { .. })) {
            return;
        }
        if self.page.section() != Section::Account && self.hub.social.due() {
            self.fetch_social();
        }
        if let Some(scope) = scope_of(self.page) {
            if self.hub.feeds.get(&scope).is_none_or(Loaded::due) {
                self.fetch_feed(scope, false);
            }
        }
        if matches!(self.page, Page::GuildHome | Page::Review) && self.hub.review.due() {
            self.fetch_review();
        }
        if let Some(&id) = self.hub.picked.get(&self.page) {
            if self.hub.details.get(&id).is_none_or(Loaded::due) {
                self.fetch_detail(id);
            }
        }
    }

    /// One request from these pages; `op` is what the page greys out until the answer comes.
    fn send(&mut self, op: Op, work: impl FnOnce(&Server, &str, Cred) -> Result<String, String> + Send + 'static) {
        if self.hub.busy.contains(&op) {
            return;
        }
        let o = op.clone();
        if self.ask(work, move |r| ShareEvent::Done(o, r)) {
            self.hub.busy.insert(op);
            self.hub.said = None;
        } else {
            self.hub.said = Some(Err("Sign in first, on a server with an account API.".into()));
        }
    }

    pub(in crate::gui) fn shares_do(&mut self, d: Do) {
        match d {
            Do::Rename(n, title) => {
                let t = title.trim().to_string();
                self.send(Op::Rename(n), move |s, u, c| api::rename(s, u, c, n, &t).map(|name| if name.is_some() { "Renamed.".into() } else { "The capture's own title is back.".into() }));
            }
            Do::Share(n, audience, note) => {
                if let Some(d) = self.hub.dialog.as_mut() {
                    d.sending = true;
                    d.error = None;
                }
                self.send(Op::Share(n), move |s, u, c| api::share(s, u, c, n, audience, note.trim()).map(|_| format!("Shared with {}.", match audience {
                    Audience::Friends => "your friends",
                    Audience::Guild => "your guild",
                    Audience::Community => "the community",
                })));
            }
            Do::Unshare(id) => self.send(Op::Unshare(id), move |s, u, c| api::unshare(s, u, c, id).map(|_| "No longer shared.".into())),
            Do::Reply(id, text) => {
                let t = text.trim().to_string();
                if t.is_empty() {
                    return;
                }
                self.send(Op::Reply(id), move |s, u, c| api::reply(s, u, c, id, &t).map(|_| "Reply posted.".into()));
            }
            Do::Save(id, on) => self.send(Op::Save(id, on), move |s, u, c| api::save(s, u, c, id, on).map(|_| if on { "Saved.".into() } else { "Removed from saved.".into() })),
            Do::Follow(id, on) => self.send(Op::Follow(id, on), move |s, u, c| api::follow(s, u, c, id, on).map(|_| if on { "Following: their next moments show in Following.".into() } else { "No longer following.".into() })),
            Do::Reviewed(id, done) => self.send(Op::Reviewed(id, done), move |s, u, c| api::reviewed(s, u, c, id, done).map(|_| if done { "Marked reviewed.".into() } else { "Back in the review queue.".into() })),
            Do::Note(id, text) => {
                let marks = api::parse_marks(&text);
                // the note is the lines without a time; the marks are the lines with one
                let body: Vec<&str> = text.lines().filter(|l| api::mark_seconds(l.split_whitespace().next().unwrap_or("")).is_none()).collect();
                let body = body.join("\n").trim().to_string();
                self.send(Op::Note(id), move |s, u, c| api::note(s, u, c, id, &body, &marks).map(|_| "Guild note saved.".into()));
            }
            Do::Watch(id) => self.watch_share(id),
            Do::Refresh => {
                self.hub.all_stale();
                self.hub.social.stale();
                self.hub.details.values_mut().for_each(Loaded::stale);
            }
            Do::More(scope) => self.fetch_feed(scope, true),
        }
    }

    /// Jump into ArcheAge on a share: the server checks this account may see it and lets the game play it once
    /// (net::watch_share); the game is started when it is not open, and its replay player plays the moment.
    fn watch_share(&mut self, id: i64) {
        if matches!(self.hub.watch, Some((w, Watching::Asking)) if w == id) {
            return;
        }
        if self.ask(move |s, u, c| crate::net::watch_share(s, u, c, id), move |r| ShareEvent::Watch(id, r)) {
            self.hub.watch = Some((id, Watching::Asking));
        }
    }

    pub(in crate::gui) fn shares_came(&mut self, e: ShareEvent) {
        match e {
            ShareEvent::Feed(scope, more, r) => {
                let f = self.hub.feeds.entry(scope).or_default();
                match (more, r, f.data.as_mut()) {
                    (true, Ok(next), Some(have)) => {
                        let fresh: Vec<Share> = next.shares.into_iter().filter(|s| !have.shares.iter().any(|h| h.id == s.id)).collect();
                        have.shares.extend(fresh);
                        have.next = next.next;
                        f.loading = false;
                        f.error = None;
                    }
                    (_, r, _) => f.came(r),
                }
            }
            ShareEvent::Detail(id, r) => self.hub.details.entry(id).or_default().came(r),
            ShareEvent::Social(r) => self.hub.social.came(r),
            ShareEvent::Review(r) => self.hub.review.came(r),
            ShareEvent::Done(op, r) => {
                self.hub.busy.remove(&op);
                match r {
                    Ok(said) => {
                        self.hub.said = Some(Ok(said));
                        self.after(op);
                    }
                    Err(e) => {
                        if let (Op::Share(n), Some(d)) = (&op, self.hub.dialog.as_mut()) {
                            if d.n == *n {
                                d.sending = false;
                                d.error = Some(e.clone());
                            }
                        }
                        if e.contains("gone") {
                            self.hub.all_stale();
                        }
                        self.hub.said = Some(Err(e));
                    }
                }
            }
            ShareEvent::Watch(id, r) => {
                let w = match r {
                    Err(e) => Watching::Failed(e),
                    Ok(_) if self.child.is_some() => Watching::Playing,
                    Ok(_) => {
                        self.hint = None;
                        self.press_play();
                        match self.hint.clone() {
                            Some(h) => Watching::Failed(format!("{h} The server keeps the moment for you for 15 minutes: start ArcheAge and it plays.")),
                            None => Watching::Opening,
                        }
                    }
                };
                self.hub.watch = Some((id, w));
            }
        }
    }

    /// What a request that went through changes on screen.
    fn after(&mut self, op: Op) {
        match op {
            Op::Rename(_) => {
                self.hub.rename = None;
                self.fetch_library_now();
            }
            Op::Share(_) => {
                self.hub.dialog = None;
                self.fetch_library_now();
                self.hub.all_stale();
            }
            Op::Unshare(id) => {
                self.fetch_library_now();
                self.hub.all_stale();
                self.hub.picked.retain(|_, p| *p != id);
            }
            Op::Reply(id) => {
                self.hub.reply.clear();
                self.hub.mark(id, |s| s.replies += 1);
                if let Some(d) = self.hub.details.get_mut(&id) {
                    d.stale();
                }
            }
            Op::Save(id, on) => {
                self.hub.mark(id, |s| s.saved = on);
                if let Some(f) = self.hub.feeds.get_mut(&Scope::Saved) {
                    f.stale();
                }
            }
            Op::Follow(id, on) => {
                let by = self.hub.find(id).map(|s| s.by);
                for s in self.hub.feeds.values_mut().filter_map(|l| l.data.as_mut()).flat_map(|d| d.shares.iter_mut()) {
                    if Some(&s.by) == by.as_ref() {
                        s.following = on;
                    }
                }
                self.hub.mark(id, |s| s.following = on);
                if let Some(f) = self.hub.feeds.get_mut(&Scope::Following) {
                    f.stale();
                }
            }
            Op::Reviewed(id, _) | Op::Note(id) => {
                self.hub.note_edit = None;
                self.hub.review.stale();
                if let Some(d) = self.hub.details.get_mut(&id) {
                    d.stale();
                }
            }
        }
    }

    /// The library's list again, even while a fetch is out (its names and shares changed).
    fn fetch_library_now(&mut self) {
        self.lib.loading = false;
        self.fetch_library();
    }

    /// Opens the Share clip dialog for one of the account's captures.
    pub(in crate::gui) fn open_share(&mut self, n: i32) {
        let audience = if self.hub.social.data.as_ref().is_some_and(|s| s.guild.is_some()) { Audience::Guild } else { Audience::Friends };
        self.hub.dialog = Some(ShareDialog { n, audience, note: String::new(), sending: false, error: None });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn share(id: i64, by: &str) -> Share {
        Share { id, by: by.into(), ..Default::default() }
    }

    #[test]
    fn a_share_is_found_wherever_the_app_has_it_and_marked_everywhere() {
        let mut h = Hub::default();
        h.feeds.entry(Scope::Community).or_default().came(Ok(Feed { shares: vec![share(1, "Ayla"), share(2, "Brannoc")], next: None }));
        h.feeds.entry(Scope::WithMe).or_default().came(Ok(Feed { shares: vec![share(2, "Brannoc")], next: None }));
        assert_eq!(h.find(2).map(|s| s.by), Some("Brannoc".into()));
        assert!(h.find(3).is_none());
        h.mark(2, |s| s.saved = true);
        assert!(h.feed(Scope::Community).unwrap().shares[1].saved);
        assert!(h.feed(Scope::WithMe).unwrap().shares[0].saved);
        assert!(!h.feed(Scope::Community).unwrap().shares[0].saved);
    }

    #[test]
    fn a_feed_is_asked_again_once_stale() {
        let mut l: Loaded<Feed> = Loaded::default();
        assert!(l.due());
        l.loading = true;
        assert!(!l.due());
        l.came(Err("down".into()));
        assert!(!l.due());
        assert_eq!(l.error.as_deref(), Some("down"));
        l.stale();
        assert!(l.due());
    }

    #[test]
    fn pages_show_their_feed() {
        assert_eq!(scope_of(Page::SharedWithMe), Some(Scope::WithMe));
        assert_eq!(scope_of(Page::CommunityShared), Some(Scope::WithMe));
        assert_eq!(scope_of(Page::Discover), Some(Scope::Community));
        assert_eq!(scope_of(Page::GuildHome), Some(Scope::Guild));
        assert_eq!(scope_of(Page::Review), None);
        assert_eq!(scope_of(Page::Captures), None);
    }
}
