//! Copying the player's graphics and sound settings from AA Classic into our client's own settings file.
//!
//! Only when the player asks for it. The real `Documents\AAClassic\system.cfg` is opened for reading and nothing in
//! that folder is ever written. Only the settings named below are taken: display, quality and sound. Never a
//! server address, a language or locale, or anything about an account; every other line of our own file stays.

use crate::core::Paths;
use std::fs;
use std::path::{Path, PathBuf};

/// Settings that are taken when their name is exactly one of these ...
const NAMES: &[&str] = &["sys_spec_full", "r_driver", "r_multithreaded", "r_windowx", "r_windowy", "r_width", "r_height", "r_fullscreen", "r_vsync", "r_pixelsync", "login_first_movie"];
/// ... or starts with one of these (the game's own options: quality levels, effects, sound volumes).
const PREFIXES: &[&str] = &["option_", "e_use_"];
const MAX_FILE: u64 = 1 << 20;

fn allowed(name: &str) -> bool {
    NAMES.contains(&name) || PREFIXES.iter().any(|p| name.starts_with(p))
}

/// `name = value` of a settings line; None for comments, blank lines and anything else.
fn parse(line: &str) -> Option<(&str, &str)> {
    let l = line.trim();
    if l.is_empty() || l.starts_with("--") {
        return None;
    }
    let (name, value) = l.split_once('=')?;
    let name = name.trim();
    let ok = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then_some((name, value.trim()))
}

/// The allowed settings of `theirs` laid over `ours`: a setting we already have gets their value in place, new
/// ones go to the end, everything else of ours stays as it is. Returns the new text and how many were taken.
pub fn merge(ours: &str, theirs: &str) -> (String, usize) {
    let mut take: Vec<(&str, &str)> = Vec::new();
    for (name, value) in theirs.lines().filter_map(parse) {
        // a value is one plain token or a quoted word: nothing that could carry a second setting
        let plain = !value.is_empty() && value.len() <= 64 && value.chars().all(|c| c.is_ascii_alphanumeric() || "._-\"".contains(c));
        if allowed(name) && plain && !take.iter().any(|(n, _)| *n == name) {
            take.push((name, value));
        }
    }
    let mut used = vec![false; take.len()];
    let mut out: Vec<String> = Vec::new();
    for line in ours.lines() {
        match parse(line).and_then(|(name, _)| take.iter().position(|(n, _)| *n == name)) {
            Some(i) => {
                if !used[i] {
                    out.push(format!("{} = {}", take[i].0, take[i].1));
                    used[i] = true;
                }
            }
            None => out.push(line.to_string()),
        }
    }
    for (i, (name, value)) in take.iter().enumerate() {
        if !used[i] {
            out.push(format!("{name} = {value}"));
        }
    }
    (out.join("\r\n") + "\r\n", take.len())
}

/// `ours` with `locale = <lang>` as its language line (in place when there is one, else at the end).
fn with_language(ours: &str, lang: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut done = false;
    for line in ours.lines() {
        if parse(line).is_some_and(|(name, _)| name == "locale") {
            if !done {
                out.push(format!("locale = {lang}"));
                done = true;
            }
        } else {
            out.push(line.to_string());
        }
    }
    if !done {
        out.push(format!("locale = {lang}"));
    }
    out.join("\r\n") + "\r\n"
}

/// What our client's own settings file must hold before a start:
/// - the language: without that line the client shows its interface in Korean, whatever `--lang` says;
/// - `login_first_movie = 1` when the file says nothing about it: otherwise the client plays its intro movie for
///   about 150 seconds before it logs in, and a launch ticket is long dead by then. A value already there stays.
/// The file is written only when something changes.
pub fn set_language(paths: &Paths, lang: &str) -> Result<(), String> {
    if lang.is_empty() || lang.len() > 16 || !lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("the language is a short name such as en_us".into());
    }
    let file = paths.docs().join("system.cfg");
    let current = fs::read(&file).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
    let has = |n: &str| current.lines().filter_map(parse).any(|(name, _)| name == n);
    let movie = has("login_first_movie");
    if movie && current.lines().filter_map(parse).any(|(name, value)| name == "locale" && value.trim_matches('"') == lang) {
        return Ok(());
    }
    let mut text = with_language(&current, lang);
    if !movie {
        text.push_str("login_first_movie = 1\r\n");
    }
    fs::create_dir_all(paths.docs()).map_err(|e| e.to_string())?;
    fs::write(&file, text).map_err(|e| format!("cannot write the settings: {e}"))
}

/// The player's real settings file, if there is one.
pub fn real_file() -> Option<PathBuf> {
    let mut places = Vec::new();
    if let Ok(p) = std::env::var("USERPROFILE") {
        places.push(PathBuf::from(p).join("Documents"));
    }
    for v in ["OneDrive", "OneDriveConsumer"] {
        if let Ok(p) = std::env::var(v) {
            places.push(PathBuf::from(p).join("Documents"));
        }
    }
    places.into_iter().map(|d| d.join("AAClassic").join("system.cfg")).find(|f| f.is_file())
}

