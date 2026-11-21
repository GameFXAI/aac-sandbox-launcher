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
    let fresh = InstallState { manifest: manifest_sha.to_string(), version: manifest.version, num_chunks: manifest.chunks.len(), done: String::new() };
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
}
