//! The AA REPLAY art: our loading screens, login logo, loading bar, intro movie, startup splash and window icon in
//! the tester's client, installed before every play and put back after an AA Classic update.
//!
//! Where it comes from. `art.json` on the launcher's own host (next to the self-update manifest) names the art pack,
//! and is signed with the same ed25519 key as the launcher's updates (`art.json.sig`). The pack is checked by size and
//! SHA-256 against that signed manifest before a byte of it is used, and kept in `<data>\art` so it is downloaded once.
//! When the host can't be reached, the pack already kept is used (its manifest is checked again).
//!
//! The pack (made by the server's `Tools/branding/aa_replay_art.py launcher-pack`, zstd-compressed): the bytes
//! `AAART01\n`, the header length (u32 LE), the header JSON, then the blobs in order:
//!   { format: 1, items: [...], blobs: [{ sha256, size }], pak: [{ name, blob }], loader: blob,
//!     res: [{ file, original_sha256, changes: [{ type, id, lang, blob }] }] }
//!
//! What it changes, all inside our own copy (never AA Classic's folder):
//! - `<client>\aa-replay-art\overlay.bin` + `overlay.json`: the new game files, then a file table in which only those
//!   files point at them, then the header (`Pak::overlay_tail`). It is built here against this PC's own `game_pak`,
//!   which is only read: in Standard mode it is AA Classic's own file through a hard link.
//! - `binNext\version.dll`: the overlay loader. The client loads it (xldiag.dll imports VERSION.dll); it shows
//!   `game_pak` + `overlay.bin` to the game as one archive, and stays off when `game_pak`'s length is not the one in
//!   `overlay.json` (log: `aa-replay-art\overlay.log`).
//! - The splash (`res_ru.dll`, `res_us.dll`) and the window icon (`archeage.exe`): resources changed with
//!   UpdateResource, only in a file whose SHA-256 is the original the pack was made for. The original is kept in
//!   `aa-replay-art\originals` so a newer pack can be put on it again.
//! An AA Classic update makes the launcher copy `binNext` again (core::setup), which drops the loader and the
//! resources; a changed `game_pak` turns the overlay off. Both are found and redone at the next play
//! (`aa-replay-art\launcher.json` says what is in place). Art never stops a play: errors are reported and the game
//! starts with its own pictures.

use crate::core::Paths;
use crate::pak::Pak;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Where the signed art manifest is (static files on the arena box, like the self-update manifest).
pub const ART_MANIFEST: &str = "https://77-42-46-123.sslip.io/launcher/art/art.json";
const MAGIC: &[u8; 8] = b"AAART01\n";
const MAX_PACK: u64 = 256 << 20;
const LOADER: &str = "version.dll";
const MARKER: &str = "launcher.json";

/// Files in binNext the art changes or adds: core::status leaves them out when it compares two copies by name,
/// size and time.
pub fn touched(rel: &str) -> bool {
    let r = rel.replace('\\', "/").to_lowercase();
    [LOADER, "res_ru.dll", "res_us.dll", "archeage.exe"].contains(&r.as_str())
}

fn manifest_url() -> String {
    std::env::var("AAREPLAY_ART_MANIFEST").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| ART_MANIFEST.to_string())
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Release {
    pub id: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub unpacked_size: u64,
}

