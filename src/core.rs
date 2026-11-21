//! Everything the launcher does to the disk and to processes, without any window code.
//!
//! The rules this module keeps (see README.md):
//! - From the AA Classic install it only ever opens `binNext\*` (to copy it) and `game_pak` (to hardlink or copy it).
//!   `sfcl` (the AAC launcher's saved login), the AAC launcher and every other file there are never opened.
//! - It never writes outside its own folder (`%LOCALAPPDATA%\AAEmuSandbox`).
//! - The client it starts uses a sandbox user folder instead of `Documents\AAClassic`, and has the client's own
//!   crash and performance uploads to AA Classic switched off.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::SystemTime;

pub const APP_DIR: &str = "AAEmuSandbox";

/// Process names that must not be running when our client starts (compared case-insensitively).
pub const BLOCKING_PROCESSES: [&str; 2] = ["archeage.exe", "archeage classic launcher.exe"];

/// Environment variables removed from the client's environment.
const ENV_REMOVE: [&str; 4] = ["SteamAppId", "SteamGameId", "AAC_PERF_SESSION", "AAC_PERF_SESSION_SAMPLER"];

// ---------------------------------------------------------------------------------------------------------------
// Paths and config
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    /// `%LOCALAPPDATA%\AAEmuSandbox`, or `AAEMU_SANDBOX_ROOT` when set (tests).
    pub fn new() -> Paths {
        if let Ok(r) = std::env::var("AAEMU_SANDBOX_ROOT") {
            return Paths { root: PathBuf::from(r) };
        }
        let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
        Paths { root: PathBuf::from(base).join(APP_DIR) }
    }
    #[allow(dead_code)] // tests
    pub fn at(root: impl Into<PathBuf>) -> Paths {
        Paths { root: root.into() }
    }
    pub fn client(&self) -> PathBuf {
        self.root.join("client")
    }
    pub fn bin(&self) -> PathBuf {
        self.client().join("binNext")
    }
    pub fn exe(&self) -> PathBuf {
        self.bin().join("archeage.exe")
    }
    pub fn pak(&self) -> PathBuf {
        self.client().join("game_pak")
    }
    /// What the client sees as the user's profile folder.
    pub fn profile(&self) -> PathBuf {
        self.root.join("profile")
    }
    /// The client's user folder: logs, settings, addons, crash dumps.
    pub fn docs(&self) -> PathBuf {
        self.profile().join("Documents").join("AAClassic")
    }
    pub fn config_file(&self) -> PathBuf {
        self.root.join("launcher.json")
    }
    pub fn log_file(&self) -> PathBuf {
        self.root.join("launcher.log")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    /// Copy `binNext` (65 MB), hardlink `game_pak` (same drive only, no extra space).
    Standard,
    /// Copy `binNext` and `game_pak` (about 45 GB): nothing shared with AA Classic.
    Full,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub aac_dir: Option<PathBuf>,
    pub mode: Mode,
    pub login_ip: String,
    pub game_ip: String,
    pub username: String,
    pub lang: String,
    /// The login server's port when it is not the usual 1237 (0 = the usual one).
    pub login_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            aac_dir: None,
            mode: Mode::Standard,
            login_ip: "127.0.0.1".into(),
            game_ip: "127.0.0.1".into(),
            username: String::new(),
            lang: "en_us".into(),
            login_port: 0,
        }
    }
}

