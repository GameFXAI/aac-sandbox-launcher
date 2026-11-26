//! The replay Library's state outside the window: the last list the server gave (shown at once next time while a
//! fresh one is fetched), the one-shot file that has the game's replay player open a recording, and the dates the
//! Library shows.
//!
//! - `<data>\recordings\library.json`  the last list, with the server and account it belongs to
//! - `<profile>\Documents\AAClassic\Addon\aac_replay\pending_watch.txt`  `{ n = 5, }`: the recording Play in the
//!   Library asked for. The server's aac_replay addon takes it once when the game loads (it empties the file) and
//!   watches that recording through the game's own replay player. The launcher never sees a fight's packets.
//!
//! Nothing here is trusted by the server: the plan, the cap and the list are what the server last said, kept for
//! display only.

use crate::core::Paths;
use crate::net::Library;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
struct Cached {
    server: String,
    username: String,
    library: Library,
}

fn cache_file(paths: &Paths) -> PathBuf {
    crate::recorder::data_dir(paths).join("library.json")
}

/// The last list fetched for this server and account, if there is one.
pub fn load_cached(paths: &Paths, server: &str, username: &str) -> Option<Library> {
    let c: Cached = serde_json::from_slice(&fs::read(cache_file(paths)).ok()?).ok()?;
    (c.server == server.trim() && c.username.eq_ignore_ascii_case(username.trim())).then_some(c.library)
}

pub fn save_cached(paths: &Paths, server: &str, username: &str, library: &Library) {
    let c = Cached { server: server.trim().into(), username: username.trim().into(), library: library.clone() };
    let file = cache_file(paths);
    if let Some(d) = file.parent() {
        let _ = fs::create_dir_all(d);
    }
    if let Ok(b) = serde_json::to_vec_pretty(&c) {
        let _ = fs::write(file, b);
    }
}

/// Forgets the cached list (sign-out: the next account must not see this one's).
pub fn forget_cached(paths: &Paths) {
    let _ = fs::remove_file(cache_file(paths));
}

fn pending_file(paths: &Paths) -> PathBuf {
    crate::addons::addon_dir(paths).join("aac_replay").join("pending_watch.txt")
}

/// Leaves the recording `n` for the game's replay player, or (None) makes sure no older one is left: written right
/// before the game starts, after the server's addons are in place (installing them replaces the addon's folder).
pub fn set_pending_watch(paths: &Paths, n: Option<i32>) -> Result<(), String> {
    let file = pending_file(paths);
    match n {
        Some(n) => {
            let dir = file.parent().unwrap();
            if !dir.is_dir() {
                return Err("the server's replay player (aac_replay) is not installed in this client".into());
            }
            // the format the addon's own api.File:Write uses, which its api.File:Read reads back as a table
            fs::write(&file, format!("{{\n    n = {n},\n}}\n")).map_err(|e| format!("cannot leave the replay for the game: {e}"))
        }
        None => match fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        },
    }
}

// ---- dates ------------------------------------------------------------------------------------------------------

