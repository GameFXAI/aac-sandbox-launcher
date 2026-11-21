//! "Transfer from AA Classic": one button that brings a player's setup over.
//!
//! What is where (found on 2026-11-21, see Docs/transfer.md):
//! - **Graphics and sound**: `Documents\AAClassic\system.cfg` (the allowlist of settings.rs).
//! - **Interface options** (interface scale, camera, field of view, nametag colours, volumes, max fps, auction
//!   favourites): the client's LevelDB `Documents\AAClassic\USER\udf`, key `0:255:0:system_option`.
//! - **Window layout** (window positions `ui_bound_*`, where the action bars dock, chat window): the same LevelDB, keys
//!   `<account>:<world>:<character>:<name>` of one AA Classic character.
//! - **Keybinds, character options, map and quest tracker state, action bars**: kept by AA Classic's servers, not on
//!   the PC. They come from a recording of an AA Classic session (recording.rs) and are sent to our server, which
//!   keeps them for the account (table `options` and `characters.slots`), so they follow the player.
//! - **Gear and build**: a DaruGear link, sent to our server, which puts the build on at the next world entry.
//!
//! AA Classic's files are only read (the LevelDB through a copy). Everything written goes into the launcher's own
//! profile, after a backup that [`undo`] puts back.

use crate::core::Paths;
use crate::{ldb, net, recording, settings};
use base64::Engine;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Per-character LevelDB values that are layout: copied to each of the sandbox's characters.
fn layout_key(name: &str) -> bool {
    name == "common_local_data_2" || name.starts_with("ui_bound_") || name == "chat_tab_info" || name == "chat_option"
}

const INTERFACE_KEY: &[u8] = b"0:255:0:system_option";

/// One AA Classic character whose window layout can be copied.
#[derive(Clone, Debug)]
pub struct Source {
    /// `<account>:<world>:<character>`
    pub prefix: String,
    pub label: String,
    /// the newest sequence number of its values: the last played has the highest
    pub seq: u64,
    pub names: Vec<String>,
}

/// Everything found, before anything is written.
#[derive(Default, Clone)]
pub struct Scan {
    /// AA Classic's settings file and its text
    pub cfg: Option<(PathBuf, String)>,
    /// AA Classic's interface store (a copy, read)
    pub store: ldb::Store,
    pub store_dir: Option<PathBuf>,
    pub store_error: Option<String>,
    pub sources: Vec<Source>,
    /// index into `sources`
    pub source: usize,
    pub recording: Option<recording::Found>,
    pub recording_error: Option<String>,
    pub build: Option<net::Build>,
    pub build_error: Option<String>,
    /// the sandbox's own characters (prefixes) that a layout is copied to
    pub targets: Vec<String>,
}

/// One line of the checklist.
#[derive(Clone, Debug)]
pub struct Item {
    pub id: &'static str,
    pub title: &'static str,
    /// one line: what will move
    pub summary: String,
    /// the preview: what exactly
    pub lines: Vec<String>,
    /// can be moved now
    pub ready: bool,
    /// goes to the server (needs the account) rather than into this PC's profile
    pub server: bool,
}

pub const IDS: [&str; 7] = ["graphics", "interface", "layout", "keybinds", "options", "bars", "gear"];

/// The player's real `Documents\AAClassic`, if there is one.
pub fn real_docs() -> Option<PathBuf> {
    settings::real_file().and_then(|f| f.parent().map(Path::to_path_buf))
}

fn text(v: &[u8]) -> String {
    String::from_utf8_lossy(v).to_string()
}

fn value_of(t: &str, name: &str) -> Option<String> {
    t.lines().find_map(|l| {
        let mut w = l.split_whitespace();
        (w.next() == Some(name)).then(|| w.collect::<Vec<_>>().join(" ").trim_matches('"').to_string())
    })
}

