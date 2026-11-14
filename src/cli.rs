//! Command-line mode, for testing and scripting:
//!
//!   aac-sandbox-launcher status
//!   aac-sandbox-launcher setup [--aac <AA Classic folder>] [--mode standard|full]
//!   aac-sandbox-launcher play --user <name> (--password <pw> | --token <64 hex>) [--server <ip>] [--wait]
//!   aac-sandbox-launcher remove [--profile]

use crate::core::*;
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
            let token = match (opt(args, "--token"), opt(args, "--password")) {
                (Some(t), _) => t.to_lowercase(),
                (None, Some(p)) => token_from_password(&p),
                _ => return Err("--password or --token is required".into()),
            };
            let l = Launch { username: user.clone(), token, login_ip: cfg.login_ip.clone(), game_ip: cfg.game_ip.clone(), lang: cfg.lang.clone() };
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
        "remove" => {
            remove_client(&paths)?;
            println!("client copy removed");
            if flag(args, "--profile") {
                remove_profile(&paths)?;
                println!("sandbox user folder removed");
            }
        }
        _ => return Err("commands: status | setup [--aac DIR] [--mode standard|full] | play --user U (--password P | --token HEX) [--server IP] [--wait] | remove [--profile]".into()),
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
