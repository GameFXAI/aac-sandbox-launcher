//! Keybinds, character options and action bars from a recording of an AA Classic session.
//!
//! AA Classic keeps these on its own servers, not on the PC: the client asks for them right after it connects
//! (`SCResponseUIData`), saves them back when they change (`CSSaveUIData`), and gets the bars at world entry
//! (`SCActionSlots`) with every change sent as `CSUpdateActionSlot`. A recording made by the capture tool holds
//! those packets already decoded, one JSON object per line. This module reads such a file (read only), keeps
//! only those four packets and throws everything else away: nothing else of the recording is kept or sent.
//!
//! Layouts (client build 297, the same as the server's packet classes):
//! - S2C 0x258 SCResponseUIData: u32 character, u16 type, u16 length + text, u32
//! - C2S 0x054 CSSaveUIData:     u16 type, u32 character, u16 length + text
//! - S2C 0x0D8 SCActionSlots:    133 slots, each u8 kind then u32 (kinds 1, 2, 5, 6) or u64 (kind 4) or nothing
//! - C2S 0x17E CSUpdateActionSlot: u8 slot, then one slot as above

use base64::Engine;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

pub const SLOTS: usize = 133;
/// UI data kinds that are copied: 1 character options, 2 keybindings, 5 map / quest tracker state.
pub const KINDS: [u16; 3] = [1, 2, 5];
const MAX_TEXT: usize = 16 * 1024;
/// A line longer than this is not one of the four packets.
const MAX_LINE: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Empty,
    /// an item by template id (potions, food): the same id on every server
    ItemType(u32),
    Skill(u32),
    Macro,
    /// one particular item of AA Classic's inventory: means nothing on another server, so it is not copied
    Item(u64),
    RidePetSkill(u32),
    BattlePetSkill(u32),
    Other(u8),
}

impl Slot {
    fn read(b: &[u8], at: &mut usize) -> Option<Slot> {
        let kind = *b.get(*at)?;
        *at += 1;
        let u32_at = |at: &mut usize| -> Option<u32> {
            let v = u32::from_le_bytes(b.get(*at..*at + 4)?.try_into().ok()?);
            *at += 4;
            Some(v)
        };
        Some(match kind {
            0 => Slot::Empty,
            1 => Slot::ItemType(u32_at(at)?),
            2 => Slot::Skill(u32_at(at)?),
            3 => Slot::Macro,
            4 => {
                let v = u64::from_le_bytes(b.get(*at..*at + 8)?.try_into().ok()?);
                *at += 8;
                Slot::Item(v)
            }
            5 => Slot::RidePetSkill(u32_at(at)?),
            6 => Slot::BattlePetSkill(u32_at(at)?),
            k => Slot::Other(k),
        })
    }

    /// What goes to our server: the same slot, or empty for what cannot carry over.
    fn portable(self) -> Slot {
        match self {
            Slot::Item(_) | Slot::Macro | Slot::Other(_) => Slot::Empty,
            s => s,
        }
    }

    fn write(self, out: &mut Vec<u8>) {
        match self.portable() {
            Slot::ItemType(v) => {
                out.push(1);
                out.extend_from_slice(&v.to_le_bytes())
            }
            Slot::Skill(v) => {
                out.push(2);
                out.extend_from_slice(&v.to_le_bytes())
            }
            Slot::RidePetSkill(v) => {
                out.push(5);
                out.extend_from_slice(&v.to_le_bytes())
            }
            Slot::BattlePetSkill(v) => {
                out.push(6);
                out.extend_from_slice(&v.to_le_bytes())
            }
            _ => out.push(0),
        }
    }
}

/// The 133 slots of an `SCActionSlots` body; None unless the body is exactly that.
pub fn parse_slots(b: &[u8]) -> Option<Vec<Slot>> {
    let mut at = 0;
    let mut out = Vec::with_capacity(SLOTS);
    for _ in 0..SLOTS {
        out.push(Slot::read(b, &mut at)?);
    }
    (at == b.len()).then_some(out)
}

/// The slots as our server takes them: the same 133-slot layout, with what cannot carry over left empty.
pub fn encode_slots(slots: &[Slot]) -> Vec<u8> {
    let mut out = Vec::new();
    for s in slots.iter().take(SLOTS) {
        s.write(&mut out);
    }
    for _ in slots.len()..SLOTS {
        out.push(0);
    }
    out
}