/// The characters of a store that have a window layout, newest first, named as well as the store allows.
pub fn sources(store: &ldb::Store) -> Vec<Source> {
    let mut by: BTreeMap<String, Source> = BTreeMap::new();
    for (k, (seq, _)) in store {
        let k = text(k);
        let parts: Vec<&str> = k.splitn(4, ':').collect();
        if parts.len() != 4 || parts[2] == "0" || !layout_key(parts[3]) {
            continue;
        }
        let prefix = format!("{}:{}:{}", parts[0], parts[1], parts[2]);
        let e = by.entry(prefix.clone()).or_insert_with(|| Source { prefix, label: String::new(), seq: 0, names: vec![] });
        e.seq = e.seq.max(*seq);
        e.names.push(parts[3].to_string());
    }
    let mut out: Vec<Source> = by.into_values().collect();
    out.sort_by(|a, b| b.seq.cmp(&a.seq));
    for s in out.iter_mut() {
        let p: Vec<&str> = s.prefix.split(':').collect();
        let world = store.get(format!("{}:255:0:common_account_data", p[0]).as_bytes()).and_then(|(_, v)| value_of(&text(v), "last_played_world"));
        let who = store.get(format!("{}:{}:0:common_world_data", p[0], p[1]).as_bytes()).and_then(|(_, v)| value_of(&text(v), "last_played_character"));
        let short = if p[2].len() > 4 { format!("…{}", &p[2][p[2].len() - 4..]) } else { p[2].to_string() };
        s.label = format!("{} · character {short}", world.unwrap_or_else(|| format!("world {}", p[1])));
        if let Some(w) = who {
            s.label.push_str(&format!(" (last played there: {w})"));
        }
    }
    out
}

/// The sandbox's own characters: where a copied layout goes.
pub fn targets(paths: &Paths) -> Vec<String> {
    let Ok(store) = ldb::read_copy(&sandbox_store(paths)) else { return vec![] };
    let mut out: Vec<String> = store
        .keys()
        .filter_map(|k| {
            let k = text(k);
            let p: Vec<&str> = k.splitn(4, ':').collect();
            (p.len() == 4 && p[2] != "0" && p[1] != "255").then(|| format!("{}:{}:{}", p[0], p[1], p[2]))
        })
        .collect();
    out.dedup();
    out
}

pub fn sandbox_store(paths: &Paths) -> PathBuf {
    paths.docs().join("USER").join("udf")
}

/// Looks at everything that could move. Reads only.
pub fn scan(paths: &Paths, docs: Option<&Path>, recording_file: Option<&Path>) -> Scan {
    let mut s = Scan::default();
    if let Some(d) = docs {
        let cfg = d.join("system.cfg");
        if let Ok(b) = fs::read(&cfg) {
            if b.len() < (1 << 20) {
                s.cfg = Some((cfg, text(&b)));
            }
        }
        let udf = d.join("USER").join("udf");
        if udf.is_dir() && !crate::core::is_inside(&udf, &paths.root) {
            match ldb::read_copy(&udf) {
                Ok(st) => {
                    s.sources = sources(&st);
                    s.store = st;
                    s.store_dir = Some(udf);
                }
                Err(e) => s.store_error = Some(e),
            }
        }
    }
    if let Some(f) = recording_file {
        match recording::read(f) {
            Ok(r) => s.recording = Some(r),
            Err(e) => s.recording_error = Some(e),
        }
    }
    // the window layout of the character the recording is of, when the store has it
    if let Some(ch) = s.recording.as_ref().map(|r| r.character.to_string()) {
        if let Some(i) = s.sources.iter().position(|x| x.prefix.split(':').nth(2) == Some(ch.as_str())) {
            s.source = i;
        }
    }
    s.targets = targets(paths);
    s
}

fn nice(name: &str) -> String {
    let n = name.trim_start_matches("ui_bound_").replace(['[', ']'], " ");
    let mut out = String::new();
    for (i, c) in n.trim().chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push(' ');
        }
        out.push(c.to_ascii_lowercase());
    }
    out.replace('_', " ").replace("  ", " ")
}

impl Scan {
    pub fn source(&self) -> Option<&Source> {
        self.sources.get(self.source)
    }

