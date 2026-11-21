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
//!   aac-sandbox-launcher install [--to VERSION] [--max-chunks N]   game data from AA Classic's public file host

use crate::core::*;
use crate::net::{self, Server};
use crate::{install, vault};
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
            _ => return Err("--mode is standard or full".into()),
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
    match cmd {
        "status" => {
            let a = aac()?;
            println!("AA Classic: {}", a.root().display());
            println!("launcher folder: {}", paths.root.display());
            println!("mode: {:?}", cfg.mode);
            println!("copy: {}", status(&paths, &a, cfg.mode).describe());
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
            let a = aac()?;
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
            let mut child = play(&paths, &a, cfg.mode, &l)?;
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
            let src = install::Source::default();
            let (m, sha) = install::fetch_manifest(&src)?;
            let to: u32 = match opt(args, "--to") {
                Some(v) => v.parse().map_err(|_| "--to is a version number")?,
                None => m.version,
            };
            if to < m.version {
                return Err(format!("the file host's base is version {}: an older version cannot be installed", m.version));
            }
            let max_chunks = opt(args, "--max-chunks").and_then(|n| n.parse().ok()).unwrap_or(0);
            println!("base version {}: {} chunks, {:.1} GB to download, {:.1} GB on disk", m.version, m.chunks.len(), m.download_size() as f64 / 1e9, m.uncompressed_size as f64 / 1e9);
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let started = std::time::Instant::now();
            let s = install::download_base(&paths, &src, &m, &sha, install::Options { max_chunks, ..Default::default() }, &cancel, &|p| {
                let secs = started.elapsed().as_secs_f64().max(0.1);
                println!("chunk {}/{}  {:.1} MB/s", p.chunks_done, p.chunks_total, p.bytes as f64 / 1e6 / secs);
            })?;
            log_line(&paths, &format!("install: {}/{} chunks of base {}", s.count_done(), s.num_chunks, s.version));
            if s.count_done() < s.num_chunks {
                println!("{} of {} chunks done; run install again to go on", s.count_done(), s.num_chunks);
                return Ok(());
            }
            println!("base file complete");
            for v in m.version + 1..=to {
                let size = install::download_patch(&paths, &src, v, &cancel)?;
                println!("patch {v}: {:.1} MB", size as f64 / 1e6);
            }
            println!("downloads complete. Not built yet: merging the patches into game_pak, so this copy cannot start the game yet.");
        }
        "remove" => {
            remove_client(&paths)?;
            println!("client copy removed");
            if flag(args, "--profile") {
                remove_profile(&paths)?;
                println!("sandbox user folder removed");
            }
        }
        _ => return Err("commands: status | setup [--aac DIR] [--mode standard|full] | play --user U (--password P | --token HEX) [--server IP-or-LINK] [--login-port N] [--wait] | info | signup --user U --password P [--invite CODE] | remove [--profile] | vault | pack export FILE | pack import FILE | install [--to V]".into()),
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
