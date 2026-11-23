//! Recording's one admin prompt.
//!
//! AA Classic's own launcher runs as admin, so the recorder's watcher needs admin rights to point it at the relay.
//! Without help, Windows would ask for "AA Replay" at every AA Classic start. Instead the launcher asks once, when
//! recording is set up: "AA Replay.exe" (in the recorder) runs `admin_task.py install` as admin, which checks the
//! recorder against its signed manifest, copies it to `%ProgramFiles%\AA Replay\recorder\<id>` and registers the
//! scheduled task "AA Replay Watcher" (this user, highest privileges, started on demand; the user may read and run
//! it, never change it). From then on the watcher starts that task with `schtasks /run` and Windows asks nothing.
//!
//! The user's side never runs anything as admin itself: it writes a request (`recordings\logs\admin-request.json`,
//! `{kind, time}`) and starts the task, and the task checks the request (Tools/AACCapture/admin_task.py):
//! - `update`: a newer recorder was set up here; the task checks and copies it the same way (no prompt);
//! - `remove`: uninstall; the task deletes itself and its folder (no prompt).
//!
//! Declining the prompt keeps the per-start prompt: recording still works, but Windows asks at each AA Classic start.

use crate::core::Paths;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

pub const TASK: &str = "AA Replay Watcher";
/// What the card says about the prompt, before and while it is up.
pub const WHY: &str = "Windows asks you once for admin rights: AA Classic runs as admin, so recording it needs them.";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const ERROR_CANCELLED: i32 = 1223;

/// How this Windows user's rights look to the recorder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elevation {
    /// an admin with UAC: can be asked once (or already is elevated)
    CanElevate,
    /// everything runs as admin already (UAC off, or the built-in Administrator): nothing to ask
    AlwaysAdmin,
    /// not an admin: only an admin's password helps, at each AA Classic start
    Standard,
}

/// What the recording card says about admin rights.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum State {
    /// the task is there: AA Classic starts are recorded with no prompt
    Ready,
    /// not asked yet
    #[default]
    NotSetUp,
    /// the user said No: Windows asks at each AA Classic start
    Declined,
    /// nothing to set up
    NotNeeded,
    /// this user is not an admin
    NoAdmin,
}

pub fn state_of(task: bool, elevation: Elevation, declined: bool) -> State {
    match (task, elevation) {
        (true, _) => State::Ready,
        (false, Elevation::AlwaysAdmin) => State::NotNeeded,
        (false, Elevation::Standard) => State::NoAdmin,
        (false, Elevation::CanElevate) if declined => State::Declined,
        (false, Elevation::CanElevate) => State::NotSetUp,
    }
}

pub fn state(paths: &Paths) -> State {
    state_of(task_exists(TASK), elevation(), declined_file(paths).exists())
}

/// One line for the card, or none when there is nothing to say.
pub fn line(s: State) -> Option<&'static str> {
    match s {
        State::Ready | State::NotNeeded => None,
        State::NotSetUp => Some("Admin rights are not set up yet: press Set up admin once, or Windows asks at each AA Classic start."),
        State::Declined => Some("Admin rights were not given, so Windows asks for AA Replay at each AA Classic start (No there: that session is not recorded). Set up admin once to stop that."),
        State::NoAdmin => Some("This Windows user is not an admin: each AA Classic start asks for an admin's password for AA Replay."),
    }
}

fn declined_file(paths: &Paths) -> PathBuf {
    crate::recorder::data_dir(paths).join("admin-declined.txt")
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn elevation() -> Elevation {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TokenElevationType};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let mut token: HANDLE = std::ptr::null_mut();
    let mut kind: i32 = 0;
    let mut size = 0u32;
    let ok = unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) != 0 && {
            let r = GetTokenInformation(token, TokenElevationType, (&mut kind as *mut i32).cast(), 4, &mut size) != 0;
            CloseHandle(token);
            r
        }
    };
    match (ok, kind) {
        (true, 2) | (true, 3) => Elevation::CanElevate, // TokenElevationTypeFull / Limited: a split admin token
        _ if unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() } != 0 => Elevation::AlwaysAdmin,
        _ => Elevation::Standard,
    }
}

/// This user's SID, as the task is registered for it.
pub fn user_sid() -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut buf = vec![0u64; 64]; // aligned, 512 bytes: a TOKEN_USER and its SID
        let mut size = 0u32;
        let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), (buf.len() * 8) as u32, &mut size) != 0;
        CloseHandle(token);
        if !ok {
            return None;
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut text: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
            return None;
        }
        let mut n = 0;
        while *text.add(n) != 0 {
            n += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(text, n));
        LocalFree(text.cast());
        Some(s)
    }
}