    /// The checklist with its preview.
    pub fn items(&self) -> Vec<Item> {
        let mut out = Vec::new();

        // graphics and sound
        let (summary, lines, ready) = match &self.cfg {
            Some((f, t)) => {
                let (_, n) = settings::merge("", t);
                let size = match (value_of(&t.replace('=', " "), "r_width"), value_of(&t.replace('=', " "), "r_height")) {
                    (Some(w), Some(h)) => format!("{w} × {h}"),
                    _ => "your screen size".into(),
                };
                let full = value_of(&t.replace('=', " "), "r_fullscreen").is_some_and(|v| v == "1");
                (
                    format!("{n} settings: {size}{}, quality and sound", if full { ", full screen" } else { "" }),
                    vec![format!("From {}", f.display()), "Never taken: server address, language, account.".into()],
                    n > 0,
                )
            }
            None => ("No AA Classic settings file was found".into(), vec![], false),
        };
        out.push(Item { id: "graphics", title: "Graphics and sound", summary, lines, ready, server: false });

        // interface options
        let iface = self.store.get(INTERFACE_KEY).map(|(_, v)| text(v));
        let (summary, lines, ready) = match &iface {
            Some(t) if t.lines().count() > 2 => {
                let n = t.lines().filter(|l| !l.trim().is_empty() && !l.starts_with("version")).count();
                let mut bits = vec![];
                if let Some(v) = value_of(t, "ui_scale") {
                    bits.push(format!("interface scale {v}"));
                }
                if let Some(v) = value_of(t, "CameraMaxDist") {
                    bits.push(format!("camera distance {v}"));
                }
                if let Some(v) = value_of(t, "FovValue") {
                    bits.push(format!("field of view {v}"));
                }
                (
                    format!("{n} options: {}", if bits.is_empty() { "camera, nametags, volumes".into() } else { bits.join(", ") }),
                    vec!["Also: nametag colours, sound volumes, frame limit, auction house favourites.".into()],
                    true,
                )
            }
            _ => (self.store_error.clone().unwrap_or_else(|| "No interface options were found".into()), vec![], false),
        };
        out.push(Item { id: "interface", title: "Interface options", summary, lines, ready, server: false });

        // window layout
        let (summary, lines, ready) = match self.source() {
            Some(src) => {
                let mut wins: Vec<String> = src.names.iter().filter(|n| n.starts_with("ui_bound_")).map(|n| nice(n)).collect();
                wins.sort();
                let bars = src.names.iter().any(|n| n == "common_local_data_2");
                let mut lines = vec![format!("From {}", src.label)];
                if !wins.is_empty() {
                    lines.push(format!("Window positions: {}", wins.join(", ")));
                }
                if bars {
                    lines.push("Where the action bars dock and which way they run.".into());
                }
                if self.targets.is_empty() {
                    lines.push("Play once on the server first: the layout is copied to the characters this client has played.".into());
                } else {
                    lines.push(format!("Copied to {} character{} of this client.", self.targets.len(), if self.targets.len() == 1 { "" } else { "s" }));
                }
                (format!("{} window{}{}", wins.len(), if wins.len() == 1 { "" } else { "s" }, if bars { " and action bar docking" } else { "" }), lines, !self.targets.is_empty())
            }
            None => ("No window layout was found".into(), vec![], false),
        };
        out.push(Item { id: "layout", title: "Window layout", summary, lines, ready, server: false });

        // the server-kept parts, from a recording
        let rec = self.recording.as_ref();
        let need = self.recording_error.clone().unwrap_or_else(|| "Needs a recording of an AA Classic session (AA Classic keeps these on its own servers)".into());
        let from = rec.map(|r| format!("From the recording {} ({})", r.file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), r.when.get(..16).unwrap_or(&r.when).replace('T', " "))).unwrap_or_default();

        let (summary, lines, ready) = match rec.and_then(|r| r.ui.get(&2)) {
            Some(k) => {
                let (n, ex) = recording::describe_keys(k);
                (format!("{n} key bindings"), vec![from.clone(), format!("For example: {}", ex.join(", "))], n > 0)
            }
            None => (need.clone(), vec![], false),
        };
        out.push(Item { id: "keybinds", title: "Keybinds", summary, lines, ready, server: true });

        let (summary, lines, ready) = match rec.filter(|r| r.ui.contains_key(&1) || r.ui.contains_key(&5)) {
            Some(r) => {
                let names: Vec<String> = r.ui.get(&1).map(|t| t.lines().filter_map(|l| l.split_whitespace().next()).filter(|n| *n != "version").map(String::from).collect()).unwrap_or_default();
                let mut lines = vec![from.clone()];
                if !names.is_empty() {
                    lines.push(format!("Character options: {}", names.iter().take(8).cloned().collect::<Vec<_>>().join(", ")));
                }
                if r.ui.contains_key(&5) {
                    lines.push("Map, quest tracker and portal list state.".into());
                }
                (format!("{} character options{}", names.len(), if r.ui.contains_key(&5) { ", map and tracker state" } else { "" }), lines, true)
            }
            None => (need.clone(), vec![], false),
        };
        out.push(Item { id: "options", title: "Game options", summary, lines, ready, server: true });