#[derive(Deserialize)]
struct Manifest {
    art: Release,
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Checks the signature, then reads the manifest.
pub fn read_manifest(manifest: &[u8], signature_hex: &str, key: &str) -> Result<Release, String> {
    crate::update::verify(manifest, signature_hex, key).map_err(|_| "the art manifest is not signed by the launcher's maker: ignored".to_string())?;
    let r = serde_json::from_slice::<Manifest>(manifest).map_err(|e| format!("the art manifest was not understood: {e}"))?.art;
    if !is_hex64(&r.id) || !is_hex64(&r.sha256) || r.size == 0 || r.size > MAX_PACK || r.unpacked_size > MAX_PACK || !crate::update::allowed_url(&r.url) {
        return Err("the art manifest names a download this launcher will not take".into());
    }
    Ok(r)
}

fn art_dir(paths: &Paths) -> PathBuf {
    paths.root.join("art")
}

fn cached_pack(paths: &Paths, r: &Release) -> PathBuf {
    art_dir(paths).join(format!("{}.pack.zst", r.sha256.to_lowercase()))
}

fn sha_hex(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

fn sha_file(p: &Path) -> Option<String> {
    fs::read(p).ok().map(|b| sha_hex(&b))
}

/// The pack's compressed bytes, checked against the release.
fn checked(r: &Release, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 != r.size || sha_hex(bytes) != r.sha256.to_lowercase() {
        return Err("the art pack is damaged or is not the one the manifest names".into());
    }
    Ok(())
}

/// The newest signed art, downloaded once; the one kept from before when the host can't be reached.
/// `Ok(None)`: there is no art (none published, and none kept).
pub fn fetch(paths: &Paths) -> Result<Option<(Release, Vec<u8>)>, String> {
    let url = manifest_url();
    let dir = art_dir(paths);
    let fresh = crate::update::fetch(&url, 1 << 20).and_then(|m| {
        let sig = crate::update::fetch(&format!("{url}.sig"), 1024)?;
        let sig = String::from_utf8_lossy(&sig).to_string();
        read_manifest(&m, &sig, crate::update::UPDATE_KEY).map(|r| (r, m, sig))
    });
    let r = match fresh {
        Ok((r, m, sig)) => {
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let _ = fs::write(dir.join("art.json"), &m);
            let _ = fs::write(dir.join("art.json.sig"), &sig);
            r
        }
        Err(e) => {
            // offline, or nothing published: the art kept from before, checked again
            let kept = fs::read(dir.join("art.json")).ok().zip(fs::read_to_string(dir.join("art.json.sig")).ok());
            match kept.map(|(m, s)| read_manifest(&m, &s, crate::update::UPDATE_KEY)) {
                Some(Ok(r)) => r,
                _ if e.contains("answered 404") => return Ok(None),
                _ => return Err(e),
            }
        }
    };
    let file = cached_pack(paths, &r);
    if let Ok(b) = fs::read(&file) {
        if checked(&r, &b).is_ok() {
            return Ok(Some((r, b)));
        }
    }
    let b = crate::update::fetch(&r.url, r.size)?;
    checked(&r, &b)?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(&file, &b).map_err(|e| format!("cannot keep the art pack: {e}"))?;
    // older packs are not needed any more
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p != file && p.to_string_lossy().ends_with(".pack.zst") {
                let _ = fs::remove_file(p);
            }
        }
    }
    Ok(Some((r, b)))
}

// ---------------------------------------------------------------------------------------------------------------
// The pack
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct BlobHead {
    sha256: String,
    size: u64,
}
#[derive(Deserialize)]
struct PakHead {
    name: String,
    blob: usize,
}
#[derive(Deserialize, Clone)]
pub struct Change {
    #[serde(rename = "type")]
    pub typ: u16,
    pub id: u16,
    pub lang: u16,
    pub blob: usize,
}
#[derive(Deserialize, Clone)]
pub struct Res {
    pub file: String,
    pub original_sha256: String,
    pub changes: Vec<Change>,
}
#[derive(Deserialize)]
struct Head {
    format: u32,
    #[serde(default)]
    items: Vec<String>,
    blobs: Vec<BlobHead>,
    pak: Vec<PakHead>,
    loader: usize,
    #[serde(default)]
    res: Vec<Res>,
}

pub struct Pack {
    pub id: String,
    pub items: Vec<String>,
    raw: Vec<u8>,
    blobs: Vec<(usize, usize)>,
    pub pak: Vec<(String, usize)>,
    pub loader: usize,
    pub res: Vec<Res>,
}

impl Pack {
    pub fn blob(&self, i: usize) -> &[u8] {
        let (a, n) = self.blobs[i];
        &self.raw[a..a + n]
    }
}

fn safe_bin_file(f: &str) -> bool {
    let l = f.to_lowercase();
    !f.is_empty() && f.len() < 64 && f.chars().all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c)) && !f.starts_with('.') && (l.ends_with(".dll") || l.ends_with(".exe")) && l != LOADER
}

