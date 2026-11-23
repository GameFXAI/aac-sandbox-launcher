//! Command-line mode, for testing and scripting:
//!
//!   aac-sandbox-launcher status
//!   aac-sandbox-launcher setup [--aac <AA Classic folder>] [--mode standard|full]
//!   aac-sandbox-launcher play --user <name> (--password <pw> | --token <64 hex>) [--server <ip or link>] [--login-port N] [--wait]
//!   aac-sandbox-launcher info [--server <link>]
//!   aac-sandbox-launcher signup --user <name> --password <pw> [--invite <code>] [--server <link>]
//!
//! --server is a bare address (a development server: the client gets the password hash) or a link to a server's
//! account API (https://..., or http:// on this PC): then --password is checked there and the client gets a ticket.
//!   aac-sandbox-launcher remove [--profile]
//!   aac-sandbox-launcher update [--check]              a newer launcher from the signed manifest
//!   aac-sandbox-launcher manifest keygen|make|sign     for the owner: sign a release (see update.rs)
//!   aac-sandbox-launcher settings copy [--from FILE]   your display and sound settings from AA Classic (read only)
//!   aac-sandbox-launcher transfer [--recording FILE] [--gear LINK] [--only a,b] [--dry-run] [--aac-docs DIR]
//!                                   [--user U --password P]   your AA Classic setup (see transfer.rs); shows the
//!                                   preview, then moves the ticked parts; --dry-run only shows it
//!   aac-sandbox-launcher transfer undo [--user U --password P]   puts back what the last transfer replaced
//!   aac-sandbox-launcher addons [--server <link>]     install the addons the server names (play does this too)
//!   aac-sandbox-launcher recorder setup|on|off|admin|remove|status|send [--user U (--password P | --token T)]   record AA Classic
//!                                   and send your fights to your replays (see recorder.rs; send uses the remembered
//!                                   sign-in when no --user is given)
//!   aac-sandbox-launcher art                           put the signed AA REPLAY art in our copy (play does this too)
//!   aac-sandbox-launcher vault                        the client builds kept, by build id
//!   aac-sandbox-launcher pack export <file> [--build ID]   one build as a file for a friend
//!   aac-sandbox-launcher pack import <file> [--server <link>]   a friend's build into the vault
//!   aac-sandbox-launcher install [--to VERSION] [--build ID] [--offline]   the game without an AA Classic folder:
//!       game data from AA Classic's public file host, patches merged, client build from the vault
//!   aac-sandbox-launcher pak info|list|check FILE   look at a game archive

use crate::core::*;
use crate::net::{self, Server};
use crate::{addons, install, pak, settings, transfer, update, vault};
use std::path::PathBuf;
use std::time::SystemTime;

