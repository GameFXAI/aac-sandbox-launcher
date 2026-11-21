# AAEmu sandbox launcher

Starts your own ArcheAge Classic client against an AAEmu server (local research), kept apart from AA Classic itself.

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
- **Sign-up in the window.** With a server link, "No account yet? Create one" asks for a name, a password and the invite code from the server's owner.

## Use

With no arguments it opens a window. With arguments it runs on the command line:

```
aac-sandbox-launcher status
aac-sandbox-launcher setup [--aac C:\Games\AAClassic] [--mode standard|full]
aac-sandbox-launcher play --user NAME (--password P | --token HEX64) [--server ADDRESS-OR-LINK] [--login-port N] [--wait]
aac-sandbox-launcher info [--server LINK]
aac-sandbox-launcher signup --user NAME --password P [--invite CODE] [--server LINK]
aac-sandbox-launcher vault
aac-sandbox-launcher pack export FILE [--build ID]
aac-sandbox-launcher pack import FILE [--server LINK]
aac-sandbox-launcher install [--to VERSION]
aac-sandbox-launcher remove [--profile]
```

When AA Classic updates, `status` says the copy is outdated. Run `setup` again.

## Client builds: vault and build packs

A *build* is one `binNext` folder (the client program, about 65 MB), named by the SHA-256 over its file list. Every build `setup` copies is also kept in `vault\<build id>\`, so it is still there after AA Classic updates. `vault` lists them.

`binNext` cannot be downloaded without an AA Classic login, and this launcher never asks for one and we never host it. A player without AA Classic gets it from a friend: `pack export` writes one build to a file, `pack import` takes it into the vault. An import checks every file against the pack's list, refuses names that would land outside the vault, and, with a server link, refuses a build the server does not list in its accepted build ids. A pack is a program from whoever gave it to you: only import one from someone you trust.

## Game data without AA Classic

`install` downloads the game data from AA Classic's own public file host into the launcher's folder: the base `game_pak` as 64 MiB chunks (each checked by size and SHA-256, resumable), then the patch files up to `--to`. No account is used. About 23.6 GB to download and 46 GB of disk.

Not finished: merging the patch files into `game_pak`. Until that is built, a copy made by `install` cannot start the game, and the window has no Install game button.

## Build

```
cargo build --release
cargo test
```

## Not built yet

- Merging the downloaded patch files into `game_pak`, and setting up a client from the vault without an AA Classic folder; then the Install game button.
- Addon packs and settings copy.
- Crash reports sent to our own receiver.
- Installer and updates.
- Importing an action-bar layout from an existing profile.
