# AA Replay launcher

Starts your own ArcheAge Classic client against an AA Replay server, kept apart from AA Classic itself. (The exe is
still `aac-sandbox-launcher.exe` and its folder `%LOCALAPPDATA%\AAEmuSandbox`, so earlier installs keep working.)

## A tester's first start

1. The tester opens their invite link. The page has one download button: `AAReplaySetup-<invite code>.exe`, the same
   installer for everyone, handed out under a name that carries that tester's code.
2. The installer takes the code from its own file name (or `/INVITE=<code>`) and leaves it in
   `%LOCALAPPDATA%\AAEmuSandbox\invite.txt`. It has one page (Install) and needs no administrator rights.
3. The launcher opens on one screen: **Create your account**, name, password, **PLAY**. PLAY makes the account with
   the invite, makes the game copy, signs in and starts the game.
4. Once, after the first sign-in: "Bring your AA Classic settings?" (graphics and sound, interface options), with
   **Bring them and play** / **Not now, just play**. Settings (the gear) can do it, or undo it, any time.

Everything else (install mode, client builds, game download, bringing settings) is behind the gear.
There is no server choice: the launcher always plays on the arena box (`core::DEFAULT_SERVER`); an address saved by
an older launcher is ignored. For development only, the environment variable `AAEMU_SANDBOX_SERVER` (a link or a
bare address such as `127.0.0.1`) points it elsewhere, and `play --server` does the same on the command line.
**Report a problem** (top bar) sends what the tester writes, plus the tail of `launcher.log` and the game's
`ArcheAge.log` (long hex runs, such as sign-in tokens, are blanked), to the server's account API. When that fails,
it saves the report as a file and opens its folder, so it can be sent by hand.

The installer is not code-signed yet: Windows SmartScreen says "Windows protected your PC". **More info**, then
**Run anyway**. The download page says so too.

## Remembered sign-in

"Remember me on this PC" (on by default) asks the server for a session token at sign-in. Only that token is kept, in
`signin.bin`, sealed with Windows DPAPI for this Windows user; the password is never stored. Next time the window
says "Welcome back" and PLAY needs no password. The server keeps only the token's SHA-256; a token lasts 30 days from
its last use and 180 days at most, and an account has at most 5. "Not you? Sign out" ends the token on the server too.
A token the server no longer takes (ran out, signed out everywhere, banned) is forgotten and the password is asked again.

## Replays

**Replays** in the top bar lists the account's recordings as the server keeps them: newest first, with Play (starts
the game and opens that recording in the arena's replay player) and Delete (asks first; it cannot be undone). The plan
badge next to it says FREE or REPLAY+ (amber in its last five days); Settings → Account shows the plan, its end date and
"Contact to upgrade" when the server names an upgrade page. Recordings past the plan's cap are shown dimmed with a lock,
not hidden. The last list is kept in `recordings\library.json` so the screen fills at once; it is forgotten at sign-out.
Play leaves `pending_watch.txt` in the `aac_replay` addon folder, which the addon takes once when the game loads.

Nothing here is trusted by the server: it works out the account from the sign-in, the plan and cap from its own
database, and deletes only a number in that account's own list.

## Install check

A release build records its own SHA-256 (or, after an update, the signed release's) in `installed.json` in the data
folder and compares it at every start. When this exe and version were recorded with another hash, the bottom bar says
"This install looks modified" and asks for a reinstall; Play still works. A new version, another folder or a debug
build is never flagged.

## Finding AA Classic

Without a saved folder the launcher looks, in this order: the uninstall entries of AA Classic's installer (in all four
registry views: id `{DEBDBBE9-230E-42C4-B44D-E0DC0CD81791}_is1`, or a name starting with "ArcheAge Classic"), XLGames'
`setup_path`, then the usual folders on every drive. A folder counts only with `binNext\archeage.exe` and `game_pak`.
Settings says how it was found; **FIND IT** picks one by hand.