        let (summary, lines, ready) = match rec.and_then(|r| r.slots.as_ref()) {
            Some(bar) => {
                let skills = bar.iter().filter(|s| matches!(s, recording::Slot::Skill(_) | recording::Slot::RidePetSkill(_) | recording::Slot::BattlePetSkill(_))).count();
                let items = bar.iter().filter(|s| matches!(s, recording::Slot::ItemType(_))).count();
                let left = bar.iter().filter(|s| matches!(s, recording::Slot::Item(_) | recording::Slot::Macro | recording::Slot::Other(_))).count();
                let mut lines = vec![from.clone(), "Skills sit on the same keys; a skill your arena class lacks shows greyed out.".into()];
                if left > 0 {
                    lines.push(format!("{left} slot{} left empty: {} AA Classic's own inventory items or macros.", if left == 1 { "" } else { "s" }, if left == 1 { "it holds one of" } else { "they hold" }));
                }
                (format!("{skills} skills and {items} items on your bars"), lines, skills + items > 0)
            }
            None => (need, vec![], false),
        };
        out.push(Item { id: "bars", title: "Action bars", summary, lines, ready, server: true });

        let (summary, lines, ready) = match &self.build {
            Some(b) => {
                let mut lines: Vec<String> = b.pieces.iter().take(6).map(|(slot, item, grade)| if grade.is_empty() { format!("{slot}: {item}") } else { format!("{slot}: {item} ({grade})") }).collect();
                if b.pieces.len() > 6 {
                    lines.push(format!("and {} more", b.pieces.len() - 6));
                }
                lines.push("Worn at your next login, with its skill trees, at the level cap.".into());
                (format!("{}{} · {} pieces", b.title, if b.class.is_empty() { String::new() } else { format!(" · {}", b.class) }, b.pieces.len()), lines, !b.pieces.is_empty())
            }
            None => (self.build_error.clone().unwrap_or_else(|| "Paste a DaruGear link (darugear.com/?s=...)".into()), vec![], false),
        };
        out.push(Item { id: "gear", title: "Gear and build", summary, lines, ready, server: true });
        out
    }

    /// The part for our server, or None when nothing ticked goes there.
    pub fn payload(&self, ticked: &[&str]) -> Option<serde_json::Value> {
        let on = |id: &str| ticked.contains(&id);
        let mut p = serde_json::Map::new();
        if on("gear") {
            if let Some(b) = &self.build {
                p.insert("gear".into(), b.code.clone().into());
            }
        }
        if let Some(r) = &self.recording {
            let mut ui = serde_json::Map::new();
            for (kind, id) in [(1u16, "options"), (2, "keybinds"), (5, "options")] {
                if let (true, Some(t)) = (on(id), r.ui.get(&kind)) {
                    ui.insert(kind.to_string(), t.clone().into());
                }
            }
            if !ui.is_empty() {
                p.insert("uiData".into(), ui.into());
            }
            if let (true, Some(bar)) = (on("bars"), &r.slots) {
                p.insert("slots".into(), base64::engine::general_purpose::STANDARD.encode(recording::encode_slots(bar)).into());
            }
        }
        (!p.is_empty()).then(|| p.into())
    }
}

// ---- applying and undoing ------------------------------------------------------------------------------------------

fn transfer_dir(paths: &Paths) -> PathBuf {
    paths.root.join("transfer")
}

#[derive(serde::Serialize, serde::Deserialize, Default, Debug)]
#[serde(default)]
pub struct Last {
    pub when: u64,
    pub backup: PathBuf,
    /// the ids moved
    pub items: Vec<String>,
    /// the server the account part went to, if any
    pub server: String,
    pub account: String,
}

