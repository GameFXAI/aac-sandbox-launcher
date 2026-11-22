//! "Remember me": the remembered sign-in the server gave, and the invite the installer left.
//!
//! Only the server's session token is kept, never the password. It is stored with Windows' own data protection
//! (DPAPI, for this Windows user only) in `signin.bin` in the launcher's folder, so another user of the PC or a copy
//! of the file elsewhere cannot use it. Signing out deletes the file and ends the token on the server.
//!
//! The installer reads the tester's invite code from its own file name (`AAReplaySetup-CODE.exe`) and writes it to
//! `invite.txt`; the launcher uses it for the first sign-up and deletes it once the account exists.

use crate::core::Paths;
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    /// the server link the token is for
    pub server: String,
    pub username: String,
    /// 64 hex characters; never shown or logged
    pub session: String,
}

fn file(paths: &Paths) -> std::path::PathBuf {
    paths.root.join("signin.bin")
}

pub fn load(paths: &Paths) -> Option<Saved> {
    let sealed = fs::read(file(paths)).ok()?;
    let plain = unprotect(&sealed)?;
    let s: Saved = serde_json::from_slice(&plain).ok()?;
    crate::core::valid_token(&s.session).then_some(s)
}

pub fn save(paths: &Paths, s: &Saved) -> Result<(), String> {
    let sealed = protect(&serde_json::to_vec(s).map_err(|e| e.to_string())?)?;
    fs::create_dir_all(&paths.root).map_err(|e| e.to_string())?;
    fs::write(file(paths), sealed).map_err(|e| format!("cannot keep the sign-in: {e}"))
}

pub fn forget(paths: &Paths) {
    let _ = fs::remove_file(file(paths));
}

const ALPHABET: &str = "ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// An invite code as the server makes them: 20 characters without look-alikes.
pub fn valid_invite(code: &str) -> bool {
    code.len() == 20 && code.chars().all(|c| ALPHABET.contains(c))
}

/// The invite the installer left, if any.
pub fn pending_invite(paths: &Paths) -> Option<String> {
    let t = fs::read_to_string(paths.root.join("invite.txt")).ok()?;
    let code = t.trim().to_ascii_uppercase();
    valid_invite(&code).then_some(code)
}

pub fn clear_invite(paths: &Paths) {
    let _ = fs::remove_file(paths.root.join("invite.txt"));
}

// --- DPAPI --------------------------------------------------------------------------------------------------------

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData};

/// Extra entropy: a file sealed for another program with the same Windows user does not open here by accident.
const ENTROPY: &[u8] = b"AA Replay launcher sign-in";

fn blob(b: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB { cbData: b.len() as u32, pbData: b.as_ptr() as *mut u8 }
}

fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    unsafe { LocalFree(out.pbData as _) };
    v
}

fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    let (input, entropy) = (blob(plain), blob(ENTROPY));
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe { CryptProtectData(&input, std::ptr::null(), &entropy, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) };
    if ok == 0 { Err("Windows could not protect the sign-in".into()) } else { Ok(take(out)) }
}

fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
    let (input, entropy) = (blob(sealed), blob(ENTROPY));
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe { CryptUnprotectData(&input, std::ptr::null_mut(), &entropy, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) };
    (ok != 0).then(|| take(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> Paths {
        let d = std::env::temp_dir().join(format!("aareplay-signin-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        Paths::at(d)
    }

    #[test]
    fn the_sign_in_is_kept_sealed_and_comes_back() {
        let paths = temp("keep");
        assert_eq!(load(&paths), None);
        let s = Saved { server: "https://arena.example".into(), username: "Friend".into(), session: "ab".repeat(32) };
        save(&paths, &s).unwrap();
        assert_eq!(load(&paths), Some(s.clone()));
        // the file does not hold the token or the name in the clear
        let raw = fs::read(file(&paths)).unwrap();
        let text = String::from_utf8_lossy(&raw);
        assert!(!text.contains(&s.session) && !text.contains("Friend"));
        // a damaged file is no sign-in
        let mut bad = raw.clone();
        let n = bad.len();
        bad[n - 1] ^= 0xff;
        fs::write(file(&paths), bad).unwrap();
        assert_eq!(load(&paths), None);
        forget(&paths);
        assert!(!file(&paths).exists());
        let _ = fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn only_a_well_formed_invite_is_taken() {
        let paths = temp("invite");
        fs::create_dir_all(&paths.root).unwrap();
        assert_eq!(pending_invite(&paths), None);
        fs::write(paths.root.join("invite.txt"), " abcdefghjkmnpqrstuvw \r\n").unwrap();
        assert_eq!(pending_invite(&paths).as_deref(), Some("ABCDEFGHJKMNPQRSTUVW"));
        fs::write(paths.root.join("invite.txt"), "ABCDEFGHJKMNPQRSTUV0").unwrap();
        assert_eq!(pending_invite(&paths), None);
        clear_invite(&paths);
        assert!(!paths.root.join("invite.txt").exists());
        let _ = fs::remove_dir_all(&paths.root);
    }
}
