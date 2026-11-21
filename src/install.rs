//! "Install game" for a PC without AA Classic: fetches the game data from AA Classic's own public file host into
//! the launcher's folder. Nothing is hosted by us and no AA Classic account is used or asked for.
//!
//! What it fetches (see Docs/launcher-install-source.md in the server repo):
//! - the install manifest, then the base `game_pak` as 64 MiB zstd chunks, each checked by length and SHA-256 and
//!   written at its offset into `client\game_pak.tmp`; the chunks done are remembered, so a stopped download goes on
//!   where it was;
//! - the patch files from the base version up to the version the server wants, into `client\patch\`.
//!
//! What it does not do yet: merging the patch files into `game_pak` (the last step before the game can start), and
//! `binNext`, which needs an AA Classic login and so comes from a build pack (vault.rs).
//!
//! Rules kept here: only the manifest, chunk and patch addresses below are ever requested (never a login, gateway,
//! guard, crash or telemetry address); requests carry no cookies and no browser-like User-Agent; redirects are not
//! followed; every file written is inside the launcher's folder.

use crate::core::Paths;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

/// Where the files come from. The defaults are AA Classic's public file host; tests point it at a local server.
#[derive(Clone, Debug)]
pub struct Source {
    pub manifest_url: String,
    /// chunk i is `{parts}/{sha[0..2]}/{sha}.zst`
    pub parts: String,
    /// patch v is `{patch}{v}`
    pub patch: String,
}

impl Default for Source {
    fn default() -> Self {
        Source {
            manifest_url: "https://patch.aa-classic.com/installer/manifest.v2.json".into(),
            parts: "https://patch.aa-classic.com/installer/parts".into(),
            patch: "https://patch.aa-classic.com/pak".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Chunk {
    pub offset: u64,
    pub size: u64,
    pub csize: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub version: u32,
    pub compression: String,
    pub uncompressed_size: u64,
    pub chunks: Vec<Chunk>,
}

const MAX_CHUNK: u64 = 256 << 20;
const MAX_MANIFEST: u64 = 16 << 20;
const MAX_PATCH: u64 = 4 << 30;
const TRIES: u32 = 5;

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

impl Manifest {
    /// Parses and checks a manifest: only the layout we know, chunks in order without gaps, sane sizes.
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let m: Manifest = serde_json::from_str(text).map_err(|e| format!("the install manifest was not understood: {e}"))?;
        if m.schema != 2 {
            return Err(format!("the install manifest has a layout this launcher does not know (schema {})", m.schema));
        }
        if m.compression != "zstd" {
            return Err(format!("the install manifest uses a compression this launcher does not know ({})", m.compression));
        }
        if m.chunks.is_empty() || m.chunks.len() > 100_000 {
            return Err("the install manifest lists no usable chunks".into());
        }
        let mut at = 0u64;
        for (i, c) in m.chunks.iter().enumerate() {
            if c.offset != at || c.size == 0 || c.size > MAX_CHUNK || c.csize == 0 || c.csize > MAX_CHUNK || !is_sha256(&c.sha256) {
                return Err(format!("the install manifest is not consistent at chunk {i}"));
            }
            at += c.size;
        }
        if at != m.uncompressed_size {
            return Err("the install manifest's chunks do not add up to its size".into());
        }
        Ok(m)
    }

    pub fn download_size(&self) -> u64 {
        self.chunks.iter().map(|c| c.csize).sum()
    }
}

/// What is on disk of an install: which manifest it belongs to and which chunks are done.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallState {
    /// SHA-256 of the manifest text: another manifest means starting over
    pub manifest: String,
    pub version: u32,
    pub num_chunks: usize,
    /// one bit per chunk, base64
    pub done: String,
    /// the last patch merged into the game data (0: none yet, the data is the base version)
    #[serde(default)]
    pub merged: u32,
    /// the game data was moved to `client\game_pak`: the copy can be played
    #[serde(default)]
    pub finished: bool,
}

impl InstallState {
    fn bits(&self) -> Vec<u8> {
        let mut b = base64::engine::general_purpose::STANDARD.decode(&self.done).unwrap_or_default();
        b.resize(self.num_chunks.div_ceil(8), 0);
        b
    }
    pub fn is_done(&self, i: usize) -> bool {
        i < self.num_chunks && self.bits()[i / 8] & (1 << (i % 8)) != 0
    }
    pub fn base_complete(&self) -> bool {
        self.count_done() == self.num_chunks
    }
    /// The version the game data is at.
    pub fn pak_version(&self) -> u32 {
        self.merged.max(self.version)
    }
    pub fn count_done(&self) -> usize {
        (0..self.num_chunks).filter(|&i| self.is_done(i)).count()
    }
    fn set(&mut self, i: usize) {
        let mut b = self.bits();
        b[i / 8] |= 1 << (i % 8);
        self.done = base64::engine::general_purpose::STANDARD.encode(b);
    }
}

pub fn staging(paths: &Paths) -> PathBuf {
    paths.client().join("game_pak.tmp")
}
pub fn state_file(paths: &Paths) -> PathBuf {
    paths.client().join("install_state.json")
}
pub fn patch_dir(paths: &Paths) -> PathBuf {
    paths.client().join("patch")
}

pub fn load_state(paths: &Paths) -> Option<InstallState> {
    serde_json::from_str(&fs::read_to_string(state_file(paths)).ok()?).ok()
}

fn save_state(paths: &Paths, s: &InstallState) -> Result<(), String> {
    let tmp = state_file(paths).with_extension("json.new");
    fs::write(&tmp, serde_json::to_string(s).unwrap()).and_then(|_| fs::rename(&tmp, state_file(paths))).map_err(|e| format!("cannot save the install state: {e}"))
}

fn agent() -> ureq::Agent {
    // not a browser's name: the file host challenges browser-like clients
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(20)).timeout_read(Duration::from_secs(60)).redirects(0).user_agent("aaemu-sandbox-launcher").build()
}

