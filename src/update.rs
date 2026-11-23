//! The launcher updating itself from a signed manifest.
//!
//! The manifest is a small JSON file on a static file host, with its ed25519 signature next to it
//! (`<manifest url>.sig`, 128 hex characters). The public key is part of this program (`UPDATE_KEY`), so only who
//! holds the private key can offer an update, whatever host the files are on:
//!
//!   { "launcher": { "version": "0.2.0", "url": "https://.../aac-sandbox-launcher.exe", "size": 9285632,
//!                   "sha256": "..." } }
//!
//! Rules: the signature is checked before the manifest is read; only a higher version is taken (an old, validly
//! signed manifest cannot bring an old launcher back); the new exe is checked by size and SHA-256 before it replaces
//! this one; downloads are HTTPS only (plain HTTP to this PC alone, for tests) and redirects are not followed.
//!
//! Self-update is off until a manifest address is set: `MANIFEST_URL` below once the files have a home, or
//! `manifest_url` in launcher.json. Signing is done with `aac-sandbox-launcher manifest keygen|sign` (cli.rs).

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The public key updates must be signed with (hex). The private key is the owner's and is in no repository.
pub const UPDATE_KEY: &str = "fef8a3544a2b38be22877cf633804ed61c614aacd6d9ea32bd5dd0ca317b70cc";
/// Where the manifest is (static files on the arena box, served by Caddy without redirects).
pub const MANIFEST_URL: &str = "https://77-42-46-123.sslip.io/launcher/manifest.json";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const MAX_MANIFEST: u64 = 1 << 20;
const MAX_EXE: u64 = 256 << 20;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Deserialize)]
struct Manifest {
    launcher: Release,
}

/// "1.2.3" as numbers; None for anything else.
fn version(v: &str) -> Option<[u32; 3]> {
    let mut parts = v.trim().split('.');
    let out = [parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?];
    parts.next().is_none().then_some(out)
}

fn key_from_hex(key: &str) -> Result<VerifyingKey, String> {
    let bytes: [u8; 32] = hex::decode(key).ok().and_then(|b| b.try_into().ok()).ok_or("this launcher has no update key")?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| "this launcher's update key is not valid".to_string())
}

/// Checks that `bytes` carry the maker's ed25519 signature (`signature_hex`, 128 hex characters) under `key`.
/// The art pack's manifest (art.rs) is signed with the same key as the launcher's own updates.
pub fn verify(bytes: &[u8], signature_hex: &str, key: &str) -> Result<(), String> {
    let sig: [u8; 64] = hex::decode(signature_hex.trim()).ok().and_then(|b| b.try_into().ok()).ok_or("the update's signature is not in the expected form")?;
    key_from_hex(key)?.verify(bytes, &Signature::from_bytes(&sig)).map_err(|_| "the update is not signed by this launcher's maker: ignored".to_string())
}

/// Checks the signature, then reads the manifest. Returns the release when it is newer than `current`.
pub fn read_manifest(manifest: &[u8], signature_hex: &str, key: &str, current: &str) -> Result<Option<Release>, String> {
    verify(manifest, signature_hex, key)?;
    let m: Manifest = serde_json::from_slice(manifest).map_err(|e| format!("the update manifest was not understood: {e}"))?;
    let r = m.launcher;
    let (new, now) = (version(&r.version).ok_or("the update manifest has no usable version")?, version(current).ok_or("this launcher has no usable version")?);
    if new <= now {
        return Ok(None);
    }
    let sha_ok = r.sha256.len() == 64 && r.sha256.chars().all(|c| c.is_ascii_hexdigit());
    if !sha_ok || r.size == 0 || r.size > MAX_EXE || !allowed_url(&r.url) {
        return Err("the update manifest names a download this launcher will not take".into());
    }
    Ok(Some(r))
}

/// HTTPS, or plain HTTP to this PC.
pub(crate) fn allowed_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://") || ["http://127.0.0.1:", "http://127.0.0.1/", "http://localhost:", "http://localhost/"].iter().any(|p| lower.starts_with(p))
}

