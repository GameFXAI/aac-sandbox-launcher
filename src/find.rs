//! Finding the player's AA Classic install, and deciding where our own copy goes.
//!
//! AA Classic's installer (Inno Setup) leaves its folder in the Windows uninstall list, and its game writes
//! `XLGames\ArcheAge\<language>\setup_path` (the folder of the program files). Both are only read. When neither is
//! there, the usual folders on every drive are tried. A folder counts only when it has `binNext\archeage.exe` and
//! `game_pak`.
//!
//! The copy: Standard mode links AA Classic's 45 GB game data instead of copying it, and Windows can only link a
//! file within one drive. When AA Classic is on another drive than the launcher's folder, our small copy (65 MB)
//! goes to `<that drive>\AAReplay\client` instead, so the link still works. AA Classic's own folder is never written.

use std::path::{Path, PathBuf};

/// AA Classic's entry in the Windows uninstall list (its Inno Setup id).
const UNINSTALL_ID: &str = r"{DEBDBBE9-230E-42C4-B44D-E0DC0CD81791}_is1";
const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
const XLGAMES: &str = r"Software\XLGames\ArcheAge";

/// The name of the folder our copy gets on AA Classic's drive.
pub const OTHER_DRIVE_DIR: &str = "AAReplay";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub dir: PathBuf,
    /// where it was found, in words for the Settings screen
    pub how: &'static str,
}

pub fn is_aac(dir: &Path) -> bool {
    dir.join("binNext").join("archeage.exe").is_file() && dir.join("game_pak").is_file()
}

/// The AA Classic install on this PC, if one is found.
pub fn aac() -> Option<Hit> {
    candidates().into_iter().find(|h| is_aac(&h.dir))
}

/// Every place to look, in order: the uninstall list, the game's own key, then the usual folders.
pub fn candidates() -> Vec<Hit> {
    let mut out: Vec<Hit> = Vec::new();
    let mut add = |dir: PathBuf, how: &'static str| {
        let dir = PathBuf::from(dir.to_string_lossy().trim().trim_end_matches(['\\', '/']));
        if !dir.as_os_str().is_empty() && !out.iter().any(|h| crate::core::is_inside(&h.dir, &dir) && crate::core::is_inside(&dir, &h.dir)) {
            out.push(Hit { dir, how });
        }
    };
    for (root, view) in reg::VIEWS {
        // the known id first, then any entry that names itself ArcheAge Classic
        let mut keys = vec![format!(r"{UNINSTALL}\{UNINSTALL_ID}")];
        keys.extend(reg::subkeys(root, view, UNINSTALL).into_iter().map(|k| format!(r"{UNINSTALL}\{k}")));
        for k in keys {
            let named = reg::string(root, view, &k, "DisplayName").unwrap_or_default();
            if !k.ends_with(UNINSTALL_ID) && !named.to_ascii_lowercase().starts_with("archeage classic") {
                continue;
            }
            for v in ["InstallLocation", "Inno Setup: App Path"] {
                if let Some(d) = reg::string(root, view, &k, v) {
                    add(PathBuf::from(d), "the Windows list of installed programs");
                }
            }
        }
    }
    for (root, view) in reg::VIEWS {
        for lang in reg::subkeys(root, view, XLGAMES) {
            if let Some(p) = reg::string(root, view, &format!(r"{XLGAMES}\{lang}"), "setup_path") {
                // C:\Games\AAClassic\binNext\ -> C:\Games\AAClassic
                let p = PathBuf::from(p.trim().trim_end_matches(['\\', '/']));
                if let Some(parent) = p.parent() {
                    add(parent.to_path_buf(), "AA Classic's own setting in Windows");
                }
            }
        }
    }
    for d in drives() {
        for rel in [r"Games\AAClassic", "AAClassic", r"Program Files\AAClassic", r"Program Files (x86)\AAClassic", "ArcheAge Classic", r"Games\ArcheAge Classic"] {
            add(PathBuf::from(format!(r"{d}\{rel}")), "a usual install folder");
        }
    }
    out
}

/// The drive letters that exist, C: first.
fn drives() -> Vec<String> {
    let mask = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
    let mut v: Vec<String> = (0..26u8).filter(|i| mask & (1 << i) != 0).map(|i| format!("{}:", (b'A' + i) as char)).filter(|d| d != "A:" && d != "B:").collect();
    v.sort_by_key(|d| d != "C:");
    v
}

/// The root of the volume a path is on (`C:\`, or a mount folder), lower case; for a path that does not exist yet,
/// the nearest folder above it that does.
pub fn volume(p: &Path) -> String {
    use std::os::windows::ffi::OsStrExt;
    let mut probe = p.to_path_buf();
    while !probe.exists() {
        match probe.parent() {
            Some(q) if q != probe => probe = q.to_path_buf(),
            _ => break,
        }
    }
    let w: Vec<u16> = probe.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut buf = [0u16; 512];
    let ok = unsafe { windows_sys::Win32::Storage::FileSystem::GetVolumePathNameW(w.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    let s = if ok != 0 {
        String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)])
    } else {
        p.to_string_lossy().chars().take(3).collect()
    };
    s.to_lowercase()
}

