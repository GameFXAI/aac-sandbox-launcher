# AA Replay companion app (Obsidian Ember)

The launcher becomes the companion app the owner picked (`obsidian-ember-v2`, eight screens): where players find,
discuss and share their recorded moments, and from where **Jump into ArcheAge** opens a moment in the game client, in
the world itself. The designs are the reference for every pixel; this file is what is built and how.

## The one rule (unchanged)

The server is the only trust boundary. Who you are, which recordings exist, who may see a shared one, your friends,
your guild, your plan: all come from the server. The app shows what the server last said and sends requests; it never
decides access. A watch of someone else's moment is only played by the game when the server granted it.

## Shell

One window, resizable (default 1240x794, at least 1100x700), drawn edge to edge:

| Part | What is in it |
|---|---|
| Title bar | logo + "AA Replay"; tabs **Library**, **Community**, **Guild** (amber underline glow on the picked one); the account button (avatar initial: Settings, Report a problem, Sign out); minimise / maximise / close |
| Left rail (~15%) | **Record** with its key (F8) and its state; the section's own pages; **Friends** at the bottom (online count, each friend's character and what they are doing) |
| Centre (~62%) | the page: heading, toolbar (search, filters, sort, grid/list), content |
| Inspector (~23%) | the selected moment: title, who, where and how long; **Jump into ArcheAge ↗** and Copy link; the page's own detail (sharing, discussion, guild notes, saved details) |
| Status bar | left: what the page shows ("9 captures"); right: the game's state (dot + "ArcheAge running" / "starting" / "offline") and **Play** |

Pages:

- **Library**: My captures (the account's recordings: grid or list, search, visibility filter, newest first; inspector:
  Share clip, Jump into ArcheAge, Rename, Sharing), Shared with me, Saved, Guild review (a shortcut into Guild).
- **Community**: Discover (feed: a focused post and post cards; activity tabs All / Duels / Tournaments / Sieges /
  Naval / Trade runs; inspector: discussion and Reply), Following, Shared with me.
- **Guild**: Overview (crest, name, members online; Needs review with progress; Recent guild shares; inspector with
  Open review and the guild note), Guild shares, Review queue (progress, filters, rows; Mark reviewed; notes with
  timestamps), Members.
- **Share clip** dialog: the capture, Share with Friends / Guild / Community, who will see it, an optional note,
  Cancel / Share to …
- Launch states in the inspector: **Opening ArcheAge…** (busy, browsing stays open), **ArcheAge didn't start** (Retry
  launch, Copy link).
- First start and setup (no sign-in, game not set up yet, recording not set up) are pages of the same shell, not the
  old screens; Settings and the transfer from AA Classic are pages under the account button.

## Look

