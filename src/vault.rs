//! The build vault and build packs.
//!
//! A *build* is one `binNext` folder (the 64-bit client program, about 65 MB). It is named by its build id: the
//! SHA-256 over the sorted list of its files and their SHA-256. Every build the launcher has seen is kept in
//! `vault\<id>\`, so an old build stays available after AA Classic updates (old replays, a server that has not
//! caught up).
//!
//! A *build pack* is one file holding a build, to pass from one player to another: a player who has AA Classic
//! exports it, a player who has not imports it. We never host these files. An import is checked file by file, and
//! against the build ids the chosen server says it accepts, before anything of it can be started.
//!
//! Rules kept here: nothing is written outside the launcher's folder except the pack file the player asked for;
//! a pack can only create files inside the vault (no absolute paths, no `..`); `sfcl` and the AA Classic launcher
//! are never read (only `binNext\*`).

use crate::core::{Paths, is_inside};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// AA Classic's own bookkeeping file inside `binNext`: not part of a build.
pub const STATE_FILE: &str = ".state.json";
const MAGIC: &[u8; 9] = b"AABUILD1\n";
const MAX_FILES: usize = 4000;
const MAX_TOTAL: u64 = 1 << 30; // 1 GiB: a build is about 65 MB
const MAX_HEADER: u32 = 4 << 20;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// relative path with forward slashes, as on disk
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    pub id: String,
    pub files: Vec<FileEntry>,
}

