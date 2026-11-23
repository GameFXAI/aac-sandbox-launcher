//! AAReplaySetup.exe: the small file people download from the browser.
//!
//! It fetches everything else. The installer it runs changes with every launcher release; this file does not, so
//! Windows SmartScreen's reputation for it (per file hash while it is unsigned) keeps growing instead of starting
//! again at each release. What it does:
//!
//! 1. reads `setup.json` and `setup.json.sig` from the update host, and checks the ed25519 signature with the
//!    launcher's update key (the same key as the launcher's own updates and the recorder);
//! 2. downloads the installer that manifest names (HTTPS, no redirects) and checks its size and SHA-256;
//! 3. writes it to a temp folder of its own (with no "downloaded from the internet" mark, so no second SmartScreen
//!    warning), runs it with `/SILENT` for this Windows user only (no admin rights), and passes the invite code;
//! 4. starts AA Replay and deletes the installer.
//!
//! The invite: the account site hands this file out as `AAReplaySetup-<code>.exe` (the same bytes); the code is
//! read from the file name, or from `/INVITE=<code>`.
//!
//! `setup.json`: `{ "setup": { "version": "0.3.1", "url": "https://...", "size": 5422404, "sha256": "..." } }`,
//! made and signed by the launcher's `installer\release.ps1`.
#![cfg_attr(not(test), windows_subsystem = "windows")]

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// The launcher's update key (aac-sandbox-launcher src/update.rs UPDATE_KEY): the public half only.
const UPDATE_KEY: &str = "fef8a3544a2b38be22877cf633804ed61c614aacd6d9ea32bd5dd0ca317b70cc";
const SETUP_MANIFEST: &str = "https://77-42-46-123.sslip.io/launcher/setup.json";
/// The installer's AppId (installer\aac-sandbox-launcher.iss): where an earlier install is.
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{6E0B3C52-8D0C-4E1B-9A55-5B7C1F0A35A1}_is1";
const LAUNCHER_EXE: &str = "aac-sandbox-launcher.exe";
const INVITE_ALPHABET: &str = "ABCDEFGHJKMNPQRSTUVWXYZ23456789";
const MAX_MANIFEST: u64 = 64 << 10;
const MAX_SETUP: u64 = 256 << 20;
const TITLE: &str = "AA Replay setup";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

// ---- the manifest ---------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
struct Setup {
    version: String,
    url: String,
    size: u64,
    sha256: String,
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Checks the signature first, then reads the manifest.
fn read_manifest(manifest: &[u8], signature_hex: &str, key_hex: &str) -> Result<Setup, String> {
    let key: [u8; 32] = unhex(key_hex).and_then(|k| k.try_into().ok()).ok_or("bad key")?;
    let sig: [u8; 64] = unhex(signature_hex).and_then(|s| s.try_into().ok()).ok_or("the setup's signature is missing or damaged")?;
    VerifyingKey::from_bytes(&key)
        .map_err(|_| "bad key".to_string())?
        .verify(manifest, &Signature::from_bytes(&sig))
        .map_err(|_| "the setup manifest is not signed by AA Replay: nothing was installed".to_string())?;
    let v: serde_json::Value = serde_json::from_slice(manifest).map_err(|_| "the setup manifest was not understood")?;
    let s = &v["setup"];
    let setup = Setup {
        version: s["version"].as_str().unwrap_or_default().to_string(),
        url: s["url"].as_str().unwrap_or_default().to_string(),
        size: s["size"].as_u64().unwrap_or(0),
        sha256: s["sha256"].as_str().unwrap_or_default().to_ascii_lowercase(),
    };
    if setup.size == 0 || setup.size > MAX_SETUP || setup.sha256.len() != 64 || unhex(&setup.sha256).is_none() || parse_url(&setup.url).is_none() {
        return Err("the setup manifest names a download this setup will not take".into());
    }
    Ok(setup)
}

// ---- downloads (WinHTTP: no TLS library of our own, Windows' proxy settings) -------------------------------------

/// (https, host, port, path). Plain http only to this PC (tests).
fn parse_url(url: &str) -> Option<(bool, String, u16, String)> {
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else {
        (false, url.strip_prefix("http://")?)
    };
    let (hostport, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().ok()?),
        None => (hostport, if secure { 443 } else { 80 }),
    };
    if host.is_empty() || host.contains(['@', ' ', '\\']) || (!secure && host != "127.0.0.1" && host != "localhost") {
        return None;
    }
    Some((secure, host.to_string(), port, if path.is_empty() { "/".into() } else { path.to_string() }))
}

