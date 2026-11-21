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
aac-sandbox-launcher remove [--profile]
```

When AA Classic updates, `status` says the copy is outdated. Run `setup` again.

## Build

```
cargo build --release
cargo test
```

## Not built yet

- "Install game" for a PC without AA Classic, the build vault and build packs.
- Addon packs and settings copy.
- Crash reports sent to our own receiver.
- Installer and updates.
- Importing an action-bar layout from an existing profile.
