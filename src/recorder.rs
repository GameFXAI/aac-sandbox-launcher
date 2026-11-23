//! Recording AA Classic: the player's own live fights, made into replays on our server.
//!
//! The recorder is the capture stack from the server's repo (Tools/AACCapture: recorder.py, the relay, the watcher
//! and the "AA Replay.exe" admin helper) with its own Python, as one signed pack. Nothing else is installed: no
//! Python, no admin rights, nothing outside the launcher's folder except one entry in the user's own Run key so it
//! starts at Windows sign-in.
//!
//! Where it comes from. `recorder.json` on the launcher's host (next to the self-update manifest), signed with the
//! same ed25519 key as the launcher's updates (`recorder.json.sig`), names the pack by size and SHA-256. The pack
//! (made by Tools/AACCapture/recorder_pack.py, zstd-compressed) is the bytes `AAREC01\n`, the header length (u32 LE),
//! the header JSON `{ format: 1, version, python, files: [{ path, size, sha256 }] }`, then each file's bytes in
//! order. Every file is checked by its own SHA-256 and its path must stay inside the folder.
//!
//! On this PC:
//! - `<data>\recorder\<id>\`     the unpacked pack: `python\pythonw.exe`, `app\recorder.py`, ...
//! - `<data>\recorder\current.txt`  the id in use
//! - `<data>\recordings\`        what the recorder writes: captures, `fights\*.fight`, `status.json`, logs
//! - `<data>\recordings\sent.json`  the fights already sent to the server
//! - HKCU\...\Run "AA Replay Recorder" = `"<pythonw>" "<recorder.py>" run --data "<recordings>"`
//!
//! The recorder writes `status.json` every second or two: its state, one line for the player, and a problem said
//! with its fix. A file that is not rewritten any more, or names a process that is gone, reads as "off".

use crate::core::Paths;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Where the signed recorder manifest is (static files on the arena box, like the self-update manifest).
pub const RECORDER_MANIFEST: &str = "https://77-42-46-123.sslip.io/launcher/recorder/recorder.json";
const MAGIC: &[u8; 8] = b"AAREC01\n";
const MAX_PACK: u64 = 128 << 20;
const MAX_UNPACKED: u64 = 512 << 20;
const MAX_HEADER: usize = 4 << 20;
const MAX_FILES: usize = 20_000;
/// The name of the Windows sign-in entry.
pub const RUN_VALUE: &str = "AA Replay Recorder";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// A status not rewritten for this long is a recorder that is not running.
const STALE_S: f64 = 30.0;
/// The server keeps the newest ten per account: older unsent ones are not sent.
const SEND_AT_MOST: usize = 10;
/// The server's limit on one fight.
const MAX_FIGHT: u64 = 1 << 20;

fn manifest_url() -> String {
    std::env::var("AAREPLAY_RECORDER_MANIFEST").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| RECORDER_MANIFEST.to_string())
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Release {
    pub id: String,
    pub version: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub unpacked_size: u64,
    #[serde(default)]
    pub python: String,
}

