//! The game's archive (`game_pak`) and the patch archives (`pakNNN`): reading, changing and merging.
//!
//! Layout (the "WIBO" kind, the only one AA Classic uses): file data first, each file in a slot padded to 512 bytes;
//! then the file table, one 336-byte record per file, each record AES-128-CBC encrypted on its own with a zero IV;
//! then zero padding to 512 bytes; then a 512-byte header whose first 32 bytes are encrypted the same way and hold
//! the magic and the two record counts (files, then unused slots). The key is the one every client has.
//!
//! A patch archive holds complete replacement files and sometimes a `deleted.txt` naming files to remove. Merging a
//! patch = put each of its files into the base (in its old slot when it fits, else in a free slot or at the end),
//! add the patch's `deleted.txt` to the end of the base's own `deleted.txt` (a running log, as AA Classic keeps it),
//! remove the files it names, write the table and header again.
//!
//! Everything read from an archive is checked before it is used: counts, record bounds, sizes. A patch file's bytes
//! are checked against the MD5 its own table states before the base's table is saved.

use aes::Aes128;
use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use md5::{Digest, Md5};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::windows::fs::FileExt;
use std::path::Path;

const KEY: [u8; 16] = [0x32, 0x1F, 0x2A, 0xEE, 0xAA, 0x58, 0x4A, 0xB4, 0x9A, 0x6C, 0x9E, 0x09, 0xD5, 0x9E, 0x9C, 0x6F];
const HEADER: u64 = 0x200;
const RECORD: usize = 0x150;
const NAME: usize = 0x108;
const BLOCK: u64 = 0x200;
const MAX_RECORDS: u64 = 4_000_000;
const UNUSED: &str = "__unused__";
const MAX_PADDING: u64 = i32::MAX as u64;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub offset: u64,
    pub size: u64,
    /// free bytes after the file up to the end of its slot
    pub padding: u32,
    pub md5: [u8; 16],
    pub flags: u32,
    pub created: i64,
    pub modified: i64,
    pub extra: u64,
}

impl Entry {
    fn slot(&self) -> u64 {
        self.size + self.padding as u64
    }
}

fn cbc(buf: &mut [u8], encrypt: bool) {
    let aes = Aes128::new(GenericArray::from_slice(&KEY));
    let mut prev = [0u8; 16];
    for block in buf.chunks_exact_mut(16) {
        if encrypt {
            for (b, p) in block.iter_mut().zip(prev) {
                *b ^= p;
            }
            aes.encrypt_block(GenericArray::from_mut_slice(block));
            prev.copy_from_slice(block);
        } else {
            let mut cipher = [0u8; 16];
            cipher.copy_from_slice(block);
            aes.decrypt_block(GenericArray::from_mut_slice(block));
            for (b, p) in block.iter_mut().zip(prev) {
                *b ^= p;
            }
            prev = cipher;
        }
    }
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn parse_record(b: &[u8]) -> Entry {
    let end = b[..NAME].iter().position(|&c| c == 0).unwrap_or(NAME);
    Entry {
        name: b[..end].iter().map(|&c| c as char).collect(),
        offset: u64_at(b, NAME),
        size: u64_at(b, NAME + 8),
        padding: u32_at(b, NAME + 24),
        md5: b[NAME + 28..NAME + 44].try_into().unwrap(),
        flags: u32_at(b, NAME + 44),
        created: u64_at(b, NAME + 48) as i64,
        modified: u64_at(b, NAME + 56) as i64,
        extra: u64_at(b, NAME + 64),
    }
}

fn write_record(e: &Entry, b: &mut [u8]) {
    b.fill(0);
    for (i, c) in e.name.chars().take(NAME - 1).enumerate() {
        b[i] = c as u32 as u8;
    }
    b[NAME..NAME + 8].copy_from_slice(&e.offset.to_le_bytes());
    b[NAME + 8..NAME + 16].copy_from_slice(&e.size.to_le_bytes());
    b[NAME + 16..NAME + 24].copy_from_slice(&e.size.to_le_bytes());
    b[NAME + 24..NAME + 28].copy_from_slice(&e.padding.to_le_bytes());
    b[NAME + 28..NAME + 44].copy_from_slice(&e.md5);
    b[NAME + 44..NAME + 48].copy_from_slice(&e.flags.to_le_bytes());
    b[NAME + 48..NAME + 56].copy_from_slice(&e.created.to_le_bytes());
    b[NAME + 56..NAME + 64].copy_from_slice(&e.modified.to_le_bytes());
    b[NAME + 64..NAME + 72].copy_from_slice(&e.extra.to_le_bytes());
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() < NAME && name.chars().all(|c| (c as u32) < 256 && c != '\0')
}

fn read_at(f: &File, mut buf: &mut [u8], mut offset: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        let n = f.seek_read(buf, offset)?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf = &mut buf[n..];
        offset += n as u64;
    }
    Ok(())
}