fn read_limited(r: ureq::Response, limit: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    r.into_reader().take(limit + 1).read_to_end(&mut out).map_err(|e| format!("the download broke off: {e}"))?;
    if out.len() as u64 > limit {
        return Err("the server sent more than expected".into());
    }
    Ok(out)
}

/// Fetches the manifest. The answer is the parsed manifest and the SHA-256 of its text.
pub fn fetch_manifest(src: &Source) -> Result<(Manifest, String), String> {
    let r = agent().get(&src.manifest_url).call().map_err(|e| format!("cannot get the install manifest: {e}"))?;
    let body = read_limited(r, MAX_MANIFEST)?;
    let text = String::from_utf8(body).map_err(|_| "the install manifest is not text".to_string())?;
    Ok((Manifest::parse(&text)?, hex::encode(Sha256::digest(text.as_bytes()))))
}

/// One chunk: download, check the length, decompress, check size and hash.
fn fetch_chunk(agent: &ureq::Agent, src: &Source, c: &Chunk) -> Result<Vec<u8>, String> {
    let url = format!("{}/{}/{}.zst", src.parts, &c.sha256[..2], c.sha256);
    let r = agent.get(&url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("the file host answered {code}"),
        ureq::Error::Transport(t) => format!("{t}"),
    })?;
    let packed = read_limited(r, c.csize)?;
    if packed.len() as u64 != c.csize {
        return Err(format!("got {} bytes, expected {}", packed.len(), c.csize));
    }
    let mut dec = ruzstd::decoding::StreamingDecoder::new(packed.as_slice()).map_err(|e| format!("cannot unpack: {e}"))?;
    let mut data = Vec::with_capacity(c.size as usize);
    (&mut dec).take(c.size + 1).read_to_end(&mut data).map_err(|e| format!("cannot unpack: {e}"))?;
    if data.len() as u64 != c.size {
        return Err("the unpacked size is wrong".into());
    }
    if hex::encode(Sha256::digest(&data)) != c.sha256 {
        return Err("the contents do not match the manifest's hash".into());
    }
    Ok(data)
}

#[cfg(windows)]
fn write_at(f: &fs::File, data: &[u8], mut offset: u64) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    let mut data = data;
    while !data.is_empty() {
        let n = f.seek_write(data, offset)?;
        data = &data[n..];
        offset += n as u64;
    }
    Ok(())
}

#[cfg(windows)]
fn free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let w: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free = 0u64;
    let ok = unsafe { windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(w.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub chunks_done: usize,
    pub chunks_total: usize,
    /// compressed bytes fetched in this run
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub workers: usize,
    /// stop after this many chunks in this run (testing); 0 = all
    pub max_chunks: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options { workers: 3, max_chunks: 0 }
    }
}