#[derive(Deserialize)]
struct Manifest {
    recorder: Release,
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn sha_hex(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

/// Checks the signature, then reads the manifest.
pub fn read_manifest(manifest: &[u8], signature_hex: &str, key: &str) -> Result<Release, String> {
    crate::update::verify(manifest, signature_hex, key).map_err(|_| "the recorder's manifest is not signed by the launcher's maker: ignored".to_string())?;
    let r = serde_json::from_slice::<Manifest>(manifest).map_err(|e| format!("the recorder's manifest was not understood: {e}"))?.recorder;
    if !is_hex64(&r.id) || !is_hex64(&r.sha256) || r.size == 0 || r.size > MAX_PACK || r.unpacked_size > MAX_UNPACKED || !crate::update::allowed_url(&r.url) {
        return Err("the recorder's manifest names a download this launcher will not take".into());
    }
    Ok(r)
}

fn dir(paths: &Paths) -> PathBuf {
    paths.root.join("recorder")
}

/// The recorder's own folder: captures, fights, status.
pub fn data_dir(paths: &Paths) -> PathBuf {
    paths.root.join("recordings")
}

/// The unpacked recorder in use, when it is complete.
pub fn installed(paths: &Paths) -> Option<PathBuf> {
    let id = fs::read_to_string(dir(paths).join("current.txt")).ok()?;
    let id = id.trim();
    if !is_hex64(id) {
        return None;
    }
    let d = dir(paths).join(id);
    (pythonw(&d).is_file() && script(&d).is_file()).then_some(d)
}

fn pythonw(d: &Path) -> PathBuf {
    d.join("python").join("pythonw.exe")
}

fn script(d: &Path) -> PathBuf {
    d.join("app").join("recorder.py")
}

/// A pack path that stays inside the folder: forward slashes, no root, no drive, no `..`, no empty parts.
fn safe_path(p: &str) -> bool {
    !p.is_empty()
        && p.len() < 400
        && !p.contains('\\')
        && !p.contains(':')
        && !p.starts_with('/')
        && p.split('/').all(|part| !part.is_empty() && part != "." && part != ".." && !part.ends_with('.') && !part.ends_with(' '))
}

#[derive(Deserialize)]
struct PackFile {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Deserialize)]
struct Header {
    format: u32,
    files: Vec<PackFile>,
}

/// Unpacks a pack's compressed bytes into `to` (which must not exist yet). Returns the number of files.
pub fn unpack(packed: &[u8], unpacked_size: u64, to: &Path) -> Result<usize, String> {
    let bad = |what: &str| format!("the recorder pack is damaged ({what})");
    let dec = ruzstd::decoding::StreamingDecoder::new(packed).map_err(|e| format!("cannot unpack the recorder: {e}"))?;
    let limit = if unpacked_size > 0 { unpacked_size } else { MAX_UNPACKED };
    let mut dec = dec.take(limit + 1);
    let mut read = 0u64;
    let mut exact = |n: usize, dec: &mut dyn Read| -> Result<Vec<u8>, String> {
        let mut b = vec![0u8; n];
        dec.read_exact(&mut b).map_err(|_| bad("cut short"))?;
        read += n as u64;
        Ok(b)
    };
    if exact(8, &mut dec)? != MAGIC {
        return Err("this is not a recorder pack".into());
    }
    let n = u32::from_le_bytes(exact(4, &mut dec)?.try_into().unwrap()) as usize;
    if n == 0 || n > MAX_HEADER {
        return Err(bad("header"));
    }
    let head: Header = serde_json::from_slice(&exact(n, &mut dec)?).map_err(|_| bad("header"))?;
    if head.format != 1 || head.files.is_empty() || head.files.len() > MAX_FILES {
        return Err(bad("header"));
    }
    let total: u64 = head.files.iter().map(|f| f.size).sum();
    if total > MAX_UNPACKED {
        return Err(bad("too large"));
    }
    let mut seen = std::collections::HashSet::new();
    for f in &head.files {
        if !safe_path(&f.path) || !is_hex64(&f.sha256) || !seen.insert(f.path.to_lowercase()) {
            return Err(format!("the recorder pack names a file this launcher will not write: {}", f.path));
        }
    }
    fs::create_dir_all(to).map_err(|e| format!("cannot make {}: {e}", to.display()))?;
    for f in &head.files {
        let b = exact(f.size as usize, &mut dec)?;
        if sha_hex(&b) != f.sha256.to_lowercase() {
            return Err(bad(&f.path));
        }
        let dst = to.join(f.path.replace('/', "\\"));
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("cannot make {}: {e}", parent.display()))?;
        }
        fs::write(&dst, &b).map_err(|e| format!("cannot write {}: {e}", dst.display()))?;
    }
    let mut rest = [0u8; 1];
    if dec.read(&mut rest).map_err(|_| bad("end"))? != 0 {
        return Err(bad("bytes after the last file"));
    }
    if unpacked_size > 0 && read != unpacked_size {
        return Err(bad("size"));
    }
    Ok(head.files.len())
}

