//! Just enough LevelDB to read the client's `USER\udf` store and to write a fresh one.
//!
//! The client keeps its interface data (camera, nametags, volumes, interface scale, where the action bars dock) in a
//! LevelDB under `Documents\AAClassic\USER\udf`, keyed `<account>:<world>:<character>:<name>`. AA Classic's own store
//! is never opened in place: [`read_copy`] copies the files into a temporary folder first and reads the copy, so the
//! real folder is only ever read. Writing builds a brand-new store in a new folder ([`write_fresh`]): a MANIFEST that
//! names one log file, and that log holding every value. The client's LevelDB replays the log when it opens the store,
//! exactly as after a crash, and writes its own tables from it.
//!
//! What is read: `.log` files (write-ahead logs) and `.ldb`/`.sst` tables, raw or Snappy-compressed blocks. The newest
//! sequence number of a key wins and a deletion removes it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const BLOCK: usize = 32768;
const TABLE_MAGIC: u64 = 0xdb4775248b80fb57;
/// A store bigger than this is not the client's interface store.
const MAX_STORE: u64 = 64 << 20;

/// Every live key of a store with its value and the sequence number that wrote it.
pub type Store = BTreeMap<Vec<u8>, (u64, Vec<u8>)>;

// ---- checksums ------------------------------------------------------------------------------------------------

fn crc32c_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    for (i, e) in t.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0x82F63B78 ^ (c >> 1) } else { c >> 1 };
        }
        *e = c;
    }
    t
}

