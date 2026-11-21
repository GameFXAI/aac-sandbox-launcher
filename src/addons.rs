//! The server's addon pack: the addons a server's game modes need (their menus and HUD), installed into our
//! client's own addon folder before play. Never the real `Documents\AAClassic`.
//!
//! A pack is one file (made by the server's Tools/gamemodes/make-addon-pack.py): the bytes `AAADDON1\n`, the header
//! length (u32 LE), the header JSON `{ id, files: [{ path, size, sha256 }] }`, then each file's bytes in that order.
//! A path is `<addon name>/<file>`. The id is the hash over the file list, and `/api/info` names the id the server
//! wants, so a pack is checked against what the server said before a byte of it is written.
//!
//! What is installed is remembered in `Addon\.server_pack.json`: the next pack replaces exactly those addons and
//! never touches addons the player put there themself.

use crate::core::Paths;
use crate::net::{self, Server};
use crate::vault::{FileEntry, build_id, safe_rel};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 9] = b"AAADDON1\n";
const MAX_FILES: usize = 4000;
pub const MAX_PACK: u64 = 64 << 20;
const MARKER: &str = ".server_pack.json";

#[derive(Deserialize)]
struct Header {
    id: String,
    files: Vec<FileEntry>,
}

#[derive(Serialize, Deserialize, Default)]
struct Installed {
    id: String,
    addons: Vec<String>,
}

pub fn addon_dir(paths: &Paths) -> PathBuf {
    paths.docs().join("Addon")
}

fn valid_addon_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// A checked pack: its id, addon names and files with their bytes.
struct Pack<'a> {
    id: String,
    addons: Vec<String>,
    files: Vec<(PathBuf, &'a [u8])>,
}

fn parse(bytes: &[u8]) -> Result<Pack<'_>, String> {
    let not = || "this is not an addon pack".to_string();
    if bytes.len() as u64 > MAX_PACK || bytes.len() < MAGIC.len() + 4 || &bytes[..MAGIC.len()] != MAGIC {
        return Err(not());
    }
    let len = u32::from_le_bytes(bytes[9..13].try_into().unwrap()) as usize;
    let body = bytes.get(13 + len..).ok_or_else(not)?;
    let h: Header = serde_json::from_slice(&bytes[13..13 + len]).map_err(|_| not())?;
    if h.files.is_empty() || h.files.len() > MAX_FILES || build_id(&h.files) != h.id {
        return Err("the addon pack's file list does not match its id".into());
    }
    let (mut at, mut files, mut addons, mut seen) = (0usize, Vec::new(), BTreeSet::new(), BTreeSet::new());
    for e in &h.files {
        let rel = safe_rel(&e.path)?;
        let name = e.path.split('/').next().unwrap_or("");
        if !valid_addon_name(name) || rel.components().count() < 2 || !seen.insert(e.path.to_lowercase()) {
            return Err(format!("the addon pack has a file it may not have: {:?}", e.path));
        }
        let end = at.checked_add(e.size as usize).filter(|&x| x <= body.len()).ok_or("the addon pack is cut short")?;
        let data = &body[at..end];
        if hex::encode(Sha256::digest(data)) != e.sha256.to_lowercase() {
            return Err(format!("{} in the addon pack is damaged or was changed", e.path));
        }
        addons.insert(name.to_string());
        files.push((rel, data));
        at = end;
    }
    if at != body.len() {
        return Err("the addon pack has extra data after its last file".into());
    }
    Ok(Pack { id: h.id, addons: addons.into_iter().collect(), files })
}