struct Handle(*mut core::ffi::c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { windows_sys::Win32::Networking::WinHttp::WinHttpCloseHandle(self.0) };
        }
    }
}

/// GETs a URL, at most `limit` bytes; `progress(done, total)` as it goes; stops when CANCEL is set.
fn fetch(url: &str, limit: u64, progress: &dyn Fn(u64, u64)) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Networking::WinHttp::*;
    let (secure, host, port, path) = parse_url(url).ok_or("not an address this setup downloads from")?;
    let unreachable = |what: &str| format!("the AA Replay site could not be reached ({what}, error {})", unsafe { windows_sys::Win32::Foundation::GetLastError() });
    unsafe {
        let agent = wide("AAReplaySetup/1");
        let mut session = Handle(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, std::ptr::null(), std::ptr::null(), 0));
        if session.0.is_null() {
            session = Handle(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, std::ptr::null(), std::ptr::null(), 0));
        }
        if session.0.is_null() {
            return Err(unreachable("open"));
        }
        WinHttpSetTimeouts(session.0, 15_000, 15_000, 30_000, 60_000);
        let h = wide(&host);
        let connect = Handle(WinHttpConnect(session.0, h.as_ptr(), port, 0));
        if connect.0.is_null() {
            return Err(unreachable("connect"));
        }
        let (verb, object) = (wide("GET"), wide(&path));
        let request = Handle(WinHttpOpenRequest(connect.0, verb.as_ptr(), object.as_ptr(), std::ptr::null(), std::ptr::null(), std::ptr::null(), if secure { WINHTTP_FLAG_SECURE } else { 0 }));
        if request.0.is_null() {
            return Err(unreachable("request"));
        }
        let never: u32 = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
        WinHttpSetOption(request.0, WINHTTP_OPTION_REDIRECT_POLICY, (&never as *const u32).cast(), 4);
        if WinHttpSendRequest(request.0, std::ptr::null(), 0, std::ptr::null(), 0, 0, 0) == 0 || WinHttpReceiveResponse(request.0, std::ptr::null_mut()) == 0 {
            return Err(unreachable("send"));
        }
        let (mut status, mut len) = (0u32, 4u32);
        WinHttpQueryHeaders(request.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, std::ptr::null(), (&mut status as *mut u32).cast(), &mut len, std::ptr::null_mut());
        if status != 200 {
            return Err(format!("the AA Replay site answered {status}"));
        }
        let (mut total, mut tlen) = (0u32, 4u32);
        let has_total = WinHttpQueryHeaders(request.0, WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER, std::ptr::null(), (&mut total as *mut u32).cast(), &mut tlen, std::ptr::null_mut()) != 0;
        let mut out = Vec::new();
        let mut buf = vec![0u8; 64 << 10];
        loop {
            if CANCEL.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            let mut read = 0u32;
            if WinHttpReadData(request.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut read) == 0 {
                return Err(unreachable("the download broke off"));
            }
            if read == 0 {
                break;
            }
            out.extend_from_slice(&buf[..read as usize]);
            if out.len() as u64 > limit {
                return Err("the download is larger than it can be".into());
            }
            progress(out.len() as u64, if has_total { total as u64 } else { limit });
        }
        Ok(out)
    }
}

// ---- the download mark -----------------------------------------------------------------------------------------

