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
//!   aac-sandbox-launcher vault                        the client builds kept, by build id
//!   aac-sandbox-launcher pack export <file> [--build ID]   one build as a file for a friend
//!   aac-sandbox-launcher pack import <file> [--server <link>]   a friend's build into the vault
//!   aac-sandbox-launcher install [--to VERSION] [--build ID] [--offline]   the game without an AA Classic folder:
//!       game data from AA Classic's public file host, patches merged, client build from the vault
//!   aac-sandbox-launcher pak info|list|check FILE   look at a game archive

use crate::core::*;
use crate::net::{self, Server};
use crate::{install, pak, vault};
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
            let (user, token) = match (opt(args, "--token"), opt(args, "--password"), &server) {
                (Some(t), _, _) => (user, t.to_lowercase()),
                (None, Some(p), Server::Api { .. }) => {
                    let t = net::login(&server, &user, &p)?;
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
        _ => return Err("commands: status | setup [--aac DIR] [--mode standard|full] | play --user U (--password P | --token HEX) [--server IP-or-LINK] [--login-port N] [--wait] | info | signup --user U --password P [--invite CODE] | remove [--profile] | vault | pack export FILE | pack import FILE | install [--to V] [--build ID] [--offline] | pak info|list|check FILE".into()),
    }
    Ok(())
}

/// The usual install folder, if it is there.
pub fn default_aac_dir() -> Option<PathBuf> {
    [r"C:\Games\AAClassic", r"C:\Program Files\AAClassic", r"D:\Games\AAClassic"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("binNext").join("archeage.exe").is_file())
}