pub(crate) fn fetch(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    if !allowed_url(url) {
        return Err("updates are only fetched over https".into());
    }
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(60)).redirects(0).user_agent("aaemu-sandbox-launcher").build();
    let r = agent.get(url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("the update host answered {code}"),
        e => format!("the update host could not be reached: {e}"),
    })?;
    let mut out = Vec::new();
    r.into_reader().take(limit + 1).read_to_end(&mut out).map_err(|e| format!("the download broke off: {e}"))?;
    if out.len() as u64 > limit {
        return Err("the download is larger than it can be".into());
    }
    Ok(out)
}

/// Asks the manifest whether there is a newer launcher. `Ok(None)`: up to date, or self-update is off.
pub fn check(manifest_url: &str) -> Result<Option<Release>, String> {
    if manifest_url.is_empty() {
        return Ok(None);
    }
    let manifest = fetch(manifest_url, MAX_MANIFEST)?;
    let sig = fetch(&format!("{manifest_url}.sig"), 1024)?;
    read_manifest(&manifest, &String::from_utf8_lossy(&sig), UPDATE_KEY, VERSION)
}

/// Checks the downloaded exe against the release and puts it in the place of `exe`. The running exe cannot be
/// overwritten but can be renamed: it becomes `<exe>.old` and is removed at the next start (`clean_up`).
pub fn install(r: &Release, bytes: &[u8], exe: &Path) -> Result<(), String> {
    if bytes.len() as u64 != r.size || hex::encode(Sha256::digest(bytes)) != r.sha256.to_lowercase() {
        return Err("the downloaded update is damaged or is not the one the manifest names".into());
    }
    let (new, old) = (with_suffix(exe, "new"), with_suffix(exe, "old"));
    let io = |e: std::io::Error| format!("cannot replace the launcher: {e}");
    fs::write(&new, bytes).map_err(io)?;
    strip_mark_of_the_web(&new);
    let _ = fs::remove_file(&old);
    fs::rename(exe, &old).map_err(io)?;
    if let Err(e) = fs::rename(&new, exe) {
        let _ = fs::rename(&old, exe); // put the running one back
        return Err(io(e));
    }
    Ok(())
}

/// Downloads and installs a release over the running launcher. The caller starts the new exe and exits.
pub fn apply(r: &Release) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    install(r, &fetch(&r.url, r.size)?, &exe)?;
    Ok(exe)
}

fn with_suffix(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{suffix}"));
    exe.with_file_name(name)
}

/// Takes the "downloaded from the internet" mark (the `Zone.Identifier` stream) off a file this launcher put in
/// place, so Windows SmartScreen never asks about it. The launcher's own downloads never get one (only browsers and
/// Explorer add it), but a launcher taken out of a downloaded zip carries it, and so would what it installs.
pub fn strip_mark_of_the_web(file: &Path) {
    let mut stream = file.as_os_str().to_os_string();
    stream.push(":Zone.Identifier");
    let w: Vec<u16> = std::os::windows::ffi::OsStrExt::encode_wide(stream.as_os_str()).chain(Some(0)).collect();
    unsafe { windows_sys::Win32::Storage::FileSystem::DeleteFileW(w.as_ptr()) };
}

/// Whether a file carries the mark.
#[cfg(test)]
pub fn has_mark_of_the_web(file: &Path) -> bool {
    let mut stream = file.as_os_str().to_os_string();
    stream.push(":Zone.Identifier");
    fs::metadata(PathBuf::from(stream)).is_ok()
}

/// Removes what an earlier update left behind, and the download mark from this exe.
pub fn clean_up() {
    if let Ok(exe) = std::env::current_exe() {
        strip_mark_of_the_web(&exe);
        let _ = fs::remove_file(with_suffix(&exe, "old"));
        let _ = fs::remove_file(with_suffix(&exe, "new"));
    }
}

// --- for the owner: making keys and signing a manifest -------------------------------------------------------

/// A new key pair as (private hex, public hex).
pub fn keygen() -> Result<(String, String), String> {
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| format!("no random numbers: {e}"))?;
    let key = SigningKey::from_bytes(&seed);
    Ok((hex::encode(seed), hex::encode(key.verifying_key().to_bytes())))
}