/// Takes the "downloaded from the internet" mark (`Zone.Identifier`) off a file, so SmartScreen does not ask again.
/// WinHTTP never adds one; this is for a setup file copied from a marked folder.
fn strip_mark_of_the_web(file: &Path) {
    let mut s = file.as_os_str().to_os_string();
    s.push(":Zone.Identifier");
    let w: Vec<u16> = std::os::windows::ffi::OsStrExt::encode_wide(s.as_os_str()).chain(Some(0)).collect();
    unsafe { windows_sys::Win32::Storage::FileSystem::DeleteFileW(w.as_ptr()) };
}

// ---- the invite ----------------------------------------------------------------------------------------------

fn valid_invite(c: &str) -> bool {
    c.len() == 20 && c.chars().all(|ch| INVITE_ALPHABET.contains(ch))
}

/// `/INVITE=<code>`, else `AAReplaySetup-<code>.exe` (a browser may add " (1)").
fn invite_code(args: &[String], exe_name: &str) -> Option<String> {
    for a in args {
        if let Some(c) = a.get(..8).filter(|p| p.eq_ignore_ascii_case("/INVITE=")).map(|_| a[8..].trim().to_ascii_uppercase()) {
            if valid_invite(&c) {
                return Some(c);
            }
        }
    }
    let name = exe_name.to_ascii_uppercase();
    let at = name.find("AAREPLAYSETUP-")? + "AAREPLAYSETUP-".len();
    let c = name.get(at..at + 20)?;
    valid_invite(c).then(|| c.to_string())
}

// ---- where AA Replay is installed ------------------------------------------------------------------------------

fn install_location() -> PathBuf {
    use windows_sys::Win32::System::Registry::*;
    let (k, v) = (wide(UNINSTALL_KEY), wide("InstallLocation"));
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size) };
    if r == 0 {
        let n = (size as usize / 2).min(buf.len());
        let s = String::from_utf16_lossy(&buf[..n]).trim_end_matches('\0').trim().to_string();
        if !s.is_empty() {
            return PathBuf::from(s);
        }
    }
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())).join("Programs").join("AAReplay")
}

// ---- the window ------------------------------------------------------------------------------------------------

static CANCEL: AtomicBool = AtomicBool::new(false);
static DONE: AtomicBool = AtomicBool::new(false);
/// per mille, or u32::MAX while the size is not known
static PROGRESS: AtomicU32 = AtomicU32::new(u32::MAX);
static TEXT: Mutex<String> = Mutex::new(String::new());
static RESULT: Mutex<Option<Result<PathBuf, String>>> = Mutex::new(None);

fn say(t: &str) {
    *TEXT.lock().unwrap() = t.to_string();
}

const WM_USER: u32 = 0x0400;
const TDM_CLICK_BUTTON: u32 = WM_USER + 102;
const TDM_SET_PROGRESS_BAR_STATE: u32 = WM_USER + 104;
const TDM_SET_PROGRESS_BAR_RANGE: u32 = WM_USER + 105;
const TDM_SET_PROGRESS_BAR_POS: u32 = WM_USER + 106;
const TDM_SET_PROGRESS_BAR_MARQUEE: u32 = WM_USER + 107;
const TDM_SET_ELEMENT_TEXT: u32 = WM_USER + 108;
const TDN_CREATED: u32 = 0;
const TDN_BUTTON_CLICKED: u32 = 2;
const TDN_TIMER: u32 = 4;
const IDCANCEL: i32 = 2;

