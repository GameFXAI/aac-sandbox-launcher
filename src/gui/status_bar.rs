//! The bar along the bottom: on the left what the page shows ("9 captures") or what needs saying (a problem, the
//! install going on); on the right the game's state and the button that does the next thing (Play).

use super::theme::*;
use super::widgets::{primary_button, sentence};
use super::{Action, App, Page};
use crate::core::*;
use crate::{find, install, vault};
use eframe::egui::{self, Align2, Color32, Rect, Stroke, UiBuilder, pos2, vec2};
use std::time::Duration;

/// What the bar says: the dot and words on the left (None: nothing to say beyond the page), the button and what it does.
struct Said {
    note: Option<(Color32, String, String)>,
    button: &'static str,
    action: Option<Action>,
}

impl App {
    fn said(&self) -> Said {
        let say = |c: Color32, t: &str, s: String, button, action| Said { note: Some((c, t.to_string(), s)), button, action };
        if self.asking {
            return say(GOLD, "Asking the server", self.log.last().cloned().unwrap_or_default(), "WAIT", None);
        }
        if let (Some(m), None, None) = (self.modified, &self.child, &self.hint) {
            // said every time, not once: Play still works (the server decides what an account may do)
            return say(RED, "This install looks modified", m.to_string(), "PLAY", Some(Action::Play));
        }
        if self.xfer.working && self.play_after_transfer {
            return say(GOLD, "Bringing your settings", "The game starts right after.".into(), "WAIT", None);
        }
        if self.page == Page::Transfer && !self.installing && !self.busy {
            let (c, t, s, b, a) = self.transfer_status();
            return say(c, &t, s, b, a);
        }
        if self.installing {
            return say(GOLD, "Installing the game", self.log.last().cloned().unwrap_or_default(), "STOP", Some(Action::Stop));
        }
        if self.busy {
            return say(GOLD, "Installing your copy", self.log.last().cloned().unwrap_or_default(), "INSTALLING", None);
        }
        if self.child.is_some() {
            return Said { note: self.hint.clone().map(|h| (RED, "Playing".into(), h)), button: "PLAYING", action: None };
        }
        match &self.state {
            Err(e) => say(RED, "AA Classic was not found", e.clone(), "FIND IT", Some(Action::ChooseFolder)),
            Ok(State::NotSetUp) if self.cfg.mode == Mode::Own => {
                let last = self.log.last().filter(|l| l.starts_with("install:") || l.starts_with("build pack:")).cloned();
                if vault::builds(&self.paths).is_empty() {
                    say(MUTED, "First: a build pack", last.unwrap_or_else(|| "The program files (65 MB) come from a friend who has the game: ask for a build pack and import it.".into()), "IMPORT PACK", Some(Action::ImportPack))
                } else if install::load_state(&self.paths).is_some() {
                    say(GOLD, "Install not finished", last.unwrap_or_else(|| "Goes on where it stopped.".into()), "CONTINUE", Some(Action::Install))
                } else {
                    say(MUTED, "Not installed yet", last.unwrap_or_else(|| "Downloads the game data from AA Classic's file host: about 24 GB to download, 46 GB on disk.".into()), "INSTALL GAME", Some(Action::Install))
                }
            }
            Ok(State::NotSetUp) => {
                // AA Classic on another drive: say where the copy goes and why
                let other = match (&self.cfg.aac_dir, self.cfg.mode) {
                    (Some(d), Mode::Standard) if find::client_place(&self.paths.client(), d).is_some() => Some(find::place_note(&self.paths.root, &self.paths.client(), d)),
                    _ => None,
                };
                let line = self.hint.clone().or(other).unwrap_or_else(|| {
                    format!(
                        "The first Play makes your own copy of the game ({}), then it starts. AA Classic stays exactly as it is.",
                        if self.cfg.mode == Mode::Full { "about 45 GB" } else { "about 65 MB, a minute or two" }
                    )
                });
                say(if self.hint.is_some() { RED } else { MUTED }, "One step before you play", line, "PLAY", Some(Action::Play))
            }
            Ok(State::BinOutdated) | Ok(State::PakOutdated) => say(GOLD, "AA Classic was updated", "Play updates your copy to match it first (a minute or two).".into(), "PLAY", Some(Action::Play)),
            Ok(State::Ready) => Said { note: self.hint.clone().map(|h| (RED, "Could not start".into(), h)), button: "PLAY", action: Some(Action::Play) },
        }
    }