/// The signature of `manifest` (hex) with a private key from `keygen`.
pub fn sign(manifest: &[u8], private_hex: &str) -> Result<String, String> {
    let seed: [u8; 32] = hex::decode(private_hex.trim()).ok().and_then(|b| b.try_into().ok()).ok_or("that is not a private key made by keygen")?;
    Ok(hex::encode(SigningKey::from_bytes(&seed).sign(manifest).to_bytes()))
}

/// The manifest text for an exe file.
pub fn manifest_for(exe: &Path, version: &str, url: &str) -> Result<String, String> {
    let bytes = fs::read(exe).map_err(|e| format!("cannot read {}: {e}", exe.display()))?;
    if self::version(version).is_none() {
        return Err("the version is three numbers, like 0.2.0".into());
    }
    let m = serde_json::json!({ "launcher": { "version": version, "url": url, "size": bytes.len(), "sha256": hex::encode(Sha256::digest(&bytes)) } });
    Ok(serde_json::to_string_pretty(&m).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> (String, String) {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        (hex::encode([7u8; 32]), hex::encode(key.verifying_key().to_bytes()))
    }

    fn manifest(version: &str, url: &str, body: &[u8]) -> Vec<u8> {
        serde_json::json!({ "launcher": { "version": version, "url": url, "size": body.len(), "sha256": hex::encode(Sha256::digest(body)) } }).to_string().into_bytes()
    }

    #[test]
    fn a_signed_newer_release_is_taken() {
        let (private, public) = keys();
        let m = manifest("0.2.0", "https://example.org/l.exe", b"new exe");
        let sig = sign(&m, &private).unwrap();
        let r = read_manifest(&m, &sig, &public, "0.1.9").unwrap().unwrap();
        assert_eq!((r.version.as_str(), r.size), ("0.2.0", 7));
        // with a line break after the signature, as a text file has
        assert!(read_manifest(&m, &format!("{sig}\r\n"), &public, "0.1.9").unwrap().is_some());
    }

    #[test]
    fn same_or_older_versions_are_not_updates() {
        let (private, public) = keys();
        for (offered, current) in [("0.2.0", "0.2.0"), ("0.1.9", "0.2.0"), ("0.9.9", "1.0.0"), ("0.2.0", "0.10.0")] {
            let m = manifest(offered, "https://example.org/l.exe", b"x");
            assert_eq!(read_manifest(&m, &sign(&m, &private).unwrap(), &public, current).unwrap(), None, "{offered} over {current}");
        }
    }

    #[test]
    fn an_unsigned_or_changed_manifest_is_ignored() {
        let (private, public) = keys();
        let m = manifest("9.0.0", "https://example.org/l.exe", b"x");
        let sig = sign(&m, &private).unwrap();
        // one byte of the manifest changed after signing
        let mut changed = m.clone();
        let at = changed.iter().position(|&b| b == b'9').unwrap();
        changed[at] = b'8';
        assert!(read_manifest(&changed, &sig, &public, "0.1.0").is_err_and(|e| e.contains("not signed")));
        // signed with another key
        let other = hex::encode([9u8; 32]);
        assert!(read_manifest(&m, &sign(&m, &other).unwrap(), &public, "0.1.0").is_err_and(|e| e.contains("not signed")));
        // no signature, a short one, and a launcher without a key
        assert!(read_manifest(&m, "", &public, "0.1.0").is_err());
        assert!(read_manifest(&m, &sig[..100], &public, "0.1.0").is_err());
        assert!(read_manifest(&m, &sig, "PUBLIC-KEY-GOES-HERE", "0.1.0").is_err_and(|e| e.contains("no update key")));
    }

    #[test]
    fn a_signed_manifest_with_a_bad_download_is_refused() {
        let (private, public) = keys();
        for url in ["http://example.org/l.exe", "ftp://example.org/l.exe", "file:///c:/x.exe", ""] {
            let m = manifest("9.0.0", url, b"x");
            assert!(read_manifest(&m, &sign(&m, &private).unwrap(), &public, "0.1.0").is_err(), "{url}");
        }
        let m = manifest("9.0.0", "http://127.0.0.1:8080/l.exe", b"x");
        assert!(read_manifest(&m, &sign(&m, &private).unwrap(), &public, "0.1.0").unwrap().is_some());
        let m = br#"{"launcher":{"version":"nine","url":"https://e.org/l.exe","size":1,"sha256":"00"}}"#;
        assert!(read_manifest(m, &sign(m, &private).unwrap(), &public, "0.1.0").is_err());
    }

    #[test]
    fn install_swaps_the_exe_only_for_the_right_bytes() {
        let d = std::env::temp_dir().join(format!("aaemu-update-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let exe = d.join("launcher.exe");
        fs::write(&exe, b"old").unwrap();
        let r = Release { version: "0.2.0".into(), url: "https://e.org/l.exe".into(), size: 3, sha256: hex::encode(Sha256::digest(b"new")) };
        assert!(install(&r, b"bad", &exe).is_err());
        assert!(install(&r, b"new!", &exe).is_err());
        assert_eq!(fs::read(&exe).unwrap(), b"old");
        install(&r, b"new", &exe).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new");
        assert_eq!(fs::read(d.join("launcher.exe.old")).unwrap(), b"old");
        assert!(!d.join("launcher.exe.new").exists());
    }

    #[test]
    fn what_the_launcher_installs_never_carries_the_download_mark() {
        let d = std::env::temp_dir().join(format!("aar-motw-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let exe = d.join("launcher.exe");
        // a launcher taken out of a downloaded zip: Explorer gave it the mark
        fs::write(&exe, b"old").unwrap();
        let mut ads = exe.as_os_str().to_os_string();
        ads.push(":Zone.Identifier");
        fs::write(PathBuf::from(&ads), "[ZoneTransfer]\r\nZoneId=3\r\n").unwrap();
        assert!(has_mark_of_the_web(&exe));
        strip_mark_of_the_web(&exe);
        assert!(!has_mark_of_the_web(&exe));
        assert_eq!(fs::read(&exe).unwrap(), b"old", "only the mark goes, not the file");
        // an update over a marked exe: the new exe has none
        fs::write(PathBuf::from(&ads), "[ZoneTransfer]\r\nZoneId=3\r\n").unwrap();
        let body = b"new launcher";
        let r = Release { version: "9.9.9".into(), url: "https://h/x.exe".into(), size: body.len() as u64, sha256: hex::encode(Sha256::digest(body)) };
        install(&r, body, &exe).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), body);
        assert!(!has_mark_of_the_web(&exe));
        // a file with no mark: nothing happens
        strip_mark_of_the_web(&exe);
        assert!(exe.is_file());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_real_download_carries_no_mark() {
        // the launcher's own HTTP client (a local server: no network needed)
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            use std::io::Write;
            let (mut c, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut c, &mut buf);
            c.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello").unwrap();
        });
        let bytes = fetch(&format!("http://127.0.0.1:{port}/x.exe"), 100).unwrap();
        t.join().unwrap();
        let d = std::env::temp_dir().join(format!("aar-motw-dl-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let exe = d.join("launcher.exe");
        fs::write(&exe, b"old").unwrap();
        let r = Release { version: "9.9.9".into(), url: "https://h/x.exe".into(), size: 5, sha256: hex::encode(Sha256::digest(&bytes)) };
        install(&r, &bytes, &exe).unwrap();
        assert!(!has_mark_of_the_web(&exe));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn keygen_sign_and_manifest_for_work_together() {
        let (private, public) = keygen().unwrap();
        assert_eq!((private.len(), public.len()), (64, 64));
        let d = std::env::temp_dir().join(format!("aaemu-update-test2-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("l.exe"), b"exe bytes").unwrap();
        let m = manifest_for(&d.join("l.exe"), "1.2.3", "https://example.org/l.exe").unwrap();
        let r = read_manifest(m.as_bytes(), &sign(m.as_bytes(), &private).unwrap(), &public, "1.2.2").unwrap().unwrap();
        assert_eq!(r.size, 9);
        assert!(manifest_for(&d.join("l.exe"), "1.2", "https://example.org/l.exe").is_err());
    }
}