impl Config {
    pub fn load(paths: &Paths) -> Config {
        fs::read_to_string(paths.config_file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, paths: &Paths) -> io::Result<()> {
        fs::create_dir_all(&paths.root)?;
        fs::write(paths.config_file(), serde_json::to_string_pretty(self).unwrap())
    }
}

pub fn log_line(paths: &Paths, line: &str) {
    use std::io::Write;
    let _ = fs::create_dir_all(&paths.root);
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(paths.log_file()) {
        let t = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "{t} {line}");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The AA Classic install (read only)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct AacInstall {
    root: PathBuf,
}

impl AacInstall {
    /// Checks that `dir` looks like an AA Classic install. Only `binNext\archeage.exe` and `game_pak` are looked at.
    pub fn open(dir: &Path, paths: &Paths) -> Result<AacInstall, String> {
        let root = dir.to_path_buf();
        if !root.join("binNext").join("archeage.exe").is_file() {
            return Err(format!("{} has no binNext\\archeage.exe: not an AA Classic folder", root.display()));
        }
        if !root.join("game_pak").is_file() {
            return Err(format!("{} has no game_pak file", root.display()));
        }
        if is_inside(&root, &paths.root) || is_inside(&paths.root, &root) {
            return Err("the AA Classic folder and the launcher's folder must not contain each other".into());
        }
        Ok(AacInstall { root })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// The only two paths of the install the launcher ever opens.
    pub fn bin(&self) -> PathBuf {
        self.root.join("binNext")
    }
    pub fn pak(&self) -> PathBuf {
        self.root.join("game_pak")
    }
}

/// Lexical check (no file access): is `p` equal to or inside `dir`?
pub fn is_inside(p: &Path, dir: &Path) -> bool {
    let norm = |x: &Path| x.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
    let (p, d) = (norm(p), norm(dir));
    p == d || p.starts_with(&(d + "\\"))
}

/// Refuses any write that would land outside the launcher's own folder.
fn ensure_ours(paths: &Paths, target: &Path) -> Result<(), String> {
    if is_inside(target, &paths.root) && target != paths.root {
        Ok(())
    } else {
        Err(format!("refusing to write outside {}: {}", paths.root.display(), target.display()))
    }
}

// ---------------------------------------------------------------------------------------------------------------
// State of our copy
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    NotSetUp,
    /// AA Classic's `binNext` changed (an AAC update): our copy needs refreshing.
    BinOutdated,
    /// Our `game_pak` is missing or no longer the same data as AA Classic's.
    PakOutdated,
    Ready,
}

impl State {
    pub fn describe(&self) -> &'static str {
        match self {
            State::NotSetUp => "not set up yet",
            State::BinOutdated => "AA Classic was updated: refresh the copy",
            State::PakOutdated => "the game data changed: refresh the copy",
            State::Ready => "ready",
        }
    }
}

/// Name, size and modified time of every file under `dir`, sorted (copies keep the modified time).
fn fingerprint(dir: &Path) -> io::Result<Vec<(String, u64, u128)>> {
    let mut out = Vec::new();
    walk(dir, dir, &mut |rel, meta| {
        let t = meta.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_nanos());
        out.push((rel.to_lowercase(), meta.len(), t.unwrap_or(0) / 1_000_000));
    })?;
    out.sort();
    Ok(out)
}

fn walk(base: &Path, dir: &Path, f: &mut dyn FnMut(&str, &fs::Metadata)) -> io::Result<()> {
    for e in fs::read_dir(dir)? {
        let e = e?;
        let ft = e.file_type()?;
        let p = e.path();
        if ft.is_symlink() {
            continue; // never follow links
        }
        if ft.is_dir() {
            walk(base, &p, f)?;
        } else if ft.is_file() {
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().to_string();
            f(&rel, &e.metadata()?)
        }
    }
    Ok(())
}

pub fn status(paths: &Paths, aac: &AacInstall, mode: Mode) -> State {
    if !paths.exe().is_file() {
        return State::NotSetUp;
    }
    // AA Classic's launcher keeps a SHA-256 of every binNext file; our copy carries the list it was made from
    match (crate::vault::recorded_hashes(&aac.bin()), crate::vault::recorded_hashes(&paths.bin())) {
        (Some(a), Some(b)) => {
            if a != b {
                return State::BinOutdated;
            }
        }
        // no such list (an old or hand-made install): names, sizes and modified times
        _ => match (fingerprint(&aac.bin()), fingerprint(&paths.bin())) {
            (Ok(a), Ok(b)) if a == b => {}
            _ => return State::BinOutdated,
        },
    }
    let pak_ok = match mode {
        Mode::Standard => same_file(&paths.pak(), &aac.pak()).unwrap_or(false),
        Mode::Full => match (fs::metadata(paths.pak()), fs::metadata(aac.pak())) {
            (Ok(a), Ok(b)) => a.len() == b.len() && a.modified().ok() == b.modified().ok() && !same_file(&paths.pak(), &aac.pak()).unwrap_or(false),
            _ => false,
        },
    };
    if pak_ok { State::Ready } else { State::PakOutdated }
}

// ---------------------------------------------------------------------------------------------------------------
// Set up / refresh / remove
// ---------------------------------------------------------------------------------------------------------------

