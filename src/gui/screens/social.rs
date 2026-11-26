//! The pages that show other people's moments: Shared with me, Saved, Community (Discover, Following) and Guild
//! (Overview, Guild shares, Review queue, Members). What they show comes from the server's shares, friends and
//! guild (docs/companion-app.md); until it sends them each page says plainly that there is nothing yet, never an
//! invented post or number.

use crate::gui::Page;
use crate::gui::theme::*;
use crate::gui::widgets::empty_state;
use eframe::egui::{self, Align2, Rect, pos2, vec2};

/// The page's heading, its line, and what it says while it has nothing.
fn words(page: Page) -> (&'static str, &'static str, Icon, &'static str, &'static str) {
    match page {
        Page::SharedWithMe | Page::CommunityShared => (
            "Shared with me",
            "Moments friends and your guild shared with you",
            Icon::People,
            "Nothing shared with you yet",
            "When a friend or your guild shares a moment with you, it shows here. Open it and jump into ArcheAge to watch it unfold.",
        ),
        Page::Saved => ("Saved", "Moments you kept to watch again", Icon::Bookmark, "No saved moments", "Save a moment from Community or from a share, and it stays here for you."),
        Page::Discover => (
            "Discover",
            "Moments players shared with the community",
            Icon::Compass,
            "No community posts yet",
            "Duels, sieges, naval fights and trade runs players share with everyone show here, newest first.",
        ),
        Page::Following => ("Following", "New moments from players you follow", Icon::Rss, "You're not following anyone yet", "Follow a player from one of their shared moments to see what they post next."),
        Page::GuildHome => (
            "Your guild",
            "Shared moments, reviews and notes, together",
            Icon::Shield,
            "Your guild shows here",
            "Your in-game guild, who is online, the moments your guild shared and what still needs a review come from the server once it shares them with the app.",
        ),
        Page::GuildShares => ("Guild shares", "Moments shared with your guild", Icon::Layers, "No guild shares yet", "Moments your guild members share with the guild show here."),
        Page::Review => (
            "Review queue",
            "Guild moments waiting for a look",
            Icon::CalendarCheck,
            "Nothing to review",
            "Moments your guild shares for review line up here, with notes at the moment they matter.",
        ),
        Page::Members => ("Members", "Your guild's members", Icon::People, "No members to show yet", "Your guild's members and who is online come from your in-game guild."),
        _ => ("", "", Icon::Film, "", ""),
    }
}

/// The centre of a social page: its heading, then what it holds (nothing yet).
pub fn page(ui: &mut egui::Ui, centre: Rect, page: Page) {
    let (title, line, ic, empty, sub) = words(page);
    let p = ui.painter();
    let x = centre.left() + 28.0;
    let t = p.text(pos2(x, centre.top() + 26.0), Align2::LEFT_TOP, title, title_font(27.0), INK);
    p.text(pos2(t.right() + 14.0, t.bottom() - 6.0), Align2::LEFT_BOTTOM, line, text_font(14.0), MUTED);
    let area = Rect::from_min_max(pos2(centre.left() + 24.0, t.bottom() + 24.0), centre.max - vec2(24.0, 24.0));
    empty_state(ui, area, ic, empty, sub);
}

/// The inspector of a social page: what it will hold, as there is nothing to select.
pub fn inspector(ui: &mut egui::Ui, r: Rect, page: Page) {
    let p = ui.painter();
    let x = r.left() + 28.0;
    let w = r.width() - 56.0;
    caps(p, pos2(x, r.top() + 32.0), "Nothing selected", MUTED);
    let what = match page {
        Page::Discover | Page::Following => "Pick a post to see who shared it, where it happened, and its discussion. Jump into ArcheAge opens the moment in the game.",
        Page::GuildHome | Page::GuildShares | Page::Review | Page::Members => "Pick a guild share to read the guild's note and its marks, and to open the review.",
        _ => "Pick a moment to see who shared it and where it happened. Jump into ArcheAge opens it in the game, in the world itself.",
    };
    let g = p.layout(what.into(), text_font(13.5), MUTED, w);
    p.galley(pos2(x, r.top() + 60.0), g, MUTED);
}