fn installed(paths: &Paths) -> Installed {
    fs::read_to_string(addon_dir(paths).join(MARKER)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Is the pack with this id in place?
pub fn is_installed(paths: &Paths, id: &str) -> bool {
    let i = installed(paths);
    i.id == id && !i.addons.is_empty() && i.addons.iter().all(|a| addon_dir(paths).join(a).join("main.lua").is_file())
}

/// The client loads the addons listed in `addons.txt`, one name per line. Ours are added, the previous pack's
/// that are gone are taken out, everything else stays as the player has it.
fn update_list(dir: &Path, add: &[String], remove: &[String]) -> Result<(), String> {
    let path = dir.join("addons.txt");
    let old = fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = old.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && !remove.contains(l) && !add.contains(l)).collect();
    lines.extend(add.iter().cloned());
    fs::write(&path, lines.iter().map(|l| format!("{l}\r\n")).collect::<String>()).map_err(|e| format!("cannot write addons.txt: {e}"))
}

/// `addon_settings` says which addons are switched on: `{ name = { enabled = true, }, ... }`. An addon of ours
/// that is not in it yet gets its entry; entries that are there (the player may have switched one off) stay.
fn update_settings(dir: &Path, add: &[String]) -> Result<(), String> {
    let path = dir.join("addon_settings");
    let old = fs::read_to_string(&path).unwrap_or_default();
    let has = |name: &str| old.lines().any(|l| l.trim().strip_prefix(name).is_some_and(|rest| rest.trim_start().starts_with('=')));
    let entries: String = add.iter().filter(|a| !has(a)).map(|a| format!("    {a} = {{\r\n        enabled = true,\r\n    }},\r\n")).collect();
    if entries.is_empty() && !old.is_empty() {
        return Ok(());
    }
    let new = match old.rfind('}') {
        Some(end) if old.trim_start().starts_with('{') => format!("{}{}{}", old[..end].trim_end_matches([' ', '\t']), entries, &old[end..]),
        _ => format!("{{\r\n{entries}}}\r\n"),
    };
    fs::write(&path, new).map_err(|e| format!("cannot write addon_settings: {e}"))
}

/// Installs a pack. With `expect`, the pack must have that id. Returns the addon names.
pub fn install(paths: &Paths, bytes: &[u8], expect: Option<&str>) -> Result<Vec<String>, String> {
    let pack = parse(bytes)?;
    if expect.is_some_and(|id| !id.eq_ignore_ascii_case(&pack.id)) {
        return Err("this is not the addon pack the server named".into());
    }
    let dir = addon_dir(paths);
    if !crate::core::is_inside(&dir, &paths.root) {
        return Err(format!("refusing to write outside {}", paths.root.display()));
    }
    let tmp = dir.join(".pack.tmp");
    let _ = fs::remove_dir_all(&tmp);
    let io = |e: std::io::Error| format!("cannot write the addons: {e}");
    for (rel, data) in &pack.files {
        let dst = tmp.join(rel);
        fs::create_dir_all(dst.parent().unwrap()).map_err(io)?;
        fs::write(&dst, data).map_err(io)?;
    }
    let before = installed(paths);
    let gone: Vec<String> = before.addons.iter().filter(|a| !pack.addons.contains(a) && valid_addon_name(a)).cloned().collect();
    for a in &gone {
        let _ = fs::remove_dir_all(dir.join(a));
    }
    for a in &pack.addons {
        let dst = dir.join(a);
        if dst.exists() {
            fs::remove_dir_all(&dst).map_err(io)?;
        }
        fs::rename(tmp.join(a), &dst).map_err(io)?;
    }
    let _ = fs::remove_dir_all(&tmp);
    update_list(&dir, &pack.addons, &gone)?;
    update_settings(&dir, &pack.addons)?;
    let marker = Installed { id: pack.id.clone(), addons: pack.addons.clone() };
    fs::write(dir.join(MARKER), serde_json::to_string(&marker).unwrap()).map_err(io)?;
    Ok(pack.addons)
}

/// Before play on a server with an account API: brings the addons to what the server names. Returns a line for
/// the log when something was installed.
pub fn ensure(paths: &Paths, server: &Server) -> Result<Option<String>, String> {
    if !matches!(server, Server::Api { .. }) {
        return Ok(None);
    }
    let id = net::info(server)?.addon_pack;
    if id.is_empty() || is_installed(paths, &id) {
        return Ok(None);
    }
    if !crate::vault::valid_id(&id) {
        return Err("the server names an addon pack in a form this launcher does not know".into());
    }
    let bytes = net::pack(server, &id)?;
    let addons = install(paths, &bytes, Some(&id))?;
    Ok(Some(format!("server addons installed: {}", addons.join(", "))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> Paths {
        let d = std::env::temp_dir().join(format!("aaemu-addons-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        Paths::at(d.join("ours"))
    }

    /// The same bytes the server's make-addon-pack.py writes.
    pub fn pack(files: &[(&str, &[u8])]) -> (String, Vec<u8>) {
        let entries: Vec<FileEntry> = files.iter().map(|(p, d)| FileEntry { path: p.to_string(), size: d.len() as u64, sha256: hex::encode(Sha256::digest(d)) }).collect();
        let id = build_id(&entries);
        let header = serde_json::to_vec(&serde_json::json!({ "id": id, "files": entries })).unwrap();
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&(header.len() as u32).to_le_bytes());
        out.extend_from_slice(&header);
        for (_, d) in files {
            out.extend_from_slice(d);
        }
        (id, out)
    }

    fn read(p: &Paths, rel: &str) -> String {
        fs::read_to_string(addon_dir(p).join(rel)).unwrap()
    }

    #[test]
    fn installs_lists_and_switches_on() {
        let p = tmp("install");
        let (id, bytes) = pack(&[("pts_menu/main.lua", b"-- menu"), ("pts_menu/img/a.png", b"png")]);
        assert!(!is_installed(&p, &id));
        assert_eq!(install(&p, &bytes, Some(&id)).unwrap(), vec!["pts_menu"]);
        assert!(is_installed(&p, &id));
        assert_eq!(read(&p, "pts_menu/main.lua"), "-- menu");
        assert_eq!(read(&p, "pts_menu/img/a.png"), "png");
        assert_eq!(read(&p, "addons.txt"), "pts_menu\r\n");
        assert_eq!(read(&p, "addon_settings"), "{\r\n    pts_menu = {\r\n        enabled = true,\r\n    },\r\n}\r\n");
        assert!(!addon_dir(&p).join(".pack.tmp").exists());
    }

    #[test]
    fn the_next_pack_replaces_ours_and_leaves_the_players_own() {
        let p = tmp("next");
        let dir = addon_dir(&p);
        fs::create_dir_all(dir.join("my_addon")).unwrap();
        fs::write(dir.join("my_addon/main.lua"), "mine").unwrap();
        fs::write(dir.join("addons.txt"), "my_addon\r\n").unwrap();
        fs::write(dir.join("addon_settings"), "{\r\n    my_addon = {\r\n        enabled = false,\r\n    },\r\n}\r\n").unwrap();
        let (_, one) = pack(&[("old_hud/main.lua", b"1"), ("old_hud/gone.lua", b"x"), ("pts_menu/main.lua", b"1")]);
        install(&p, &one, None).unwrap();
        assert_eq!(read(&p, "addons.txt"), "my_addon\r\nold_hud\r\npts_menu\r\n");
        let (id, two) = pack(&[("pts_menu/main.lua", b"2"), ("royale_hud/main.lua", b"r")]);
        install(&p, &two, Some(&id)).unwrap();
        assert!(!dir.join("old_hud").exists(), "an addon of the old pack that the new one lacks is removed");
        assert_eq!(read(&p, "pts_menu/main.lua"), "2");
        assert_eq!(read(&p, "my_addon/main.lua"), "mine");
        assert_eq!(read(&p, "addons.txt"), "my_addon\r\npts_menu\r\nroyale_hud\r\n");
        let settings = read(&p, "addon_settings");
        assert!(settings.contains("my_addon = {\r\n        enabled = false"), "{settings}");
        assert!(settings.contains("royale_hud = {\r\n        enabled = true") && settings.contains("pts_menu = {"), "{settings}");
        assert_eq!(settings.matches("pts_menu =").count(), 1);
        assert!(settings.trim_end().ends_with('}'));
    }

    #[test]
    fn refuses_packs_that_are_wrong_or_reach_outside() {
        let p = tmp("bad");
        let (id, good) = pack(&[("pts_menu/main.lua", b"ok")]);
        assert!(install(&p, &good, Some(&"0".repeat(64))).is_err_and(|e| e.contains("not the addon pack the server named")));
        // a changed byte in a file
        let mut changed = good.clone();
        *changed.last_mut().unwrap() ^= 1;
        assert!(install(&p, &changed, Some(&id)).is_err_and(|e| e.contains("damaged")));
        // cut short, extra data, not a pack
        assert!(install(&p, &good[..good.len() - 1], None).is_err());
        let mut extra = good.clone();
        extra.push(0);
        assert!(install(&p, &extra, None).is_err_and(|e| e.contains("extra data")));
        assert!(install(&p, b"hello", None).is_err());
        // names that would land outside an addon folder
        for path in ["../x/main.lua", "main.lua", "a/../../x.lua", "C:/x/main.lua", "bad name/main.lua", "a\\b.lua"] {
            let (_, bytes) = pack(&[(path, b"x")]);
            assert!(install(&p, &bytes, None).is_err(), "{path}");
        }
        assert!(!addon_dir(&p).join("pts_menu").exists());
        assert!(!p.root.parent().unwrap().join("x").exists());
    }
}