**AA Classic on another drive.** Standard mode shares `game_pak` through a hard link, which Windows only allows on one
drive. So when AA Classic is on another drive than `%LOCALAPPDATA%`, the copy goes to `<that drive>\AAReplay\client`
(still about 65 MB; `client-dir.txt` in the data folder points at it) and the window says so before PLAY. If that
folder cannot be made, it says to use Full separation instead (a full copy, about 45 GB, on the launcher's own drive).
Uninstall offers to remove that folder too.

## What it does

- **Never writes to your AA Classic folder.** It reads `binNext` and `game_pak` only. It never opens `sfcl`, the official launcher, `features.json` or `version`.
- **Its own copy** lives in `%LOCALAPPDATA%\AAEmuSandbox\client` (override with `AAEMU_SANDBOX_ROOT`).
  - **Standard** (~65 MB): copies `binNext` and hardlinks `game_pak`. The hardlink only works on the same drive. The client opens `game_pak` read-only.
  - **Full** (~45 GB): copies everything. Nothing is shared with AA Classic.
- **Its own user folder.** The client starts with `USERPROFILE`, `HOMEDRIVE` and `HOMEPATH` pointing to `%LOCALAPPDATA%\AAEmuSandbox\profile`. Logs, addons, `system.cfg` and the `USER` data go to `profile\Documents\AAClassic` and never to your real `Documents\AAClassic`.
- **No reports.** It sets `AAC_NO_CRASH_UPLOAD=1` and `AAC_NO_PERF_UPLOAD=1`, which stops the crash and perf uploads in the client. Dumps stay in the sandbox.
- **Won't start while AA Classic runs.** It refuses if `archeage.exe` or the official launcher is running.
- **Asks for this server's account only.** Never type your AA Classic password.
  - Server given as a link (`https://...`): the password goes to that server's account API and the client gets a one-time launch ticket. Plain `http://` is accepted only for this PC, and redirects are not followed, so the password never crosses the network in the clear.
  - Server given as a bare address (a development server): the client gets the username and the SHA-256 of the password.
- **Sign-up in the window.** With a server link and an invite from the installer, PLAY makes the account; without one, "Have an invite code? Create an account" asks for the code too.
- **Readable by screen readers and UI Automation** (AccessKit): every button and field has a name, so the window can be driven without a mouse, for tests too.

## Use

With no arguments it opens a window. With arguments it runs on the command line:

```
aac-sandbox-launcher status
aac-sandbox-launcher setup [--aac C:\Games\AAClassic] [--mode standard|full]
aac-sandbox-launcher play --user NAME (--password P | --token HEX64) [--server ADDRESS-OR-LINK] [--login-port N] [--wait]
aac-sandbox-launcher info [--server LINK]
aac-sandbox-launcher signup --user NAME --password P [--invite CODE] [--server LINK]
aac-sandbox-launcher settings copy [--from FILE]
aac-sandbox-launcher addons [--server LINK]
aac-sandbox-launcher art
aac-sandbox-launcher update [--check]
aac-sandbox-launcher vault
aac-sandbox-launcher pack export FILE [--build ID]
aac-sandbox-launcher pack import FILE [--server LINK]
aac-sandbox-launcher install [--to VERSION] [--build ID] [--offline]
aac-sandbox-launcher pak info|list|check FILE
aac-sandbox-launcher remove [--profile]
```

When AA Classic updates, `status` says the copy is outdated. Run `setup` again.

## Your AA Classic graphics settings

Only when asked ("Copy my graphics settings" in Settings, or `settings copy`): the launcher reads the real `Documents\AAClassic\system.cfg` and takes the display, quality and sound settings into the sandbox client's own `system.cfg`. That file is only read. A server address, language or anything about an account is never taken. Keybinds, in-game options and action bars are stored on AA Classic's server and cannot be copied this way.

## The server's addons

A server with an account API can name an addon pack (the menus and HUD of its game modes). Before play the launcher downloads it from that server, checks every file against the pack's list and the pack's id against the one the server named, and installs it into the sandbox client's own `Documents\AAClassic\Addon` (never the real one). It adds the addons to `addons.txt` and `addon_settings`; addons the player put there themself and their on/off settings are left alone. The next pack replaces exactly the addons the last one installed.

Checked 2026-11-21 against a local login server: the arena pack (`pts_menu`, 174 files) downloads and installs; the files equal the server's package. Not yet seen: the client loading it from there (the game was not started).

## The AA REPLAY art

Before every play the launcher puts the AA REPLAY pictures in its own copy of the client: the loading screens, the
login logo and server-select and rating backgrounds, the loading bar, the intro movie, the startup splash and the
window icon (art.rs). AA Classic's folder is never touched.

- **Where it comes from.** `https://77-42-46-123.sslip.io/launcher/art/art.json` names the art pack (id, size,
  SHA-256). It is signed with the same key as the launcher's own updates (`art.json.sig`), so a pack nobody signed is
  never used. The pack (about 12 MB) is downloaded once, checked, and kept in `<data>rt`; when the host can't be
  reached, the kept one is used.
- **What it changes, in our copy only:** `clienta-replay-art\overlay.bin` + `overlay.json` (the new game files, a
  file table pointing at them and the header, built on this PC against its own `game_pak`, which is only read);
  `binNextersion.dll`, the loader that makes the client read `game_pak` + `overlay.bin` as one archive; and the
  splash and icon resources in `binNext
es_ru.dll`, `res_us.dll` and `archeage.exe`, only when those are the build
  the art was made for (the originals are kept in `aa-replay-art\originals`).
- **After an AA Classic update.** A new `binNext` is copied as before, which drops the loader and the resources; a
  changed `game_pak` turns the loader off by itself (the length no longer matches `overlay.json`). The next PLAY sees
  this (`aa-replay-art\launcher.json`) and builds it again.
- **Required on the hosted Replay server.** Play checks the signed art and stops before sign-in if the full pack cannot be applied.
  On other servers art is optional; `AAREPLAY_ART=off` skips it there. `AAREPLAY_ART_MANIFEST` can point at a different signed manifest for testing.
- **Publishing new art.** On the server repo: `python Tools/branding/aa_replay_art.py launcher-pack` (it needs
  `zstandard`), then `aac-sandbox-launcher manifest sign art.json <key>`, then upload the pack, `art.json` and
  `art.json.sig` to `/srv/aareplay/www/launcher/art/`. `art` on the command line does the install by hand.

## Client builds: vault and build packs

A *build* is one `binNext` folder (the client program, about 65 MB), named by the SHA-256 over its file list. Every build `setup` copies is also kept in `vault\<build id>\`, so it is still there after AA Classic updates. `vault` lists them.

`binNext` cannot be downloaded without an AA Classic login, and this launcher never asks for one and we never host it. A player without AA Classic gets it from a friend: `pack export` writes one build to a file, `pack import` takes it into the vault. An import checks every file against the pack's list, refuses names that would land outside the vault, and, with a server link, refuses a build the server does not list in its accepted build ids. A pack is a program from whoever gave it to you: only import one from someone you trust.

## The game without AA Classic ("Download" mode)

`install` (the window's **Install game** button) sets up a playable client on a PC that has no AA Classic folder:

1. the base `game_pak` from AA Classic's own public file host, as 64 MiB chunks, each checked by size and SHA-256;
2. the patch files up to the version the server wants (`--to`, or every patch the host has);
3. the patches merged into `game_pak` (pak.rs): every patch file is checked against the patch's own MD5, the patch's `deleted.txt` is added to the game data's own and the files it names are removed;
4. `binNext` from the build vault (a friend's build pack).

No account is used, nothing is hosted by us. About 24 GB to download and 46 GB of disk. It can be stopped at any point and goes on where it was; a merge that was cut off is undone from an undo file and done again; a damaged patch is deleted and downloaded again. Running `install` on a finished copy brings it up to a newer version.

The game data file is only ever changed when no other name on disk points at it, so a Standard-mode link to AA Classic's `game_pak` can never be written to.

Checked 2026-11-21 against real files: the 34 patch files 264-297 merged by this code give 6,995 files, each with the size and MD5 that AA Classic's own build-297 `game_pak` lists, and every file's bytes match its MD5. Not yet run: a whole install from an empty disk (no room on the development PC), and a game start on a copy made this way.

## Updating the launcher itself

The launcher can replace itself with a newer version named in a signed manifest (update.rs). The manifest is a small JSON file on any static file host, with its ed25519 signature next to it (`<manifest>.sig`); the public key is built into the exe, so only the holder of the private key can offer an update. Only a higher version is taken, the new exe is checked by size and SHA-256, downloads are HTTPS only.

It is off until a manifest address exists: `MANIFEST_URL` in `src/update.rs`, or `manifest_url` in `launcher.json`. The window then shows "Update launcher to X" in the top bar; `update` does it from the command line.

Publishing a release: bump `version` in `Cargo.toml`, then

```
.\installer\release.ps1 -Version 0.2.1 -KeyFile <private key file>
```

It tests and builds, Authenticode-signs the exe when signing is set up (`docs/code-signing.md`; otherwise the release
is unsigned and says so), makes `release\aac-sandbox-launcher-<version>.exe` and `release\manifest-<version>.json`
with its signature, and builds the signed (or unsigned) `installer\out\AAReplaySetup.exe`. By hand, the update part
is `manifest make <exe> <version> <url> <out>` and `manifest sign <out> <key>`.

Then upload the exe and the manifest (as `manifest.json` and `manifest.json.sig`, keeping the last one as
`manifest-<old version>.json`) to the box's `/srv/aareplay/www/launcher/`, and check with the previous exe's
`update --check`. The private key was made with `manifest keygen` and is kept outside every repository; whoever has it can update every launcher, and a lost key means a new launcher has to be handed out by hand.

Checked 2026-11-21 with a file server on this PC: a 0.1.0 exe found, downloaded and swapped in a 0.2.0; a manifest changed after signing was refused. Not set up: a real host for the files.

## Build

```
cargo build --release
cargo test
```

## Installer

`installer\aac-sandbox-launcher.iss` is an Inno Setup 6 script: per user, no administrator rights, the launcher
only (no game files). It keeps the first installer's AppId, so an AAEmu Sandbox Launcher 0.1.x install is upgraded in
place (same folder, one uninstall entry, the old shortcuts replaced by "AA Replay"). Uninstall removes the launcher;
it asks before removing the data folder with the game copy (and a copy on AA Classic's drive).

```
cargo build --release
iscc /DAppVersion=0.2.1 installer\aac-sandbox-launcher.iss     -> installer\out\AAReplaySetup.exe (unsigned)
```

`release.ps1` does this too, and signs the setup and its uninstaller when signing is set up (`/DSign` and the
`aareplay` sign tool, `installer\sign.ps1`).

The account API serves the file in `Accounts:DownloadsDir` as `/api/download/AAReplaySetup-<code>.exe`, and the
invite page `/api/join/<code>` links to it (AA Replay server: `Tools/invites/new-invite.ps1`).

Since 0.3.1, the file there is the **bootstrapper** (`bootstrap/`, `docs/bootstrapper.md`), not this installer. It is
a small `AAReplaySetup.exe` that stays the same across releases, so SmartScreen warns only on that first download. It
fetches the signed `launcher/setup.json` and the installer that names (`release.ps1` step 5), and runs it `/SILENT`.
The uninstaller runs `recorder remove`: recording off, and the admin task removed with no prompt (`src/admin.rs`).

Checked 2026-11-22 on the development PC (Inno Setup 6.7.3): a 0.1.1 install upgraded in place; the invite in the file
name reached the launcher; first start to account, copy, remembered sign-in, settings offer, report and sign out.

## Licence

GPL-3.0, see `LICENSE`.

## Not built yet

- Crash reports sent to our own receiver (Report a problem sends the logs by hand).
- Code signing: the release steps sign when an Artifact Signing account is set up (`docs/code-signing.md`); until the
  owner's account and identity check are done, releases are unsigned and SmartScreen warns once.
- Importing an action-bar layout from an existing profile.