/// Puts the newest signed recorder in place (once; again only when a newer one is published). Returns its folder.
/// A running recorder is stopped first when it is replaced, and started again by the caller.
pub fn install(paths: &Paths, say: &dyn Fn(String)) -> Result<PathBuf, String> {
    let url = manifest_url();
    let fresh = crate::update::fetch(&url, 1 << 20).and_then(|m| {
        let sig = crate::update::fetch(&format!("{url}.sig"), 1024)?;
        read_manifest(&m, &String::from_utf8_lossy(&sig), crate::update::UPDATE_KEY)
    });
    let r = match fresh {
        Ok(r) => r,
        Err(e) => {
            if let Some(d) = installed(paths) {
                return Ok(d); // offline: the one in place
            }
            return Err(if e.contains("answered 404") { "the recorder is not published on the update host yet".into() } else { format!("cannot get the recorder: {e}") });
        }
    };
    let root = dir(paths);
    let target = root.join(&r.id);
    if installed(paths).as_deref() == Some(target.as_path()) {
        return Ok(target);
    }
    say(format!("Downloading the recorder ({} MB)…", r.size.div_ceil(1 << 20)));
    let packed = crate::update::fetch(&r.url, r.size)?;
    if packed.len() as u64 != r.size || sha_hex(&packed) != r.sha256.to_lowercase() {
        return Err("the recorder download is damaged or is not the one the manifest names".into());
    }
    say("Unpacking the recorder…".into());
    let tmp = root.join(format!("{}.part", r.id));
    let _ = fs::remove_dir_all(&tmp);
    if let Err(e) = unpack(&packed, r.unpacked_size, &tmp) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }
    let was_running = status(paths).running();
    if was_running {
        stop(paths);
        wait_stopped(paths, 15);
    }
    let _ = fs::remove_dir_all(&target);
    fs::rename(&tmp, &target).map_err(|e| format!("cannot put the recorder in place: {e}"))?;
    fs::write(root.join("current.txt"), &r.id).map_err(|e| format!("cannot put the recorder in place: {e}"))?;
    // older recorders are not needed any more (one still in use stays until the next time)
    if let Ok(rd) = fs::read_dir(&root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && p != target {
                let _ = fs::remove_dir_all(p);
            }
        }
    }
    if was_running || run_entry(paths).is_some() {
        let _ = set_run_entry(paths, true);
    }
    Ok(target)
}

// ---------------------------------------------------------------------------------------------------------------
// Starting, stopping, Windows sign-in
// ---------------------------------------------------------------------------------------------------------------

/// The command the Run entry holds: every path quoted (spaces, non-ASCII user names).
pub fn command_line(rec: &Path, data: &Path) -> String {
    format!("\"{}\" \"{}\" run --data \"{}\"", pythonw(rec).display(), script(rec).display(), data.display())
}

/// Starts the recorder now, hidden. It keeps running after the launcher closes.
pub fn start(paths: &Paths) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let rec = installed(paths).ok_or("the recorder is not set up yet")?;
    let data = data_dir(paths);
    fs::create_dir_all(&data).map_err(|e| format!("cannot make {}: {e}", data.display()))?;
    let _ = fs::remove_file(data.join("stop.request"));
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    std::process::Command::new(pythonw(&rec))
        .arg(script(&rec))
        .arg("run")
        .arg("--data")
        .arg(&data)
        .current_dir(rec.join("app"))
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start the recorder: {e}"))?;
    Ok(())
}

/// Asks the recorder to stop (it closes its relay and watcher and says "stopped").
pub fn stop(paths: &Paths) {
    let data = data_dir(paths);
    let _ = fs::create_dir_all(&data);
    let _ = fs::write(data.join("stop.request"), "stop");
}

fn wait_stopped(paths: &Paths, seconds: u64) {
    for _ in 0..seconds * 4 {
        if !status(paths).running() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// The Run entry's command, when there is one.
pub fn run_entry(_paths: &Paths) -> Option<String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::*;
    let (k, v) = (wide(RUN_KEY), wide(RUN_VALUE));
    let mut buf = vec![0u16; 2048];
    let mut size = (buf.len() * 2) as u32;
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size) };
    if r != ERROR_SUCCESS {
        return None;
    }
    let n = (size as usize / 2).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..n]).trim_end_matches('\0').to_string())
}