fn write_at(f: &File, mut data: &[u8], mut offset: u64) -> std::io::Result<()> {
    while !data.is_empty() {
        let n = f.seek_write(data, offset)?;
        data = &data[n..];
        offset += n as u64;
    }
    Ok(())
}

fn round_up(n: u64) -> u64 {
    n.div_ceil(BLOCK) * BLOCK
}

pub struct Pak {
    file: File,
    writable: bool,
    files: Vec<Entry>,
    index: HashMap<String, usize>,
    /// unused slots: only offset and size matter
    free: Vec<(u64, u64)>,
    /// where the file data ends and the table starts
    end: u64,
    /// the header's second encrypted block as read, written back unchanged
    header_rest: [u8; 16],
    header_word: u32,
}

/// What one `put` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Put {
    Replaced,
    Moved,
    Added,
}

/// Reads one file of an archive.
pub struct Reader<'a> {
    file: &'a File,
    at: u64,
    left: u64,
}

impl Read for Reader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let want = (buf.len() as u64).min(self.left) as usize;
        if want == 0 {
            return Ok(0);
        }
        let n = self.file.seek_read(&mut buf[..want], self.at)?;
        self.at += n as u64;
        self.left -= n as u64;
        Ok(n)
    }
}

impl Pak {
    /// Opens an archive. With `write`, the file must be one only we use: a file with a second hard link (the
    /// Standard mode link to AA Classic's own `game_pak`) is refused.
    pub fn open(path: &Path, write: bool) -> Result<Pak, String> {
        let name = path.display();
        if write && crate::core::link_count(path).map_err(|e| format!("{name}: {e}"))? > 1 {
            return Err(format!("{name} is linked to another file and will not be changed"));
        }
        let file = OpenOptions::new().read(true).write(write).open(path).map_err(|e| format!("cannot open {name}: {e}"))?;
        let len = file.metadata().map_err(|e| format!("{name}: {e}"))?.len();
        if len < HEADER {
            return Err(format!("{name} is not a game archive (too short)"));
        }
        let mut head = [0u8; 32];
        read_at(&file, &mut head, len - HEADER).map_err(|e| format!("{name}: {e}"))?;
        let mut header_rest = [0u8; 16];
        cbc(&mut head, false);
        header_rest.copy_from_slice(&head[16..]);
        if &head[..4] != b"WIBO" {
            return Err(format!("{name} is not a game archive of the kind this launcher knows"));
        }
        let header_word = u32_at(&head, 4);
        let (nfiles, nfree) = (u32_at(&head, 8) as u64, u32_at(&head, 12) as u64);
        let total = nfiles + nfree;
        if total > MAX_RECORDS {
            return Err(format!("{name}: the file table is implausibly large ({total} records)"));
        }
        let table = round_up(total * RECORD as u64);
        if table + HEADER > len {
            return Err(format!("{name}: the file table does not fit in the file"));
        }
        let end = (len - HEADER - total * RECORD as u64) / BLOCK * BLOCK;
        let mut raw = vec![0u8; (total as usize) * RECORD];
        read_at(&file, &mut raw, end).map_err(|e| format!("{name}: {e}"))?;
        let mut files = Vec::with_capacity(nfiles as usize);
        let mut index = HashMap::with_capacity(nfiles as usize);
        let mut free = Vec::with_capacity(nfree as usize);
        for (i, rec) in raw.chunks_exact_mut(RECORD).enumerate() {
            cbc(rec, false);
            let e = parse_record(rec);
            let in_bounds = e.offset.checked_add(e.size).and_then(|x| x.checked_add(e.padding as u64)).is_some_and(|x| x <= end);
            if !in_bounds {
                return Err(format!("{name}: record {i} ({}) points outside the file data", e.name));
            }
            if (i as u64) < nfiles {
                if !valid_name(&e.name) {
                    return Err(format!("{name}: record {i} has no usable name"));
                }
                if index.insert(e.name.clone(), files.len()).is_some() {
                    return Err(format!("{name}: {} is listed twice", e.name));
                }
                files.push(e);
            } else if e.size > 0 {
                free.push((e.offset, e.size));
            }
        }
        Ok(Pak { file, writable: write, files, index, free, end, header_rest, header_word })
    }