pub fn sha256_file(p: &Path) -> io::Result<String> {
    let mut f = fs::File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// The id of a file list: independent of the order and of the letter case of the paths.
pub fn build_id(files: &[FileEntry]) -> String {
    let mut lines: Vec<String> = files.iter().map(|f| format!("{}\t{}\n", f.path.to_lowercase(), f.sha256.to_lowercase())).collect();
    lines.sort();
    let mut h = Sha256::new();
    for l in &lines {
        h.update(l.as_bytes());
    }
    hex::encode(h.finalize())
}

fn list(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for e in fs::read_dir(dir)? {
        let e = e?;
        let ft = e.file_type()?;
        if ft.is_symlink() {
            continue; // never follow links
        }
        if ft.is_dir() {
            list(base, &e.path(), out)?;
        } else if ft.is_file() {
            out.push(e.path().strip_prefix(base).unwrap().to_path_buf());
        }
    }
    Ok(())
}

/// Hashes every file of a `binNext` folder (not AA Classic's state file).
pub fn scan(dir: &Path) -> Result<Build, String> {
    let mut rels = Vec::new();
    list(dir, dir, &mut rels).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut files = Vec::new();
    for rel in rels {
        let path = rel.to_string_lossy().replace('\\', "/");
        if path.eq_ignore_ascii_case(STATE_FILE) {
            continue;
        }
        let full = dir.join(&rel);
        let size = fs::metadata(&full).map_err(|e| e.to_string())?.len();
        files.push(FileEntry { path, size, sha256: sha256_file(&full).map_err(|e| format!("cannot read {}: {e}", full.display()))? });
    }
    if !files.iter().any(|f| f.path.eq_ignore_ascii_case("archeage.exe")) {
        return Err(format!("{} has no archeage.exe: not a client build", dir.display()));
    }
    files.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    Ok(Build { id: build_id(&files), files })
}

/// The file hashes AA Classic's launcher recorded in `binNext\.state.json` (path -> sha256), if it is there and
/// complete. Used to notice an AA Classic update without hashing 65 MB on every look.
pub fn recorded_hashes(bin: &Path) -> Option<Vec<(String, String)>> {
    #[derive(Deserialize)]
    struct F {
        sha256: String,
    }
    #[derive(Deserialize)]
    struct S {
        #[serde(default)]
        complete: bool,
        files: std::collections::BTreeMap<String, F>,
    }
    let s: S = serde_json::from_str(&fs::read_to_string(bin.join(STATE_FILE)).ok()?).ok()?;
    if !s.complete || s.files.is_empty() {
        return None;
    }
    let mut v: Vec<(String, String)> = s.files.into_iter().map(|(k, f)| (k.replace('\\', "/").to_lowercase(), f.sha256.to_lowercase())).collect();
    v.sort();
    Some(v)
}

// ---------------------------------------------------------------------------------------------------------------
// The vault
// ---------------------------------------------------------------------------------------------------------------

pub fn vault_dir(paths: &Paths) -> PathBuf {
    paths.root.join("vault")
}

fn valid_id(id: &str) -> bool {
    id.len() == 64 && id.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

pub fn build_dir(paths: &Paths, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("a build id is 64 lowercase hex characters".into());
    }
    Ok(vault_dir(paths).join(id))
}

fn files_dir(dir: &Path) -> PathBuf {
    dir.join("files")
}

/// The builds in the vault, by id.
pub fn builds(paths: &Paths) -> Vec<Build> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(vault_dir(paths)) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !valid_id(&name) {
                continue;
            }
            if let Some(b) = fs::read_to_string(e.path().join("build.json")).ok().and_then(|s| serde_json::from_str::<Build>(&s).ok()) {
                if b.id == name {
                    out.push(b);
                }
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub fn find(paths: &Paths, id: &str) -> Option<Build> {
    builds(paths).into_iter().find(|b| b.id == id)
}

fn copy_files(from: &Path, to: &Path, build: &Build) -> Result<(), String> {
    for f in &build.files {
        let rel = safe_rel(&f.path)?;
        let dst = to.join(&rel);
        if let Some(p) = dst.parent() {
            fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        fs::copy(from.join(&rel), &dst).map_err(|e| format!("cannot copy {}: {e}", f.path))?;
    }
    Ok(())
}

/// Checks that the files under `dir` are exactly the build's (sizes and hashes).
pub fn verify(dir: &Path, build: &Build) -> Result<(), String> {
    let found = scan(dir)?;
    if found.id != build.id || found.files != build.files {
        return Err("the files do not match the build's list".into());
    }
    Ok(())
}

/// Puts a `binNext` folder into the vault (a no-op when that build is already there). Returns the build.
pub fn add(paths: &Paths, bin: &Path) -> Result<Build, String> {
    let build = scan(bin)?;
    let dir = build_dir(paths, &build.id)?;
    if find(paths, &build.id).is_some() {
        return Ok(build);
    }
    let tmp = vault_dir(paths).join(format!("{}.tmp", build.id));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(files_dir(&tmp)).map_err(|e| e.to_string())?;
    let r = copy_files(bin, &files_dir(&tmp), &build).and_then(|_| verify(&files_dir(&tmp), &build)).and_then(|_| finish(&tmp, &dir, &build));
    if r.is_err() {
        let _ = fs::remove_dir_all(&tmp);
    }
    r.map(|_| build)
}

fn finish(tmp: &Path, dir: &Path, build: &Build) -> Result<(), String> {
    fs::write(tmp.join("build.json"), serde_json::to_string_pretty(build).unwrap()).map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(dir);
    fs::rename(tmp, dir).map_err(|e| format!("cannot store the build: {e}"))
}

/// Copies a vault build to `dst` (the client's `binNext`), replacing what is there, and checks the result.
pub fn restore(paths: &Paths, id: &str, dst: &Path) -> Result<Build, String> {
    let build = find(paths, id).ok_or("that build is not in the vault")?;
    if !is_inside(dst, &paths.root) {
        return Err(format!("refusing to write outside {}", paths.root.display()));
    }
    let src = files_dir(&build_dir(paths, id)?);
    if dst.exists() {
        fs::remove_dir_all(dst).map_err(|e| format!("cannot remove the old copy: {e}"))?;
    }
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    copy_files(&src, dst, &build)?;
    verify(dst, &build)?;
    Ok(build)
}

// ---------------------------------------------------------------------------------------------------------------
// Build packs
// ---------------------------------------------------------------------------------------------------------------

/// A path from a pack, made safe: relative, no drive, no `..`, no empty or dotted-only parts, no stream names.
fn safe_rel(p: &str) -> Result<PathBuf, String> {
    let bad = || format!("the pack names a file outside the build: {p:?}");
    if p.is_empty() || p.starts_with('/') || p.contains('\\') || p.contains(':') || p.contains('\0') {
        return Err(bad());
    }
    let mut out = PathBuf::new();
    for part in p.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.ends_with('.') || part.ends_with(' ') {
            return Err(bad());
        }
        out.push(part);
    }
    Ok(out)
}

/// Writes a vault build to one file. Layout: magic, header length (u32 LE), header JSON (the build), then the bytes
/// of each file in the header's order.
pub fn export(paths: &Paths, id: &str, out: &Path) -> Result<Build, String> {
    let build = find(paths, id).ok_or("that build is not in the vault")?;
    let src = files_dir(&build_dir(paths, id)?);
    verify(&src, &build)?;
    let header = serde_json::to_vec(&build).unwrap();
    let mut f = io::BufWriter::new(fs::File::create(out).map_err(|e| format!("cannot write {}: {e}", out.display()))?);
    let io = |e: io::Error| format!("cannot write the pack: {e}");
    f.write_all(MAGIC).map_err(io)?;
    f.write_all(&(header.len() as u32).to_le_bytes()).map_err(io)?;
    f.write_all(&header).map_err(io)?;
    for e in &build.files {
        let mut r = fs::File::open(src.join(safe_rel(&e.path)?)).map_err(io)?;
        if io::copy(&mut r, &mut f).map_err(io)? != e.size {
            return Err(format!("{} changed while it was read", e.path));
        }
    }
    f.flush().map_err(io)?;
    Ok(build)
}

#[allow(dead_code)] // for the window's import dialog
/// Reads only the head of a pack: which build it says it is.
pub fn peek(pack: &Path) -> Result<Build, String> {
    let mut f = fs::File::open(pack).map_err(|e| format!("cannot open {}: {e}", pack.display()))?;
    read_header(&mut f)
}

fn read_header(f: &mut impl Read) -> Result<Build, String> {
    let not = || "this file is not a build pack".to_string();
    let mut magic = [0u8; 9];
    f.read_exact(&mut magic).map_err(|_| not())?;
    if &magic != MAGIC {
        return Err(not());
    }
    let mut len = [0u8; 4];
    f.read_exact(&mut len).map_err(|_| not())?;
    let len = u32::from_le_bytes(len);
    if len == 0 || len > MAX_HEADER {
        return Err(not());
    }
    let mut header = vec![0u8; len as usize];
    f.read_exact(&mut header).map_err(|_| not())?;
    let build: Build = serde_json::from_slice(&header).map_err(|_| not())?;
    if !valid_id(&build.id) || build.files.is_empty() || build.files.len() > MAX_FILES {
        return Err(not());
    }
    if build.files.iter().map(|f| f.size).fold(0u64, |a, b| a.saturating_add(b)) > MAX_TOTAL {
        return Err("the pack is larger than a client build can be".into());
    }
    let mut seen = std::collections::HashSet::new();
    for e in &build.files {
        safe_rel(&e.path)?;
        if e.path.eq_ignore_ascii_case(STATE_FILE) || !seen.insert(e.path.to_lowercase()) {
            return Err(not());
        }
    }
    if build_id(&build.files) != build.id {
        return Err("the pack's file list does not match its build id".into());
    }
    Ok(build)
}

/// Takes a build pack into the vault. Every file is checked against the pack's list while it is written, and the
/// finished folder is hashed again. `accepted`, when not empty, is the list of build ids the chosen server says it
/// takes: a pack with another id is refused before a byte of it is written.
pub fn import(paths: &Paths, pack: &Path, accepted: &[String]) -> Result<Build, String> {
    let mut f = io::BufReader::new(fs::File::open(pack).map_err(|e| format!("cannot open {}: {e}", pack.display()))?);
    let build = read_header(&mut f)?;
    if !accepted.is_empty() && !accepted.iter().any(|a| a.eq_ignore_ascii_case(&build.id)) {
        return Err(format!("this server does not accept that client build ({}…)", &build.id[..12]));
    }
    let dir = build_dir(paths, &build.id)?;
    if find(paths, &build.id).is_some() {
        return Ok(build);
    }
    let tmp = vault_dir(paths).join(format!("{}.tmp", build.id));
    let _ = fs::remove_dir_all(&tmp);
    let r = (|| -> Result<(), String> {
        let files = files_dir(&tmp);
        fs::create_dir_all(&files).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 1 << 16];
        for e in &build.files {
            let dst = files.join(safe_rel(&e.path)?);
            if let Some(p) = dst.parent() {
                fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut w = fs::File::create(&dst).map_err(|e| e.to_string())?;
            let (mut left, mut h) = (e.size, Sha256::new());
            while left > 0 {
                let n = buf.len().min(left as usize);
                f.read_exact(&mut buf[..n]).map_err(|_| "the pack is cut short".to_string())?;
                h.update(&buf[..n]);
                w.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                left -= n as u64;
            }
            if hex::encode(h.finalize()) != e.sha256.to_lowercase() {
                return Err(format!("{} in the pack is damaged or was changed", e.path));
            }
        }
        if f.read(&mut buf).map_err(|e| e.to_string())? != 0 {
            return Err("the pack has extra data after its last file".into());
        }
        verify(&files, &build)?;
        finish(&tmp, &dir, &build)
    })();
    if r.is_err() {
        let _ = fs::remove_dir_all(&tmp);
    }
    r.map(|_| build)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aaemu-vault-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn fake_bin(dir: &Path, exe: &[u8]) {
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("archeage.exe"), exe).unwrap();
        fs::write(dir.join("x2game.dll"), b"game").unwrap();
        fs::write(dir.join("sub").join("data.bin"), b"data").unwrap();
        fs::write(dir.join(STATE_FILE), b"{}").unwrap();
    }

    #[test]
    fn build_id_ignores_order_case_and_the_state_file() {
        let d = tmp("id");
        fake_bin(&d.join("a"), b"exe");
        let a = scan(&d.join("a")).unwrap();
        assert_eq!(a.files.len(), 3);
        assert!(a.files.iter().all(|f| f.path != STATE_FILE));
        let mut shuffled = a.files.clone();
        shuffled.reverse();
        shuffled[0].path = shuffled[0].path.to_uppercase();
        assert_eq!(build_id(&shuffled), a.id);
        fake_bin(&d.join("b"), b"another exe");
        assert_ne!(scan(&d.join("b")).unwrap().id, a.id);
        assert!(scan(&d.join("nothing")).is_err());
    }

    #[test]
    fn vault_keeps_every_build_and_restores_it() {
        let d = tmp("vault");
        let paths = Paths::at(d.join("ours"));
        fake_bin(&d.join("v1"), b"exe one");
        fake_bin(&d.join("v2"), b"exe two");
        let b1 = add(&paths, &d.join("v1")).unwrap();
        let b2 = add(&paths, &d.join("v2")).unwrap();
        assert_eq!(add(&paths, &d.join("v1")).unwrap().id, b1.id);
        assert_eq!(builds(&paths).len(), 2);
        restore(&paths, &b1.id, &paths.bin()).unwrap();
        assert_eq!(fs::read(paths.exe()).unwrap(), b"exe one");
        restore(&paths, &b2.id, &paths.bin()).unwrap();
        assert_eq!(fs::read(paths.exe()).unwrap(), b"exe two");
        assert!(!paths.bin().join(STATE_FILE).exists());
        assert!(restore(&paths, &b1.id, &d.join("elsewhere")).is_err());
        assert!(restore(&paths, &"0".repeat(64), &paths.bin()).is_err());
    }

    #[test]
    fn pack_round_trip() {
        let d = tmp("pack");
        let (mine, friend) = (Paths::at(d.join("mine")), Paths::at(d.join("friend")));
        fake_bin(&d.join("bin"), b"exe");
        let b = add(&mine, &d.join("bin")).unwrap();
        let pack = d.join("build.aabuild");
        export(&mine, &b.id, &pack).unwrap();
        assert_eq!(peek(&pack).unwrap(), b);
        // a server that takes another build refuses it; one that takes this build, or names none, accepts
        assert!(import(&friend, &pack, &["f".repeat(64)]).is_err());
        assert!(builds(&friend).is_empty());
        assert_eq!(import(&friend, &pack, &[b.id.to_uppercase()]).unwrap(), b);
        assert_eq!(import(&friend, &pack, &[]).unwrap(), b);
        restore(&friend, &b.id, &friend.bin()).unwrap();
        assert_eq!(fs::read(friend.bin().join("sub").join("data.bin")).unwrap(), b"data");
    }

    #[test]
    fn a_changed_pack_is_refused_and_leaves_nothing() {
        let d = tmp("tamper");
        let (mine, friend) = (Paths::at(d.join("mine")), Paths::at(d.join("friend")));
        fake_bin(&d.join("bin"), b"exe");
        let b = add(&mine, &d.join("bin")).unwrap();
        let pack = d.join("build.aabuild");
        export(&mine, &b.id, &pack).unwrap();
        let good = fs::read(&pack).unwrap();

        let mut flipped = good.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 1; // one bit of a file's contents
        fs::write(&pack, &flipped).unwrap();
        assert!(import(&friend, &pack, &[]).unwrap_err().contains("damaged"));

        fs::write(&pack, &good[..good.len() - 2]).unwrap();
        assert!(import(&friend, &pack, &[]).unwrap_err().contains("cut short"));

        let mut longer = good.clone();
        longer.extend_from_slice(b"extra");
        fs::write(&pack, &longer).unwrap();
        assert!(import(&friend, &pack, &[]).is_err());

        fs::write(&pack, b"not a pack at all").unwrap();
        assert!(import(&friend, &pack, &[]).is_err());

        assert!(builds(&friend).is_empty());
        assert!(fs::read_dir(vault_dir(&friend)).map(|r| r.count()).unwrap_or(0) == 0);
    }

    fn pack_with(files: Vec<FileEntry>, id: Option<String>, body: &[u8]) -> Vec<u8> {
        let b = Build { id: id.unwrap_or_else(|| build_id(&files)), files };
        let header = serde_json::to_vec(&b).unwrap();
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&(header.len() as u32).to_le_bytes());
        out.extend_from_slice(&header);
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn a_pack_cannot_write_outside_the_vault() {
        let d = tmp("escape");
        let friend = Paths::at(d.join("friend"));
        let sha = hex::encode(Sha256::digest(b"x"));
        for bad in ["../evil.exe", "/evil.exe", "C:/evil.exe", "a/../../evil.exe", "sub\\evil.exe", "archeage.exe:stream", "trailing./x", ""] {
            let files = vec![FileEntry { path: "archeage.exe".into(), size: 1, sha256: sha.clone() }, FileEntry { path: bad.into(), size: 1, sha256: sha.clone() }];
            let pack = d.join("bad.aabuild");
            fs::write(&pack, pack_with(files, None, b"xx")).unwrap();
            assert!(import(&friend, &pack, &[]).is_err(), "{bad}");
        }
        assert!(!d.join("evil.exe").exists());
        assert!(builds(&friend).is_empty());
    }

    #[test]
    fn a_pack_with_a_false_id_or_doubled_names_is_refused() {
        let d = tmp("falseid");
        let friend = Paths::at(d.join("friend"));
        let sha = hex::encode(Sha256::digest(b"x"));
        let one = FileEntry { path: "archeage.exe".into(), size: 1, sha256: sha.clone() };
        let pack = d.join("bad.aabuild");
        fs::write(&pack, pack_with(vec![one.clone()], Some("a".repeat(64)), b"x")).unwrap();
        assert!(import(&friend, &pack, &[]).unwrap_err().contains("build id"));
        let twice = vec![one.clone(), FileEntry { path: "ArcheAge.exe".into(), ..one.clone() }];
        fs::write(&pack, pack_with(twice, None, b"xx")).unwrap();
        assert!(import(&friend, &pack, &[]).is_err());
        let huge = vec![FileEntry { size: MAX_TOTAL + 1, ..one }];
        fs::write(&pack, pack_with(huge, None, b"x")).unwrap();
        assert!(import(&friend, &pack, &[]).is_err());
    }

    #[test]
    fn recorded_hashes_need_a_complete_state_file() {
        let d = tmp("state");
        assert!(recorded_hashes(&d).is_none());
        fs::write(d.join(STATE_FILE), r#"{"build":"x","complete":false,"files":{"a.dll":{"size":1,"sha256":"AA"}}}"#).unwrap();
        assert!(recorded_hashes(&d).is_none());
        fs::write(d.join(STATE_FILE), r#"{"build":"x","complete":true,"files":{"B.dll":{"size":1,"sha256":"BB"},"a.dll":{"size":1,"sha256":"AA"}}}"#).unwrap();
        assert_eq!(recorded_hashes(&d).unwrap(), vec![("a.dll".to_string(), "aa".to_string()), ("b.dll".to_string(), "bb".to_string())]);
    }
}