/// Starts the recorder at every Windows sign-in (this user only), or not.
pub fn set_run_entry(paths: &Paths, on: bool) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::*;
    let (k, v) = (wide(RUN_KEY), wide(RUN_VALUE));
    if !on {
        let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr()) };
        return if r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND { Ok(()) } else { Err(format!("cannot remove the Windows sign-in entry (error {r})")) };
    }
    let rec = installed(paths).ok_or("the recorder is not set up yet")?;
    let cmd = wide(&command_line(&rec, &data_dir(paths)));
    let r = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr(), REG_SZ, cmd.as_ptr().cast(), (cmd.len() * 2) as u32) };
    if r != ERROR_SUCCESS {
        return Err(format!("cannot add the recorder to Windows sign-in (error {r}): it records only while it was started by hand"));
    }
    Ok(())
}

/// Set up: the recorder in place, started now and at every Windows sign-in.
pub fn turn_on(paths: &Paths, say: &dyn Fn(String)) -> Result<(), String> {
    install(paths, say)?;
    set_run_entry(paths, true)?;
    if !status(paths).running() {
        start(paths)?;
    }
    Ok(())
}

/// Off: stopped now and not started at sign-in. What was recorded stays.
pub fn turn_off(paths: &Paths) -> Result<(), String> {
    set_run_entry(paths, false)?;
    stop(paths);
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// What it says
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Problem {
    pub code: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Last {
    pub ended: f64,
    pub fights: u32,
    pub text: String,
}

/// The recorder's status.json, as the launcher shows it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Status {
    pub state: String,
    pub line: String,
    pub problem: Option<Problem>,
    pub last: Option<Last>,
    pub fights: u32,
    pub pid: u32,
    pub updated: f64,
}

impl Status {
    pub fn off() -> Status {
        Status { state: "stopped".into(), line: "Recording is off.".into(), ..Default::default() }
    }

    pub fn running(&self) -> bool {
        !matches!(self.state.as_str(), "stopped" | "")
    }
}

fn now_s() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    if pid == 0 {
        return false;
    }
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return true; // cannot tell: trust the file's age
    }
    let mut e: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut found = false;
    let mut ok = unsafe { Process32FirstW(snap, &mut e) };
    while ok != 0 {
        if e.th32ProcessID == pid {
            found = true;
            break;
        }
        ok = unsafe { Process32NextW(snap, &mut e) };
    }
    unsafe { CloseHandle(snap) };
    found
}

/// Reads a status file: a recorder that is gone, or has not written for half a minute, reads as off.
pub fn read_status(file: &Path, now: f64, alive: &dyn Fn(u32) -> bool) -> Status {
    let Some(mut s) = fs::read(file).ok().and_then(|b| serde_json::from_slice::<Status>(&b).ok()) else { return Status::off() };
    if s.running() && (now - s.updated > STALE_S || !alive(s.pid)) {
        let (last, fights) = (s.last.take(), s.fights);
        s = Status { last, fights, ..Status::off() };
    }
    if !s.running() && s.problem.as_ref().is_some_and(|p| p.code != "worker_stopped") {
        s.problem = None;
    }
    s
}

pub fn status(paths: &Paths) -> Status {
    read_status(&data_dir(paths).join("status.json"), now_s(), &process_alive)
}

/// What the Play screen says about recording: the line, then what is wrong with its fix.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Readiness {
    pub set_up: bool,
    /// starts at Windows sign-in
    pub at_sign_in: bool,
    pub status: Status,
    pub sent: usize,
    pub waiting: usize,
}

pub fn readiness(paths: &Paths) -> Readiness {
    let set_up = installed(paths).is_some();
    let (sent, waiting) = if set_up { sent_and_waiting(paths) } else { (0, 0) };
    Readiness { set_up, at_sign_in: set_up && run_entry(paths).is_some(), status: if set_up { status(paths) } else { Status::off() }, sent, waiting }
}

// ---------------------------------------------------------------------------------------------------------------
// Sending fights
// ---------------------------------------------------------------------------------------------------------------

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Sent {
    /// file name -> the server's account it went to
    files: std::collections::BTreeMap<String, String>,
}