/// Copies the allowed settings from `from` into our client's settings file. Returns how many.
pub fn copy_from(paths: &Paths, from: &Path) -> Result<usize, String> {
    let ours = paths.docs().join("system.cfg");
    if crate::core::is_inside(from, &paths.root) {
        return Err("that is the launcher's own settings file".into());
    }
    if fs::metadata(from).map_err(|e| format!("cannot read {}: {e}", from.display()))?.len() > MAX_FILE {
        return Err("that settings file is larger than a settings file can be".into());
    }
    let theirs = fs::read(from).map_err(|e| format!("cannot read {}: {e}", from.display()))?;
    let current = fs::read(&ours).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
    let (text, n) = merge(&current, &String::from_utf8_lossy(&theirs));
    if n == 0 {
        return Err("no display or sound settings were found in that file".into());
    }
    fs::create_dir_all(paths.docs()).map_err(|e| e.to_string())?;
    fs::write(&ours, text).map_err(|e| format!("cannot write the settings: {e}"))?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_display_and_quality_and_nothing_else() {
        let theirs = "-- [SYSTEM CONFIGURATION]\r\nr_width = 2560\r\nr_height = 1080\r\noption_sound = 4\r\ne_use_gem_effect = 1\r\ncl_serveraddr = 1.2.3.4\r\nlocale = \"en_us\"\r\nauth_ip = 9.9.9.9\r\nuid = someone\r\nr_driver = \"DX10\"\r\n";
        let (text, n) = merge("", theirs);
        assert_eq!(n, 5);
        assert_eq!(text, "r_width = 2560\r\nr_height = 1080\r\noption_sound = 4\r\ne_use_gem_effect = 1\r\nr_driver = \"DX10\"\r\n");
    }

    #[test]
    fn our_own_lines_stay_and_values_are_replaced_in_place() {
        let ours = "-- ours\r\nr_width = 800\r\ncl_serveraddr = 127.0.0.1\r\nr_fullscreen = 0\r\nmy_own = 1\r\n";
        let theirs = "r_fullscreen = 1\nr_width = 1920\noption_view_distance = 4\n";
        let (text, n) = merge(ours, theirs);
        assert_eq!(n, 3);
        assert_eq!(text, "-- ours\r\nr_width = 1920\r\ncl_serveraddr = 127.0.0.1\r\nr_fullscreen = 1\r\nmy_own = 1\r\noption_view_distance = 4\r\n");
    }

    #[test]
    fn odd_lines_and_values_are_not_taken() {
        let theirs = "r_width = 1920; cl_serveraddr = 6.6.6.6\noption_x = a b\nr_height=\n = 5\nr_vsync 1\noption_ok = 2\nr_height = 1080\nr_height = 2\n";
        let (text, n) = merge("", theirs);
        assert_eq!(n, 2, "{text}");
        assert_eq!(text, "option_ok = 2\r\nr_height = 1080\r\n");
    }

    #[test]
    fn the_language_line_is_written_once_and_other_lines_stay() {
        let d = std::env::temp_dir().join(format!("aaemu-settings-lang-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let paths = Paths::at(d.join("ours"));
        let file = paths.docs().join("system.cfg");
        set_language(&paths, "en_us").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "locale = en_us\r\nlogin_first_movie = 1\r\n");
        // the player's own choice about the movie stays
        fs::write(&file, "r_width = 800\r\nlocale = \"ko\"\r\nlocale = de\r\nlogin_first_movie = 0\r\nmy_own = 1\r\n").unwrap();
        set_language(&paths, "en_us").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "r_width = 800\r\nlocale = en_us\r\nlogin_first_movie = 0\r\nmy_own = 1\r\n");
        let before = fs::metadata(&file).unwrap().modified().unwrap();
        set_language(&paths, "en_us").unwrap();
        assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), before);
        // copying the player's display settings keeps it
        let (text, _) = merge(&fs::read_to_string(&file).unwrap(), "r_width = 1920\nlocale = fr\n");
        assert_eq!(text, "r_width = 1920\r\nlocale = en_us\r\nlogin_first_movie = 0\r\nmy_own = 1\r\n");
        assert!(set_language(&paths, "en us; x").is_err());
    }

    #[test]
    fn copies_into_our_file_and_never_touches_theirs() {
        let d = std::env::temp_dir().join(format!("aaemu-settings-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let paths = Paths::at(d.join("ours"));
        let real = d.join("real").join("system.cfg");
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::write(&real, "r_width = 1280\r\nlocale = \"de\"\r\n").unwrap();
        let before = fs::metadata(&real).unwrap().modified().unwrap();
        assert_eq!(copy_from(&paths, &real).unwrap(), 1);
        assert_eq!(fs::read_to_string(paths.docs().join("system.cfg")).unwrap(), "r_width = 1280\r\n");
        assert_eq!(fs::read_to_string(&real).unwrap(), "r_width = 1280\r\nlocale = \"de\"\r\n");
        assert_eq!(fs::metadata(&real).unwrap().modified().unwrap(), before);
        // our own file is not a source, and a file without such settings is said so
        assert!(copy_from(&paths, &paths.docs().join("system.cfg")).is_err());
        fs::write(&real, "locale = \"de\"\r\n").unwrap();
        assert!(copy_from(&paths, &real).is_err_and(|e| e.contains("no display")));
    }
}