/// What a recording holds for one character.
#[derive(Clone, Debug, Default)]
pub struct Found {
    pub file: PathBuf,
    /// AA Classic's id of the character (only shown, never sent)
    pub character: u32,
    /// UI data kind -> text, the newest seen
    pub ui: BTreeMap<u16, String>,
    pub slots: Option<Vec<Slot>>,
    /// time of the newest packet taken (as the recording writes it)
    pub when: String,
}

fn text_ok(t: &[u8]) -> bool {
    t.len() <= MAX_TEXT && t.iter().all(|&c| c == b'\r' || c == b'\n' || c == b'\t' || (0x20..0x7f).contains(&c))
}

fn string_at(b: &[u8], at: usize) -> Option<(&[u8], usize)> {
    let n = u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as usize;
    Some((b.get(at + 2..at + 2 + n)?, at + 2 + n))
}

/// (character, kind, text) of a UI data packet body.
fn ui_data(opcode: u32, s2c: bool, b: &[u8]) -> Option<(u32, u16, String)> {
    let (ch, kind, text) = if s2c && opcode == 0x258 {
        let ch = u32::from_le_bytes(b.get(0..4)?.try_into().ok()?);
        let kind = u16::from_le_bytes(b.get(4..6)?.try_into().ok()?);
        let (t, end) = string_at(b, 6)?;
        // the trailing u32 is the text length plus one
        if b.len() != end + 4 {
            return None;
        }
        (ch, kind, t)
    } else if !s2c && opcode == 0x054 {
        let kind = u16::from_le_bytes(b.get(0..2)?.try_into().ok()?);
        let ch = u32::from_le_bytes(b.get(2..6)?.try_into().ok()?);
        let (t, end) = string_at(b, 6)?;
        if b.len() != end {
            return None;
        }
        (ch, kind, t)
    } else {
        return None;
    };
    if !KINDS.contains(&kind) || !text_ok(text) {
        return None;
    }
    let s = String::from_utf8(text.to_vec()).ok()?;
    // a real blob always ends with its "version N" line; an empty answer is not worth copying
    let versioned = s.lines().any(|l| l.trim().strip_prefix("version ").is_some_and(|n| !n.is_empty() && n.len() <= 6 && n.bytes().all(|b| b.is_ascii_digit())));
    versioned.then_some((ch, kind, s))
}

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("\"{name}\":");
    let at = line.find(&key)? + key.len();
    let rest = line[at..].trim_start();
    if let Some(r) = rest.strip_prefix('"') {
        r.find('"').map(|e| &r[..e])
    } else {
        let e = rest.find([',', '}']).unwrap_or(rest.len());
        Some(rest[..e].trim())
    }
}

/// Reads a recording and returns what it holds for the character it saw last. The file is only read; nothing but
/// the four packets above is looked at past a quick text check.
pub fn read(file: &Path) -> Result<Found, String> {
    let f = std::fs::File::open(file).map_err(|e| format!("cannot open {}: {e}", file.display()))?;
    let mut r = BufReader::with_capacity(1 << 20, f);
    let mut per: BTreeMap<u32, Found> = BTreeMap::new();
    let mut last_char: Option<u32> = None;
    // bars arrive without a character id: they belong to the character whose UI data came last
    let mut slots: Option<(Vec<Slot>, String)> = None;
    let mut buf = Vec::new();
    let b64 = base64::engine::general_purpose::STANDARD;
    loop {
        buf.clear();
        let n = r.read_until(b'\n', &mut buf).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        if n == 0 {
            break;
        }
        if buf.len() > MAX_LINE {
            continue;
        }
        let Ok(line) = std::str::from_utf8(&buf) else { continue };
        // cheap filter before any parsing
        let wanted = ["\"opcode\":600,", "\"opcode\":84,", "\"opcode\":216,", "\"opcode\":382,"];
        if !wanted.iter().any(|w| line.contains(w)) || field(line, "channel") != Some("game") {
            continue;
        }
        let s2c = match field(line, "direction") {
            Some("S2C") => true,
            Some("C2S") => false,
            _ => continue,
        };
        let Some(opcode) = field(line, "opcode").and_then(|o| o.parse::<u32>().ok()) else { continue };
        let Some(body) = field(line, "body_b64").and_then(|b| b64.decode(b).ok()) else { continue };
        let when = field(line, "time_utc").unwrap_or("").to_string();
        if let Some((ch, kind, text)) = ui_data(opcode, s2c, &body) {
            let e = per.entry(ch).or_insert_with(|| Found { file: file.to_path_buf(), character: ch, ..Default::default() });
            e.ui.insert(kind, text);
            e.when = when.clone();
            if last_char != Some(ch) {
                last_char = Some(ch);
            }
            if let Some((s, w)) = slots.take() {
                e.slots = Some(s);
                if w > e.when {
                    e.when = w;
                }
            }
        } else if s2c && opcode == 0x0D8 {
            if let Some(s) = parse_slots(&body) {
                match last_char.and_then(|c| per.get_mut(&c)) {
                    Some(e) => {
                        e.slots = Some(s);
                        e.when = when;
                    }
                    None => slots = Some((s, when)),
                }
            }
        } else if !s2c && opcode == 0x17E {
            let mut at = 1;
            if let (Some(&i), Some(s)) = (body.first(), Slot::read(&body, &mut at)) {
                if at == body.len() && (i as usize) < SLOTS {
                    if let Some(e) = last_char.and_then(|c| per.get_mut(&c)) {
                        if let Some(bar) = e.slots.as_mut() {
                            bar[i as usize] = s;
                            e.when = when;
                        }
                    }
                }
            }
        }
    }
    let ch = last_char.ok_or("this recording holds no keybinds or options (it must start before the character is chosen)")?;
    Ok(per.remove(&ch).unwrap())
}