fn schtasks(args: &[&str]) -> i32 {
    std::process::Command::new("schtasks.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()
        .and_then(|s| s.code())
        .unwrap_or(-1)
}

pub fn task_exists(task: &str) -> bool {
    schtasks(&["/query", "/tn", task]) == 0
}

/// Starts the task (no prompt): 0 when Windows started it.
pub fn run_task(task: &str) -> i32 {
    schtasks(&["/run", "/tn", task])
}

/// The folder the task's copy lives in (only admins can change it).
pub fn admin_root() -> PathBuf {
    PathBuf::from(std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into())).join("AA Replay").join("recorder")
}

/// The recorder id the task runs.
fn task_recorder(root: &Path) -> Option<String> {
    fs::read_to_string(root.join("current.txt")).ok().map(|s| s.trim().to_string())
}

/// The request the task reads (admin_task.read_request: at most two minutes old).
pub fn request_json(kind: &str, now: f64) -> String {
    serde_json::json!({ "kind": kind, "time": now }).to_string()
}

fn write_request(paths: &Paths, kind: &str) -> Result<(), String> {
    let logs = crate::recorder::data_dir(paths).join("logs");
    fs::create_dir_all(&logs).map_err(|e| format!("cannot make {}: {e}", logs.display()))?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    let (tmp, file) = (logs.join("admin-request.json.tmp"), logs.join("admin-request.json"));
    fs::write(&tmp, request_json(kind, now)).and_then(|()| fs::rename(&tmp, &file)).map_err(|e| format!("cannot ask AA Replay's task: {e}"))
}

/// The command "AA Replay.exe" runs as admin at setup (every path quoted).
pub fn install_command(rec: &Path, paths: &Paths, sid: &str) -> String {
    format!(
        "\"{}\" \"{}\" install --staged \"{}\" --data \"{}\" --sid {}",
        rec.join("python").join("python.exe").display(),
        rec.join("app").join("admin_task.py").display(),
        paths.root.join("recorder").display(),
        crate::recorder::data_dir(paths).display(),
        sid
    )
}

/// What the one prompt came to.
#[derive(Debug, PartialEq, Eq)]
pub enum SetUp {
    Done,
    Declined,
}

/// The one admin prompt: sets up the task for the recorder in `rec`. `Ok(Declined)` when the user said No.
pub fn set_up(paths: &Paths, rec: &Path) -> Result<SetUp, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    let exe = rec.join("app").join("AA Replay.exe");
    if !exe.is_file() || !rec.join("app").join("admin_task.py").is_file() {
        return Err("this recorder cannot set up admin rights once: set up recording again to get the newer one".into());
    }
    let sid = user_sid().ok_or("Windows did not say who this user is")?;
    let declined = declined_file(paths);
    let _ = fs::create_dir_all(declined.parent().unwrap());
    // for tests: as if the user said No, with no prompt
    let code = if std::env::var("AAREPLAY_TEST_DECLINE").is_ok_and(|v| v == "1") {
        Err(ERROR_CANCELLED)
    } else {
        let (verb, file, params, dir) = (wide("runas"), wide(&exe.display().to_string()), wide(&install_command(rec, paths, &sid)), wide(&rec.join("app").display().to_string()));
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.lpDirectory = dir.as_ptr();
        info.nShow = 0; // SW_HIDE
        if unsafe { ShellExecuteExW(&mut info) } == 0 {
            Err(unsafe { GetLastError() } as i32)
        } else {
            let mut code = 1u32;
            unsafe {
                if WaitForSingleObject(info.hProcess, 10 * 60 * 1000) == WAIT_OBJECT_0 {
                    GetExitCodeProcess(info.hProcess, &mut code);
                }
                CloseHandle(info.hProcess);
            }
            Ok(code)
        }
    };
    match code {
        Err(ERROR_CANCELLED) => {
            let _ = fs::write(&declined, "The admin prompt at setup was answered No.\n");
            Ok(SetUp::Declined)
        }
        Err(e) => Err(format!("Windows could not ask for admin rights (error {e})")),
        Ok(0) if task_exists(TASK) => {
            let _ = fs::remove_file(&declined);
            Ok(SetUp::Done)
        }
        Ok(c) => Err(format!(
            "admin rights were given, but the task was not set up (code {c}; {}): Windows asks at each AA Classic start instead",
            crate::recorder::data_dir(paths).join("logs").join("admin-task.log").display()
        )),
    }
}

/// After recording is set up: the one admin prompt, when it was never asked (`again`: also after a No).
/// Returns what the player reads, with whether it is good news.
pub fn after_setup(paths: &Paths, again: bool, say: &dyn Fn(String)) -> Option<Result<String, String>> {
    let s = state(paths);
    if !(s == State::NotSetUp || (again && s == State::Declined)) {
        return None;
    }
    let rec = crate::recorder::installed(paths)?;
    say(WHY.to_string());
    Some(match set_up(paths, &rec) {
        Ok(SetUp::Done) => Ok("Admin rights are set up once: Windows will not ask again when AA Classic starts.".into()),
        Ok(SetUp::Declined) => Err(line(State::Declined).unwrap().into()),
        Err(e) => Err(e),
    })
}