unsafe extern "system" fn callback(hwnd: windows_sys::Win32::Foundation::HWND, msg: u32, wparam: usize, _lparam: isize, _data: isize) -> i32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;
    static SHOWN: Mutex<String> = Mutex::new(String::new());
    static MARQUEE: AtomicBool = AtomicBool::new(true);
    unsafe {
        match msg {
            TDN_CREATED => {
                SendMessageW(hwnd, TDM_SET_PROGRESS_BAR_MARQUEE, 1, 0);
            }
            TDN_TIMER => {
                let p = PROGRESS.load(Ordering::Relaxed);
                if p != u32::MAX && MARQUEE.swap(false, Ordering::Relaxed) {
                    SendMessageW(hwnd, TDM_SET_PROGRESS_BAR_MARQUEE, 0, 0);
                    SendMessageW(hwnd, TDM_SET_PROGRESS_BAR_STATE, 1, 0);
                    SendMessageW(hwnd, TDM_SET_PROGRESS_BAR_RANGE, 0, (1000 << 16) as isize);
                }
                if p != u32::MAX {
                    SendMessageW(hwnd, TDM_SET_PROGRESS_BAR_POS, p.min(1000) as usize, 0);
                }
                let t = TEXT.lock().unwrap().clone();
                let mut shown = SHOWN.lock().unwrap();
                if *shown != t {
                    let w = wide(&t);
                    SendMessageW(hwnd, TDM_SET_ELEMENT_TEXT, 0, w.as_ptr() as isize); // TDE_CONTENT
                    *shown = t;
                }
                if DONE.load(Ordering::Relaxed) {
                    SendMessageW(hwnd, TDM_CLICK_BUTTON, IDCANCEL as usize, 0);
                }
            }
            TDN_BUTTON_CLICKED if wparam as i32 == IDCANCEL && !DONE.load(Ordering::Relaxed) => {
                CANCEL.store(true, Ordering::Relaxed);
            }
            _ => {}
        }
    }
    0
}

fn progress_window() {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Controls::*;
    let (title, main, content) = (wide(TITLE), wide("Installing AA Replay"), wide("Getting the installer from the AA Replay site…"));
    let mut cfg: TASKDIALOGCONFIG = unsafe { std::mem::zeroed() };
    cfg.cbSize = std::mem::size_of::<TASKDIALOGCONFIG>() as u32;
    cfg.hInstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    cfg.dwFlags = TDF_SHOW_MARQUEE_PROGRESS_BAR | TDF_CALLBACK_TIMER | TDF_ALLOW_DIALOG_CANCELLATION;
    cfg.dwCommonButtons = TDCBF_CANCEL_BUTTON;
    cfg.pszWindowTitle = title.as_ptr();
    cfg.Anonymous1.pszMainIcon = 1 as *const u16; // the icon resource (setup.rc)
    cfg.pszMainInstruction = main.as_ptr();
    cfg.pszContent = content.as_ptr();
    cfg.pfCallback = Some(callback);
    cfg.cxWidth = 260;
    unsafe { TaskDialogIndirect(&cfg, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
}

fn message(text: &str, error: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let (t, c) = (wide(text), wide(TITLE));
    unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), if error { MB_ICONERROR } else { MB_ICONINFORMATION } | MB_OK | MB_SETFOREGROUND) };
}

// ---- the steps -------------------------------------------------------------------------------------------------

fn manifest_url() -> String {
    // tests only: a manifest on this PC (http://127.0.0.1:...)
    std::env::var("AAREPLAY_SETUP_MANIFEST").ok().filter(|u| !u.trim().is_empty()).unwrap_or_else(|| SETUP_MANIFEST.to_string())
}

fn key() -> String {
    std::env::var("AAREPLAY_SETUP_TEST_KEY").ok().filter(|_| manifest_url() != SETUP_MANIFEST).unwrap_or_else(|| UPDATE_KEY.to_string())
}

/// Downloads and checks the installer; returns where it was written.
fn download(to: &Path) -> Result<PathBuf, String> {
    let url = manifest_url();
    let manifest = fetch(&url, MAX_MANIFEST, &|_, _| {})?;
    let sig = fetch(&format!("{url}.sig"), 1024, &|_, _| {})?;
    let setup = read_manifest(&manifest, &String::from_utf8_lossy(&sig), &key())?;
    say(&format!("Downloading AA Replay {} ({:.1} MB)…", setup.version, setup.size as f64 / 1048576.0));
    let bytes = fetch(&setup.url, setup.size, &|done, total| PROGRESS.store((done * 1000 / total.max(1)) as u32, Ordering::Relaxed))?;
    if bytes.len() as u64 != setup.size || hex(&Sha256::digest(&bytes)) != setup.sha256 {
        return Err("the download is damaged or is not the installer AA Replay signed: nothing was installed".into());
    }
    let dir = to.join(format!("AA Replay setup {}", &setup.sha256[..12]));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot write to {}: {e}", dir.display()))?;
    let file = dir.join("AA Replay installer.exe");
    std::fs::write(&file, &bytes).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    strip_mark_of_the_web(&file);
    say("Installing…");
    Ok(file)
}