/// Downloads the chunks that are still missing. Returns the state; `count_done() == num_chunks` means the base file
/// is complete. Can be stopped (`cancel`) and started again at any time.
pub fn download_base(paths: &Paths, src: &Source, manifest: &Manifest, manifest_sha: &str, opt: Options, cancel: &AtomicBool, progress: &(dyn Fn(Progress) + Sync)) -> Result<InstallState, String> {
    let client = paths.client();
    fs::create_dir_all(&client).map_err(|e| format!("cannot create {}: {e}", client.display()))?;
    let fresh = InstallState { manifest: manifest_sha.to_string(), version: manifest.version, num_chunks: manifest.chunks.len(), done: String::new(), merged: 0, finished: false };
    let state = match load_state(paths) {
        Some(s) if s.manifest == fresh.manifest && s.num_chunks == fresh.num_chunks && staging(paths).is_file() => s,
        _ => {
            // another manifest, or nothing usable on disk: start over
            let _ = fs::remove_file(staging(paths));
            fresh
        }
    };
    let mut todo: Vec<usize> = (0..manifest.chunks.len()).filter(|&i| !state.is_done(i)).collect();
    if opt.max_chunks > 0 {
        todo.truncate(opt.max_chunks);
    }
    let need: u64 = todo.iter().map(|&i| manifest.chunks[i].size).sum();
    if let Some(free) = free_space(&client) {
        if free < need + (1 << 30) {
            return Err(format!("not enough disk space: {:.1} GB free, {:.1} GB needed", free as f64 / 1e9, (need + (1 << 30)) as f64 / 1e9));
        }
    }
    let file = fs::OpenOptions::new().create(true).write(true).truncate(false).open(staging(paths)).map_err(|e| format!("cannot open {}: {e}", staging(paths).display()))?;
    save_state(paths, &state)?;

    let state = Mutex::new(state);
    let (next, bytes) = (AtomicUsize::new(0), AtomicU64::new(0));
    let failure: Mutex<Option<String>> = Mutex::new(None);
    let total = manifest.chunks.len();
    std::thread::scope(|scope| {
        for _ in 0..opt.workers.clamp(1, 8) {
            scope.spawn(|| {
                let agent = agent();
                loop {
                    let n = next.fetch_add(1, Ordering::SeqCst);
                    if n >= todo.len() || cancel.load(Ordering::SeqCst) || failure.lock().unwrap().is_some() {
                        return;
                    }
                    let i = todo[n];
                    let c = &manifest.chunks[i];
                    let mut last = String::new();
                    let mut data = None;
                    for attempt in 0..TRIES {
                        if cancel.load(Ordering::SeqCst) {
                            return;
                        }
                        match fetch_chunk(&agent, src, c) {
                            Ok(d) => {
                                data = Some(d);
                                break;
                            }
                            Err(e) => {
                                last = e;
                                std::thread::sleep(Duration::from_millis(if cfg!(test) { 1 } else { 1500 * (attempt as u64 + 1) }));
                            }
                        }
                    }
                    let Some(data) = data else {
                        *failure.lock().unwrap() = Some(format!("chunk {i} failed after {TRIES} tries: {last}"));
                        return;
                    };
                    if let Err(e) = write_at(&file, &data, c.offset) {
                        *failure.lock().unwrap() = Some(format!("cannot write the game data: {e}"));
                        return;
                    }
                    bytes.fetch_add(c.csize, Ordering::SeqCst);
                    let mut s = state.lock().unwrap();
                    s.set(i);
                    if let Err(e) = save_state(paths, &s) {
                        *failure.lock().unwrap() = Some(e);
                        return;
                    }
                    progress(Progress { chunks_done: s.count_done(), chunks_total: total, bytes: bytes.load(Ordering::SeqCst) });
                }
            });
        }
    });
    file.sync_all().map_err(|e| e.to_string())?;
    if let Some(e) = failure.into_inner().unwrap() {
        return Err(e);
    }
    Ok(state.into_inner().unwrap())
}

/// Downloads patch `v` into `client\patch\pak{v}`, going on from a partial file. Returns its size.
pub fn download_patch(paths: &Paths, src: &Source, v: u32, cancel: &AtomicBool) -> Result<u64, String> {
    let dir = patch_dir(paths);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (done, part) = (dir.join(format!("pak{v}")), dir.join(format!("pak{v}.part")));
    if let Ok(m) = fs::metadata(&done) {
        return Ok(m.len());
    }
    let url = format!("{}{v}", src.patch);
    let mut last = String::new();
    for attempt in 0..TRIES {
        if cancel.load(Ordering::SeqCst) {
            return Err("stopped".into());
        }
        let have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let mut req = agent().get(&url);
        if have > 0 {
            req = req.set("Range", &format!("bytes={have}-"));
        }
        let r = match req.call() {
            Ok(r) => r,
            // the file host answers 403 for a file it does not have
            Err(ureq::Error::Status(code @ (403 | 404), _)) => return Err(format!("patch {v} is not on the file host ({code})")),
            Err(ureq::Error::Status(416, _)) => {
                let _ = fs::remove_file(&part); // our partial file is longer than the real one: start over
                last = "the partial file did not fit".into();
                continue;
            }
            Err(e) => {
                last = format!("{e}");
                std::thread::sleep(Duration::from_millis(if cfg!(test) { 1 } else { 1500 * (attempt as u64 + 1) }));
                continue;
            }
        };
        let resumed = r.status() == 206;
        let mut out = fs::OpenOptions::new().create(true).write(true).append(resumed).truncate(!resumed).open(&part).map_err(|e| e.to_string())?;
        let base = if resumed { have } else { 0 };
        match std::io::copy(&mut r.into_reader().take(MAX_PATCH - base + 1), &mut out) {
            Ok(n) if base + n > MAX_PATCH => return Err(format!("patch {v} is larger than a patch can be")),
            Ok(n) => {
                out.sync_all().map_err(|e| e.to_string())?;
                drop(out);
                fs::rename(&part, &done).map_err(|e| e.to_string())?;
                return Ok(base + n);
            }
            Err(e) => last = format!("the download broke off: {e}"),
        }
    }
    Err(format!("patch {v} failed after {TRIES} tries: {last}"))
}

// ---------------------------------------------------------------------------------------------------------------
// Merging the patches, finishing, the whole install
// ---------------------------------------------------------------------------------------------------------------

/// The game data file as it is now: the staging file until the install is finished, `game_pak` after.
pub fn pak_file(paths: &Paths, s: &InstallState) -> PathBuf {
    if s.finished { paths.pak() } else { staging(paths) }
}

fn undo_file(paths: &Paths) -> PathBuf {
    paths.client().join("merge_undo.bin")
}

fn write_undo(paths: &Paths, end: u64, table: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let tmp = undo_file(paths).with_extension("new");
    let go = || -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&end.to_le_bytes())?;
        f.write_all(table)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, undo_file(paths))
    };
    go().map_err(|e| format!("cannot save the undo file: {e}"))
}