/// Builds or refreshes our copy. `log` gets progress lines.
pub fn setup(paths: &Paths, aac: &AacInstall, mode: Mode, log: &dyn Fn(String)) -> Result<(), String> {
    let client = paths.client();
    ensure_ours(paths, &client)?;
    fs::create_dir_all(&client).map_err(|e| format!("cannot create {}: {e}", client.display()))?;

    // binNext: fresh copy
    let bin = paths.bin();
    ensure_ours(paths, &bin)?;
    if bin.exists() {
        fs::remove_dir_all(&bin).map_err(|e| format!("cannot remove the old copy: {e}"))?;
    }
    log(format!("copying {} ...", aac.bin().display()));
    let mut n = 0;
    copy_tree(&aac.bin(), &bin, &mut n).map_err(|e| format!("copy of binNext failed: {e}"))?;
    log(format!("copied {n} files"));
    // every build the launcher has copied stays in the vault, so it is still there after AA Classic updates
    let build = crate::vault::add(paths, &bin)?;
    log(format!("client build {}", &build.id[..12]));

    // game_pak
    let pak = paths.pak();
    ensure_ours(paths, &pak)?;
    if fs::symlink_metadata(&pak).is_ok() {
        // removes only our name for the file; with a hardlink the AA Classic file keeps its data
        fs::remove_file(&pak).map_err(|e| format!("cannot remove the old game_pak: {e}"))?;
    }
    match mode {
        Mode::Standard => {
            hardlink(&pak, &aac.pak()).map_err(|e| {
                if e.raw_os_error() == Some(17) {
                    "AA Classic is on another drive than the launcher's folder: a hardlink is not possible. Use Full mode.".to_string()
                } else {
                    format!("hardlink to game_pak failed: {e}")
                }
            })?;
            log("game_pak linked (no extra disk space)".into());
        }
        Mode::Full => {
            let size = fs::metadata(aac.pak()).map_err(|e| e.to_string())?.len();
            log(format!("copying game_pak ({:.1} GB) ...", size as f64 / 1e9));
            copy_with_progress(&aac.pak(), &pak, size, log).map_err(|e| format!("copy of game_pak failed: {e}"))?;
            log("game_pak copied".into());
        }
    }
    match status(paths, aac, mode) {
        State::Ready => Ok(()),
        s => Err(format!("set up finished but the copy is not ready: {}", s.describe())),
    }
}

fn copy_tree(src: &Path, dst: &Path, n: &mut usize) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for e in fs::read_dir(src)? {
        let e = e?;
        let ft = e.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        let to = dst.join(e.file_name());
        if ft.is_dir() {
            copy_tree(&e.path(), &to, n)?;
        } else if ft.is_file() {
            fs::copy(e.path(), &to)?; // keeps the modified time on Windows
            *n += 1;
        }
    }
    Ok(())
}

/// `fs::copy` on a thread, reporting the destination size every few seconds.
fn copy_with_progress(src: &Path, dst: &Path, size: u64, log: &dyn Fn(String)) -> io::Result<()> {
    let (s, d) = (src.to_path_buf(), dst.to_path_buf());
    let h = std::thread::spawn(move || fs::copy(s, d));
    let mut last = 0u64;
    while !h.is_finished() {
        std::thread::sleep(std::time::Duration::from_secs(3));
        let now = fs::metadata(dst).map(|m| m.len()).unwrap_or(0);
        if now != last && size > 0 {
            log(format!("  {:.0}%", now as f64 * 100.0 / size as f64));
            last = now;
        }
    }
    h.join().unwrap().map(|_| ())
}

/// Deletes our client copy (the AA Classic install is never touched; a hardlinked game_pak only loses our name).
pub fn remove_client(paths: &Paths) -> Result<(), String> {
    let c = paths.client();
    ensure_ours(paths, &c)?;
    if c.exists() {
        fs::remove_dir_all(&c).map_err(|e| e.to_string())?; // does not follow links
    }
    Ok(())
}