/// Unpacks and checks a pack: every blob by its SHA-256, every index, every file name.
pub fn parse(r: &Release, packed: &[u8]) -> Result<Pack, String> {
    let not = || "this is not an art pack".to_string();
    let mut dec = ruzstd::decoding::StreamingDecoder::new(packed).map_err(|e| format!("cannot unpack the art: {e}"))?;
    let mut raw = Vec::new();
    (&mut dec).take(MAX_PACK + 1).read_to_end(&mut raw).map_err(|e| format!("cannot unpack the art: {e}"))?;
    if raw.len() as u64 > MAX_PACK || sha_hex(&raw) != r.id.to_lowercase() {
        return Err("the unpacked art is not the one the manifest names".into());
    }
    if raw.len() < 12 || &raw[..8] != MAGIC {
        return Err(not());
    }
    let len = u32::from_le_bytes(raw[8..12].try_into().unwrap()) as usize;
    let h: Head = serde_json::from_slice(raw.get(12..12 + len).ok_or_else(not)?).map_err(|_| not())?;
    if h.format != 1 {
        return Err(format!("art pack format {} is newer than this launcher: update the launcher", h.format));
    }
    let mut blobs = Vec::with_capacity(h.blobs.len());
    let mut at = 12 + len;
    for b in &h.blobs {
        let n = usize::try_from(b.size).map_err(|_| not())?;
        let data = raw.get(at..at.checked_add(n).ok_or_else(not)?).ok_or_else(not)?;
        if sha_hex(data) != b.sha256.to_lowercase() {
            return Err("a file in the art pack is damaged".into());
        }
        blobs.push((at, n));
        at += n;
    }
    if at != raw.len() {
        return Err(not());
    }
    let n = blobs.len();
    let ok_name = |s: &str| !s.is_empty() && s.len() < 0x108 && !s.contains('\0');
    if h.loader >= n || h.pak.iter().any(|p| p.blob >= n || !ok_name(&p.name)) {
        return Err(not());
    }
    if h.res.iter().any(|x| !safe_bin_file(&x.file) || !is_hex64(&x.original_sha256) || x.changes.iter().any(|c| c.blob >= n)) {
        return Err(not());
    }
    Ok(Pack { id: r.id.to_lowercase(), items: h.items, raw, blobs, pak: h.pak.into_iter().map(|p| (p.name, p.blob)).collect(), loader: h.loader, res: h.res })
}

// ---------------------------------------------------------------------------------------------------------------
// Putting it in the client
// ---------------------------------------------------------------------------------------------------------------

/// What this launcher put in the client, in `<client>\aa-replay-art\launcher.json`.
#[derive(Serialize, Deserialize, Default, Debug)]
struct Marker {
    pack: String,
    pak_length: u64,
    /// game_pak's modified time (ms): an update that keeps the length still changes it
    #[serde(default)]
    pak_modified: u64,
    overlay_length: u64,
    loader_sha256: String,
    /// binNext file -> its SHA-256 after our change
    res: BTreeMap<String, String>,
    /// binNext file -> the SHA-256 of a build the art was not made for (said once)
    #[serde(default)]
    skipped: BTreeMap<String, String>,
}

fn client_art(paths: &Paths) -> PathBuf {
    paths.client().join("aa-replay-art")
}

