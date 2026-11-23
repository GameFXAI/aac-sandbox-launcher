# The download people get: AAReplaySetup.exe (the bootstrapper)

AA Replay ships unsigned for now. Authenticode signing (Certum) comes later as the build step that is ready for it
(`installer/sign.ps1`, `docs/code-signing.md`). Until then, Windows warns once, and only once:

| When | What Windows shows | Why only then |
|---|---|---|
| The first download, from the browser | SmartScreen: "Windows protected your PC", then More info, then Run anyway | The browser marks the download as "from the internet" (the `Zone.Identifier` stream). SmartScreen checks marked, unsigned files that few people have run. |
| Set up recording | One admin prompt for "AA Replay" (Unknown publisher) | AA Classic runs as admin, so recording it needs admin rights. They are asked for once, and a scheduled task keeps them (`src/admin.rs`). |
| Updates, every later AA Classic start | Nothing | The launcher downloads updates itself, and its downloads carry no mark. The admin task starts with no prompt. |

## Why a bootstrapper

SmartScreen builds an unsigned file's reputation per file hash. An installer that changes with every release starts
again from zero each time, and so would its warning count. So the file people download is small and stays the same:

- `bootstrap/` builds `AAReplaySetup.exe`, about 380 KB. It holds no launcher version.
- It reads `launcher/setup.json` (+ `.sig`, the launcher's update key) from the update host. Then it downloads the
  installer that file names, checks its size and SHA-256, and runs it for this Windows user (`/SILENT`, no admin).
  Finally it starts AA Replay.
- What changes with a release is `setup.json` and the installer it names (`release.ps1` step 5). The bootstrapper
  stays the same.
- Its file name is always `AAReplaySetup.exe`. The account site hands out the same bytes as
  `AAReplaySetup-<invite>.exe`, and the bootstrapper passes the invite on to the installer.
- It has full version info (product "AA Replay", company "AA Replay", description "AA Replay setup") and its own
  icon (`aareplay.ico`, drawn by `make_icon.py`, no client art).
- Its manifest says `asInvoker`. Without that, Windows guesses from the word "Setup" in the name that it wants admin
  rights.
- It uses WinHTTP (Windows' own HTTPS and proxy settings) and follows no redirects.
- `AAReplaySetup.exe /CHECK` downloads and checks the signed installer, then installs nothing. Exit 0 means it is
  good.

The release that is published, and must not be rebuilt for a launcher release:

| File | Size | SHA-256 |
|---|---|---|
| bootstrapper 1.0.0 | 385,536 | `6c5e73a82f2157a31d99e20a8366deb9757d732e27b391a7e98752fb66e869ca` |

Change the bootstrapper only when it must change: the host, the key, or a bug. Each change starts its reputation
again. When Certum signing is set up, sign it once. Reputation then follows the certificate, not the hash.

Build (only for a change): `cargo build --release` in `bootstrap/`, then `cargo test --release`. The file is
`bootstrap/target/release/AAReplaySetup.exe`.

## No mark on what the launcher installs

The launcher's downloads (self-update, the recorder pack) never carry a `Zone.Identifier`: only browsers and
Explorer add one. A launcher taken out of a downloaded zip would carry one, and so would files copied from it. So:

- `update::strip_mark_of_the_web` runs on the launcher's own exe at every start, on an update's new exe, and on
  every file of the recorder it unpacks.
- The bootstrapper strips the installer it wrote.
- Tests: `update::tests::what_the_launcher_installs_never_carries_the_download_mark` (a marked exe, then an update
  over it), `update::tests::a_real_download_carries_no_mark`, and the bootstrapper's
  `a_signed_installer_is_downloaded_and_checked`.

## Release checklist (unsigned)

1. `.\installer\release.ps1 -Version X.Y.Z -KeyFile <update key> -NoAuthenticode`
2. Scan every file that goes out with Defender:

   ```
   & "$env:ProgramFiles\Windows Defender\MpCmdRun.exe" -Scan -ScanType 3 -File <file> -DisableRemediation
   ```

   Scan the bootstrapper, `release\aac-sandbox-launcher-X.Y.Z.exe`, `release\AAReplaySetup-X.Y.Z.exe`, the recorder
   pack, and the unpacked recorder folder (Frida's `_frida.pyd`, `AA Replay.exe`). Each must say "found no threats".
3. Upload to `/srv/aareplay/www/launcher/` on the box:
   - the exe and `manifest.json` (+ `.sig`), as the README says;
   - `AAReplaySetup-X.Y.Z.exe` into `setup/`;
   - `setup-X.Y.Z.json` (+ `.sig`) as `setup.json` (+ `.sig`).

   The bootstrapper in `/srv/aareplay/downloads/AAReplaySetup.exe` stays as it is.
4. `AAReplaySetup.exe /CHECK` on any PC: exit 0.
5. **Microsoft submission (the owner does this; it is only prepared here).** For each new bootstrapper (rare), and
   for any file Defender or SmartScreen flags:
   - At https://www.microsoft.com/en-us/wdsi/filesubmission, sign in and choose **Software developer**.
   - Product: **Microsoft Defender Antivirus** (and **Microsoft Defender SmartScreen** for a SmartScreen block).
   - Upload the file: the bootstrapper, or the one flagged. For a zipped submission, use password `infected`.
   - Choose **Incorrectly detected as malware/malicious** when something was detected, with the detection name and
     the signature version. When nothing was detected yet, submit it the same way for analysis ("incorrectly
     detected / for analysis"). That gives Microsoft the file before players report it.
   - In the comments, say what it is: "AA Replay setup: downloads the AA Replay launcher installer from our update
     host (signed manifest) and runs it per user. Open-source launcher. Unsigned until our code-signing certificate
     arrives."
   - Keep the submission ID with the release.

## What Defender says (checked 2026-11-24)

- **Versions.** Defender engine 4.18.26100.4 with signatures 1.463.431.0 (2026-11-23), real-time protection on.
- **Files.** It found no threats in:
  - the bootstrapper 1.0.0;
  - launcher 0.3.0;
  - the 0.3.0 installer;
  - recorder pack 1.1.0 (`aa-replay-recorder-1.1.0-4220762b6a09f975.pack.zst`);
  - the unpacked recorder: 226 files, including Frida 17.9.10's `_frida.pyd` and `AA Replay.exe`.
- **Frida.** It was the likely trigger, but it is not flagged by signatures today. What can still happen:
  - Defender's cloud protection or behaviour monitoring may react when Frida attaches to AA Classic's launcher.
  - If it does, the recorder shows the `attach_failed` problem, which names the folder to allow.
  - That is the case for a "for analysis" submission of `_frida.pyd`.
- **Downloaders.** A small exe that downloads and runs another exe looks like a dropper to machine-learning
  detection.
  - That is why the bootstrapper downloads only what a signed manifest names, from one host.
  - It is also why it has full version info, and why it is submitted for analysis once.