pub fn last(paths: &Paths) -> Option<Last> {
    let t = fs::read_to_string(transfer_dir(paths).join("last.json")).ok()?;
    serde_json::from_str(&t).ok()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Writes the ticked local parts into the sandbox profile, after a backup of what is there. Returns one line per
/// part done. The sandbox client must be closed (it holds its store open).
pub fn apply_local(paths: &Paths, scan: &Scan, ticked: &[&str]) -> Result<Vec<String>, String> {
    let on = |id: &str| ticked.contains(&id);
    let mut done = Vec::new();
    let backup = transfer_dir(paths).join(format!("backup-{}", now()));
    fs::create_dir_all(&backup).map_err(|e| format!("cannot make the backup folder: {e}"))?;

    let ours_cfg = paths.docs().join("system.cfg");
    if on("graphics") {
        if let Some((from, _)) = &scan.cfg {
            match fs::read(&ours_cfg) {
                Ok(b) => fs::write(backup.join("system.cfg"), b).map_err(|e| format!("backup: {e}"))?,
                Err(_) => fs::write(backup.join("system.cfg.absent"), b"").map_err(|e| format!("backup: {e}"))?,
            }
            let n = settings::copy_from(paths, from)?;
            done.push(format!("graphics and sound: {n} settings"));
        }
    }

    let want_iface = on("interface") && scan.store.contains_key(INTERFACE_KEY);
    let src = scan.source().filter(|_| on("layout") && !scan.targets.is_empty());
    if want_iface || src.is_some() {
        let dir = sandbox_store(paths);
        let mut values = if dir.is_dir() { ldb::values(&ldb::read_copy(&dir)?) } else { BTreeMap::new() };
        if want_iface {
            values.insert(INTERFACE_KEY.to_vec(), scan.store[INTERFACE_KEY].1.clone());
            done.push("interface options".into());
        }
        if let Some(src) = src {
            let mut n = 0;
            for t in &scan.targets {
                for name in &src.names {
                    let from = format!("{}:{name}", src.prefix);
                    if let Some((_, v)) = scan.store.get(from.as_bytes()) {
                        values.insert(format!("{t}:{name}").into_bytes(), v.clone());
                        n += 1;
                    }
                }
            }
            done.push(format!("window layout: {n} values for {} character{}", scan.targets.len(), if scan.targets.len() == 1 { "" } else { "s" }));
        }
        let fresh = dir.with_file_name(format!("udf.new-{}", now()));
        let _ = fs::remove_dir_all(&fresh);
        ldb::write_fresh(&fresh, &values)?;
        if dir.is_dir() {
            // a running client holds the store: the rename fails and nothing is lost
            if let Err(e) = fs::rename(&dir, backup.join("udf")) {
                let _ = fs::remove_dir_all(&fresh);
                return Err(format!("close the game first (its interface store is in use: {e})"));
            }
        } else {
            fs::write(backup.join("udf.absent"), b"").map_err(|e| format!("backup: {e}"))?;
        }
        fs::rename(&fresh, &dir).map_err(|e| format!("cannot put the new interface store in place: {e}"))?;
    }
    Ok(done)
}

/// Records what was moved, for Undo.
pub fn record(paths: &Paths, items: &[&str], backup_of: Option<PathBuf>, server: &str, account: &str) -> Result<(), String> {
    let backup = backup_of.or_else(|| newest_backup(paths)).unwrap_or_default();
    let l = Last { when: now(), backup, items: items.iter().map(|s| s.to_string()).collect(), server: server.into(), account: account.into() };
    fs::create_dir_all(transfer_dir(paths)).map_err(|e| e.to_string())?;
    fs::write(transfer_dir(paths).join("last.json"), serde_json::to_string_pretty(&l).unwrap()).map_err(|e| e.to_string())
}

pub fn newest_backup(paths: &Paths) -> Option<PathBuf> {
    let mut b: Vec<PathBuf> = fs::read_dir(transfer_dir(paths)).ok()?.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("backup-"))).collect();
    b.sort();
    b.pop()
}

/// Puts back what the last transfer replaced in this PC's profile. The server part is undone with
/// `net::transfer(.., {"undo": true})`.
pub fn undo_local(paths: &Paths) -> Result<Vec<String>, String> {
    let l = last(paths).ok_or("nothing to undo")?;
    let b = &l.backup;
    let mut done = vec![];
    if !b.as_os_str().is_empty() && b.is_dir() && crate::core::is_inside(b, &paths.root) {
        let ours_cfg = paths.docs().join("system.cfg");
        if b.join("system.cfg").is_file() {
            fs::copy(b.join("system.cfg"), &ours_cfg).map_err(|e| format!("cannot put the settings back: {e}"))?;
            done.push("graphics and sound".to_string());
        } else if b.join("system.cfg.absent").is_file() {
            let _ = fs::remove_file(&ours_cfg);
            done.push("graphics and sound".to_string());
        }
        let dir = sandbox_store(paths);
        if b.join("udf").is_dir() || b.join("udf.absent").is_file() {
            if dir.is_dir() {
                let gone = dir.with_file_name(format!("udf.undone-{}", now()));
                fs::rename(&dir, &gone).map_err(|e| format!("close the game first (its interface store is in use: {e})"))?;
                let _ = fs::remove_dir_all(&gone);
            }
            if b.join("udf").is_dir() {
                fs::rename(b.join("udf"), &dir).map_err(|e| format!("cannot put the interface store back: {e}"))?;
            }
            done.push("interface and window layout".to_string());
        }
        let _ = fs::remove_dir_all(b);
    }
    let _ = fs::remove_file(transfer_dir(paths).join("last.json"));
    Ok(done)
}