    /// Makes a new empty archive.
    pub fn create(path: &Path) -> Result<Pak, String> {
        let file = OpenOptions::new().read(true).write(true).create_new(true).open(path).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
        let mut pak = Pak { file, writable: true, files: Vec::new(), index: HashMap::new(), free: Vec::new(), end: 0, header_rest: [0; 16], header_word: 0 };
        pak.save()?;
        Ok(pak)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.files
    }

    pub fn free_slots(&self) -> usize {
        self.free.len()
    }

    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.index.get(name).map(|&i| &self.files[i])
    }

    pub fn reader(&self, e: &Entry) -> Reader<'_> {
        Reader { file: &self.file, at: e.offset, left: e.size }
    }

    /// The whole of one file; for small files only.
    pub fn read(&self, e: &Entry, limit: u64) -> Result<Vec<u8>, String> {
        if e.size > limit {
            return Err(format!("{} is larger than expected ({} bytes)", e.name, e.size));
        }
        let mut buf = vec![0u8; e.size as usize];
        read_at(&self.file, &mut buf, e.offset).map_err(|e| e.to_string())?;
        Ok(buf)
    }

    /// Reads every file and compares it with the checksum in the table. Returns the names that do not match and
    /// how many files have no checksum.
    pub fn check(&self, progress: &dyn Fn(usize, usize)) -> Result<(Vec<String>, usize), String> {
        let mut order: Vec<&Entry> = self.files.iter().collect();
        order.sort_by_key(|e| e.offset);
        let (mut bad, mut unsummed) = (Vec::new(), 0);
        let mut buf = vec![0u8; 1 << 20];
        for (i, e) in order.iter().enumerate() {
            progress(i, order.len());
            if e.md5 == [0u8; 16] {
                unsummed += 1;
                continue;
            }
            let mut hash = Md5::new();
            let mut r = self.reader(e);
            loop {
                let n = r.read(&mut buf).map_err(|x| format!("reading {}: {x}", e.name))?;
                if n == 0 {
                    break;
                }
                hash.update(&buf[..n]);
            }
            let got: [u8; 16] = hash.finalize().into();
            if got != e.md5 {
                bad.push(e.name.clone());
            }
        }
        Ok((bad, unsummed))
    }

    /// The table and header as they would be written at `self.end`.
    fn table(&self) -> Vec<u8> {
        let total = self.files.len() + self.free.len();
        let body = round_up((total * RECORD) as u64) as usize;
        let mut out = vec![0u8; body + HEADER as usize];
        let unused = |&(offset, size): &(u64, u64)| Entry { name: UNUSED.into(), offset, size, padding: 0, md5: [0; 16], flags: 0, created: 0, modified: 0, extra: 0 };
        let all = self.files.iter().cloned().chain(self.free.iter().map(unused));
        for (e, rec) in all.zip(out.chunks_exact_mut(RECORD)) {
            write_record(&e, rec);
            cbc(rec, true);
        }
        let head = &mut out[body..body + 32];
        head[..4].copy_from_slice(b"WIBO");
        head[4..8].copy_from_slice(&self.header_word.to_le_bytes());
        head[8..12].copy_from_slice(&(self.files.len() as u32).to_le_bytes());
        head[12..16].copy_from_slice(&(self.free.len() as u32).to_le_bytes());
        head[16..32].copy_from_slice(&self.header_rest);
        cbc(head, true);
        out
    }

    /// Writes the table and header after the file data and cuts the file there. Until this is called after a
    /// change, the file on disk is not a valid archive.
    pub fn save(&mut self) -> Result<(), String> {
        if !self.writable {
            return Err("the archive was opened for reading only".into());
        }
        let table = self.table();
        write_at(&self.file, &table, self.end).map_err(|e| format!("writing the file table: {e}"))?;
        self.file.set_len(self.end + table.len() as u64).map_err(|e| e.to_string())?;
        self.file.sync_all().map_err(|e| e.to_string())
    }

    /// The table and header as they are on disk now, with where they start: what `restore_table` needs to undo
    /// an unfinished change.
    pub fn table_backup(&self) -> (u64, Vec<u8>) {
        (self.end, self.table())
    }

    /// Puts a saved table back at `end` and cuts the file after it. Everything a later, unfinished change wrote
    /// past `end` is dropped; slots it overwrote belong to files that same change replaces again when it is redone.
    pub fn restore_table(path: &Path, end: u64, table: &[u8]) -> Result<(), String> {
        if crate::core::link_count(path).map_err(|e| e.to_string())? > 1 {
            return Err(format!("{} is linked to another file and will not be changed", path.display()));
        }
        let f = OpenOptions::new().write(true).open(path).map_err(|e| e.to_string())?;
        write_at(&f, table, end).map_err(|e| e.to_string())?;
        f.set_len(end + table.len() as u64).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())
    }

    fn release(&mut self, offset: u64, size: u64) {
        if size > 0 {
            self.free.push((offset, size));
        }
    }

    /// Removes a file; its slot becomes free. False when there is no such file.
    pub fn delete(&mut self, name: &str) -> bool {
        let Some(i) = self.index.remove(name) else { return false };
        let e = self.files.swap_remove(i);
        if let Some(moved) = self.files.get(i) {
            self.index.insert(moved.name.clone(), i);
        }
        self.release(e.offset, e.slot());
        true
    }

    /// A place for `size` bytes: a free slot when one is large enough, else the end of the data.
    fn place(&mut self, size: u64) -> (u64, u32) {
        if let Some(i) = self.free.iter().position(|&(_, s)| s >= size) {
            let (offset, slot) = self.free[i];
            let need = round_up(size);
            if offset % BLOCK == 0 && slot % BLOCK == 0 && slot > need {
                self.free[i] = (offset + need, slot - need);
                return (offset, (need - size) as u32);
            }
            if slot - size <= MAX_PADDING {
                self.free.swap_remove(i);
                return (offset, (slot - size) as u32);
            }
        }
        let offset = self.end;
        self.end += round_up(size);
        (offset, (round_up(size) - size) as u32)
    }

    /// Adds or replaces one file with `size` bytes from `src`. With `md5`, the bytes must have that MD5; without,
    /// the MD5 is computed. `like` gives the times and flags to store.
    pub fn put(&mut self, name: &str, src: &mut dyn Read, size: u64, md5: Option<[u8; 16]>, like: &Entry) -> Result<Put, String> {
        if !self.writable {
            return Err("the archive was opened for reading only".into());
        }
        if !valid_name(name) || name == UNUSED {
            return Err(format!("{name:?} is not a usable file name"));
        }
        let (offset, padding, what) = match self.index.get(name).map(|&i| (self.files[i].offset, self.files[i].slot())) {
            Some((offset, slot)) if size <= slot && slot - size <= MAX_PADDING => (offset, (slot - size) as u32, Put::Replaced),
            Some(_) => {
                self.delete(name);
                let (o, p) = self.place(size);
                (o, p, Put::Moved)
            }
            None => {
                let (o, p) = self.place(size);
                (o, p, Put::Added)
            }
        };
        let mut hash = Md5::new();
        let mut buf = vec![0u8; 1 << 20];
        let (mut at, mut left) = (offset, size);
        while left > 0 {
            let want = (buf.len() as u64).min(left) as usize;
            let n = src.read(&mut buf[..want]).map_err(|e| format!("reading {name}: {e}"))?;
            if n == 0 {
                return Err(format!("{name} ended {left} bytes early"));
            }
            hash.update(&buf[..n]);
            write_at(&self.file, &buf[..n], at).map_err(|e| format!("writing {name}: {e}"))?;
            at += n as u64;
            left -= n as u64;
        }
        let got: [u8; 16] = hash.finalize().into();
        if md5.is_some_and(|want| want != got) {
            return Err(format!("{name} is damaged (its checksum does not match)"));
        }
        let e = Entry { name: name.into(), offset, size, padding, md5: got, ..like.clone() };
        match self.index.get(name) {
            Some(&i) => self.files[i] = e,
            None => {
                self.index.insert(name.into(), self.files.len());
                self.files.push(e);
            }
        }
        Ok(what)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Merged {
    pub replaced: usize,
    pub moved: usize,
    pub added: usize,
    pub deleted: usize,
}

const DELETED: &str = "deleted.txt";
const MAX_DELETED_LOG: u64 = 64 << 20;

/// The files a patch's `deleted.txt` names, as archive names.
pub fn deletions(patch: &Pak) -> Result<Vec<String>, String> {
    let Some(e) = patch.get(DELETED) else { return Ok(Vec::new()) };
    let text = patch.read(e, MAX_DELETED_LOG)?;
    Ok(String::from_utf8_lossy(&text).lines().map(|l| l.trim().replace('\\', "/")).filter(|l| !l.is_empty()).collect())
}

/// Applies one patch archive to `base`; the caller saves `base` afterwards. A patch file whose bytes do not match
/// the patch's own checksum is an error, and the base's table is then not to be saved.
pub fn apply_patch(base: &mut Pak, patch: &Pak, stop: &dyn Fn() -> bool) -> Result<Merged, String> {
    let mut m = Merged::default();
    let zero = [0u8; 16];
    for e in patch.entries() {
        if e.name == DELETED {
            continue;
        }
        if stop() {
            return Err("stopped".into());
        }
        let md5 = (e.md5 != zero).then_some(e.md5);
        match base.put(&e.name, &mut patch.reader(e), e.size, md5, e)? {
            Put::Replaced => m.replaced += 1,
            Put::Moved => m.moved += 1,
            Put::Added => m.added += 1,
        }
    }
    if let Some(list) = patch.get(DELETED) {
        // the base keeps a running log of everything ever removed: the patch's list is added to its end
        let mut log = match base.get(DELETED) {
            Some(e) => base.read(e, MAX_DELETED_LOG)?,
            None => Vec::new(),
        };
        log.extend_from_slice(&patch.read(list, MAX_DELETED_LOG)?);
        base.put(DELETED, &mut &log[..], log.len() as u64, None, list)?;
        for name in deletions(patch)? {
            if name != DELETED && base.delete(&name) {
                m.deleted += 1;
            }
        }
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aacsl-pak-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn like() -> Entry {
        Entry { name: String::new(), offset: 0, size: 0, padding: 0, md5: [0; 16], flags: 0, created: 11, modified: 22, extra: 0 }
    }

    fn add(p: &mut Pak, name: &str, data: &[u8]) -> Put {
        p.put(name, &mut &data[..], data.len() as u64, None, &like()).unwrap()
    }

    fn body(p: &Pak, name: &str) -> Vec<u8> {
        p.read(p.get(name).unwrap(), 1 << 30).unwrap()
    }

    fn bytes(n: usize, seed: u8) -> Vec<u8> {
        (0..n).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
    }

    #[test]
    fn cbc_round_trips_and_chains() {
        let mut a = bytes(RECORD, 3);
        let plain = a.clone();
        cbc(&mut a, true);
        assert_ne!(a, plain);
        // two equal plain blocks must not give two equal cipher blocks
        let mut same = vec![7u8; 32];
        cbc(&mut same, true);
        assert_ne!(same[..16], same[16..]);
        cbc(&mut a, false);
        assert_eq!(a, plain);
    }

    #[test]
    fn written_archive_reads_back() {
        let d = tmp("rw");
        let path = d.join("a_pak");
        let mut p = Pak::create(&path).unwrap();
        assert_eq!(add(&mut p, "game/a.txt", b"hello"), Put::Added);
        assert_eq!(add(&mut p, "game/b.bin", &bytes(1300, 1)), Put::Added);
        p.save().unwrap();
        drop(p);
        assert_eq!(std::fs::metadata(&path).unwrap().len() % BLOCK, 0);
        let p = Pak::open(&path, false).unwrap();
        assert_eq!(p.entries().len(), 2);
        assert_eq!(body(&p, "game/a.txt"), b"hello");
        assert_eq!(body(&p, "game/b.bin"), bytes(1300, 1));
        let a = p.get("game/a.txt").unwrap();
        assert_eq!((a.offset, a.size, a.padding, a.created, a.modified), (0, 5, 507, 11, 22));
        assert_eq!(hex::encode(a.md5), "5d41402abc4b2a76b9719d911017c592");
        assert_eq!(p.get("game/b.bin").unwrap().offset, 512);
    }

    #[test]
    fn replace_in_place_move_and_reuse() {
        let d = tmp("put");
        let path = d.join("a_pak");
        let mut p = Pak::create(&path).unwrap();
        add(&mut p, "a", &bytes(600, 1)); // slot 0..1024
        add(&mut p, "b", &bytes(100, 2)); // slot 1024..1536
        assert_eq!(add(&mut p, "a", &bytes(1000, 3)), Put::Replaced);
        assert_eq!(p.get("a").unwrap().padding, 24);
        assert_eq!(add(&mut p, "a", &bytes(3000, 4)), Put::Moved); // too big: goes to the end, old slot is free
        assert_eq!(p.get("a").unwrap().offset, 1536);
        assert_eq!(p.free_slots(), 1);
        assert_eq!(add(&mut p, "c", &bytes(10, 5)), Put::Added); // takes the first 512 of the free slot
        assert_eq!((p.get("c").unwrap().offset, p.free_slots()), (0, 1));
        assert_eq!(add(&mut p, "d", &bytes(512, 6)), Put::Added); // takes the rest
        assert_eq!((p.get("d").unwrap().offset, p.free_slots()), (512, 0));
        assert!(p.delete("b"));
        assert!(!p.delete("b"));
        p.save().unwrap();
        drop(p);
        let p = Pak::open(&path, false).unwrap();
        assert_eq!(p.entries().len(), 3);
        assert_eq!(p.free_slots(), 1);
        assert_eq!(body(&p, "a"), bytes(3000, 4));
        assert_eq!(body(&p, "c"), bytes(10, 5));
        assert_eq!(body(&p, "d"), bytes(512, 6));
        assert!(p.get("b").is_none());
    }

    #[test]
    fn patch_merge_with_deleted_txt() {
        let d = tmp("merge");
        let mut base = Pak::create(&d.join("base")).unwrap();
        add(&mut base, "game/keep.txt", b"keep");
        add(&mut base, "game/old.txt", b"old");
        add(&mut base, "game/gone/x.dds", &bytes(700, 1));
        add(&mut base, DELETED, b"game\\earlier.txt\n");
        base.save().unwrap();
        let mut patch = Pak::create(&d.join("patch")).unwrap();
        add(&mut patch, "game/old.txt", &bytes(2000, 2));
        add(&mut patch, "game/new.txt", b"new");
        add(&mut patch, DELETED, b"game\\gone\\x.dds\r\ngame\\never_there.txt\r\n\r\n");
        patch.save().unwrap();
        let m = apply_patch(&mut base, &patch, &|| false).unwrap();
        assert_eq!(m, Merged { replaced: 0, moved: 1, added: 1, deleted: 1 });
        base.save().unwrap();
        drop(base);
        let base = Pak::open(&d.join("base"), false).unwrap();
        assert_eq!(base.entries().len(), 4);
        // the base's own log of removed files grows by the patch's list
        assert_eq!(body(&base, DELETED), b"game\\earlier.txt\ngame\\gone\\x.dds\r\ngame\\never_there.txt\r\n\r\n");
        assert_eq!(body(&base, "game/keep.txt"), b"keep");
        assert_eq!(body(&base, "game/old.txt"), bytes(2000, 2));
        assert_eq!(body(&base, "game/new.txt"), b"new");
        assert_eq!(base.get("game/new.txt").unwrap().md5, patch.get("game/new.txt").unwrap().md5);
    }

    #[test]
    fn damaged_patch_file_is_refused() {
        let d = tmp("bad");
        let mut base = Pak::create(&d.join("base")).unwrap();
        let mut wrong = [0u8; 16];
        wrong[0] = 1;
        let err = base.put("a", &mut &b"data"[..], 4, Some(wrong), &like()).unwrap_err();
        assert!(err.contains("damaged"), "{err}");
        assert!(base.get("a").is_none());
        let err = base.put("a", &mut &b"da"[..], 4, None, &like()).unwrap_err();
        assert!(err.contains("early"), "{err}");
    }

    #[test]
    fn restore_table_undoes_an_unfinished_merge() {
        let d = tmp("undo");
        let path = d.join("base");
        let mut base = Pak::create(&path).unwrap();
        add(&mut base, "a", &bytes(600, 1));
        add(&mut base, "b", &bytes(100, 2));
        base.save().unwrap();
        let (end, table) = base.table_backup();
        // an unfinished change: data written over the table, nothing saved
        add(&mut base, "a", &bytes(5000, 9));
        add(&mut base, "c", &bytes(900, 8));
        drop(base);
        assert!(Pak::open(&path, false).is_err() || Pak::open(&path, false).unwrap().get("c").is_none());
        Pak::restore_table(&path, end, &table).unwrap();
        let base = Pak::open(&path, false).unwrap();
        assert_eq!(base.entries().len(), 2);
        assert_eq!(body(&base, "b"), bytes(100, 2));
        assert_eq!(base.get("a").unwrap().size, 600);
    }

    #[test]
    fn rejects_what_is_not_an_archive() {
        let d = tmp("not");
        let path = d.join("x");
        std::fs::write(&path, vec![0u8; 4096]).unwrap();
        assert!(Pak::open(&path, false).is_err());
        std::fs::write(&path, b"short").unwrap();
        assert!(Pak::open(&path, false).is_err());
        // a table that claims more records than the file can hold
        let mut p = Pak::create(&d.join("y")).unwrap();
        add(&mut p, "a", b"1");
        p.save().unwrap();
        let (end, mut table) = p.table_backup();
        drop(p);
        let n = table.len();
        let head = &mut table[n - 512..n - 480];
        cbc(head, false);
        head[8..12].copy_from_slice(&3_000_000u32.to_le_bytes());
        cbc(head, true);
        Pak::restore_table(&d.join("y"), end, &table).unwrap();
        assert!(Pak::open(&d.join("y"), false).is_err());
    }

    #[test]
    fn a_linked_file_is_never_opened_for_writing() {
        let d = tmp("link");
        let path = d.join("base");
        Pak::create(&path).unwrap();
        std::fs::hard_link(&path, d.join("other")).unwrap();
        assert!(Pak::open(&path, true).is_err_and(|e| e.contains("linked")));
        assert!(Pak::open(&path, false).is_ok());
    }
}