fn load_marker(paths: &Paths) -> Marker {
    fs::read(client_art(paths).join(MARKER)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn wide(s: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Win32 UpdateResource on `path`: each change replaces one resource (type, id, language).
fn update_resources(path: &Path, pack: &Pack, changes: &[Change]) -> Result<(), String> {
    use windows_sys::Win32::System::LibraryLoader::{BeginUpdateResourceW, EndUpdateResourceW, UpdateResourceW};
    let w = wide(path);
    unsafe {
        let h = BeginUpdateResourceW(w.as_ptr(), 0);
        if h.is_null() {
            return Err(format!("cannot open {} for its resources: {}", path.display(), std::io::Error::last_os_error()));
        }
        for c in changes {
            let data = pack.blob(c.blob);
            if UpdateResourceW(h, c.typ as usize as *const u16, c.id as usize as *const u16, c.lang, data.as_ptr().cast(), data.len() as u32) == 0 {
                let e = std::io::Error::last_os_error();
                EndUpdateResourceW(h, 1);
                return Err(format!("changing {} failed: {e}", path.display()));
            }
        }
        if EndUpdateResourceW(h, 0) == 0 {
            return Err(format!("writing the resources of {} failed: {}", path.display(), std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

/// Writes `bytes` beside `dst`, then moves it over `dst` (fails, leaving `dst` as it was, while the client runs).
fn replace_file(dst: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = dst.with_extension("art-new");
    fs::write(&tmp, bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, dst).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("cannot replace {} (is the game still open?): {e}", dst.display())
    })
}

/// Puts the pack's art in our client, or makes sure it is still there. Returns what was done, in words (`None`:
/// everything was in place already).
pub fn apply(paths: &Paths, pack: &Pack) -> Result<Option<String>, String> {
    let (bin, pak_path) = (paths.bin(), paths.pak());
    if !paths.exe().is_file() || !pak_path.is_file() {
        return Err("the game copy is not set up yet".into());
    }
    let dir = client_art(paths);
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let mut m = load_marker(paths);
    let mut did = Vec::new();
    let mut notes = Vec::new();

    // 1. the overlay, built against this PC's own game_pak
    let meta = fs::metadata(&pak_path).map_err(|e| e.to_string())?;
    let pak_len = meta.len();
    let pak_modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
    let ov = dir.join("overlay.bin");
    let ov_ok = m.pack == pack.id && m.pak_length == pak_len && m.pak_modified == pak_modified && fs::metadata(&ov).map(|x| x.len()).ok() == Some(m.overlay_length) && dir.join("overlay.json").is_file();
    if !ov_ok {
        let pak = Pak::open(&pak_path, false)?;
        let files: Vec<(&str, &[u8])> = pack.pak.iter().filter(|(n, _)| pak.get(n).is_some()).map(|(n, b)| (n.as_str(), pack.blob(*b))).collect();
        if files.len() < pack.pak.len() {
            notes.push(format!("{} files of the art are not in this game's data and were left out", pack.pak.len() - files.len()));
        }
        let tmp = dir.join("overlay.bin.new");
        let n = {
            let f = fs::File::create(&tmp).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
            let mut w = std::io::BufWriter::new(f);
            let n = pak.overlay_tail(&files, &mut w)?;
            let f = w.into_inner().map_err(|e| e.to_string())?;
            f.sync_all().map_err(|e| e.to_string())?;
            n
        };
        // the loader reads pak_length; the rest is for people
        let names: Vec<&str> = files.iter().map(|(n, _)| *n).collect();
        let json = serde_json::json!({ "pak_length": pak_len, "total_length": pak_len + n, "items": pack.items, "files": names, "made_by": "aa-replay-launcher", "pack": pack.id });
        let _ = fs::remove_file(dir.join("overlay.json")); // the loader stays off while the two don't match
        fs::rename(&tmp, &ov).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("cannot replace {} (is the game still open?): {e}", ov.display())
        })?;
        fs::write(dir.join("overlay.json"), serde_json::to_vec_pretty(&json).unwrap()).map_err(|e| e.to_string())?;
        m.pak_length = pak_len;
        m.pak_modified = pak_modified;
        m.overlay_length = n;
        did.push(format!("{} game pictures", files.len()));
    }
    m.pack = pack.id.clone();

    // 2. the loader
    let loader = pack.blob(pack.loader);
    let want = sha_hex(loader);
    let dll = bin.join(LOADER);
    match sha_file(&dll) {
        Some(s) if s == want => {}
        Some(s) if s != m.loader_sha256 => notes.push(format!("{} is not ours and was left alone: the art stays off", dll.display())),
        _ => {
            replace_file(&dll, loader)?;
            did.push("the art loader".into());
        }
    }
    m.loader_sha256 = want;

    // 3. the splash and the window icon
    let keep = dir.join("originals");
    for r in &pack.res {
        let path = bin.join(&r.file);
        let Some(now) = sha_file(&path) else { continue };
        let ours = m.res.get(&r.file).is_some_and(|s| *s == now);
        let base = if now == r.original_sha256.to_lowercase() {
            let b = fs::read(&path).map_err(|e| e.to_string())?;
            fs::create_dir_all(&keep).map_err(|e| e.to_string())?;
            fs::write(keep.join(&r.file), &b).map_err(|e| e.to_string())?;
            b
        } else if ours && m.pack == pack.id && load_marker(paths).pack == pack.id {
            continue; // in place already
        } else if ours && sha_file(&keep.join(&r.file)).as_deref() == Some(&r.original_sha256.to_lowercase()) {
            fs::read(keep.join(&r.file)).map_err(|e| e.to_string())? // an older pack's change: redone from the original
        } else {
            if m.skipped.get(&r.file) != Some(&now) {
                notes.push(format!("binNext\\{} is another build than the art was made for: left as it is", r.file));
                m.skipped.insert(r.file.clone(), now);
            }
            continue;
        };
        let tmp = path.with_extension("art-res");
        fs::write(&tmp, &base).map_err(|e| e.to_string())?;
        if let Err(e) = update_resources(&tmp, pack, &r.changes) {
            let _ = fs::remove_file(&tmp);
            notes.push(e);
            continue;
        }
        let new = fs::read(&tmp).map_err(|e| e.to_string())?;
        let _ = fs::remove_file(&tmp);
        replace_file(&path, &new)?;
        m.res.insert(r.file.clone(), sha_hex(&new));
        did.push(format!("binNext\\{}", r.file));
    }

    fs::write(dir.join(MARKER), serde_json::to_vec_pretty(&m).unwrap()).map_err(|e| e.to_string())?;
    let mut line = if did.is_empty() { String::new() } else { format!("AA REPLAY art put in place: {}", did.join(", ")) };
    for n in notes {
        line.push_str(if line.is_empty() { "AA REPLAY art: " } else { "; " });
        line.push_str(&n);
    }
    Ok((!line.is_empty()).then_some(line))
}

/// Before a play: the newest art, put in place. Never stops a play; the caller shows the line it returns.
pub fn ensure(paths: &Paths) -> Option<String> {
    if std::env::var("AAREPLAY_ART").is_ok_and(|v| v.eq_ignore_ascii_case("off")) {
        return None;
    }
    let r = fetch(paths).and_then(|got| match got {
        None => Ok(None),
        Some((rel, bytes)) => apply(paths, &parse(&rel, &bytes)?),
    });
    let line = match r {
        Ok(l) => l,
        Err(e) => Some(format!("AA REPLAY art not put in place ({e}); the game starts with its own pictures")),
    };
    if let Some(l) = &line {
        crate::core::log_line(paths, l);
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aac-art-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn key() -> (SigningKey, String) {
        let k = SigningKey::from_bytes(&[5u8; 32]);
        let public = hex::encode(k.verifying_key().to_bytes());
        (k, public)
    }

    /// A pack as aa_replay_art.py launcher-pack makes it, with its release.
    fn pack(pak: &[(&str, &[u8])], loader: &[u8], res: Vec<serde_json::Value>, extra: &[&[u8]]) -> (Release, Vec<u8>) {
        let mut blobs: Vec<&[u8]> = Vec::new();
        let mut pak_h = Vec::new();
        for (n, d) in pak {
            pak_h.push(serde_json::json!({ "name": n, "blob": blobs.len() }));
            blobs.push(d);
        }
        let li = blobs.len();
        blobs.push(loader);
        blobs.extend_from_slice(extra);
        let head = serde_json::json!({ "format": 1, "items": ["x"], "blobs": blobs.iter().map(|b| serde_json::json!({ "sha256": sha_hex(b), "size": b.len() })).collect::<Vec<_>>(), "pak": pak_h, "loader": li, "res": res });
        let hj = serde_json::to_vec(&head).unwrap();
        let mut raw = MAGIC.to_vec();
        raw.extend_from_slice(&(hj.len() as u32).to_le_bytes());
        raw.extend_from_slice(&hj);
        for b in &blobs {
            raw.extend_from_slice(b);
        }
        let packed = ruzstd::encoding::compress_to_vec(&raw[..], ruzstd::encoding::CompressionLevel::Fastest);
        let r = Release { id: sha_hex(&raw), url: "https://example.invalid/a.pack.zst".into(), size: packed.len() as u64, sha256: sha_hex(&packed), unpacked_size: raw.len() as u64 };
        (r, packed)
    }

    fn client(name: &str) -> Paths {
        let root = tmp(name);
        let paths = Paths::at(&root);
        let bin = paths.bin();
        fs::create_dir_all(&bin).unwrap();
        fs::write(paths.exe(), b"exe").unwrap();
        let mut p = Pak::create(&paths.pak()).unwrap();
        let like = crate::pak::Entry::default();
        for (n, d) in [("game/ui/a.dds", &b"old a"[..]), ("game/ui/b.dds", &b"old b"[..])] {
            p.put(n, &mut &d[..], d.len() as u64, None, &like).unwrap();
        }
        p.save().unwrap();
        paths
    }

    #[test]
    fn only_a_manifest_signed_with_the_key_is_taken() {
        let (k, public) = key();
        let m = br#"{"art":{"id":"0000000000000000000000000000000000000000000000000000000000000000","url":"https://h/a.zst","size":5,"sha256":"1111111111111111111111111111111111111111111111111111111111111111"}}"#;
        let sig = hex::encode(k.sign(m).to_bytes());
        assert_eq!(read_manifest(m, &sig, &public).unwrap().size, 5);
        let mut changed = m.to_vec();
        let at = changed.len() - 3;
        changed[at] = b'2';
        assert!(read_manifest(&changed, &sig, &public).is_err());
        let other = hex::encode(SigningKey::from_bytes(&[6u8; 32]).sign(m).to_bytes());
        assert!(read_manifest(m, &other, &public).is_err());
        let plain = br#"{"art":{"id":"0000000000000000000000000000000000000000000000000000000000000000","url":"http://h/a.zst","size":5,"sha256":"1111111111111111111111111111111111111111111111111111111111111111"}}"#;
        assert!(read_manifest(plain, &hex::encode(k.sign(plain).to_bytes()), &public).is_err(), "plain http is refused");
    }

    #[test]
    fn a_damaged_pack_is_refused() {
        let (r, mut packed) = pack(&[("game/ui/a.dds", b"new a")], b"loader", vec![], &[]);
        assert!(parse(&r, &packed).is_ok());
        assert!(checked(&r, &packed).is_ok());
        let last = packed.len() - 1;
        packed[last] ^= 1;
        assert!(checked(&r, &packed).is_err());
        let bad = pack(&[("x", b"y")], b"l", vec![serde_json::json!({ "file": "..\\evil.dll", "original_sha256": "0".repeat(64), "changes": [] })], &[]);
        assert!(parse(&bad.0, &bad.1).is_err(), "a binNext name that reaches outside is refused");
    }

    #[test]
    fn puts_the_art_in_and_redoes_it_after_an_update() {
        let paths = client("apply");
        let (r, packed) = pack(&[("game/ui/a.dds", b"new a!"), ("game/ui/not-in-this-build.dds", b"z")], b"the loader", vec![], &[]);
        let p = parse(&r, &packed).unwrap();
        let line = apply(&paths, &p).unwrap().unwrap();
        assert!(line.contains("1 game pictures") && line.contains("left out"), "{line}");
        assert_eq!(fs::read(paths.bin().join(LOADER)).unwrap(), b"the loader");
        // the client sees game_pak + overlay.bin as one archive
        let joined = paths.root.join("joined");
        fs::write(&joined, [fs::read(paths.pak()).unwrap(), fs::read(client_art(&paths).join("overlay.bin")).unwrap()].concat()).unwrap();
        let j = Pak::open(&joined, false).unwrap();
        assert_eq!(j.read(j.get("game/ui/a.dds").unwrap(), 100).unwrap(), b"new a!");
        assert_eq!(j.read(j.get("game/ui/b.dds").unwrap(), 100).unwrap(), b"old b");
        let ov: serde_json::Value = serde_json::from_slice(&fs::read(client_art(&paths).join("overlay.json")).unwrap()).unwrap();
        assert_eq!(ov["pak_length"], fs::metadata(paths.pak()).unwrap().len());
        // a second play: nothing to do
        assert_eq!(apply(&paths, &p).unwrap(), None);
        // an AA Classic update: binNext copied again (no loader), game_pak changed
        fs::remove_file(paths.bin().join(LOADER)).unwrap();
        let mut pk = Pak::open(&paths.pak(), true).unwrap();
        pk.put("game/ui/b.dds", &mut &b"patched b, longer"[..], 17, None, &crate::pak::Entry::default()).unwrap();
        pk.save().unwrap();
        drop(pk);
        let line = apply(&paths, &p).unwrap().unwrap();
        assert!(line.contains("game pictures") && line.contains("loader"), "{line}");
        let ov: serde_json::Value = serde_json::from_slice(&fs::read(client_art(&paths).join("overlay.json")).unwrap()).unwrap();
        assert_eq!(ov["pak_length"], fs::metadata(paths.pak()).unwrap().len());
        let joined = paths.root.join("joined2");
        fs::write(&joined, [fs::read(paths.pak()).unwrap(), fs::read(client_art(&paths).join("overlay.bin")).unwrap()].concat()).unwrap();
        let j = Pak::open(&joined, false).unwrap();
        assert_eq!(j.read(j.get("game/ui/a.dds").unwrap(), 100).unwrap(), b"new a!");
        assert_eq!(j.read(j.get("game/ui/b.dds").unwrap(), 100).unwrap(), b"patched b, longer");
    }

    #[test]
    fn a_loader_that_is_not_ours_is_left_alone() {
        let paths = client("foreign");
        fs::write(paths.bin().join(LOADER), b"someone else's").unwrap();
        let (r, packed) = pack(&[("game/ui/a.dds", b"new")], b"the loader", vec![], &[]);
        let line = apply(&paths, &parse(&r, &packed).unwrap()).unwrap().unwrap();
        assert!(line.contains("not ours"), "{line}");
        assert_eq!(fs::read(paths.bin().join(LOADER)).unwrap(), b"someone else's");
    }

    #[test]
    fn resources_change_only_in_the_original_build() {
        // a real PE to change: a copy of this test program
        let paths = client("res");
        let me = fs::read(std::env::current_exe().unwrap()).unwrap();
        fs::write(paths.bin().join("res_ru.dll"), &me).unwrap();
        fs::write(paths.bin().join("res_us.dll"), b"another build").unwrap();
        let bmp: &[u8] = b"new splash bytes";
        let res = vec![
            serde_json::json!({ "file": "res_ru.dll", "original_sha256": sha_hex(&me), "changes": [{ "type": 2, "id": 140, "lang": 1033, "blob": 2 }] }),
            serde_json::json!({ "file": "res_us.dll", "original_sha256": sha_hex(&me), "changes": [{ "type": 2, "id": 140, "lang": 1033, "blob": 2 }] }),
        ];
        let (r, packed) = pack(&[("game/ui/a.dds", b"new")], b"loader", res, &[bmp]);
        let p = parse(&r, &packed).unwrap();
        let line = apply(&paths, &p).unwrap().unwrap();
        assert!(line.contains("binNext\\res_ru.dll") && line.contains("res_us.dll is another build"), "{line}");
        let changed = fs::read(paths.bin().join("res_ru.dll")).unwrap();
        assert_ne!(changed, me);
        assert!(changed.windows(bmp.len()).any(|w| w == bmp), "the new resource is in the file");
        assert_eq!(fs::read(paths.bin().join("res_us.dll")).unwrap(), b"another build");
        assert_eq!(fs::read(client_art(&paths).join("originals/res_ru.dll")).unwrap(), me);
        assert_eq!(apply(&paths, &p).unwrap(), None, "in place already");
    }
}