// ---- clipboard ------------------------------------------------------------------------------------------------------

/// The text on the Windows clipboard, if any (for a DaruGear link the player just copied).
pub fn clipboard_text() -> Option<String> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
        use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
        const CF_UNICODETEXT: u32 = 13;
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let h = GetClipboardData(CF_UNICODETEXT);
        let mut out = None;
        if !h.is_null() {
            let p = GlobalLock(h) as *const u16;
            if !p.is_null() {
                let max = (GlobalSize(h) / 2).min(4096);
                let mut n = 0;
                while n < max && *p.add(n) != 0 {
                    n += 1;
                }
                out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
                GlobalUnlock(h);
            }
        }
        CloseClipboard();
        out
    }
    #[cfg(not(windows))]
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::tests::{bar, row, ui_s2c, KEYS};

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aaemu-transfer-{name}-{}-{}", std::process::id(), now()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    /// A fake AA Classic Documents folder: settings file, interface store with one character, and a recording.
    fn fake_aac(d: &Path) -> (PathBuf, PathBuf) {
        let docs = d.join("real").join("AAClassic");
        fs::create_dir_all(docs.join("USER")).unwrap();
        fs::write(docs.join("system.cfg"), "r_width = 2560\r\nr_height = 1080\r\nr_fullscreen = 1\r\nlocale = de\r\ncl_serveraddr = 9.9.9.9\r\n").unwrap();
        let mut v = BTreeMap::new();
        v.insert(INTERFACE_KEY.to_vec(), b"CameraMaxDist 30\r\nui_scale 1.2\r\nversion 4\r\n".to_vec());
        v.insert(b"88:1:4242:ui_bound_chatWindow[0]".to_vec(), b"chat place".to_vec());
        v.insert(b"88:1:4242:common_local_data_2".to_vec(), b"actionBarValues ...".to_vec());
        v.insert(b"88:1:4242:character_inventory_data".to_vec(), b"bags".to_vec());
        v.insert(b"88:255:0:common_account_data".to_vec(), b"last_played_world Kyprosa\r\nversion 1\r\n".to_vec());
        ldb::write_fresh(&docs.join("USER").join("udf"), &v).unwrap();
        let rec = d.join("aac-live-1.jsonl");
        fs::write(&rec, row("S2C", 600, &ui_s2c(4242, 2, KEYS), "2026-11-20T10:00:00") + &row("S2C", 600, &ui_s2c(4242, 1, "ShowFps 1\r\nversion 1\r\n"), "t") + &row("S2C", 216, &bar(), "t")).unwrap();
        (docs, rec)
    }

    fn hash_tree(d: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = vec![];
        for e in fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(hash_tree(&p));
            } else {
                out.push((p.clone(), fs::read(&p).unwrap()));
            }
        }
        out.sort();
        out
    }

    #[test]
    fn everything_moves_into_the_profile_and_undo_puts_it_back() {
        let d = tmp("all");
        let (docs, rec) = fake_aac(&d);
        let real_before = hash_tree(&docs);
        let paths = Paths::at(d.join("sandbox"));
        // the sandbox client has played one character on the server
        let mut mine = BTreeMap::new();
        mine.insert(b"5:1:7:common_local_data_1".to_vec(), b"corpse".to_vec());
        mine.insert(INTERFACE_KEY.to_vec(), b"version 1\r\n".to_vec());
        ldb::write_fresh(&sandbox_store(&paths), &mine).unwrap();
        fs::write(paths.docs().join("system.cfg"), "locale = en_us\r\n").unwrap();

        let s = scan(&paths, Some(&docs), Some(&rec));
        let items = s.items();
        assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), IDS);
        let ready: Vec<&str> = items.iter().filter(|i| i.ready).map(|i| i.id).collect();
        assert_eq!(ready, vec!["graphics", "interface", "layout", "keybinds", "options", "bars"]);
        assert!(items[0].summary.contains("2560 × 1080, full screen"), "{}", items[0].summary);
        assert!(items[1].summary.contains("interface scale 1.2"));
        assert!(items[2].lines[0].contains("Kyprosa"), "{:?}", items[2].lines);
        assert_eq!(items[3].summary, "4 key bindings");

        let done = apply_local(&paths, &s, &IDS).unwrap();
        assert_eq!(done.len(), 3, "{done:?}");
        record(&paths, &IDS, None, "", "").unwrap();
        let cfg = fs::read_to_string(paths.docs().join("system.cfg")).unwrap();
        assert!(cfg.contains("r_width = 2560") && cfg.contains("locale = en_us") && !cfg.contains("9.9.9.9"));
        let after = ldb::values(&ldb::read_dir(&sandbox_store(&paths)).unwrap());
        assert_eq!(after[INTERFACE_KEY], b"CameraMaxDist 30\r\nui_scale 1.2\r\nversion 4\r\n");
        assert_eq!(after[&b"5:1:7:ui_bound_chatWindow[0]".to_vec()], b"chat place");
        assert_eq!(after[&b"5:1:7:common_local_data_2".to_vec()], b"actionBarValues ...");
        assert_eq!(after[&b"5:1:7:common_local_data_1".to_vec()], b"corpse", "the sandbox's own values stay");
        assert!(!after.contains_key(&b"5:1:7:character_inventory_data".to_vec()), "bag layout is not copied");
        assert!(!after.keys().any(|k| k.starts_with(b"88:")), "AA Classic's own keys are not copied");

        // the server part: keybinds, options, bars; no gear without a link
        let p = s.payload(&IDS).unwrap();
        assert!(p.get("gear").is_none());
        assert_eq!(p["uiData"]["2"], KEYS);
        let slots = base64::engine::general_purpose::STANDARD.decode(p["slots"].as_str().unwrap()).unwrap();
        assert_eq!(recording::parse_slots(&slots).unwrap()[2], recording::Slot::Empty);
        assert!(s.payload(&["graphics"]).is_none());

        // AA Classic's folder is exactly as it was
        assert_eq!(hash_tree(&docs), real_before);

        let undone = undo_local(&paths).unwrap();
        assert_eq!(undone.len(), 2, "{undone:?}");
        assert_eq!(fs::read_to_string(paths.docs().join("system.cfg")).unwrap(), "locale = en_us\r\n");
        assert_eq!(ldb::values(&ldb::read_dir(&sandbox_store(&paths)).unwrap()), mine);
        assert!(last(&paths).is_none());
        assert!(undo_local(&paths).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn without_a_recording_the_server_kept_parts_say_why() {
        let d = tmp("norec");
        let (docs, _) = fake_aac(&d);
        let paths = Paths::at(d.join("sandbox"));
        let s = scan(&paths, Some(&docs), None);
        let items = s.items();
        for id in ["keybinds", "options", "bars"] {
            let i = items.iter().find(|i| i.id == id).unwrap();
            assert!(!i.ready && i.summary.contains("recording"), "{i:?}");
        }
        // no sandbox character yet: the layout waits
        let lay = items.iter().find(|i| i.id == "layout").unwrap();
        assert!(!lay.ready && lay.lines.iter().any(|l| l.contains("Play once")));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_running_client_blocks_and_nothing_is_lost() {
        let d = tmp("busy");
        let (docs, _) = fake_aac(&d);
        let paths = Paths::at(d.join("sandbox"));
        let mut mine = BTreeMap::new();
        mine.insert(b"5:1:7:common_local_data_1".to_vec(), b"corpse".to_vec());
        ldb::write_fresh(&sandbox_store(&paths), &mine).unwrap();
        let s = scan(&paths, Some(&docs), None);
        // hold a file of the store open the way LevelDB holds its LOCK
        let lock = sandbox_store(&paths).join("LOCK");
        fs::write(&lock, b"").unwrap();
        let held = std::fs::OpenOptions::new().read(true).write(true).open(&lock).unwrap();
        let r = apply_local(&paths, &s, &["interface"]);
        drop(held);
        if let Err(e) = r {
            assert!(e.contains("close the game"), "{e}");
            assert_eq!(ldb::values(&ldb::read_dir(&sandbox_store(&paths)).unwrap()), mine);
        }
        let _ = fs::remove_dir_all(&d);
    }
}