fn run(args: &[String]) -> i32 {
    let exe_name = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
    let invite = invite_code(args, &exe_name);
    // /CHECK: download and check the signed installer, install nothing (the release checklist; exit 0 when good)
    let check = args.iter().any(|a| a.eq_ignore_ascii_case("/CHECK"));
    let quiet = check || args.iter().any(|a| a.eq_ignore_ascii_case("/VERYSILENT"));
    let temp = std::env::temp_dir();
    let worker = std::thread::spawn(move || {
        let r = download(&temp);
        *RESULT.lock().unwrap() = Some(r);
        DONE.store(true, Ordering::Relaxed);
    });
    if quiet {
        let _ = worker.join();
    } else {
        progress_window();
        if !DONE.load(Ordering::Relaxed) {
            CANCEL.store(true, Ordering::Relaxed);
            return 1;
        }
        let _ = worker.join();
    }
    let file = match RESULT.lock().unwrap().take() {
        Some(Ok(f)) => f,
        Some(Err(e)) => {
            if !quiet {
                message(&format!("AA Replay could not be installed: {e}.\n\nCheck your internet connection and start AAReplaySetup.exe again."), true);
            }
            return 2;
        }
        None => return 2,
    };
    if check {
        let _ = std::fs::remove_file(&file);
        if let Some(d) = file.parent() {
            let _ = std::fs::remove_dir(d);
        }
        return 0;
    }
    // the installer: this Windows user only, no admin rights, its own progress window
    let mut cmd = std::process::Command::new(&file);
    cmd.args([if quiet { "/VERYSILENT" } else { "/SILENT" }, "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"]);
    if let Some(c) = &invite {
        cmd.arg(format!("/INVITE={c}"));
    }
    let code = cmd.status().map(|s| s.code().unwrap_or(1)).unwrap_or(-1);
    let _ = std::fs::remove_file(&file);
    if let Some(d) = file.parent() {
        let _ = std::fs::remove_dir(d);
    }
    if code != 0 {
        if !quiet {
            message(&format!("The AA Replay installer stopped (code {code}). Start AAReplaySetup.exe again; if it happens again, tell us on the AA Replay site."), true);
        }
        return 3;
    }
    let launcher = install_location().join(LAUNCHER_EXE);
    if !quiet {
        if std::process::Command::new(&launcher).spawn().is_err() {
            message(&format!("AA Replay is installed. Start it from the Start menu (AA Replay).\n\n{}", launcher.display()), false);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn key() -> (SigningKey, String) {
        let k = SigningKey::from_bytes(&[7u8; 32]);
        let p = hex(&k.verifying_key().to_bytes());
        (k, p)
    }

    fn manifest(url: &str, body: &[u8]) -> Vec<u8> {
        format!(r#"{{"setup":{{"version":"0.3.1","url":"{url}","size":{},"sha256":"{}"}}}}"#, body.len(), hex(&Sha256::digest(body))).into_bytes()
    }

    #[test]
    fn only_a_signed_manifest_is_read() {
        let (k, public) = key();
        let m = manifest("https://h/AAReplaySetup-0.3.1.exe", b"setup");
        let sig = hex(&k.sign(&m).to_bytes());
        let s = read_manifest(&m, &sig, &public).unwrap();
        assert_eq!((s.version.as_str(), s.size), ("0.3.1", 5));
        // the real key: a test manifest is not signed with it
        assert!(read_manifest(&m, &sig, UPDATE_KEY).unwrap_err().contains("not signed"));
        let mut changed = m.clone();
        changed[20] ^= 1;
        assert!(read_manifest(&changed, &sig, &public).is_err());
        assert!(read_manifest(&m, "zz", &public).is_err());
        // signed, but plain http to another PC: refused
        let m2 = manifest("http://example.com/x.exe", b"setup");
        assert!(read_manifest(&m2, &hex(&k.sign(&m2).to_bytes()), &public).unwrap_err().contains("will not take"));
    }

    #[test]
    fn addresses() {
        assert_eq!(parse_url("https://77-42-46-123.sslip.io/launcher/setup.json"), Some((true, "77-42-46-123.sslip.io".into(), 443, "/launcher/setup.json".into())));
        assert_eq!(parse_url("http://127.0.0.1:8123/s.json"), Some((false, "127.0.0.1".into(), 8123, "/s.json".into())));
        assert_eq!(parse_url("http://example.com/s.json"), None);
        assert_eq!(parse_url("ftp://h/x"), None);
        assert_eq!(parse_url("https://user@h/x"), None);
    }

    #[test]
    fn the_invite_from_the_file_name_or_the_command_line() {
        let code = "ABCDEFGHJKMNPQRSTUVW";
        assert_eq!(invite_code(&[], &format!("AAReplaySetup-{code}.exe")).as_deref(), Some(code));
        assert_eq!(invite_code(&[], &format!("AAReplaySetup-{code} (1).exe")).as_deref(), Some(code));
        assert_eq!(invite_code(&[format!("/invite={}", code.to_lowercase())], "AAReplaySetup.exe").as_deref(), Some(code));
        assert_eq!(invite_code(&[], "AAReplaySetup.exe"), None);
        assert_eq!(invite_code(&[], "AAReplaySetup-ABCDEFGHJKMNPQRSTUV0.exe"), None); // 0 is not in the alphabet
    }

    #[test]
    fn the_mark_is_taken_off() {
        let d = std::env::temp_dir().join(format!("aar-setup-motw-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("AA Replay installer.exe");
        std::fs::write(&f, b"MZ").unwrap();
        let mut ads = f.as_os_str().to_os_string();
        ads.push(":Zone.Identifier");
        std::fs::write(PathBuf::from(&ads), "[ZoneTransfer]\r\nZoneId=3\r\n").unwrap();
        strip_mark_of_the_web(&f);
        assert!(std::fs::metadata(PathBuf::from(&ads)).is_err());
        assert_eq!(std::fs::read(&f).unwrap(), b"MZ");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The whole download on a local server: the signed manifest, the installer, its checks, and no mark on it.
    #[test]
    fn a_signed_installer_is_downloaded_and_checked() {
        use std::io::{Read, Write};
        let (k, public) = key();
        let body = b"MZ pretend installer".to_vec();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let m = manifest(&format!("http://127.0.0.1:{port}/AAReplaySetup-0.3.1.exe"), &body);
        let sig = hex(&k.sign(&m).to_bytes());
        let files = vec![("/setup.json", m.clone()), ("/setup.json.sig", sig.into_bytes()), ("/AAReplaySetup-0.3.1.exe", body.clone())];
        let server = std::thread::spawn(move || {
            for _ in 0..3 {
                let (mut c, _) = listener.accept().unwrap();
                let mut req = [0u8; 2048];
                let n = c.read(&mut req).unwrap();
                let line = String::from_utf8_lossy(&req[..n]).lines().next().unwrap_or_default().to_string();
                let path = line.split(' ').nth(1).unwrap_or_default().to_string();
                let body = files.iter().find(|(p, _)| *p == path).map(|(_, b)| b.clone()).unwrap_or_default();
                let _ = write!(c, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = c.write_all(&body);
            }
        });
        unsafe {
            std::env::set_var("AAREPLAY_SETUP_MANIFEST", format!("http://127.0.0.1:{port}/setup.json"));
            std::env::set_var("AAREPLAY_SETUP_TEST_KEY", &public);
        }
        let to = std::env::temp_dir().join(format!("aar-setup-dl Zoë {}", std::process::id()));
        let file = download(&to).unwrap();
        server.join().unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), body);
        let mut ads = file.as_os_str().to_os_string();
        ads.push(":Zone.Identifier");
        assert!(std::fs::metadata(PathBuf::from(&ads)).is_err(), "a WinHTTP download has no mark");
        let _ = std::fs::remove_dir_all(&to);
    }
}