/// Recordings on this PC, newest first: the launcher's own recordings folder and the capture tool's.
pub fn find(paths: &crate::core::Paths) -> Vec<PathBuf> {
    let mut dirs = vec![paths.root.join("recordings")];
    if let Ok(p) = std::env::var("USERPROFILE") {
        dirs.push(PathBuf::from(p).join("Documents").join("AzothCapture"));
    }
    let mut out: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            if name.ends_with(".jsonl") && !name.ends_with(".diary.jsonl") && size > 0 {
                out.push((e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), p));
            }
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.into_iter().map(|(_, p)| p).collect()
}

/// A few readable lines about a keybinding blob: how many bindings and some examples.
pub fn describe_keys(text: &str) -> (usize, Vec<String>) {
    let mut n = 0;
    let mut examples = Vec::new();
    let mut builtin = false;
    for line in text.lines() {
        let t = line.trim();
        let depth = line.len() - line.trim_start().len();
        if depth <= 8 {
            builtin = t == "builtin" || t == "builtin_multi";
            continue;
        }
        if builtin {
            if let Some((k, a)) = t.split_once(' ') {
                n += 1;
                if examples.len() < 4 && !["escape", "tab"].contains(&k) {
                    examples.push(format!("{k} = {}", a.replace('_', " ")));
                }
            }
        }
    }
    (n, examples)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn ui_s2c(ch: u32, kind: u16, text: &str) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&ch.to_le_bytes());
        b.extend_from_slice(&kind.to_le_bytes());
        b.extend_from_slice(&(text.len() as u16).to_le_bytes());
        b.extend_from_slice(text.as_bytes());
        b.extend_from_slice(&(text.len() as u32 + 1).to_le_bytes());
        b
    }

    fn ui_c2s(ch: u32, kind: u16, text: &str) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&kind.to_le_bytes());
        b.extend_from_slice(&ch.to_le_bytes());
        b.extend_from_slice(&(text.len() as u16).to_le_bytes());
        b.extend_from_slice(text.as_bytes());
        b
    }

    pub fn row(dir: &str, opcode: u32, body: &[u8], t: &str) -> String {
        let b = base64::engine::general_purpose::STANDARD.encode(body);
        format!("{{\"time_utc\":\"{t}\",\"channel\":\"game\",\"direction\":\"{dir}\",\"level\":5,\"opcode\":{opcode},\"crc_valid\":true,\"body_b64\":\"{b}\",\"wire_b64\":\"AAAA\"}}\n")
    }

    pub const KEYS: &str = "primary\r\n    normal_mode\r\n        builtin\r\n            escape open_config\r\n            w moveforward\r\n            1 action_bar_button_1\r\n        builtin_multi\r\n            2 action_bar_button\r\nversion 2\r\n";

    pub fn bar() -> Vec<u8> {
        let mut s = vec![Slot::Empty; SLOTS];
        s[0] = Slot::Skill(10223);
        s[1] = Slot::ItemType(8000);
        s[2] = Slot::Item(0x1122334455667788);
        s[3] = Slot::BattlePetSkill(5);
        let mut b = Vec::new();
        for x in s {
            match x {
                Slot::Item(v) => {
                    b.push(4);
                    b.extend_from_slice(&v.to_le_bytes())
                }
                o => o.write(&mut b),
            }
        }
        b
    }

    #[test]
    fn takes_the_four_packets_of_the_last_character_and_nothing_else() {
        let d = std::env::temp_dir().join(format!("aaemu-rec-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("aac-live-x.jsonl");
        let mut s = String::new();
        s += &row("S2C", 600, &ui_s2c(1, 1, "ShowFps 1\r\nversion 1\r\n"), "t1"); // an older character
        s += &row("S2C", 600, &ui_s2c(77, 1, "ShowFps 0\r\nversion 1\r\n"), "t2");
        s += &row("S2C", 600, &ui_s2c(77, 2, KEYS), "t3");
        s += &row("S2C", 600, &ui_s2c(77, 3, "version 1\r\n"), "t3"); // kind 3 is not copied
        s += &row("S2C", 216, &bar(), "t4");
        s += &row("S2C", 600, &ui_s2c(77, 5, "zero\r\n"), "t4"); // no version line: not a real blob
        s += "{\"channel\":\"login\",\"direction\":\"C2S\",\"opcode\":84,\"body_b64\":\"c2VjcmV0\"}\n";
        s += &row("C2S", 84, &ui_c2s(77, 1, "ShowFps 1\r\nversion 2\r\n"), "t5"); // saved later: newer
        let mut upd = vec![5u8, 2];
        upd.extend_from_slice(&999u32.to_le_bytes());
        s += &row("C2S", 382, &upd, "t6");
        s += &row("S2C", 382, &[1, 2, 3], "t7"); // same number the other way: another packet
        std::fs::write(&f, &s).unwrap();
        let before = std::fs::read(&f).unwrap();
        let r = read(&f).unwrap();
        assert_eq!(r.character, 77);
        assert_eq!(r.ui.keys().copied().collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(r.ui[&1], "ShowFps 1\r\nversion 2\r\n");
        let bar = r.slots.unwrap();
        assert_eq!((bar[0], bar[2], bar[5]), (Slot::Skill(10223), Slot::Item(0x1122334455667788), Slot::Skill(999)));
        assert_eq!(r.when, "t6");
        assert_eq!(std::fs::read(&f).unwrap(), before);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn slots_round_trip_without_the_items_of_aa_classic() {
        let s = parse_slots(&bar()).unwrap();
        let out = encode_slots(&s);
        let back = parse_slots(&out).unwrap();
        assert_eq!(back[0], Slot::Skill(10223));
        assert_eq!(back[1], Slot::ItemType(8000));
        assert_eq!(back[2], Slot::Empty, "an inventory item of AA Classic is not copied");
        assert_eq!(back[3], Slot::BattlePetSkill(5));
        assert!(parse_slots(&out[..out.len() - 1]).is_none());
        let mut long = out.clone();
        long.push(0);
        assert!(parse_slots(&long).is_none());
    }

    #[test]
    fn a_recording_without_the_packets_says_so() {
        let d = std::env::temp_dir().join(format!("aaemu-rec-empty-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("a.jsonl");
        std::fs::write(&f, row("S2C", 1, &[1, 2], "t")).unwrap();
        assert!(read(&f).is_err_and(|e| e.contains("no keybinds")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn keys_are_counted_and_shown() {
        let (n, ex) = describe_keys(KEYS);
        assert_eq!(n, 4);
        assert_eq!(ex[0], "w = moveforward");
    }
}

#[cfg(test)]
mod real {
    /// cargo test real_recording -- --ignored --nocapture, with AAEMU_TEST_RECORDING=<file>
    #[test]
    #[ignore]
    fn real_recording() {
        let f = std::env::var("AAEMU_TEST_RECORDING").unwrap();
        let t = std::time::Instant::now();
        let r = super::read(std::path::Path::new(&f)).unwrap();
        let bar = r.slots.as_ref().map(|s| s.iter().filter(|x| **x != super::Slot::Empty).count());
        println!("kinds {:?} sizes {:?} bar {:?} when {} in {:?}", r.ui.keys().collect::<Vec<_>>(), r.ui.values().map(|v| v.len()).collect::<Vec<_>>(), bar, r.when, t.elapsed());
        if let Some(k) = r.ui.get(&2) {
            let (n, ex) = super::describe_keys(k);
            println!("{n} bindings, e.g. {ex:?}");
        }
    }
}