    /// What the page shows, for the left of the bar when there is nothing else to say.
    fn page_summary(&self) -> Option<(Color32, String)> {
        if let Some(n) = &self.lib.note {
            if self.page == Page::Captures {
                return Some(match n {
                    Ok(t) => (GREEN, t.clone()),
                    Err(t) => (RED, t.clone()),
                });
            }
        }
        if let (Some(n), true) = (&self.hub.said, self.page != Page::Captures || self.lib.note.is_none()) {
            return Some(match n {
                Ok(t) => (GREEN, t.clone()),
                Err(t) => (RED, t.clone()),
            });
        }
        let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        match self.page {
            Page::Captures => {
                let n = self.lib.data.as_ref()?.replays.len();
                Some((MUTED, plural(n, "capture", "captures")))
            }
            Page::SharedWithMe | Page::CommunityShared => Some((MUTED, format!("{} shared with you", self.hub.feed(crate::shares::Scope::WithMe)?.shares.len()))),
            Page::Saved => Some((MUTED, format!("{} saved", self.hub.feed(crate::shares::Scope::Saved)?.shares.len()))),
            Page::Discover => Some((MUTED, "Community".into())),
            Page::Following => Some((MUTED, plural(self.hub.feed(crate::shares::Scope::Following)?.shares.len(), "post", "posts"))),
            Page::GuildHome | Page::Members => Some((MUTED, self.hub.social.data.as_ref()?.guild.as_ref()?.name.clone())),
            Page::GuildShares => Some((MUTED, plural(self.hub.feed(crate::shares::Scope::Guild)?.shares.len(), "guild share", "guild shares"))),
            Page::Review => {
                let r = self.hub.review.data.as_ref()?;
                Some((MUTED, format!("{} of {} reviewed", r.done(), r.shares.len())))
            }
            _ => None,
        }
    }

    pub(in crate::gui) fn status_bar(&mut self, ui: &mut egui::Ui, rect: Rect, act: &mut Option<Action>) {
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, Color32::from_rgb(9, 9, 11));
        p.line_segment([pos2(rect.left(), rect.top() + 0.5), pos2(rect.right(), rect.top() + 0.5)], Stroke::new(1.0, LINE));
        let said = self.said();

        // the button, right
        let label = sentence(said.button);
        let bw = (p.layout_no_wrap(label.clone(), medium_font(14.0), INK).size().x + 48.0).max(96.0);
        let b = Rect::from_min_size(pos2(rect.right() - 16.0 - bw, rect.center().y - 14.0), vec2(bw, 28.0));
        let enabled = said.action.is_some();
        let tip = if enabled { "Start ArcheAge with your account" } else { "" };
        let resp = ui.scope_builder(UiBuilder::new().max_rect(b), |ui| primary_button(ui, &label, if said.button == "PLAY" { Some(Icon::Play) } else { None }, false, bw, 28.0, enabled)).inner;
        if resp.on_hover_text(tip).clicked() {
            *act = said.action;
        }

        // the game's state, left of the button
        let (dot, state) = if self.child.is_some() {
            (GREEN, "ArcheAge running")
        } else if self.asking {
            (GOLD, "Signing in")
        } else if self.busy || self.installing {
            (GOLD, "Installing")
        } else if matches!(self.state, Ok(State::Ready)) {
            (FAINT, "ArcheAge not running")
        } else {
            (AMBER, "ArcheAge not set up")
        };
        let sr = p.text(pos2(b.left() - 18.0, rect.center().y), Align2::RIGHT_CENTER, state, medium_font(13.5), rgba(214, 212, 208, 255));
        p.circle_filled(pos2(sr.left() - 12.0, rect.center().y), 4.5, dot);
        if dot == GREEN {
            p.circle_filled(pos2(sr.left() - 12.0, rect.center().y), 8.0, rgba(59, 208, 127, 40));
        }

        // left: what needs saying, else what the page shows
        let x = rect.left() + 22.0;
        let room = sr.left() - 40.0 - x;
        match said.note {
            Some((c, title, sub)) => {
                p.circle_filled(pos2(x + 4.0, rect.center().y), 4.5, c);
                let t = p.text(pos2(x + 16.0, rect.center().y), Align2::LEFT_CENTER, &title, medium_font(13.5), INK);
                let line = elide(&p, &sub, room - (t.width() + 30.0));
                p.text(pos2(t.right() + 12.0, rect.center().y), Align2::LEFT_CENTER, line, text_font(13.0), MUTED);
                if !sub.is_empty() {
                    ui.interact(Rect::from_min_max(pos2(x, rect.top()), pos2(x + room, rect.bottom())), ui.scope_id().with("status-why"), egui::Sense::hover()).on_hover_text(sub);
                }
            }
            None => {
                if let Some((c, t)) = self.page_summary() {
                    let line = elide(&p, &t, room);
                    p.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, line, text_font(13.5), if c == MUTED { rgba(206, 204, 200, 255) } else { c });
                }
            }
        }

        // the install's progress along the bar's top edge
        if self.busy {
            let bar = Rect::from_min_size(rect.left_top(), vec2(rect.width(), 2.0));
            let f = match self.progress {
                Some(v) => v,
                None => ((ui.input(|i| i.time) as f32 * 0.6).sin() * 0.5 + 0.5) * 0.9 + 0.05,
            };
            p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * f.clamp(0.02, 1.0), 2.0)), 0.0, GOLD);
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
    }
}

/// `text` cut to `width` with an ellipsis.
pub fn elide(p: &egui::Painter, text: &str, width: f32) -> String {
    fit(p, text, width, text_font(13.0))
}

/// `text` cut to `width` in `font` with an ellipsis.
pub fn fit(p: &egui::Painter, text: &str, width: f32, font: egui::FontId) -> String {
    if width <= 20.0 {
        return String::new();
    }
    if p.layout_no_wrap(text.to_string(), font.clone(), INK).size().x <= width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let s: String = chars[..mid].iter().collect::<String>() + "…";
        if p.layout_no_wrap(s, font.clone(), INK).size().x <= width { lo = mid } else { hi = mid - 1 }
    }
    chars[..lo].iter().collect::<String>().trim_end().to_string() + "…"
}