- Near-black graphite (#0c0c0e base, #141417 panels, #1b1b1f raised), hairlines #2a2a30.
- One accent: amber (#f2a14a, glow #ffb35c) on the picked tab, the picked item and the primary action only; bronze
  translucent fills (amber at low alpha) for picked rows and pills.
- Warm white text (#f3eee6), cool grey microcopy (#9b9ba3), green presence (#3bd07f), terracotta alerts (#e0765a).
- Plus Jakarta Sans (OFL, static instances made from the variable font with fontTools: Regular, Medium, SemiBold,
  Bold), sizes 12-15 for text, 22-28 for headings.
- Line icons painted in code (1.5 px strokes): library, people, shield, record dot, compass, rss, star, bookmark,
  share, link, external arrow, search, filter, grid, list, sort, chevron, pencil, lock, check, home, layers, calendar
  check, user plus, chat, warning, gamepad, close, minimise, maximise.
- Rounded 8-10 px corners; thumbnails are stills with a soft top gradient for their badges.
- Moment pictures: art per kind of moment (duel, tournament, siege, naval, trade run, harbour), stand-ins cut from
  the owner's design images until the owner makes full-size ones (`tools/make_art.py`, same file names). People are
  shown by their initial in a coloured circle (no stock faces).

## Server (aaemu35), all behind the account API's sign-in

What exists: `POST /api/replays/list`, `/api/replays/delete`, the plan and cap, uploads.

New, each resolving the caller from their own credentials:

| Endpoint | Does |
|---|---|
| `POST /api/replays/rename` {n, title} | renames one of the caller's recordings (game server writes, as for delete) |
| `POST /api/replays/share` {n, audience: friends\|guild\|community, note} | shares one of the caller's recordings; returns the share id; audience checked server side (guild only when the caller's character is in one) |
| `POST /api/replays/unshare` {share} | the owner takes a share back |
| `POST /api/shares/feed` {scope: community\|following\|withme\|guild, kind, query, after} | the shares the caller may see, newest first, with title, owner name, kind, length, where, replies, saves, opens |
| `POST /api/shares/get` {share} | one share and its discussion |
| `POST /api/shares/reply` {share, text} | a reply (length-limited, rate-limited) |
| `POST /api/shares/save` / `unsave` {share}; `POST /api/shares/saved` | bookmarks |
| `POST /api/shares/watch` {share} | asks the game server to let this account watch that share once; returns a watch ticket number for the addon |
| `POST /api/social` | the caller's friends (from their characters' in-game friends) with online state and what they are doing, and their guild (in-game expedition: name, members online) |
| `POST /api/guild/review` {filter}; `/api/guild/reviewed` {share}; `/api/guild/note` {share, text, marks:[{at, text}]} | the guild's review queue over its guild shares |

Tables (login DB): `replay_shares` (id, owner account, n, audience, guild id, note, title copy, created, removed),
`share_saves`, `share_replies`, `share_reviews` (guild, share, reviewer, at), `share_notes` (+ timestamp marks).
Friends and guild membership are read from the game DB (`friends`, `expedition_members`, `characters`) at request
time, never taken from the app.

Watching someone else's moment: the app calls `/api/shares/watch`; the login server checks the caller may see the
share and sends the game server a grant (viewer account, owner account, recording number, ticket, expiry). The app
leaves `{ ticket = T }` in `pending_watch.txt`; the addon asks the game to play ticket T, and the game plays it only
for the granted viewer, once, before it expires. The file itself proves nothing.

## Order of work

1. Design system and shell in the launcher: font, palette, icons, components (tabs, rail, search, pills, segmented
   control, sort menu, grid/list toggle, moment card, list row, inspector buttons, modal, status bar), resizable
   window. Library → My captures on the real list, with Jump into ArcheAge (the existing watch), the launch states,
   delete. The account button with Settings / Report / Sign out, the setup pages.
2. Server: rename, shares, feed, saves, replies, social (friends and guild from the game DB), watch grants; tests.
3. Game: watch tickets (a granted share plays through the addon like an own recording).
4. App: Share clip, Shared with me, Saved, Community (feed, discussion), Guild (overview, review queue, notes),
   Friends module.
5. Each page screenshotted next to its design; the in-game watch of a shared moment checked on the local server.

Where it stands: step 1 is built. The shell (title bar, rail with Record and Friends, inspector, status bar,
resizable borderless window), the welcome and sign-in page, My captures on the real list (grid and list, search,
visibility, sort, inspector with Jump into ArcheAge, its launch states, Delete, sharing and the plan), the record menu,
the account menu, the first-start offer as a dialog, and Settings / From AA Classic / Log inside the shell.
Shared with me, Saved, Community and Guild say plainly that they are empty until steps 2-4 give them the server's
shares, friends and guild. Share clip and Rename appear only once the server has them.

Step 2 is built on the server (aaemu35 08c8c8948, AccountApi.Shares.cs): rename, share and unshare, the feed (community, following, withme, guild, mine, saved), a share with its replies, replies, saves, follows, the watch grant (LG 0x007 to the game server), /api/social (friends and guild by character names, online from the game's character_presence) and guild review with notes and marks. /api/info says shares = true; the replay list carries each recording's name and shares. Visibility is decided from the game database on every request. Next: step 3, the game side of the watch grant.

What the app never shows: invented numbers. A page with nothing yet says so plainly ("No community posts yet").