/// The drive as a player reads it: "D:" (or the mount folder).
pub fn drive_name(p: &Path) -> String {
    let v = volume(p);
    let v = v.trim_end_matches('\\');
    if v.len() == 2 { v.to_uppercase() } else { v.to_string() }
}

/// Where our copy should be for Standard mode, given AA Classic's folder: None when the current place is fine
/// (same drive), else the folder on AA Classic's drive.
pub fn client_place(current_client: &Path, aac_dir: &Path) -> Option<PathBuf> {
    let vol = volume(aac_dir);
    if volume(current_client) == vol {
        return None;
    }
    Some(PathBuf::from(&vol).join(OTHER_DRIVE_DIR).join("client"))
}

/// The plain words for the Settings screen and the first start.
pub fn place_note(launcher_root: &Path, client: &Path, aac_dir: &Path) -> String {
    let (aac_drive, ours) = (drive_name(aac_dir), drive_name(launcher_root));
    if volume(aac_dir) == volume(launcher_root) {
        format!("AA Classic and the launcher are both on {ours}: your copy shares AA Classic's game data through a link (no extra 45 GB), and only the 65 MB program files are copied.")
    } else if volume(client) == volume(aac_dir) {
        format!("AA Classic is on {aac_drive} and the launcher on {ours}. A link only works within one drive, so your copy is in {} (65 MB) and links AA Classic's game data from there. Nothing in AA Classic's folder changes.", client.display())
    } else {
        format!("AA Classic is on {aac_drive} and the launcher on {ours}. A link only works within one drive: press Play and the launcher puts its 65 MB copy in {aac_drive}\\{OTHER_DRIVE_DIR}, or choose Full separation to copy all 45 GB to {ours}.")
    }
}

/// Read-only registry access (Win32).
mod reg {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::*;

    pub const VIEWS: [(HKEY, u32); 4] = [
        (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY),
        (HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY),
        (HKEY_CURRENT_USER, KEY_WOW64_32KEY),
        (HKEY_CURRENT_USER, KEY_WOW64_64KEY),
    ];

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn open(root: HKEY, view: u32, key: &str) -> Option<HKEY> {
        let mut h: HKEY = std::ptr::null_mut();
        let k = wide(key);
        let r = unsafe { RegOpenKeyExW(root, k.as_ptr(), 0, KEY_READ | view, &mut h) };
        (r == ERROR_SUCCESS).then_some(h)
    }

    /// A text value; None when the key or the value is not there.
    pub fn string(root: HKEY, view: u32, key: &str, value: &str) -> Option<String> {
        let h = open(root, view, key)?;
        let v = wide(value);
        let mut buf = vec![0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let r = unsafe { RegGetValueW(h, std::ptr::null(), v.as_ptr(), RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size) };
        unsafe { RegCloseKey(h) };
        if r != ERROR_SUCCESS {
            return None;
        }
        let n = (size as usize / 2).min(buf.len());
        let s = String::from_utf16_lossy(&buf[..n]);
        let s = s.trim_end_matches('\0').trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    /// The names of a key's subkeys.
    pub fn subkeys(root: HKEY, view: u32, key: &str) -> Vec<String> {
        let Some(h) = open(root, view, key) else { return vec![] };
        let mut out = Vec::new();
        for i in 0..4096u32 {
            let mut buf = [0u16; 256];
            let mut len = buf.len() as u32;
            let r = unsafe { RegEnumKeyExW(h, i, buf.as_mut_ptr(), &mut len, std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
            if r != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        unsafe { RegCloseKey(h) };
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_counts_only_with_both_files() {
        let d = std::env::temp_dir().join(format!("aareplay-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("binNext")).unwrap();
        assert!(!is_aac(&d));
        std::fs::write(d.join("binNext").join("archeage.exe"), b"x").unwrap();
        assert!(!is_aac(&d));
        std::fs::write(d.join("game_pak"), b"x").unwrap();
        assert!(is_aac(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_copy_moves_only_for_another_drive() {
        let here = std::env::temp_dir();
        assert_eq!(client_place(&here.join("AAEmuSandbox").join("client"), &here.join("AAClassic")), None);
        // a drive letter that is not this one: the copy goes to <that drive>\AAReplay\client
        let other = if volume(&here).starts_with("c:") { r"Q:\Games\AAClassic" } else { r"C:\Games\AAClassic" };
        let to = client_place(&here.join("client"), Path::new(other)).unwrap();
        assert!(to.to_string_lossy().to_lowercase().ends_with(r":\aareplay\client"), "{}", to.display());
        assert!(to.to_string_lossy().to_lowercase().starts_with(&other[..2].to_lowercase()));
        assert!(place_note(&here, &here.join("client"), Path::new(other)).contains("only works within one drive"));
    }

    #[test]
    fn candidates_start_with_what_windows_knows() {
        // on any PC: no panic, and every place is a full path
        for h in candidates() {
            assert!(h.dir.is_absolute(), "{}", h.dir.display());
        }
    }
}