fn sent_file(paths: &Paths) -> PathBuf {
    data_dir(paths).join("sent.json")
}

fn load_sent(paths: &Paths) -> Sent {
    fs::read(sent_file(paths)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// The fights the recorder made, oldest first.
fn fights(paths: &Paths) -> Vec<PathBuf> {
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(data_dir(paths).join("fights"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("fight")))
        .map(|p| (fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), p))
        .collect();
    v.sort();
    v.into_iter().map(|(_, p)| p).collect()
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

fn sent_and_waiting(paths: &Paths) -> (usize, usize) {
    let sent = load_sent(paths);
    let done = fights(paths).iter().filter(|p| sent.files.get(&name_of(p)).is_some_and(|a| a != "too large")).count();
    (done, unsent(paths).len())
}

/// The newest ten fights not sent yet, oldest first so the newest ends up on top (the server keeps ten).
pub fn unsent(paths: &Paths) -> Vec<PathBuf> {
    let sent = load_sent(paths);
    let all = fights(paths);
    let skip = all.len().saturating_sub(SEND_AT_MOST);
    all.into_iter().skip(skip).filter(|p| !sent.files.contains_key(&name_of(p))).collect()
}

/// Sends the fights not sent yet with `send`; each one sent is written down at once. Returns how many went.
/// Stops at the first that fails; the rest are tried again next time.
pub fn send_new(paths: &Paths, account: &str, send: &dyn Fn(&[u8]) -> Result<(), String>) -> Result<usize, String> {
    let mut sent = load_sent(paths);
    let mut n = 0;
    for p in unsent(paths) {
        let b = match fs::metadata(&p) {
            Ok(m) if m.len() > MAX_FIGHT => {
                sent.files.insert(name_of(&p), "too large".into()); // never accepted: not tried again
                continue;
            }
            _ => fs::read(&p).map_err(|e| format!("cannot read {}: {e}", p.display()))?,
        };
        send(&b)?;
        sent.files.insert(name_of(&p), account.to_string());
        fs::write(sent_file(paths), serde_json::to_vec_pretty(&sent).unwrap_or_default()).map_err(|e| format!("cannot write down what was sent: {e}"))?;
        n += 1;
    }
    let _ = fs::write(sent_file(paths), serde_json::to_vec_pretty(&sent).unwrap_or_default());
    Ok(n)
}

/// Sends the new fights to the account's replays on `server`.
pub fn send(paths: &Paths, server: &crate::net::Server, username: &str, cred: crate::net::Cred) -> Result<usize, String> {
    send_new(paths, username, &|b| crate::net::upload_replay(server, username, cred, b))
}

/// The card's words, in order: what it is doing, what is wrong with its fix, the last recording, what was sent.
pub fn summary(r: &Readiness) -> Vec<String> {
    if !r.set_up {
        return vec!["Recording is not set up.".into()];
    }
    let mut v = vec![r.status.line.clone()];
    if let Some(p) = &r.status.problem {
        v.push(p.text.clone());
    } else if !r.status.running() {
        v.push(if r.at_sign_in { "It starts again with Windows, or press Turn on.".into() } else { "Press Turn on to record your AA Classic fights.".into() });
    }
    if let Some(l) = &r.status.last {
        if !l.text.is_empty() {
            v.push(l.text.clone());
        }
    }
    match (r.sent, r.waiting) {
        (0, 0) => {}
        (s, 0) => v.push(format!("{s} fight{} sent to your replays.", if s == 1 { "" } else { "s" })),
        (s, w) => v.push(format!("{s} sent, {w} waiting to be sent (they go when you are signed in).")),
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn tmp(name: &str) -> PathBuf {
        // a space and non-ASCII letters, like a real user folder can have
        let d = std::env::temp_dir().join(format!("aa replay rec Zoë {name} {}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A pack as recorder_pack.py makes it.
    fn pack(files: &[(&str, &[u8])]) -> (Vec<u8>, u64) {
        let head = serde_json::json!({ "format": 1, "version": "1.0.0", "python": "3.13.17",
            "files": files.iter().map(|(p, b)| serde_json::json!({ "path": p, "size": b.len(), "sha256": sha_hex(b) })).collect::<Vec<_>>() });
        let hj = serde_json::to_vec(&head).unwrap();
        let mut raw = MAGIC.to_vec();
        raw.extend_from_slice(&(hj.len() as u32).to_le_bytes());
        raw.extend_from_slice(&hj);
        for (_, b) in files {
            raw.extend_from_slice(b);
        }
        (ruzstd::encoding::compress_to_vec(&raw[..], ruzstd::encoding::CompressionLevel::Fastest), raw.len() as u64)
    }

    #[test]
    fn only_a_manifest_signed_with_the_key_is_taken() {
        let k = SigningKey::from_bytes(&[7u8; 32]);
        let public = hex::encode(k.verifying_key().to_bytes());
        let h = "a".repeat(64);
        let m = format!(r#"{{"recorder":{{"id":"{h}","version":"1.0.0","url":"https://h/r.pack.zst","size":5,"sha256":"{h}","unpacked_size":9,"python":"3.13.17"}}}}"#);
        let sig = hex::encode(k.sign(m.as_bytes()).to_bytes());
        assert_eq!(read_manifest(m.as_bytes(), &sig, &public).unwrap().size, 5);
        let other = SigningKey::from_bytes(&[8u8; 32]);
        assert!(read_manifest(m.as_bytes(), &hex::encode(other.sign(m.as_bytes()).to_bytes()), &public).is_err());
        let http = m.replace("https://h/", "http://evil/");
        assert!(read_manifest(http.as_bytes(), &hex::encode(k.sign(http.as_bytes()).to_bytes()), &public).is_err());
    }

    #[test]
    fn a_pack_unpacks_into_its_folder_and_nowhere_else() {
        let d = tmp("unpack");
        let (packed, size) = pack(&[("app/recorder.py", b"print(1)"), ("python/pythonw.exe", b"MZ"), ("NOTICE.txt", b"n")]);
        assert_eq!(unpack(&packed, size, &d.join("x")).unwrap(), 3);
        assert_eq!(fs::read(d.join("x").join("app").join("recorder.py")).unwrap(), b"print(1)");
        for bad in ["../evil.txt", "app/../../evil.txt", "/abs.txt", "C:/x.txt", "app\\x.txt", "app/./x", "x. "] {
            let (packed, size) = pack(&[(bad, b"x")]);
            assert!(unpack(&packed, size, &d.join("y")).is_err(), "{bad}");
        }
        assert!(!d.join("evil.txt").exists() && !std::env::temp_dir().join("evil.txt").exists());
    }

    #[test]
    fn a_damaged_pack_is_refused() {
        let d = tmp("damaged");
        let (_, size) = pack(&[("app/recorder.py", b"print(1)")]);
        // the header names one hash, the bytes are other bytes
        let head = serde_json::json!({ "format": 1, "files": [{ "path": "app/recorder.py", "size": 8, "sha256": sha_hex(b"print(2)") }] });
        let hj = serde_json::to_vec(&head).unwrap();
        let mut raw = MAGIC.to_vec();
        raw.extend_from_slice(&(hj.len() as u32).to_le_bytes());
        raw.extend_from_slice(&hj);
        raw.extend_from_slice(b"print(1)");
        let packed = ruzstd::encoding::compress_to_vec(&raw[..], ruzstd::encoding::CompressionLevel::Fastest);
        assert!(unpack(&packed, raw.len() as u64, &d.join("a")).unwrap_err().contains("damaged"));
        // extra bytes after the last file
        let (mut_packed, _) = {
            let mut r = raw.clone();
            r.splice(raw.len() - 8.., b"print(2)extra".iter().copied());
            let hj2 = serde_json::to_vec(&serde_json::json!({ "format": 1, "files": [{ "path": "app/recorder.py", "size": 8, "sha256": sha_hex(b"print(2)") }] })).unwrap();
            assert_eq!(hj2.len(), hj.len());
            (ruzstd::encoding::compress_to_vec(&r[..], ruzstd::encoding::CompressionLevel::Fastest), r.len())
        };
        assert!(unpack(&mut_packed, 0, &d.join("b")).is_err());
        assert!(unpack(b"not zstd", size, &d.join("c")).is_err());
    }

    #[test]
    fn the_run_entry_quotes_paths_with_spaces() {
        let c = command_line(Path::new(r"C:\Users\Zoë A\AppData\Local\AAEmuSandbox\recorder\ab"), Path::new(r"C:\Users\Zoë A\AppData\Local\AAEmuSandbox\recordings"));
        assert_eq!(c, r#""C:\Users\Zoë A\AppData\Local\AAEmuSandbox\recorder\ab\python\pythonw.exe" "C:\Users\Zoë A\AppData\Local\AAEmuSandbox\recorder\ab\app\recorder.py" run --data "C:\Users\Zoë A\AppData\Local\AAEmuSandbox\recordings""#);
    }

    #[test]
    fn a_status_from_a_recorder_that_is_gone_reads_as_off() {
        let d = tmp("status");
        let f = d.join("status.json");
        assert_eq!(read_status(&f, 100.0, &|_| true), Status::off());
        // as recorder.py writes it, with fields the launcher does not use
        fs::write(&f, r#"{"version":1,"state":"recording","line":"Recording.","problem":null,"aac":"game","capture":null,"converting":false,"folder":"C:\\Users\\Zoë","pid":42,"updated":100.0,"fights":3,"last":{"ended":90,"fights":2,"text":"Last recording ended at 12:00: 2 fights found."}}"#).unwrap();
        let s = read_status(&f, 101.0, &|p| p == 42);
        assert_eq!((s.state.as_str(), s.fights), ("recording", 3));
        // the process is gone: off, but the last recording is still told
        let s = read_status(&f, 101.0, &|_| false);
        assert_eq!(s.state, "stopped");
        assert_eq!(s.last.unwrap().fights, 2);
        // not rewritten for a minute: off
        assert_eq!(read_status(&f, 200.0, &|_| true).state, "stopped");
        // a problem is passed on with its text
        fs::write(&f, r#"{"state":"problem","line":"Not recording right now.","problem":{"code":"admin_declined","text":"Windows asked for admin rights and the answer was No. Close AA Classic, start it again and answer Yes."},"pid":42,"updated":100.0}"#).unwrap();
        assert_eq!(read_status(&f, 101.0, &|_| true).problem.unwrap().code, "admin_declined");
        fs::write(&f, b"{ not json").unwrap();
        assert_eq!(read_status(&f, 101.0, &|_| true), Status::off());
    }

    #[test]
    fn fights_are_sent_once_newest_ten_at_most_and_a_failure_keeps_the_rest() {
        let root = tmp("send");
        let paths = Paths::at(&root);
        let fd = data_dir(&paths).join("fights");
        fs::create_dir_all(&fd).unwrap();
        for i in 0..12 {
            fs::write(fd.join(format!("c-0-{i:02}.fight")), vec![i as u8; 20]).unwrap();
            let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 + i);
            fs::File::options().write(true).open(fd.join(format!("c-0-{i:02}.fight"))).unwrap().set_modified(t).unwrap();
        }
        fs::write(fd.join("index.json"), b"[]").unwrap();
        let got = std::cell::RefCell::new(Vec::new());
        // the third send fails: two are written down, the rest wait
        let r = send_new(&paths, "tester", &|b| {
            if got.borrow().len() == 2 {
                return Err("the game server is not running right now: try again in a minute".into());
            }
            got.borrow_mut().push(b[0]);
            Ok(())
        });
        assert!(r.is_err());
        assert_eq!(*got.borrow(), vec![2, 3]); // the oldest two are past the newest ten
        assert_eq!(send_new(&paths, "tester", &|b| { got.borrow_mut().push(b[0]); Ok(()) }).unwrap(), 8);
        assert_eq!(*got.borrow(), (2..12).collect::<Vec<u8>>());
        assert_eq!(send_new(&paths, "tester", &|_| panic!("nothing new")).unwrap(), 0);
        assert_eq!(sent_and_waiting(&paths), (10, 0)); // the two oldest are never sent
    }
}