/// Deletes the sandbox user folder (logs, settings, addons, crash dumps of our client).
pub fn remove_profile(paths: &Paths) -> Result<(), String> {
    let p = paths.profile();
    ensure_ours(paths, &p)?;
    if p.exists() {
        fs::remove_dir_all(&p).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Login and start
// ---------------------------------------------------------------------------------------------------------------

/// The login server's rule for account names: letters and digits of any script, plus `_ . - @`, 1 to 32 chars.
pub fn valid_username(u: &str) -> bool {
    let n = u.chars().count();
    (1..=32).contains(&n) && u.chars().all(|c| c.is_alphanumeric() || "_.-@".contains(c))
}

/// What provider 4 expects as the token: the password as hex SHA-256.
pub fn token_from_password(password: &str) -> String {
    hex::encode(Sha256::digest(password.as_bytes()))
}

pub fn valid_token(t: &str) -> bool {
    t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit())
}

pub struct Launch {
    pub username: String,
    pub token: String,
    pub login_ip: String,
    pub game_ip: String,
    pub lang: String,
    /// 0 = the client's own default (1237)
    pub login_port: u16,
}

/// The command line the client gets (the token is in it: never log this).
pub fn client_args(l: &Launch) -> Vec<String> {
    let mut a: Vec<String> = ["-r", "+auth_ip", &l.login_ip, "-uid", &l.username, "-token", &l.token, "--lang", &l.lang, "+cl_serveraddr", &l.game_ip]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if l.login_port != 0 {
        a.push("+auth_port".into());
        a.push(l.login_port.to_string());
    }
    a
}

/// The environment changes for the client: sandbox profile, no uploads to AA Classic.
pub fn client_env(paths: &Paths) -> (Vec<(String, String)>, Vec<String>) {
    let p = paths.profile().to_string_lossy().to_string();
    let (drive, rest) = if p.len() > 2 && p.as_bytes()[1] == b':' { (p[..2].to_string(), p[2..].to_string()) } else { (String::new(), p.clone()) };
    let set = vec![
        ("USERPROFILE".to_string(), p.clone()),
        ("HOMEDRIVE".to_string(), drive),
        ("HOMEPATH".to_string(), rest),
        ("AAC_NO_CRASH_UPLOAD".to_string(), "1".to_string()),
        ("AAC_NO_PERF_UPLOAD".to_string(), "1".to_string()),
    ];
    (set, ENV_REMOVE.iter().map(|s| s.to_string()).collect())
}

/// Starts our client. Refuses when AA Classic or its launcher is running, or the copy is not ready.
pub fn play(paths: &Paths, aac: &AacInstall, mode: Mode, l: &Launch) -> Result<Child, String> {
    let running = blocking_processes();
    if !running.is_empty() {
        return Err(format!("close these first: {}", running.join(", ")));
    }
    let st = status(paths, aac, mode);
    if st != State::Ready {
        return Err(format!("the copy is {}", st.describe()));
    }
    if !valid_username(&l.username) {
        return Err("account name: 1-32 letters, digits or _ . - @".into());
    }
    if !valid_token(&l.token) {
        return Err("internal: the token is not 64 hex characters".into());
    }
    fs::create_dir_all(paths.profile().join("Documents")).map_err(|e| e.to_string())?;
    no_crash_dialogs();
    let (set, remove) = client_env(paths);
    let mut cmd = Command::new(paths.exe());
    cmd.current_dir(paths.bin()).args(client_args(l));
    for k in &remove {
        cmd.env_remove(k);
    }
    for (k, v) in &set {
        cmd.env(k, v);
    }
    cmd.spawn().map_err(|e| format!("cannot start the client: {e}"))
}

/// Crash files our client left in its sandbox folder since `since`.
pub fn crash_files(paths: &Paths, since: SystemTime) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(paths.docs()) {
        for e in rd.flatten() {
            let p = e.path();
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            let newer = e.metadata().and_then(|m| m.modified()).map(|t| t > since).unwrap_or(false);
            if newer && (name.ends_with(".dmp") || name == "archeage.crash") {
                out.push(p);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// Win32
// ---------------------------------------------------------------------------------------------------------------

fn wide(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn hardlink(new: &Path, existing: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::CreateHardLinkW;
    let (n, e) = (wide(new), wide(existing));
    let ok = unsafe { CreateHardLinkW(n.as_ptr(), e.as_ptr(), std::ptr::null()) };
    if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// Volume serial + file index of a file (opened for reading attributes only).
fn file_id(p: &Path) -> io::Result<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};
    let f = fs::OpenOptions::new().access_mode(0x80 /* FILE_READ_ATTRIBUTES */).open(p)?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetFileInformationByHandle(f.as_raw_handle() as _, &mut info) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((info.dwVolumeSerialNumber, ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64))
}

use std::os::windows::fs::OpenOptionsExt;

/// Are the two names the same file on disk (a hardlink)?
pub fn same_file(a: &Path, b: &Path) -> io::Result<bool> {
    Ok(file_id(a)? == file_id(b)?)
}

/// Running processes from `BLOCKING_PROCESSES`.
pub fn blocking_processes() -> Vec<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
    let mut found = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return found;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            let name = String::from_utf16_lossy(&e.szExeFile[..len]);
            if BLOCKING_PROCESSES.contains(&name.to_lowercase().as_str()) && !found.contains(&name) {
                found.push(name);
            }
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
    }
    found
}

/// No Windows crash dialog and no Windows Error Reporting for our process and the client (inherited).
fn no_crash_dialogs() {
    use windows_sys::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode};
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aacsbx-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A fake AA Classic install with the files the launcher must never open.
    fn fake_aac(dir: &Path) {
        fs::create_dir_all(dir.join("binNext").join("locales")).unwrap();
        fs::write(dir.join("binNext").join("archeage.exe"), b"exe").unwrap();
        fs::write(dir.join("binNext").join("x2game.dll"), b"dll").unwrap();
        fs::write(dir.join("binNext").join(".state.json"), b"{}").unwrap();
        fs::write(dir.join("binNext").join("locales").join("en.txt"), b"en").unwrap();
        fs::write(dir.join("game_pak"), vec![7u8; 4096]).unwrap();
        fs::write(dir.join("sfcl"), b"{\"token\":\"SECRET\"}").unwrap();
        fs::write(dir.join("ArcheAge Classic Launcher.exe"), b"launcher").unwrap();
        fs::write(dir.join("features.json"), b"{}").unwrap();
    }

    #[test]
    fn standard_setup_links_pak_and_never_opens_sfcl() {
        let t = tmp("std");
        let aac_dir = t.join("AAClassic");
        fake_aac(&aac_dir);
        // hold sfcl open with no sharing: any attempt to open it during setup would fail
        let _lock = fs::OpenOptions::new().read(true).share_mode(0).open(aac_dir.join("sfcl")).unwrap();
        let paths = Paths::at(t.join("sandbox"));
        let aac = AacInstall::open(&aac_dir, &paths).unwrap();
        assert_eq!(status(&paths, &aac, Mode::Standard), State::NotSetUp);
        setup(&paths, &aac, Mode::Standard, &|_| {}).unwrap();
        assert_eq!(status(&paths, &aac, Mode::Standard), State::Ready);
        assert!(same_file(&paths.pak(), &aac.pak()).unwrap());
        assert!(paths.bin().join("locales").join("en.txt").is_file());
        for bad in ["sfcl", "ArcheAge Classic Launcher.exe", "features.json"] {
            assert!(!paths.client().join(bad).exists(), "{bad} was copied");
        }
        // removing our copy leaves the AA Classic file and its data
        remove_client(&paths).unwrap();
        assert_eq!(fs::read(aac.pak()).unwrap(), vec![7u8; 4096]);
        assert!(aac.bin().join("archeage.exe").is_file());
    }

    #[test]
    fn full_setup_copies_pak() {
        let t = tmp("full");
        let aac_dir = t.join("AAClassic");
        fake_aac(&aac_dir);
        let paths = Paths::at(t.join("sandbox"));
        let aac = AacInstall::open(&aac_dir, &paths).unwrap();
        setup(&paths, &aac, Mode::Full, &|_| {}).unwrap();
        assert_eq!(status(&paths, &aac, Mode::Full), State::Ready);
        assert!(!same_file(&paths.pak(), &aac.pak()).unwrap());
        // a hardlinked copy is not a Full copy
        assert_eq!(status(&paths, &aac, Mode::Standard), State::PakOutdated);
    }

    #[test]
    fn aac_update_is_noticed_by_its_hash_list() {
        let d = tmp("hashlist");
        let aac_dir = d.join("aac");
        fake_aac(&aac_dir);
        let state = |sha: &str| format!(r#"{{"build":"b","complete":true,"files":{{"archeage.exe":{{"size":1,"sha256":"{sha}"}}}}}}"#);
        fs::write(aac_dir.join("binNext").join(".state.json"), state("aa")).unwrap();
        let paths = Paths::at(d.join("ours"));
        let aac = AacInstall::open(&aac_dir, &paths).unwrap();
        setup(&paths, &aac, Mode::Standard, &|_| {}).unwrap();
        assert_eq!(status(&paths, &aac, Mode::Standard), State::Ready);
        assert_eq!(crate::vault::builds(&paths).len(), 1);
        // the same files with a newer list: AA Classic says a file changed
        fs::write(aac_dir.join("binNext").join(".state.json"), state("bb")).unwrap();
        assert_eq!(status(&paths, &aac, Mode::Standard), State::BinOutdated);
    }

    #[test]
    fn aac_update_is_noticed() {
        let t = tmp("upd");
        let aac_dir = t.join("AAClassic");
        fake_aac(&aac_dir);
        let paths = Paths::at(t.join("sandbox"));
        let aac = AacInstall::open(&aac_dir, &paths).unwrap();
        setup(&paths, &aac, Mode::Standard, &|_| {}).unwrap();
        fs::write(aac_dir.join("binNext").join("x2game.dll"), b"dll v2").unwrap();
        assert_eq!(status(&paths, &aac, Mode::Standard), State::BinOutdated);
        setup(&paths, &aac, Mode::Standard, &|_| {}).unwrap();
        assert_eq!(status(&paths, &aac, Mode::Standard), State::Ready);
    }

    #[test]
    fn rejects_non_aac_and_nested_folders() {
        let t = tmp("rej");
        let paths = Paths::at(t.join("sandbox"));
        assert!(AacInstall::open(&t, &paths).is_err());
        let inside = t.join("sandbox").join("AAClassic");
        fake_aac(&inside);
        assert!(AacInstall::open(&inside, &paths).is_err());
    }

    #[test]
    fn never_writes_outside_own_folder() {
        let paths = Paths::at(r"C:\x\AAEmuSandbox");
        assert!(ensure_ours(&paths, Path::new(r"C:\x\AAEmuSandbox\client")).is_ok());
        assert!(ensure_ours(&paths, Path::new(r"C:\x\AAEmuSandbox")).is_err());
        assert!(ensure_ours(&paths, Path::new(r"C:\x\AAEmuSandboxEvil\client")).is_err());
        assert!(ensure_ours(&paths, Path::new(r"C:\Games\AAClassic\game_pak")).is_err());
    }

    #[test]
    fn login_rules() {
        assert!(valid_username("tester_1"));
        assert!(valid_username("Ünïcode.name@x"));
        assert!(!valid_username(""));
        assert!(!valid_username("has space"));
        assert!(!valid_username(&"a".repeat(33)));
        let t = token_from_password("secret");
        assert!(valid_token(&t));
        assert_eq!(t, "2bb80d537b1da3e38bd30361aa855686bde0eacd7162fef6a25fe97bf527a25b");
    }

    #[test]
    fn client_environment_is_sandboxed_without_uploads() {
        let paths = Paths::at(r"C:\Users\x\AppData\Local\AAEmuSandbox");
        let (set, remove) = client_env(&paths);
        let get = |k: &str| set.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(get("USERPROFILE"), r"C:\Users\x\AppData\Local\AAEmuSandbox\profile");
        assert_eq!(get("HOMEDRIVE"), "C:");
        assert_eq!(get("HOMEPATH"), r"\Users\x\AppData\Local\AAEmuSandbox\profile");
        assert_eq!(get("AAC_NO_CRASH_UPLOAD"), "1");
        assert_eq!(get("AAC_NO_PERF_UPLOAD"), "1");
        assert!(remove.contains(&"SteamAppId".to_string()));
        let args = client_args(&Launch { username: "u".into(), token: "t".into(), login_ip: "1.2.3.4".into(), game_ip: "5.6.7.8".into(), lang: "en_us".into(), login_port: 0 });
        assert_eq!(args.join(" "), "-r +auth_ip 1.2.3.4 -uid u -token t --lang en_us +cl_serveraddr 5.6.7.8");
        let args = client_args(&Launch { username: "u".into(), token: "t".into(), login_ip: "1.2.3.4".into(), game_ip: "1.2.3.4".into(), lang: "en_us".into(), login_port: 2237 });
        assert!(args.join(" ").ends_with("+cl_serveraddr 1.2.3.4 +auth_port 2237"));
    }
}