/// Days since 1970-01-01 of a civil date (Howard Hinnant's days_from_civil).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The civil date of a day count since 1970-01-01 (civil_from_days).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// An ISO 8601 time as the server writes it ("2027-01-01T00:00:00Z", "2027-01-01T00:00:00.123+02:00"), in Unix
/// seconds; None when it is not one.
pub fn parse_iso(t: &str) -> Option<i64> {
    let t = t.trim();
    let (date, rest) = t.split_once(['T', ' '])?;
    let mut d = date.splitn(3, '-').map(|x| x.parse::<i64>().ok());
    let (y, mo, da) = (d.next()??, d.next()??, d.next()??);
    let (clock, offset) = match rest.find(['Z', 'z', '+', '-']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let mut c = clock.splitn(3, ':');
    let h = c.next()?.parse::<i64>().ok()?;
    let mi = c.next()?.parse::<i64>().ok()?;
    let s = c.next().map(|s| s.split('.').next().unwrap_or("0").parse::<i64>().ok()).unwrap_or(Some(0))?;
    let off = match offset.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let (oh, om) = offset[1..].split_once(':').unwrap_or((&offset[1..], "0"));
            let v = oh.parse::<i64>().ok()? * 3600 + om.parse::<i64>().ok()? * 60;
            if sign == '+' { v } else { -v }
        }
        _ => 0,
    };
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    Some(days_from_civil(y, mo, da) * 86400 + h * 3600 + mi * 60 + s - off)
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Whole days left until `until` (Unix seconds), rounded up; 0 once it has passed.
pub fn days_left(until: i64, now: i64) -> i64 {
    if until <= now { 0 } else { (until - now + 86399) / 86400 }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// "1 Jan 2027" for Unix seconds (UTC: an expiry is a day, not a moment).
pub fn date(unix: i64) -> String {
    let (y, m, d) = civil_from_days(unix.div_euclid(86400));
    format!("{d} {} {y}", MONTHS[(m - 1) as usize])
}

/// The PC's offset from UTC at `unix`, in seconds (Windows' own time zone rules, summer time included).
fn local_offset(unix: i64) -> i64 {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
    let ticks = (unix + 11_644_473_600) as u64 * 10_000_000;
    let utc = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    let mut local = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    if unsafe { FileTimeToLocalFileTime(&utc, &mut local) } == 0 {
        return 0;
    }
    let l = ((local.dwHighDateTime as u64) << 32) | local.dwLowDateTime as u64;
    (l as i64 - ticks as i64) / 10_000_000
}

/// "Today", "Yesterday", "21 Nov" or "21 Nov 2025" for a recording's start (Unix ms), in the PC's own time.
pub fn day(unix_ms: i64, now: i64) -> String {
    if unix_ms <= 0 {
        return "Unknown day".into();
    }
    let t = unix_ms / 1000;
    let d = (t + local_offset(t)).div_euclid(86400);
    let today = (now + local_offset(now)).div_euclid(86400);
    match today - d {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        _ => {
            let (y, m, dd) = civil_from_days(d);
            let (ny, _, _) = civil_from_days(today);
            let year = if y == ny { String::new() } else { format!(" {y}") };
            format!("{dd} {}{year}", MONTHS[(m - 1) as usize])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Replay;

    fn tmp(name: &str) -> Paths {
        let d = std::env::temp_dir().join(format!("aaemu-library-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        Paths::at(d)
    }

    #[test]
    fn iso_times_as_the_server_writes_them() {
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso("2027-01-01T00:00:00Z"), Some(1_798_761_600));
        assert_eq!(parse_iso("2027-01-01T00:00:00.1234567Z"), Some(1_798_761_600));
        assert_eq!(parse_iso("2027-01-01T02:00:00+02:00"), Some(1_798_761_600));
        assert_eq!(parse_iso("2026-12-31T19:00:00-05:00"), Some(1_798_761_600));
        assert_eq!(parse_iso("2024-02-29T12:30:15"), Some(1_709_209_815));
        assert_eq!(parse_iso("not a date"), None);
        assert_eq!(parse_iso("2027-13-01T00:00:00Z"), None);
        assert_eq!(date(1_798_761_600), "1 Jan 2027");
        assert_eq!(date(1_709_209_815), "29 Feb 2024");
    }

    #[test]
    fn days_left_round_up_and_stop_at_zero() {
        assert_eq!(days_left(100, 100), 0);
        assert_eq!(days_left(100, 200), 0);
        assert_eq!(days_left(86_400, 0), 1);
        assert_eq!(days_left(86_401, 0), 2);
        assert_eq!(days_left(3 * 86_400 - 5, 0), 3);
    }

    #[test]
    fn days_read_as_people_say_them() {
        let now = parse_iso("2026-11-25T12:00:00Z").unwrap();
        assert_eq!(day(now * 1000, now), "Today");
        assert_eq!(day((now - 86_400) * 1000, now), "Yesterday");
        let d = day(parse_iso("2026-11-10T12:00:00Z").unwrap() * 1000, now);
        assert!(d.contains("Nov") && !d.contains("2026"), "{d}");
        assert!(day(parse_iso("2025-01-21T12:00:00Z").unwrap() * 1000, now).ends_with("2025"));
    }

    #[test]
    fn the_cache_belongs_to_one_server_and_account() {
        let p = tmp("cache");
        let l = Library { plan: "free".into(), cap: 3, replays: vec![Replay { n: 5, title: "beat Opponent 0:42".into(), ..Default::default() }], ..Default::default() };
        assert!(load_cached(&p, "https://a", "friend").is_none());
        save_cached(&p, "https://a", "friend", &l);
        assert_eq!(load_cached(&p, "https://a ", "Friend"), Some(l.clone()));
        assert!(load_cached(&p, "https://b", "friend").is_none());
        assert!(load_cached(&p, "https://a", "other").is_none());
        forget_cached(&p);
        assert!(load_cached(&p, "https://a", "friend").is_none());
    }

    #[test]
    fn the_pending_watch_is_the_addons_table_and_goes_away() {
        let p = tmp("pending");
        // no replay player in this client: said, not silently skipped
        assert!(set_pending_watch(&p, Some(5)).unwrap_err().contains("aac_replay"));
        let dir = crate::addons::addon_dir(&p).join("aac_replay");
        fs::create_dir_all(&dir).unwrap();
        set_pending_watch(&p, Some(5)).unwrap();
        assert_eq!(fs::read_to_string(dir.join("pending_watch.txt")).unwrap(), "{\n    n = 5,\n}\n");
        set_pending_watch(&p, None).unwrap();
        assert!(!dir.join("pending_watch.txt").exists());
        set_pending_watch(&p, None).unwrap(); // nothing to remove is fine
    }
}