/// After a recorder update: when the task runs an older copy, it is asked to take the new one (no prompt).
/// Returns true when it was asked.
pub fn bring_up_to_date(paths: &Paths) -> bool {
    let Some(mine) = crate::recorder::installed(paths).and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned())) else { return false };
    if !task_exists(TASK) || task_recorder(&admin_root()).as_deref() == Some(mine.as_str()) {
        return false;
    }
    write_request(paths, "update").is_ok() && run_task(TASK) == 0
}

/// Uninstall: the task removes itself and its folder (no prompt). Waits for it to go.
pub fn remove(paths: &Paths) -> Result<(), String> {
    if !task_exists(TASK) {
        return Ok(());
    }
    // an admin watcher still running (AA Classic open) ends when the recorder has stopped; the task starts one
    // instance at a time, so it is asked again until it is gone
    for _ in 0..15 {
        write_request(paths, "remove")?;
        run_task(TASK);
        for _ in 0..8 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if !task_exists(TASK) {
                return Ok(());
            }
        }
    }
    Err(format!("Windows' task \"{TASK}\" is still there: close AA Classic and uninstall again, or delete it in Task Scheduler"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_card_says_for_each_case() {
        use Elevation::*;
        assert_eq!(state_of(true, CanElevate, true), State::Ready);
        assert_eq!(state_of(true, Standard, false), State::Ready);
        assert_eq!(state_of(false, CanElevate, false), State::NotSetUp);
        assert_eq!(state_of(false, CanElevate, true), State::Declined);
        assert_eq!(state_of(false, AlwaysAdmin, true), State::NotNeeded);
        assert_eq!(state_of(false, Standard, false), State::NoAdmin);
        assert_eq!(line(State::Ready), None);
        // declining: what is lost is said
        assert!(line(State::Declined).unwrap().contains("each AA Classic start"));
        assert!(line(State::Declined).unwrap().contains("not recorded"));
        assert!(WHY.contains("once") && WHY.ends_with('.'));
    }

    #[test]
    fn the_install_command_quotes_every_path() {
        let paths = Paths::at(r"C:\Users\Zoë A\AppData\Local\AAEmuSandbox");
        let rec = paths.root.join("recorder").join("ab");
        assert_eq!(
            install_command(&rec, &paths, "S-1-5-21-1-2-3-1001"),
            r#""C:\Users\Zoë A\AppData\Local\AAEmuSandbox\recorder\ab\python\python.exe" "C:\Users\Zoë A\AppData\Local\AAEmuSandbox\recorder\ab\app\admin_task.py" install --staged "C:\Users\Zoë A\AppData\Local\AAEmuSandbox\recorder" --data "C:\Users\Zoë A\AppData\Local\AAEmuSandbox\recordings" --sid S-1-5-21-1-2-3-1001"#
        );
    }

    #[test]
    fn a_request_is_what_admin_task_reads() {
        let v: serde_json::Value = serde_json::from_str(&request_json("update", 1700000000.5)).unwrap();
        assert_eq!(v["kind"], "update");
        assert_eq!(v["time"], 1700000000.5);
    }

    #[test]
    fn this_users_sid_and_rights_are_read() {
        let sid = user_sid().unwrap();
        assert!(sid.starts_with("S-1-5-"), "{sid}");
        let _ = elevation();
        let missing = format!("AA Replay test missing {}", std::process::id());
        assert!(!task_exists(&missing));
        assert_ne!(run_task(&missing), 0);
    }

    #[test]
    fn declining_keeps_recording_with_the_per_start_prompt() {
        let root = std::env::temp_dir().join(format!("aar-admin-decline-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let paths = Paths::at(&root);
        let rec = root.join("recorder").join("ab");
        fs::create_dir_all(rec.join("app")).unwrap();
        fs::write(rec.join("app").join("AA Replay.exe"), b"MZ").unwrap();
        fs::write(rec.join("app").join("admin_task.py"), b"").unwrap();
        unsafe { std::env::set_var("AAREPLAY_TEST_DECLINE", "1") };
        let r = set_up(&paths, &rec);
        unsafe { std::env::remove_var("AAREPLAY_TEST_DECLINE") };
        assert_eq!(r, Ok(SetUp::Declined));
        assert!(declined_file(&paths).exists());
        if elevation() == Elevation::CanElevate && !task_exists(TASK) {
            assert_eq!(state(&paths), State::Declined);
        }
        let _ = fs::remove_dir_all(&root);
    }
}