fn read_undo(paths: &Paths) -> Option<(u64, Vec<u8>)> {
    let b = fs::read(undo_file(paths)).ok()?;
    if b.len() < 8 + 512 {
        return None;
    }
    Some((u64::from_le_bytes(b[..8].try_into().unwrap()), b[8..].to_vec()))
}

/// Called when another kind of set up replaces the game data: the finished install is no longer what is on disk.
pub fn forget_finished(paths: &Paths) {
    if load_state(paths).is_some_and(|s| s.finished) {
        let _ = fs::remove_file(state_file(paths));
        let _ = fs::remove_file(undo_file(paths));
    }
}

/// Merges the downloaded patches up to version `to` into the game data. The file table as it was before is kept
/// in an undo file until the new one is written, so a merge that was cut off (power, crash, a damaged patch) is
/// undone and done again. A patch that fails its own checksums is deleted so it is downloaded again.
pub fn merge(paths: &Paths, to: u32, cancel: &AtomicBool, log: &dyn Fn(String)) -> Result<InstallState, String> {
    use crate::pak::{Pak, apply_patch};
    let mut s = load_state(paths).ok_or("nothing is installed yet")?;
    if !s.base_complete() {
        return Err("the base game data is not complete yet".into());
    }
    let file = pak_file(paths, &s);
    if let Some((end, table)) = read_undo(paths) {
        Pak::restore_table(&file, end, &table)?;
        let _ = fs::remove_file(undo_file(paths));
        log("an unfinished merge was undone".into());
    }
    if s.pak_version() >= to {
        return Ok(s);
    }
    let mut base = Pak::open(&file, true)?;
    let (end, table) = base.table_backup();
    write_undo(paths, end, &table)?;
    drop(table);
    let mut failed = None;
    for v in s.pak_version() + 1..=to {
        let path = patch_dir(paths).join(format!("pak{v}"));
        let r = match Pak::open(&path, false) {
            Ok(patch) => apply_patch(&mut base, &patch, &|| cancel.load(Ordering::SeqCst)).map_err(|e| (e.contains("damaged") || e.contains("early"), e)),
            // a patch that cannot even be opened was cut short or is not a patch
            Err(e) => Err((true, e)),
        };
        match r {
            Ok(m) => log(format!("patch {v}: {} files changed, {} added, {} removed", m.replaced + m.moved, m.added, m.deleted)),
            Err((damaged, e)) => {
                if damaged {
                    let _ = fs::remove_file(&path);
                    failed = Some(format!("patch {v} is damaged and will be downloaded again ({e})"));
                } else {
                    failed = Some(format!("patch {v}: {e}"));
                }
                break;
            }
        }
    }
    if let Some(e) = failed {
        // put the old table back: the data is again what it was before this merge
        drop(base);
        let (end, table) = read_undo(paths).ok_or("the undo file is gone")?;
        Pak::restore_table(&file, end, &table)?;
        let _ = fs::remove_file(undo_file(paths));
        return Err(e);
    }
    base.save()?;
    drop(base);
    // first the undo file, then the state: cut off in between, the patches are merged once more, which is harmless
    fs::remove_file(undo_file(paths)).map_err(|e| e.to_string())?;
    s.merged = to;
    save_state(paths, &s)?;
    Ok(s)
}

/// Moves the merged game data to `client\game_pak` and drops the patch files.
pub fn finish(paths: &Paths) -> Result<InstallState, String> {
    let mut s = load_state(paths).ok_or("nothing is installed yet")?;
    if !s.finished {
        let pak = paths.pak();
        if staging(paths).is_file() {
            if fs::symlink_metadata(&pak).is_ok() {
                // only our name for a file: a link to AA Classic's game data keeps its data there
                fs::remove_file(&pak).map_err(|e| format!("cannot remove the old game_pak: {e}"))?;
            }
            fs::rename(staging(paths), &pak).map_err(|e| format!("cannot move the game data into place: {e}"))?;
        } else if !pak.is_file() {
            return Err("the downloaded game data is gone".into());
        }
        s.finished = true;
        save_state(paths, &s)?;
    }
    let _ = fs::remove_dir_all(patch_dir(paths));
    Ok(s)
}

/// Which client build to use: the one asked for, else the one build in the vault the server accepts, else the
/// only build in the vault.
pub fn choose_build(paths: &Paths, wanted: Option<&str>, accepted: &[String]) -> Result<String, String> {
    let have: Vec<String> = crate::vault::builds(paths).into_iter().map(|b| b.id).collect();
    if let Some(w) = wanted {
        return have.iter().find(|id| id.starts_with(w)).cloned().ok_or(format!("build {w} is not in the vault"));
    }
    if have.is_empty() {
        return Err("there is no client build yet: ask a friend for a build pack and import it".into());
    }
    if !accepted.is_empty() {
        return have.iter().find(|id| accepted.contains(id)).cloned().ok_or("none of the client builds you have is one the server accepts: ask for its build pack".to_string());
    }
    match have.as_slice() {
        [one] => Ok(one.clone()),
        _ => Err("there are several client builds in the vault: say which one".into()),
    }
}

#[derive(Clone, Debug, Default)]
pub struct Plan {
    /// the version to end at; None = every patch the file host has
    pub to: Option<u32>,
    /// a build id (or its start) from the vault; None = chosen by `choose_build`
    pub build: Option<String>,
    /// the build ids the server accepts
    pub accepted: Vec<String>,
    /// download at most this many chunks in this run (0 = all); for tests
    pub max_chunks: usize,
    /// use only what is on disk already
    pub offline: bool,
}