pub fn crc32c(parts: &[&[u8]]) -> u32 {
    let t = crc32c_table();
    let mut c = !0u32;
    for p in parts {
        for &b in *p {
            c = t[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
        }
    }
    !c
}

fn mask(crc: u32) -> u32 {
    crc.rotate_right(15).wrapping_add(0xa282ead8)
}

// ---- varints ----------------------------------------------------------------------------------------------------

fn varint(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *b.get(*at)?;
        *at += 1;
        v |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

fn bytes<'a>(b: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
    let n = varint(b, at)? as usize;
    let s = b.get(*at..at.checked_add(n)?)?;
    *at += n;
    Some(s)
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn put_bytes(out: &mut Vec<u8>, s: &[u8]) {
    put_varint(out, s.len() as u64);
    out.extend_from_slice(s);
}

// ---- snappy (decompression only) --------------------------------------------------------------------------------

pub fn unsnappy(src: &[u8]) -> Option<Vec<u8>> {
    let mut at = 0;
    let len = varint(src, &mut at)? as usize;
    if len > (MAX_STORE as usize) {
        return None;
    }
    let mut out = Vec::with_capacity(len);
    while at < src.len() {
        let tag = src[at];
        at += 1;
        match tag & 3 {
            0 => {
                let mut n = (tag >> 2) as usize;
                if n >= 60 {
                    let extra = n - 59;
                    let b = src.get(at..at + extra)?;
                    n = b.iter().rev().fold(0usize, |a, &x| (a << 8) | x as usize);
                    at += extra;
                }
                let n = n + 1;
                out.extend_from_slice(src.get(at..at + n)?);
                at += n;
            }
            kind => {
                let (n, off) = match kind {
                    1 => {
                        let b = *src.get(at)? as usize;
                        at += 1;
                        (((tag >> 2) & 7) as usize + 4, (((tag >> 5) as usize) << 8) | b)
                    }
                    2 => {
                        let b = src.get(at..at + 2)?;
                        at += 2;
                        ((tag >> 2) as usize + 1, u16::from_le_bytes([b[0], b[1]]) as usize)
                    }
                    _ => {
                        let b = src.get(at..at + 4)?;
                        at += 4;
                        ((tag >> 2) as usize + 1, u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
                    }
                };
                if off == 0 || off > out.len() {
                    return None;
                }
                let start = out.len() - off;
                for i in 0..n {
                    let c = out[start + i];
                    out.push(c);
                }
            }
        }
        if out.len() > len {
            return None;
        }
    }
    (out.len() == len).then_some(out)
}

// ---- reading ----------------------------------------------------------------------------------------------------

fn apply(store: &mut Store, key: &[u8], seq: u64, value: Option<&[u8]>) {
    if store.get(key).is_some_and(|(s, _)| *s > seq) {
        return;
    }
    match value {
        Some(v) => {
            store.insert(key.to_vec(), (seq, v.to_vec()));
        }
        // a deletion: remembered with an empty marker so that an older value read later cannot come back
        None => {
            store.insert(key.to_vec(), (seq, DELETED.to_vec()));
        }
    }
}

const DELETED: &[u8] = b"\0\xffdeleted\xff\0";

/// The records of a log-format file (a `.log` or a MANIFEST), joined across blocks. A damaged record ends the file.
pub fn log_records(data: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut pending: Option<Vec<u8>> = None;
    let mut at = 0;
    while at + 7 <= data.len() {
        let left = BLOCK - (at % BLOCK);
        if left < 7 {
            at += left;
            continue;
        }
        let crc = u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
        let len = u16::from_le_bytes([data[at + 4], data[at + 5]]) as usize;
        let kind = data[at + 6];
        if kind == 0 && len == 0 {
            // preallocated zeros
            at += left;
            continue;
        }
        let Some(body) = data.get(at + 7..at + 7 + len) else { break };
        if mask(crc32c(&[&[kind], body])) != crc {
            break;
        }
        at += 7 + len;
        match kind {
            1 => out.push(body.to_vec()),
            2 => pending = Some(body.to_vec()),
            3 => {
                if let Some(p) = pending.as_mut() {
                    p.extend_from_slice(body)
                }
            }
            4 => {
                if let Some(mut p) = pending.take() {
                    p.extend_from_slice(body);
                    out.push(p);
                }
            }
            _ => break,
        }
    }
    out
}

fn read_batch(store: &mut Store, b: &[u8]) -> Option<()> {
    let seq = u64::from_le_bytes(b.get(0..8)?.try_into().ok()?);
    let count = u32::from_le_bytes(b.get(8..12)?.try_into().ok()?);
    let mut at = 12;
    for i in 0..count as u64 {
        let tag = *b.get(at)?;
        at += 1;
        let key = bytes(b, &mut at)?;
        match tag {
            1 => {
                let v = bytes(b, &mut at)?;
                apply(store, key, seq + i, Some(v));
            }
            0 => apply(store, key, seq + i, None),
            _ => return None,
        }
    }
    Some(())
}

fn block_entries(block: &[u8], mut each: impl FnMut(&[u8], &[u8])) -> Option<()> {
    let n = u32::from_le_bytes(block.get(block.len().checked_sub(4)?..)?.try_into().ok()?) as usize;
    let end = block.len().checked_sub(4 + n * 4)?;
    let mut at = 0;
    let mut key: Vec<u8> = Vec::new();
    while at < end {
        let shared = varint(block, &mut at)? as usize;
        let own = varint(block, &mut at)? as usize;
        let vlen = varint(block, &mut at)? as usize;
        if shared > key.len() {
            return None;
        }
        key.truncate(shared);
        key.extend_from_slice(block.get(at..at + own)?);
        at += own;
        let v = block.get(at..at + vlen)?;
        at += vlen;
        each(&key, v);
    }
    Some(())
}

fn read_block(file: &[u8], handle: &[u8]) -> Option<Vec<u8>> {
    let mut at = 0;
    let off = varint(handle, &mut at)? as usize;
    let size = varint(handle, &mut at)? as usize;
    let raw = file.get(off..off.checked_add(size)?)?;
    let kind = *file.get(off + size)?;
    match kind {
        0 => Some(raw.to_vec()),
        1 => unsnappy(raw),
        _ => None,
    }
}

fn read_table(store: &mut Store, file: &[u8]) -> Option<()> {
    if file.len() < 48 {
        return None;
    }
    let footer = &file[file.len() - 48..];
    if u64::from_le_bytes(footer[40..48].try_into().ok()?) != TABLE_MAGIC {
        return None;
    }
    let mut at = 0;
    bytes_handle(footer, &mut at)?; // metaindex
    let index_at = at;
    bytes_handle(footer, &mut at)?;
    let index = read_block(file, &footer[index_at..at])?;
    let mut handles = Vec::new();
    block_entries(&index, |_, v| handles.push(v.to_vec()))?;
    for h in handles {
        let data = read_block(file, &h)?;
        block_entries(&data, |ikey, v| {
            if ikey.len() >= 8 {
                let (k, tail) = ikey.split_at(ikey.len() - 8);
                let n = u64::from_le_bytes(tail.try_into().unwrap());
                apply(store, k, n >> 8, if n & 0xff == 1 { Some(v) } else { None });
            }
        })?;
    }
    Some(())
}

fn bytes_handle(b: &[u8], at: &mut usize) -> Option<()> {
    varint(b, at)?;
    varint(b, at)?;
    Some(())
}

/// Reads a store folder (never writes to it). Damaged files are skipped.
pub fn read_dir(dir: &Path) -> Result<Store, String> {
    let mut store = Store::new();
    let mut total = 0u64;
    let entries = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    files.sort();
    for f in files {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
        let table = name.ends_with(".ldb") || name.ends_with(".sst");
        if !table && !name.ends_with(".log") {
            continue;
        }
        total += fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
        if total > MAX_STORE {
            return Err("that store is larger than the client's interface store can be".into());
        }
        let data = fs::read(&f).map_err(|e| format!("cannot read {}: {e}", f.display()))?;
        if table {
            let _ = read_table(&mut store, &data);
        } else {
            for r in log_records(&data) {
                let _ = read_batch(&mut store, &r);
            }
        }
    }
    store.retain(|_, (_, v)| v.as_slice() != DELETED);
    Ok(store)
}

/// Copies a store folder into a new temporary folder and reads the copy: the original is opened for reading only,
/// file by file, and never locked or changed (the client may have it open).
pub fn read_copy(dir: &Path) -> Result<Store, String> {
    let tmp = std::env::temp_dir().join(format!("aaemu-udf-{}-{}", std::process::id(), nonce()));
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let r = (|| {
        for e in fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?.flatten() {
            let p = e.path();
            let name = e.file_name();
            let n = name.to_string_lossy().to_ascii_lowercase();
            if p.is_file() && (n.ends_with(".ldb") || n.ends_with(".sst") || n.ends_with(".log")) {
                let data = fs::read(&p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
                fs::write(tmp.join(&name), data).map_err(|e| e.to_string())?;
            }
        }
        read_dir(&tmp)
    })();
    let _ = fs::remove_dir_all(&tmp);
    r
}

fn nonce() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

// ---- writing ----------------------------------------------------------------------------------------------------

/// Records in LevelDB's log format: 32 KiB blocks, each record split over blocks as needed.
pub fn log_file(records: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for r in records {
        let mut rest: &[u8] = r;
        let mut first = true;
        loop {
            let left = BLOCK - (out.len() % BLOCK);
            if left < 7 {
                out.extend(std::iter::repeat_n(0u8, left));
                continue;
            }
            let room = left - 7;
            let n = rest.len().min(room);
            let last = n == rest.len();
            let kind: u8 = match (first, last) {
                (true, true) => 1,
                (true, false) => 2,
                (false, false) => 3,
                (false, true) => 4,
            };
            let (part, tail) = rest.split_at(n);
            out.extend_from_slice(&mask(crc32c(&[&[kind], part])).to_le_bytes());
            out.extend_from_slice(&(n as u16).to_le_bytes());
            out.push(kind);
            out.extend_from_slice(part);
            rest = tail;
            first = false;
            if last {
                break;
            }
        }
    }
    out
}

/// Writes a new store holding exactly `values` into `dir`, which must not exist yet.
pub fn write_fresh(dir: &Path, values: &BTreeMap<Vec<u8>, Vec<u8>>) -> Result<(), String> {
    if dir.exists() {
        return Err(format!("{} already exists", dir.display()));
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut batch = Vec::new();
    batch.extend_from_slice(&1u64.to_le_bytes());
    batch.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for (k, v) in values {
        batch.push(1);
        put_bytes(&mut batch, k);
        put_bytes(&mut batch, v);
    }
    let mut edit = Vec::new();
    put_varint(&mut edit, 1);
    put_bytes(&mut edit, b"leveldb.BytewiseComparator");
    put_varint(&mut edit, 2);
    put_varint(&mut edit, 3); // log number
    put_varint(&mut edit, 9);
    put_varint(&mut edit, 0); // previous log number
    put_varint(&mut edit, 3);
    put_varint(&mut edit, 4); // next file number
    put_varint(&mut edit, 4);
    put_varint(&mut edit, values.len() as u64); // last sequence
    let w = |name: &str, data: &[u8]| fs::write(dir.join(name), data).map_err(|e| format!("cannot write {name}: {e}"));
    w("000003.log", &log_file(&[batch]))?;
    w("MANIFEST-000002", &log_file(&[edit]))?;
    w("CURRENT", b"MANIFEST-000002\n")
}

/// The values of a store without their sequence numbers.
pub fn values(store: &Store) -> BTreeMap<Vec<u8>, Vec<u8>> {
    store.iter().map(|(k, (_, v))| (k.clone(), v.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aaemu-ldb-{name}-{}-{}", std::process::id(), nonce()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn crc32c_matches_the_standard_check_value() {
        assert_eq!(crc32c(&[b"123456789"]), 0xE3069283);
        assert_eq!(crc32c(&[b"1234", b"56789"]), 0xE3069283);
    }

    #[test]
    fn a_written_store_reads_back() {
        let d = tmp("rw");
        let mut v = BTreeMap::new();
        v.insert(b"0:255:0:system_option".to_vec(), b"ui_scale 1.1\r\nversion 3\r\n".to_vec());
        // a value bigger than a block, split over three blocks
        v.insert(b"1:1:7:common_local_data_2".to_vec(), vec![b'x'; 70_000]);
        write_fresh(&d, &v).unwrap();
        assert_eq!(fs::read_to_string(d.join("CURRENT")).unwrap(), "MANIFEST-000002\n");
        let back = read_dir(&d).unwrap();
        assert_eq!(values(&back), v);
        assert!(write_fresh(&d, &v).is_err(), "never writes over a store");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_newest_sequence_wins_and_a_deletion_removes() {
        let mut b1 = Vec::new();
        b1.extend_from_slice(&5u64.to_le_bytes());
        b1.extend_from_slice(&2u32.to_le_bytes());
        b1.push(1);
        put_bytes(&mut b1, b"a");
        put_bytes(&mut b1, b"old");
        b1.push(1);
        put_bytes(&mut b1, b"b");
        put_bytes(&mut b1, b"keep");
        let mut b2 = Vec::new();
        b2.extend_from_slice(&9u64.to_le_bytes());
        b2.extend_from_slice(&2u32.to_le_bytes());
        b2.push(1);
        put_bytes(&mut b2, b"a");
        put_bytes(&mut b2, b"new");
        b2.push(0);
        put_bytes(&mut b2, b"b");
        let d = tmp("seq");
        fs::create_dir_all(&d).unwrap();
        // the newer batch in the older-named file: order of files must not matter
        fs::write(d.join("000001.log"), log_file(&[b2])).unwrap();
        fs::write(d.join("000002.log"), log_file(&[b1])).unwrap();
        let s = read_dir(&d).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[&b"a".to_vec()], (9, b"new".to_vec()));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_table_with_raw_and_snappy_blocks_reads() {
        // build a one-block table by hand: data block, empty metaindex, index pointing at the data block
        fn block(entries: &[(&[u8], &[u8])]) -> Vec<u8> {
            let mut b = Vec::new();
            for (k, v) in entries {
                put_varint(&mut b, 0);
                put_varint(&mut b, k.len() as u64);
                put_varint(&mut b, v.len() as u64);
                b.extend_from_slice(k);
                b.extend_from_slice(v);
            }
            b.extend_from_slice(&0u32.to_le_bytes());
            b.extend_from_slice(&1u32.to_le_bytes());
            b
        }
        fn ikey(k: &[u8], seq: u64, kind: u64) -> Vec<u8> {
            let mut x = k.to_vec();
            x.extend_from_slice(&((seq << 8) | kind).to_le_bytes());
            x
        }
        let k1 = ikey(b"0:255:0:system_option", 7, 1);
        let data = block(&[(&k1, b"hello hello hello hello")]);
        // the same block compressed: a snappy literal of the whole block
        let mut snap = Vec::new();
        put_varint(&mut snap, data.len() as u64);
        snap.push(60 << 2); // a literal whose length-1 follows in one byte
        snap.push((data.len() - 1) as u8);
        snap.extend_from_slice(&data);
        assert_eq!(unsnappy(&snap).unwrap(), data);
        for (body, kind) in [(data.clone(), 0u8), (snap, 1u8)] {
            let mut file = Vec::new();
            let mut h = Vec::new();
            put_varint(&mut h, 0);
            put_varint(&mut h, body.len() as u64);
            file.extend_from_slice(&body);
            file.push(kind);
            file.extend_from_slice(&[0; 4]);
            let meta_at = file.len();
            let meta = block(&[]);
            let meta = &meta[..]; // an empty block: just its restart array
            file.extend_from_slice(meta);
            file.push(0);
            file.extend_from_slice(&[0; 4]);
            let index_at = file.len();
            let index = block(&[(&k1, &h)]);
            file.extend_from_slice(&index);
            file.push(0);
            file.extend_from_slice(&[0; 4]);
            let mut footer = Vec::new();
            put_varint(&mut footer, meta_at as u64);
            put_varint(&mut footer, meta.len() as u64);
            put_varint(&mut footer, index_at as u64);
            put_varint(&mut footer, index.len() as u64);
            footer.resize(40, 0);
            footer.extend_from_slice(&TABLE_MAGIC.to_le_bytes());
            file.extend_from_slice(&footer);
            let mut s = Store::new();
            read_table(&mut s, &file).unwrap();
            assert_eq!(s[&b"0:255:0:system_option".to_vec()], (7, b"hello hello hello hello".to_vec()));
        }
    }

    #[test]
    fn snappy_copies_work() {
        // "abcabcabcabc": literal "abc" then a copy of 9 bytes at offset 3
        let src = [12u8, 2 << 2, b'a', b'b', b'c', ((9 - 4) << 2) | 1, 3];
        assert_eq!(unsnappy(&src).unwrap(), b"abcabcabcabc");
        assert!(unsnappy(&[5, 0 << 2, b'a']).is_none(), "a short answer is refused");
    }

    #[test]
    fn reading_a_copy_leaves_the_folder_as_it_was() {
        let d = tmp("copy");
        let mut v = BTreeMap::new();
        v.insert(b"k".to_vec(), b"v".to_vec());
        write_fresh(&d, &v).unwrap();
        let before: Vec<_> = fs::read_dir(&d).unwrap().flatten().map(|e| (e.file_name(), fs::read(e.path()).unwrap())).collect();
        assert_eq!(values(&read_copy(&d).unwrap()), v);
        let after: Vec<_> = fs::read_dir(&d).unwrap().flatten().map(|e| (e.file_name(), fs::read(e.path()).unwrap())).collect();
        assert_eq!(before, after);
        let _ = fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod real {
    /// cargo test real_store -- --ignored --nocapture, with AAEMU_TEST_UDF=<folder>
    #[test]
    #[ignore]
    fn real_store() {
        let d = std::env::var("AAEMU_TEST_UDF").unwrap();
        let s = super::read_copy(std::path::Path::new(&d)).unwrap();
        for (k, (seq, v)) in &s {
            let k = String::from_utf8_lossy(k);
            let parts: Vec<&str> = k.split(':').collect();
            let masked: Vec<String> = parts.iter().enumerate().map(|(i, p)| if i < 3 && p.len() > 3 { format!("#{}", p.len()) } else { p.to_string() }).collect();
            println!("{:<60} seq {seq:<6} {} bytes", masked.join(":"), v.len());
        }
        if let Some((_, v)) = s.get(&b"0:255:0:system_option".to_vec()) {
            let t = String::from_utf8_lossy(v);
            println!("--- system_option names: {}", t.lines().filter_map(|l| l.split_whitespace().next()).collect::<Vec<_>>().join(" "));
        }
    }
}
