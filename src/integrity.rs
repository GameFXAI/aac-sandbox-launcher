//! A check that the installed launcher is still the build that was installed: its SHA-256 is recorded when it is put
//! in place (an update records the signed release's hash; the first start of a new version records its own) and
//! compared at every start. A difference says "this install looks modified — please reinstall" instead of carrying
//! on silently.
//!
//! - `<data>\installed.json`  `{ "exe": "...", "version": "0.3.1", "sha256": "..." }`
//!
//! This protects the player from a damaged or swapped launcher; it is not what keeps the game honest. The server is
//! the only trust boundary: nothing the launcher says about plan, identity or recordings is believed there.
//!
//! Only a record for this exe path and this version counts: a new version (an update, or the setup run again over an
//! older install) records itself, and a developer's debug build is never checked.

use crate::core::Paths;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub const MODIFIED: &str = "Please reinstall AA Replay from the download page: this launcher is not the build that was installed here.";

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct Record {
    exe: String,
    version: String,
    sha256: String,
}

fn record_file(paths: &Paths) -> PathBuf {
    paths.root.join("installed.json")
}

fn same_exe(a: &str, b: &Path) -> bool {
    a.eq_ignore_ascii_case(&b.to_string_lossy())
}

pub fn sha256_of(file: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut f = fs::File::open(file).map_err(|e| e.to_string())?;
    let (mut h, mut buf) = (Sha256::new(), vec![0u8; 1 << 16]);
    loop {
        match f.read(&mut buf).map_err(|e| e.to_string())? {
            0 => break,
            k => h.update(&buf[..k]),
        }
    }
    Ok(hex::encode(h.finalize()))
}

/// Records what was installed at `exe`: the update's signed hash, or the running build's own on its first start.
pub fn record(paths: &Paths, exe: &Path, version: &str, sha256: &str) -> Result<(), String> {
    let r = Record { exe: exe.to_string_lossy().into_owned(), version: version.into(), sha256: sha256.to_lowercase() };
    let _ = fs::create_dir_all(&paths.root);
    fs::write(record_file(paths), serde_json::to_vec_pretty(&r).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// The check behind the startup one, for any exe, version and hash: Some(message) only when this exe and version
/// were recorded with another hash. No record, another exe or another version records this one and says nothing.
fn check_with(paths: &Paths, exe: &Path, version: &str, actual: &str) -> Option<&'static str> {
    let rec: Option<Record> = fs::read(record_file(paths)).ok().and_then(|b| serde_json::from_slice(&b).ok());
    match rec {
        Some(r) if same_exe(&r.exe, exe) && r.version == version => (!r.sha256.eq_ignore_ascii_case(actual)).then_some(MODIFIED),
        _ => {
            let _ = record(paths, exe, version, actual);
            None
        }
    }
}

/// At startup: Some(message) when the running exe is not the build that was installed here. Never in a debug build
/// or when the exe cannot be read (nothing to compare is not a reason to alarm anyone).
pub fn check(paths: &Paths) -> Option<&'static str> {
    if cfg!(debug_assertions) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let actual = sha256_of(&exe).ok()?;
    check_with(paths, &exe, crate::update::VERSION, &actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> Paths {
        let d = std::env::temp_dir().join(format!("aaemu-integrity-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        Paths::at(d)
    }

    #[test]
    fn the_first_start_records_and_the_same_build_passes() {
        let p = tmp("first");
        let exe = Path::new(r"C:\Users\x\AppData\Local\Programs\AAReplay\aac-sandbox-launcher.exe");
        assert_eq!(check_with(&p, exe, "0.3.1", "AB12"), None);
        assert_eq!(check_with(&p, exe, "0.3.1", "ab12"), None);
    }

    #[test]
    fn a_changed_build_of_the_same_version_is_said() {
        let p = tmp("changed");
        let exe = Path::new(r"C:\a\launcher.exe");
        record(&p, exe, "0.3.1", "aa").unwrap();
        assert_eq!(check_with(&p, exe, "0.3.1", "bb"), Some(MODIFIED));
        // and it stays said: the record is not overwritten by the changed build
        assert_eq!(check_with(&p, exe, "0.3.1", "bb"), Some(MODIFIED));
    }

    #[test]
    fn a_new_version_or_another_place_records_itself() {
        let p = tmp("newer");
        let exe = Path::new(r"C:\a\launcher.exe");
        record(&p, exe, "0.3.1", "aa").unwrap();
        // the setup run again over an older install
        assert_eq!(check_with(&p, exe, "0.3.2", "bb"), None);
        assert_eq!(check_with(&p, exe, "0.3.2", "bb"), None);
        // a copy somewhere else (a developer's, a second install)
        assert_eq!(check_with(&p, Path::new(r"D:\other\launcher.exe"), "0.3.2", "cc"), None);
    }

    #[test]
    fn an_update_records_the_signed_hash() {
        let p = tmp("update");
        let exe = Path::new(r"C:\a\launcher.exe");
        record(&p, exe, "0.4.0", "CAFE").unwrap();
        assert_eq!(check_with(&p, exe, "0.4.0", "cafe"), None);
        assert_eq!(check_with(&p, exe, "0.4.0", "beef"), Some(MODIFIED));
    }

    #[test]
    fn sha256_of_a_file() {
        let p = tmp("sha");
        fs::create_dir_all(&p.root).unwrap();
        let f = p.root.join("f.bin");
        fs::write(&f, b"abc").unwrap();
        assert_eq!(sha256_of(&f).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