pub struct Installed {
    pub version: u32,
    pub build: String,
}

/// The whole "Install game": base data, patches, merge, client build from the vault. Can be stopped and run
/// again; every step goes on where it was. Also brings a finished install up to a newer version.
pub fn run(paths: &Paths, src: &Source, plan: &Plan, cancel: &AtomicBool, log: &(dyn Fn(String) + Sync), progress: &(dyn Fn(f32) + Sync)) -> Result<Installed, String> {
    // no build, no game: say so before 22 GB are downloaded
    let build = choose_build(paths, plan.build.as_deref(), &plan.accepted)?;
    let mut state = load_state(paths);
    // cut off between moving the file into place and noting it
    if let Some(s) = &state {
        if !s.finished && s.base_complete() && !staging(paths).is_file() && paths.pak().is_file() && crate::core::link_count(&paths.pak()).is_ok_and(|n| n == 1) {
            state = Some(finish(paths)?);
        }
    }
    let ready = state.as_ref().is_some_and(|s| s.finished || s.base_complete() && staging(paths).is_file());
    if !ready {
        if plan.offline {
            return Err("the base game data is not on disk yet".into());
        }
        let (m, sha) = fetch_manifest(src)?;
        log(format!("game data version {}: {:.1} GB to download, {:.1} GB on disk", m.version, m.download_size() as f64 / 1e9, m.uncompressed_size as f64 / 1e9));
        let started = std::time::Instant::now();
        let s = download_base(paths, src, &m, &sha, Options { max_chunks: plan.max_chunks, ..Default::default() }, cancel, &|p| {
            progress(p.chunks_done as f32 / p.chunks_total.max(1) as f32);
            if p.chunks_done % 10 == 0 || p.chunks_done == p.chunks_total {
                log(format!("game data: {} of {} parts, {:.1} MB/s", p.chunks_done, p.chunks_total, p.bytes as f64 / 1e6 / started.elapsed().as_secs_f64().max(0.1)));
            }
        })?;
        if cancel.load(Ordering::SeqCst) {
            return Err("stopped".into());
        }
        if !s.base_complete() {
            return Err(format!("{} of {} parts are done; run it again to go on", s.count_done(), s.num_chunks));
        }
        log("the base game data is complete".into());
    }
    let mut last = String::new();
    for _round in 0..3 {
        let s = load_state(paths).ok_or("the install state is gone")?;
        if plan.to.is_some_and(|t| t < s.pak_version()) {
            return Err(format!("the game data is at version {} already: an older version cannot be installed", s.pak_version()));
        }
        // the patches: up to the version asked for, or until the file host has no next one
        let mut to = s.pak_version();
        loop {
            let v = to + 1;
            if plan.to.is_some_and(|t| v > t) {
                break;
            }
            if !patch_dir(paths).join(format!("pak{v}")).is_file() {
                if plan.offline {
                    if plan.to.is_some() {
                        return Err(format!("patch {v} is not on disk"));
                    }
                    break;
                }
                match download_patch(paths, src, v, cancel) {
                    Ok(size) => log(format!("patch {v} downloaded ({:.1} MB)", size as f64 / 1e6)),
                    Err(e) if e.contains("is not on the file host") && plan.to.is_none() => break,
                    Err(e) => return Err(e),
                }
            }
            to = v;
        }
        if to > s.pak_version() {
            log(format!("merging the patches up to version {to} ..."));
        }
        match merge(paths, to, cancel, log) {
            Ok(_) => {
                let s = finish(paths)?;
                crate::vault::restore(paths, &build, &paths.bin())?;
                log(format!("ready: game data version {}, client build {}", s.pak_version(), &build[..12.min(build.len())]));
                return Ok(Installed { version: s.pak_version(), build });
            }
            Err(e) if e.contains("downloaded again") && !plan.offline => {
                log(e.clone());
                last = e;
            }
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::Arc;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aaemu-install-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A small file server on a loopback port. `hits` counts the requests per path.
    struct Host {
        base: String,
        files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        hits: Arc<Mutex<HashMap<String, usize>>>,
    }

    fn host() -> Host {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        let files: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::default();
        let hits: Arc<Mutex<HashMap<String, usize>>> = Arc::default();
        let (f2, h2) = (files.clone(), hits.clone());
        std::thread::spawn(move || {
            for c in l.incoming() {
                let Ok(mut c) = c else { continue };
                let (files, hits) = (f2.clone(), h2.clone());
                std::thread::spawn(move || {
                    let mut got = Vec::new();
                    let mut buf = [0u8; 2048];
                    while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                        match c.read(&mut buf) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => got.extend_from_slice(&buf[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&got).to_string();
                    let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                    *hits.lock().unwrap().entry(path.clone()).or_default() += 1;
                    let from = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("range: bytes=").and_then(|r| r.trim_end_matches('-').parse::<usize>().ok()));
                    let body = files.lock().unwrap().get(&path).cloned();
                    let _ = match (body, from) {
                        (None, _) => write!(c, "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
                        (Some(b), Some(f)) if f < b.len() => write!(c, "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len() - f).and_then(|_| c.write_all(&b[f..])),
                        (Some(_), Some(_)) => write!(c, "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
                        (Some(b), None) => write!(c, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len()).and_then(|_| c.write_all(&b)),
                    };
                });
            }
        });
        Host { base, files, hits }
    }

    impl Host {
        fn source(&self) -> Source {
            Source { manifest_url: format!("{}/manifest.json", self.base), parts: format!("{}/parts", self.base), patch: format!("{}/pak", self.base) }
        }
        fn put(&self, path: &str, body: Vec<u8>) {
            self.files.lock().unwrap().insert(path.to_string(), body);
        }
        fn hits(&self, path: &str) -> usize {
            *self.hits.lock().unwrap().get(path).unwrap_or(&0)
        }
        /// Publishes `data` as chunks of `chunk` bytes with a manifest; returns the chunk paths.
        fn publish(&self, data: &[u8], chunk: usize) -> Vec<String> {
            let (mut chunks, mut paths, mut at) = (Vec::new(), Vec::new(), 0u64);
            for part in data.chunks(chunk) {
                let packed = ruzstd::encoding::compress_to_vec(part, ruzstd::encoding::CompressionLevel::Fastest);
                let sha = hex::encode(Sha256::digest(part));
                let path = format!("/parts/{}/{}.zst", &sha[..2], sha);
                chunks.push(serde_json::json!({ "offset": at, "size": part.len(), "csize": packed.len(), "sha256": sha }));
                self.put(&path, packed);
                paths.push(path);
                at += part.len() as u64;
            }
            let m = serde_json::json!({ "schema": 2, "version": 263, "compression": "zstd", "uncompressed_size": data.len(), "uncompressed_sha256": "", "chunk_size": chunk, "chunks": chunks });
            self.put("/manifest.json", m.to_string().into_bytes());
            paths
        }
    }

    fn data(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 + i / 251) as u8).collect()
    }

    fn run(paths: &Paths, h: &Host, max: usize) -> Result<InstallState, String> {
        let (m, sha) = fetch_manifest(&h.source())?;
        download_base(paths, &h.source(), &m, &sha, Options { workers: 3, max_chunks: max }, &AtomicBool::new(false), &|_| {})
    }

    #[test]
    fn manifest_rules() {
        let ok = r#"{"schema":2,"version":263,"compression":"zstd","uncompressed_size":3,"chunk_size":2,"chunks":[{"offset":0,"size":2,"csize":5,"sha256":"SHA"},{"offset":2,"size":1,"csize":4,"sha256":"SHA"}]}"#.replace("SHA", &"ab".repeat(32));
        let m = Manifest::parse(&ok).unwrap();
        assert_eq!((m.version, m.chunks.len(), m.download_size()), (263, 2, 9));
        assert!(Manifest::parse(&ok.replace("\"schema\":2", "\"schema\":3")).is_err());
        assert!(Manifest::parse(&ok.replace("zstd", "lz4")).is_err());
        assert!(Manifest::parse(&ok.replace("\"offset\":2", "\"offset\":3")).is_err()); // a gap
        assert!(Manifest::parse(&ok.replace("\"uncompressed_size\":3", "\"uncompressed_size\":4")).is_err());
        assert!(Manifest::parse(&ok.replace(&"ab".repeat(32), "../../evil")).is_err()); // the hash becomes part of an address
        assert!(Manifest::parse(&ok.replace("\"size\":1", "\"size\":999999999999")).is_err());
        assert!(Manifest::parse("{}").is_err());
    }

    #[test]
    fn downloads_the_whole_file_and_checks_every_chunk() {
        let d = tmp("whole");
        let (paths, h) = (Paths::at(d.join("ours")), host());
        let bytes = data(10_000);
        h.publish(&bytes, 1024);
        let s = run(&paths, &h, 0).unwrap();
        assert_eq!(s.count_done(), 10);
        assert_eq!(fs::read(staging(&paths)).unwrap(), bytes);
        assert_eq!(load_state(&paths).unwrap(), s);
    }

    #[test]
    fn goes_on_where_it_stopped() {
        let d = tmp("resume");
        let (paths, h) = (Paths::at(d.join("ours")), host());
        let bytes = data(8_000);
        let parts = h.publish(&bytes, 1000);
        assert_eq!(run(&paths, &h, 3).unwrap().count_done(), 3);
        assert_eq!(run(&paths, &h, 0).unwrap().count_done(), 8);
        assert_eq!(fs::read(staging(&paths)).unwrap(), bytes);
        // no chunk was fetched twice
        assert!(parts.iter().all(|p| h.hits(p) == 1));
        // a run with nothing left asks for no chunk
        assert_eq!(run(&paths, &h, 0).unwrap().count_done(), 8);
        assert!(parts.iter().all(|p| h.hits(p) == 1));
    }

    #[test]
    fn a_wrong_chunk_is_never_written() {
        let d = tmp("wrong");
        let (paths, h) = (Paths::at(d.join("ours")), host());
        let bytes = data(3_000);
        let parts = h.publish(&bytes, 1000);
        // chunk 1 on the host is the right length but other contents
        let other = ruzstd::encoding::compress_to_vec(&vec![9u8; 1000][..], ruzstd::encoding::CompressionLevel::Uncompressed);
        let real_len = h.files.lock().unwrap()[&parts[1]].len();
        let mut fake = other;
        fake.resize(real_len.max(fake.len()), 0);
        h.put(&parts[1], fake);
        let e = run(&paths, &h, 0).unwrap_err();
        assert!(e.contains("chunk 1 failed"), "{e}");
        assert_eq!(h.hits(&parts[1]), TRIES as usize);
        let s = load_state(&paths).unwrap();
        assert!(!s.is_done(1));
        // a chunk the host does not have (it answers 403) also fails cleanly
        h.files.lock().unwrap().remove(&parts[1]);
        assert!(run(&paths, &h, 0).is_err());
    }

    #[test]
    fn another_manifest_starts_over() {
        let d = tmp("newmanifest");
        let (paths, h) = (Paths::at(d.join("ours")), host());
        h.publish(&data(4_000), 1000);
        run(&paths, &h, 2).unwrap();
        let newer = data(4_100);
        h.publish(&newer, 1000);
        let s = run(&paths, &h, 0).unwrap();
        assert_eq!(s.count_done(), 5);
        assert_eq!(fs::read(staging(&paths)).unwrap(), newer);
    }

    #[test]
    fn stops_when_asked() {
        let d = tmp("cancel");
        let (paths, h) = (Paths::at(d.join("ours")), host());
        h.publish(&data(5_000), 1000);
        let (m, sha) = fetch_manifest(&h.source()).unwrap();
        let s = download_base(&paths, &h.source(), &m, &sha, Options::default(), &AtomicBool::new(true), &|_| {}).unwrap();
        assert_eq!(s.count_done(), 0);
    }

    #[test]
    fn patch_download_resumes_and_reports_a_missing_patch() {
        let d = tmp("patch");
        let (paths, h) = (Paths::at(d.join("ours")), host());
        let body = data(5_000);
        h.put("/pak264", body.clone());
        fs::create_dir_all(patch_dir(&paths)).unwrap();
        fs::write(patch_dir(&paths).join("pak264.part"), &body[..2_000]).unwrap();
        assert_eq!(download_patch(&paths, &h.source(), 264, &AtomicBool::new(false)).unwrap(), 5_000);
        assert_eq!(fs::read(patch_dir(&paths).join("pak264")).unwrap(), body);
        assert!(!patch_dir(&paths).join("pak264.part").exists());
        // already there: no request
        let before = h.hits("/pak264");
        download_patch(&paths, &h.source(), 264, &AtomicBool::new(false)).unwrap();
        assert_eq!(h.hits("/pak264"), before);
        assert!(download_patch(&paths, &h.source(), 298, &AtomicBool::new(false)).unwrap_err().contains("not on the file host"));
    }

    // --- merge and the whole install ---------------------------------------------------------------------------

    use crate::pak::{Entry, Pak};

    /// An archive with these files, as bytes.
    fn archive(dir: &Path, name: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
        let path = dir.join(name);
        let _ = fs::remove_file(&path);
        let mut p = Pak::create(&path).unwrap();
        for (n, body) in files {
            p.put(n, &mut &body[..], body.len() as u64, None, &Entry::default()).unwrap();
        }
        p.save().unwrap();
        drop(p);
        fs::read(&path).unwrap()
    }

    fn file(p: &Pak, name: &str) -> Vec<u8> {
        p.read(p.get(name).unwrap(), 1 << 20).unwrap()
    }

    /// A host with a base (a, b, c), patch 264 (a larger, d new) and patch 265 (removes b), and a vault build.
    fn game(d: &Path, paths: &Paths, h: &Host) -> String {
        let big = data(3000);
        h.publish(&archive(d, "base", &[("game/a.txt", b"a1"), ("game/b.txt", b"b1"), ("game/c.txt", b"c1")]), 1024);
        h.put("/pak264", archive(d, "p264", &[("game/a.txt", &big), ("game/d.txt", b"d1")]));
        h.put("/pak265", archive(d, "p265", &[("game/c.txt", b"c2"), ("deleted.txt", b"game\\b.txt\r\n")]));
        let bin = d.join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("archeage.exe"), b"exe").unwrap();
        crate::vault::add(paths, &bin).unwrap().id
    }

    fn install(paths: &Paths, h: &Host, plan: &Plan) -> Result<Installed, String> {
        super::run(paths, &h.source(), plan, &AtomicBool::new(false), &|_| {}, &|_| {})
    }

    fn check_265(paths: &Paths) {
        let p = Pak::open(&paths.pak(), false).unwrap();
        assert_eq!(p.entries().len(), 4);
        assert_eq!(file(&p, "deleted.txt"), b"game\\b.txt\r\n");
        assert_eq!(file(&p, "game/a.txt"), data(3000));
        assert_eq!(file(&p, "game/c.txt"), b"c2");
        assert_eq!(file(&p, "game/d.txt"), b"d1");
        assert!(p.get("game/b.txt").is_none());
        assert_eq!(p.check(&|_, _| {}).unwrap(), (vec![], 0));
    }

    #[test]
    fn installs_the_game_without_an_aa_classic_folder_and_updates_it_later() {
        let d = tmp("game");
        let (paths, h) = (Paths::at(d.join("ours")), host());
        assert!(install(&paths, &h, &Plan::default()).is_err_and(|e| e.contains("build pack")), "no build, no download");
        assert_eq!(h.hits("/manifest.json"), 0);
        let build = game(&d, &paths, &h);
        let done = install(&paths, &h, &Plan::default()).unwrap();
        assert_eq!((done.version, &done.build), (265, &build));
        check_265(&paths);
        assert!(!staging(&paths).exists() && !patch_dir(&paths).exists() && !undo_file(&paths).exists());
        assert_eq!(fs::read(paths.exe()).unwrap(), b"exe");
        assert_eq!(crate::core::status(&paths, None, crate::core::Mode::Own), crate::core::State::Ready);
        // again: nothing to download but the question whether there is a next patch
        let before = h.hits("/manifest.json");
        assert_eq!(install(&paths, &h, &Plan::default()).unwrap().version, 265);
        assert_eq!(h.hits("/manifest.json"), before);
        // AA Classic publishes a patch: the finished install takes it
        h.put("/pak266", archive(&d, "p266", &[("game/e.txt", b"e1")]));
        assert_eq!(install(&paths, &h, &Plan::default()).unwrap().version, 266);
        let p = Pak::open(&paths.pak(), false).unwrap();
        assert_eq!(file(&p, "game/e.txt"), b"e1");
        assert_eq!(file(&p, "game/a.txt"), data(3000));
        // an older version cannot be asked for
        assert!(install(&paths, &h, &Plan { to: Some(264), ..Default::default() }).is_err());
    }

    #[test]
    fn stops_at_the_version_asked_for() {
        let d = tmp("to");
        let (paths, h) = (Paths::at(d.join("ours")), host());
        game(&d, &paths, &h);
        assert_eq!(install(&paths, &h, &Plan { to: Some(264), ..Default::default() }).unwrap().version, 264);
        let p = Pak::open(&paths.pak(), false).unwrap();
        assert_eq!(file(&p, "game/b.txt"), b"b1");
        assert_eq!(h.hits("/pak265"), 0);
        assert!(install(&paths, &h, &Plan { to: Some(270), ..Default::default() }).is_err_and(|e| e.contains("not on the file host")));
    }

    #[test]
    fn a_damaged_patch_is_fetched_again_and_the_data_is_unharmed() {
        let d = tmp("damaged");
        let (paths, h) = (Paths::at(d.join("ours")), host());
        game(&d, &paths, &h);
        run(&paths, &h, 0).unwrap();
        // patch 264 on disk was cut short, 265 is fine
        let good = archive(&d, "p264", &[("game/a.txt", &data(3000)), ("game/d.txt", b"d1")]);
        fs::create_dir_all(patch_dir(&paths)).unwrap();
        fs::write(patch_dir(&paths).join("pak264"), &good[..good.len() / 2]).unwrap();
        let err = merge(&paths, 264, &AtomicBool::new(false), &|_| {}).unwrap_err();
        assert!(err.contains("downloaded again"), "{err}");
        assert!(!patch_dir(&paths).join("pak264").exists() && !undo_file(&paths).exists());
        let p = Pak::open(&staging(&paths), false).unwrap();
        assert_eq!((p.entries().len(), file(&p, "game/a.txt")), (3, b"a1".to_vec()));
        drop(p);
        // the same through the whole install: it downloads the patch again and ends well
        fs::write(patch_dir(&paths).join("pak264"), &good[..good.len() / 2]).unwrap();
        assert_eq!(install(&paths, &h, &Plan::default()).unwrap().version, 265);
        assert_eq!(h.hits("/pak264"), 1);
        check_265(&paths);
    }

    #[test]
    fn a_merge_that_was_cut_off_is_undone_and_done_again() {
        let d = tmp("cut");
        let (paths, h) = (Paths::at(d.join("ours")), host());
        game(&d, &paths, &h);
        run(&paths, &h, 0).unwrap();
        // as if the power went during a merge: undo file written, data written over the table, nothing saved
        let mut base = Pak::open(&staging(&paths), true).unwrap();
        let (end, table) = base.table_backup();
        write_undo(&paths, end, &table).unwrap();
        let junk = data(9000);
        base.put("game/a.txt", &mut &junk[..], 9000, None, &Entry::default()).unwrap();
        drop(base);
        assert!(Pak::open(&staging(&paths), false).is_err());
        let said = Mutex::new(Vec::new());
        let done = super::run(&paths, &h.source(), &Plan::default(), &AtomicBool::new(false), &|l| said.lock().unwrap().push(l), &|_| {}).unwrap();
        assert_eq!(done.version, 265);
        assert!(said.lock().unwrap().iter().any(|l| l.contains("unfinished merge was undone")));
        check_265(&paths);
    }

    #[test]
    fn game_data_linked_to_another_file_is_never_merged_into() {
        let d = tmp("linked");
        let (paths, h) = (Paths::at(d.join("ours")), host());
        game(&d, &paths, &h);
        run(&paths, &h, 0).unwrap();
        fs::hard_link(staging(&paths), d.join("elsewhere")).unwrap();
        let before = fs::read(d.join("elsewhere")).unwrap();
        assert!(install(&paths, &h, &Plan::default()).is_err_and(|e| e.contains("linked")));
        assert_eq!(fs::read(d.join("elsewhere")).unwrap(), before);
    }
}