fn opt(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

pub fn run(args: Vec<String>) -> i32 {
    match go(&args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn go(args: &[String]) -> Result<(), String> {
    let paths = Paths::new();
    let mut cfg = Config::load(&paths);
    if let Some(d) = opt(args, "--aac") {
        cfg.aac_dir = Some(PathBuf::from(d));
    }
    if let Some(m) = opt(args, "--mode") {
        cfg.mode = match m.as_str() {
            "standard" => Mode::Standard,
            "full" => Mode::Full,
            "own" => Mode::Own,
            _ => return Err("--mode is standard, full or own".into()),
        };
    }
    if let Some(s) = opt(args, "--server") {
        cfg.login_ip = s.clone();
        cfg.game_ip = s;
    }
    if let Some(n) = opt(args, "--login-port") {
        cfg.login_port = n.parse().map_err(|_| "--login-port is a number")?;
    }
    let server = || Server::parse(&cfg.login_ip);
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let aac = || -> Result<AacInstall, String> {
        let d = cfg.aac_dir.clone().or_else(default_aac_dir).ok_or("no AA Classic folder: pass --aac <folder>")?;
        AacInstall::open(&d, &paths)
    };
    // the launcher's own download needs no AA Classic folder
    let aac_if_needed = |mode: Mode| -> Result<Option<AacInstall>, String> { if mode == Mode::Own { Ok(None) } else { aac().map(Some) } };
    match cmd {
        "status" => {
            let a = aac_if_needed(cfg.mode)?;
            match &a {
                Some(a) => println!("AA Classic: {}", a.root().display()),
                None => println!("AA Classic: not used (own download)"),
            }
            println!("launcher folder: {}", paths.root.display());
            println!("mode: {:?}", cfg.mode);
            println!("copy: {}", status(&paths, a.as_ref(), cfg.mode).describe());
            if let Some(s) = install::load_state(&paths) {
                println!("own download: game data version {}{}", s.pak_version(), if s.finished { "" } else { " (not finished)" });
            }
            let running = blocking_processes();
            println!("blocking processes: {}", if running.is_empty() { "none".into() } else { running.join(", ") });
        }
        "setup" => {
            let a = aac()?;
            cfg.aac_dir = Some(a.root().to_path_buf());
            let mut paths = paths.clone();
            if let Some(line) = place_client(&mut paths, &a, cfg.mode)? {
                println!("{line}");
            }
            setup(&paths, &a, cfg.mode, &|l| println!("{l}"))?;
            cfg.save(&paths).map_err(|e| e.to_string())?;
            log_line(&paths, &format!("setup {:?} done", cfg.mode));
            println!("ready");
        }
        "play" => {
            let a = aac_if_needed(cfg.mode)?;
            let user = opt(args, "--user").ok_or("--user is required")?;
            let server = server()?;
            let address = server.client_address()?;
            // the AA REPLAY loading screens, splash and icon (never stops a play)
            if let Some(line) = crate::art::ensure(&paths) {
                println!("{line}");
            }
            let (user, token) = match (opt(args, "--token"), opt(args, "--password"), &server) {
                (Some(t), _, _) => (user, t.to_lowercase()),
                (None, Some(p), Server::Api { .. }) => {
                    // the server's addons first: a ticket is only good for two minutes
                    if let Some(line) = addons::ensure(&paths, &server)? {
                        println!("{line}");
                    }
                    let t = net::login(&server, &user, net::Cred::Password(&p), false)?;
                    println!("logged in as {}", t.username);
                    (t.username, t.ticket)
                }
                (None, Some(p), Server::Direct(_)) => (user, token_from_password(&p)),
                _ => return Err("--password or --token is required".into()),
            };
            let l = Launch { username: user.clone(), token, login_ip: address.clone(), game_ip: address, lang: cfg.lang.clone(), login_port: cfg.login_port };
            let started = SystemTime::now();
            let mut child = play(&paths, a.as_ref(), cfg.mode, &l)?;
            cfg.username = user;
            let _ = cfg.save(&paths);
            log_line(&paths, &format!("client started, pid {}", child.id()));
            println!("client started, pid {}", child.id());
            if flag(args, "--wait") {
                let st = child.wait().map_err(|e| e.to_string())?;
                println!("client exited: {st}");
                for f in crash_files(&paths, started) {
                    println!("crash file kept in the sandbox: {}", f.display());
                }
            }
        }
        "recorder" => {
            use crate::recorder;
            let say = |l: String| println!("{l}");
            match args.get(1).map(String::as_str) {
                Some("setup") | Some("on") => {
                    recorder::turn_on(&paths, &say)?;
                    log_line(&paths, "recorder on");
                    println!("recording is on: it starts with Windows; start AA Classic from its own launcher");
                    if let Some(a) = crate::admin::after_setup(&paths, false, &say) {
                        let l = a.unwrap_or_else(|e| e);
                        log_line(&paths, &format!("recorder admin: {l}"));
                        println!("{l}");
                    }
                }
                Some("admin") => {
                    // the one admin prompt again (after a No at setup)
                    match crate::admin::after_setup(&paths, true, &say) {
                        Some(Ok(l)) => println!("{l}"),
                        Some(Err(e)) => return Err(e),
                        None => println!("nothing to set up: {:?}", crate::admin::state(&paths)),
                    }
                }
                Some("remove") => {
                    // uninstall: off, and the admin task removes itself (no prompt)
                    recorder::turn_off(&paths)?;
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    crate::admin::remove(&paths)?;
                    log_line(&paths, "recorder removed");
                    println!("recording is off and its admin task is removed");
                }
                Some("off") => {
                    recorder::turn_off(&paths)?;
                    log_line(&paths, "recorder off");
                    println!("recording is off");
                }
                Some("status") | None => {
                    let r = recorder::readiness(&paths);
                    println!("{}", recorder::summary(&r).join("
"));
                }
                Some("send") => {
                    let server = server()?;
                    let saved = crate::signin::load(&paths).filter(|s| s.server == cfg.login_ip.trim());
                    let (user, token, password) = (opt(args, "--user"), opt(args, "--token"), opt(args, "--password"));
                    let n = match (&user, &token, &password, &saved) {
                        (Some(u), Some(t), _, _) => recorder::send(&paths, &server, u, net::Cred::Session(t))?,
                        (Some(u), None, Some(p), _) => recorder::send(&paths, &server, u, net::Cred::Password(p))?,
                        (None, _, _, Some(s)) => recorder::send(&paths, &server, &s.username, net::Cred::Session(&s.session))?,
                        _ => return Err("sign in with \"Remember me\" in the launcher, or pass --user and --password".into()),
                    };
                    println!("{n} fight{} sent to your replays", if n == 1 { "" } else { "s" });
                }
                Some(x) => return Err(format!("recorder {x}: the commands are setup, on, off, admin, remove, status and send")),
            }
        }
        "info" => {
            let i = net::info(&server()?)?;
            println!("server: {}", i.name);
            println!("client builds: {}", if i.builds.is_empty() { "(not stated)".into() } else { i.builds.join(", ") });
            println!("sign-up: {}", if i.signup == "invite" { "by invite code" } else { "open" });
            println!("client login: {}", if i.tickets_only { "launch ticket only" } else { "ticket or password hash" });
        }
        "signup" => {
            let user = opt(args, "--user").ok_or("--user is required")?;
            let password = opt(args, "--password").ok_or("--password is required")?;
            let name = net::signup(&server()?, &user, &password, &opt(args, "--invite").unwrap_or_default())?;
            cfg.username = name.clone();
            let _ = cfg.save(&paths);
            println!("account {name} created");
        }
        "update" => {
            println!("this launcher is version {}", update::VERSION);
            if cfg.update_manifest().is_empty() {
                println!("self-update is off: no manifest address is set");
                return Ok(());
            }
            match update::check(cfg.update_manifest())? {
                None => println!("up to date"),
                Some(r) if flag(args, "--check") => println!("version {} is available", r.version),
                Some(r) => {
                    let exe = update::apply(&r)?;
                    log_line(&paths, &format!("launcher updated to {}", r.version));
                    println!("updated to version {}: {}", r.version, exe.display());
                }
            }
        }
        // for the owner: the key pair and the signed manifest of a release
        "manifest" => {
            let arg = |i: usize| args.get(i).cloned().ok_or("manifest keygen KEYFILE | manifest make EXE VERSION URL OUT | manifest sign MANIFEST KEYFILE".to_string());
            match args.get(1).map(String::as_str) {
                Some("keygen") => {
                    let file = PathBuf::from(arg(2)?);
                    if file.exists() {
                        return Err(format!("{} is there already: a new key would lock out every launcher built with the old one", file.display()));
                    }
                    let (private, public) = update::keygen()?;
                    std::fs::write(&file, private).map_err(|e| e.to_string())?;
                    println!("private key written to {} (keep it out of every repository)", file.display());
                    println!("public key, for UPDATE_KEY in src/update.rs: {public}");
                }
                Some("make") => {
                    let text = update::manifest_for(&PathBuf::from(arg(2)?), &arg(3)?, &arg(4)?)?;
                    std::fs::write(arg(5)?, &text).map_err(|e| e.to_string())?;
                    println!("{text}");
                }
                Some("sign") => {
                    let manifest = std::fs::read(arg(2)?).map_err(|e| e.to_string())?;
                    let key = std::fs::read_to_string(arg(3)?).map_err(|e| e.to_string())?;
                    let out = format!("{}.sig", arg(2)?);
                    std::fs::write(&out, update::sign(&manifest, &key)?).map_err(|e| e.to_string())?;
                    println!("signature written to {out}");
                }
                _ => return Err(arg(99).unwrap_err()),
            }
        }
        "settings" => {
            if args.get(1).map(String::as_str) != Some("copy") {
                return Err("settings copy [--from FILE]: takes your display and sound settings from AA Classic's system.cfg".into());
            }
            let from = opt(args, "--from").map(PathBuf::from).or_else(settings::real_file).ok_or("no AA Classic settings file was found: pass --from <system.cfg>")?;
            let n = settings::copy_from(&paths, &from)?;
            println!("{n} display and sound settings copied from {} (that file was only read)", from.display());
        }
        "transfer" => {
            let user = opt(args, "--user").unwrap_or_else(|| cfg.username.clone());
            let password = opt(args, "--password").unwrap_or_default();
            if args.get(1).map(String::as_str) == Some("undo") {
                let last = transfer::last(&paths).ok_or("nothing to undo")?;
                if !last.server.is_empty() {
                    if password.is_empty() {
                        return Err("the last transfer also went to your account: pass --password".into());
                    }
                    net::transfer(&Server::parse(&last.server)?, &last.account, net::Cred::Password(&password), &serde_json::json!({ "undo": true }))?;
                    println!("server: your keybinds, options and bars from before go back at the next login");
                }
                for l in transfer::undo_local(&paths)? {
                    println!("put back: {l}");
                }
                return Ok(());
            }
            let docs = opt(args, "--aac-docs").map(PathBuf::from).or_else(transfer::real_docs);
            let rec = opt(args, "--recording").map(PathBuf::from).or_else(|| crate::recording::find(&paths).into_iter().next());
            let mut scan = transfer::scan(&paths, docs.as_deref(), rec.as_deref());
            if let Some(link) = opt(args, "--gear") {
                let code = net::daru_code(&link).ok_or("--gear takes a DaruGear link or code")?;
                match net::daru(&code) {
                    Ok(b) => scan.build = Some(b),
                    Err(e) => scan.build_error = Some(e),
                }
            }
            let only: Option<Vec<String>> = opt(args, "--only").map(|o| o.split(',').map(|x| x.trim().to_string()).collect());
            let server_ok = !user.trim().is_empty() && !password.is_empty() && matches!(server(), Ok(Server::Api { .. }));
            let items = scan.items();
            let mut ticks: Vec<&'static str> = vec![];
            for it in &items {
                let wanted = only.as_ref().is_none_or(|o| o.iter().any(|x| x == it.id));
                let on = it.ready && wanted && (!it.server || server_ok);
                if on {
                    ticks.push(it.id);
                }
                println!("[{}] {:<20} {}{}", if on { "x" } else { " " }, it.title, it.summary, if it.server && !server_ok && it.ready { "  (needs --user/--password and a server link)" } else { "" });
                for l in &it.lines {
                    println!("      {l}");
                }
            }
            if flag(args, "--dry-run") || ticks.is_empty() {
                println!("{}", if ticks.is_empty() { "nothing to move" } else { "dry run: nothing written" });
                return Ok(());
            }
            let applied = transfer::apply_local(&paths, &scan, &ticks)?;
            let mut lines = applied.lines;
            let (mut srv, mut acct) = (String::new(), String::new());
            if let Some(p) = scan.payload(&ticks) {
                let s = server()?;
                let a = net::transfer(&s, user.trim(), net::Cred::Password(&password), &p)?;
                lines.push(format!("on account {}: {} (at the next login)", user.trim(), a.accepted.join(", ")));
                srv = cfg.login_ip.trim().to_string();
                acct = user.trim().to_string();
            }
            transfer::record(&paths, &ticks, applied.backup, &srv, &acct)?;
            for l in lines {
                println!("moved: {l}");
            }
        }
        "art" => match crate::art::ensure(&paths) {
            Some(line) => println!("{line}"),
            None => println!("the AA REPLAY art is in place (or none is published)"),
        },
        "addons" => {
            let server = server()?;
            match addons::ensure(&paths, &server)? {
                Some(line) => println!("{line}"),
                None => println!("nothing to do: {}", if matches!(server, Server::Api { .. }) { "the server's addons are in place, or it has none" } else { "a bare address has no addon pack" }),
            }
        }
        "vault" => {
            let current = vault::scan(&paths.bin()).ok().map(|b| b.id);
            let all = vault::builds(&paths);
            if all.is_empty() {
                println!("no builds kept yet (run setup, or pack import)");
            }
            for b in all {
                let size: u64 = b.files.iter().map(|f| f.size).sum();
                println!("{}  {} files, {:.1} MB{}", b.id, b.files.len(), size as f64 / 1e6, if Some(&b.id) == current.as_ref() { "  (in use)" } else { "" });
            }
        }
        "pack" => {
            let file = args.get(2).map(PathBuf::from).ok_or("pack export <file> | pack import <file>")?;
            match args.get(1).map(String::as_str) {
                Some("export") => {
                    let id = match opt(args, "--build") {
                        Some(id) => id,
                        None => vault::add(&paths, &paths.bin()).map_err(|e| format!("no client copy to export ({e}): run setup first, or pass --build ID"))?.id,
                    };
                    let b = vault::export(&paths, &id, &file)?;
                    println!("build {} written to {}", b.id, file.display());
                }
                Some("import") => {
                    // the server says which builds it takes; without a server link the pack is only checked against itself
                    let accepted = match server() {
                        Ok(s @ Server::Api { .. }) => net::info(&s)?.build_ids,
                        _ => vec![],
                    };
                    let b = vault::import(&paths, &file, &accepted)?;
                    println!("build {} is in the vault ({} files){}", b.id, b.files.len(), if accepted.is_empty() { "; no server was asked whether it accepts this build" } else { "; the server accepts it" });
                }
                _ => return Err("pack export <file> | pack import <file>".into()),
            }
        }
        "install" => {
            let to = match opt(args, "--to") {
                Some(v) => Some(v.parse::<u32>().map_err(|_| "--to is a version number")?),
                None => None,
            };
            // with a server link, the server says which client builds it takes
            let accepted = match server() {
                Ok(s @ Server::Api { .. }) => net::info(&s)?.build_ids,
                _ => vec![],
            };
            let plan = install::Plan { to, build: opt(args, "--build"), accepted, max_chunks: opt(args, "--max-chunks").and_then(|n| n.parse().ok()).unwrap_or(0), offline: flag(args, "--offline") };
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let done = install::run(&paths, &install::Source::default(), &plan, &cancel, &|l| {
                log_line(&paths, &l);
                println!("{l}")
            }, &|_| {})?;
            cfg.mode = Mode::Own;
            cfg.save(&paths).map_err(|e| e.to_string())?;
            println!("installed: game data version {}, client build {}", done.version, done.build);
        }
        // tools for looking at and checking archives; not part of a normal install
        "pak" => {
            let file = |i: usize| args.get(i).map(PathBuf::from).ok_or("pak info|list|check FILE | pak new FILE | pak apply BASE PATCH...".to_string());
            match args.get(1).map(String::as_str) {
                Some("info") => {
                    let p = pak::Pak::open(&file(2)?, false)?;
                    let bytes: u64 = p.entries().iter().map(|e| e.size).sum();
                    println!("{} files, {} bytes of file data, {} free slots", p.entries().len(), bytes, p.free_slots());
                }
                Some("list") => {
                    use std::io::Write;
                    let p = pak::Pak::open(&file(2)?, false)?;
                    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
                    for e in p.entries() {
                        let _ = writeln!(out, "{}	{}	{}", e.name, e.size, hex::encode(e.md5));
                    }
                }
                Some("check") => {
                    let p = pak::Pak::open(&file(2)?, false)?;
                    let (bad, unsummed) = p.check(&|done, total| {
                        if done % 20000 == 0 {
                            println!("{done}/{total}");
                        }
                    })?;
                    println!("{} files read; {} have no checksum in the table; {} do not match it", p.entries().len(), unsummed, bad.len());
                    for name in bad.iter().take(20) {
                        println!("  {name}");
                    }
                    if !bad.is_empty() {
                        return Err("the archive has damaged files".into());
                    }
                }
                Some("cat") => {
                    use std::io::Write;
                    let p = pak::Pak::open(&file(2)?, false)?;
                    let name = args.get(3).ok_or("pak cat FILE NAME")?;
                    let e = p.get(name).ok_or("no such file in the archive")?;
                    std::io::stdout().lock().write_all(&p.read(e, 1 << 30)?).map_err(|e| e.to_string())?;
                }
                Some("put") => {
                    let mut p = pak::Pak::open(&file(2)?, true)?;
                    let name = args.get(3).ok_or("pak put FILE NAME LOCALFILE")?;
                    let body = std::fs::read(file(4)?).map_err(|e| e.to_string())?;
                    p.put(name, &mut &body[..], body.len() as u64, None, &pak::Entry::default())?;
                    p.save()?;
                }
                Some("new") => {
                    pak::Pak::create(&file(2)?)?;
                }
                Some("apply") => {
                    let mut base = pak::Pak::open(&file(2)?, true)?;
                    for patch in &args[3..] {
                        let started = std::time::Instant::now();
                        let m = pak::apply_patch(&mut base, &pak::Pak::open(&PathBuf::from(patch), false)?, &|| false)?;
                        base.save()?;
                        println!("{patch}: replaced={} moved={} added={} deleted={} -> {} files ({:.1}s)", m.replaced, m.moved, m.added, m.deleted, base.entries().len(), started.elapsed().as_secs_f64());
                    }
                }
                _ => return Err("pak info|list|check FILE | pak new FILE | pak apply BASE PATCH...".into()),
            }
        }
        "remove" => {
            remove_client(&paths)?;
            println!("client copy removed");
            if flag(args, "--profile") {
                remove_profile(&paths)?;
                println!("sandbox user folder removed");
            }
        }
        _ => return Err("commands: status | setup [--aac DIR] [--mode standard|full] | play --user U (--password P | --token HEX) [--server IP-or-LINK] [--login-port N] [--wait] | info | signup --user U --password P [--invite CODE] | remove [--profile] | update [--check] | settings copy | transfer [undo] | addons | vault | pack export FILE | pack import FILE | install [--to V] [--build ID] [--offline] | pak info|list|check FILE".into()),
    }
    Ok(())
}

/// The AA Classic install on this PC: from the Windows registry, else the usual folders (find.rs).
pub fn default_aac_dir() -> Option<PathBuf> {
    crate::find::aac().map(|h| h.dir)
}
