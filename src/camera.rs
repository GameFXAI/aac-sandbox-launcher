//! The camera link: points the game's own camera at the character a replay possesses.
//!
//! A replay's Possess shows the fight from one character's camera, the way that player saw it on live. The server
//! can only move our (hidden) character, and an addon cannot reach the client's camera (its sandbox has no
//! `X2Camera`). The camera itself can follow any unit, though, so this link, running beside the client, tells it
//! which one.
//!
//! What the client has (x2game.dll, build 297; found by reading its code, checked in the client):
//! - One camera controller object, its pointer in a global. `X2Camera:SetUnitCameraAngles` loads it with
//!   `mov rcx,[rip+g]; xor edx,edx; add rcx,0x10; movss [rcx+0x8c],xmm0; jmp ...`, and the camera's target-change
//!   handler starts `push rbx; sub rsp,0x20; cmp ecx,[rip+id]; mov ebx,ecx; jne ..; cmp qword [rip+g],0`. Either
//!   pattern gives the global; both are found by bytes, so another build that moved them still works.
//! - At controller +0x15c the unit the camera stands at, at +0x160 the unit whose facing it turns with (0: the same
//!   one). Each frame the client reads that unit's place and rotation: the camera is the game's own, smoothed and
//!   collided as on live, with the mouse still orbiting it. The client puts our own character's id at +0x15c when
//!   it enters a world.
//!
//! Files, in the client's `Addon\aac_replay` folder:
//! - `camera.txt`       `{ unit = 23253, }` written by the replay addon: the unit to point at, 0 for our own
//! - `camera_link.txt`  `{ beat = n, unit = m, }` written here about every half second: the addon sees the link runs
//!   and stops pressing the game's back_camera key (its fallback without the link)

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const REQUEST: &str = "camera.txt";
pub const HEARTBEAT: &str = "camera_link.txt";
const OWNER: u64 = 0x15c;
const TARGET: u64 = 0x160;

/// `X2Camera:SetUnitCameraAngles`' worker; the global's 32-bit displacement is at +3, the instruction ends at +7.
const SET_ANGLES: &str = "48 8b 0d ?? ?? ?? ?? 33 d2 48 83 c1 10 f3 0f 11 81 8c 00 00 00 e9";
/// The camera's target-change handler; `cmp qword [rip+g],0` has its displacement at +19 and ends at +24.
const TARGET_CHANGED: &str = "40 53 48 83 ec 20 3b 0d ?? ?? ?? ?? 8b d9 75 ?? 48 83 3d ?? ?? ?? ?? 00 74";

fn parse_pattern(p: &str) -> Vec<Option<u8>> {
    p.split_whitespace().map(|t| if t == "??" { None } else { Some(u8::from_str_radix(t, 16).unwrap()) }).collect()
}

fn find_all(image: &[u8], pattern: &[Option<u8>]) -> Vec<usize> {
    let Some(Some(first)) = pattern.first().copied() else { return Vec::new() };
    let mut out = Vec::new();
    if image.len() < pattern.len() {
        return out;
    }
    for i in 0..=image.len() - pattern.len() {
        if image[i] == first && pattern.iter().enumerate().all(|(k, b)| b.is_none_or(|b| image[i + k] == b)) {
            out.push(i);
        }
    }
    out
}

fn rip(image: &[u8], at: usize, disp: usize, end: usize) -> Option<u64> {
    let d = i32::from_le_bytes(image.get(at + disp..at + disp + 4)?.try_into().ok()?);
    u64::try_from(at as i64 + end as i64 + d as i64).ok()
}

/// The controller global's offset in x2game's loaded image: both patterns, each found once, naming the same global.
pub fn controller_global(image: &[u8]) -> Option<u64> {
    let a = find_all(image, &parse_pattern(SET_ANGLES));
    let b = find_all(image, &parse_pattern(TARGET_CHANGED));
    let ga = match a.as_slice() {
        [h] => rip(image, *h, 3, 7),
        _ => None,
    };
    let gb = match b.as_slice() {
        [h] => rip(image, *h, 19, 24),
        _ => None,
    };
    match (ga, gb) {
        (Some(x), Some(y)) if x == y => Some(x),
        (Some(x), None) | (None, Some(x)) => Some(x),
        _ => None,
    }
}

/// The unit `camera.txt` asks for (0 for our own character); None when the file says nothing usable.
pub fn parse_request(text: &str) -> Option<u32> {
    let at = text.find("unit")?;
    let rest = text[at + 4..].trim_start().strip_prefix('=')?.trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// What the link has done to the camera. `own` is our character's id as the client last set it; `pointed` the unit
/// the camera was pointed at (0: none); `held` a request the link no longer obeys because the client took the camera
/// back itself (it entered another world while pointed): that unit is gone, and the next request differs.
#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub struct Link {
    pub own: u32,
    pub pointed: u32,
    pub held: u32,
}

impl Link {
    /// Given the camera's owner and target ids now and the unit asked for, what to write at (+0x15c, +0x160).
    pub fn step(&mut self, owner: u32, target: u32, want: u32) -> Option<(u32, u32)> {
        if owner == 0 {
            return None; // no character in the world yet
        }
        if self.pointed == 0 {
            self.own = owner;
        } else if owner != self.pointed {
            // the client put a character of its own back: a new world. The old unit is not in it.
            self.own = owner;
            self.held = self.pointed;
            self.pointed = 0;
        }
        if want != self.held {
            self.held = 0;
        }
        if want != 0 && want != self.held {
            self.pointed = want;
            return (owner != want || target != want).then_some((want, want));
        }
        if self.pointed != 0 {
            self.pointed = 0;
            return Some((self.own, 0));
        }
        None
    }
}

/// The heartbeat file's text, in the format the addon's `api.File:Read` reads back as a table.
pub fn heartbeat_text(beat: u64, unit: u32) -> String {
    format!("{{\n    beat = {beat},\n    unit = {unit},\n}}\n")
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, STILL_ACTIVE};
    use windows_sys::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
        PROCESS_VM_WRITE,
    };

    pub struct Process(HANDLE, u32);

    impl Drop for Process {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    impl Process {
        pub fn open(pid: u32) -> Result<Process, String> {
            let access = PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION | PROCESS_QUERY_LIMITED_INFORMATION;
            let h = unsafe { OpenProcess(access, 0, pid) };
            if h.is_null() { Err(format!("cannot open the game's process {pid}")) } else { Ok(Process(h, pid)) }
        }

        pub fn alive(&self) -> bool {
            let mut code = 0u32;
            unsafe { GetExitCodeProcess(self.0, &mut code) != 0 && code == STILL_ACTIVE as u32 }
        }

        /// A module's base and size, by its file name.
        pub fn module(&self, name: &str) -> Option<(u64, usize)> {
            let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, self.1) };
            if snap == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut e: MODULEENTRY32W = unsafe { std::mem::zeroed() };
            e.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
            let mut found = None;
            let mut ok = unsafe { Module32FirstW(snap, &mut e) } != 0;
            while ok {
                let len = e.szModule.iter().position(|&c| c == 0).unwrap_or(e.szModule.len());
                if String::from_utf16_lossy(&e.szModule[..len]).eq_ignore_ascii_case(name) {
                    found = Some((e.modBaseAddr as u64, e.modBaseSize as usize));
                    break;
                }
                ok = unsafe { Module32NextW(snap, &mut e) } != 0;
            }
            unsafe { CloseHandle(snap) };
            found
        }

        pub fn read(&self, at: u64, buf: &mut [u8]) -> bool {
            let mut got = 0usize;
            unsafe { ReadProcessMemory(self.0, at as *const c_void, buf.as_mut_ptr().cast(), buf.len(), &mut got) != 0 && got == buf.len() }
        }

        pub fn write(&self, at: u64, buf: &[u8]) -> bool {
            let mut put = 0usize;
            unsafe { WriteProcessMemory(self.0, at as *const c_void, buf.as_ptr().cast(), buf.len(), &mut put) != 0 && put == buf.len() }
        }

        pub fn u32_at(&self, at: u64) -> Option<u32> {
            let mut b = [0u8; 4];
            self.read(at, &mut b).then(|| u32::from_le_bytes(b))
        }

        pub fn u64_at(&self, at: u64) -> Option<u64> {
            let mut b = [0u8; 8];
            self.read(at, &mut b).then(|| u64::from_le_bytes(b))
        }

        /// The module's loaded image; pages that cannot be read stay zero.
        pub fn image(&self, base: u64, size: usize) -> Vec<u8> {
            let mut img = vec![0u8; size];
            for (k, chunk) in img.chunks_mut(1 << 16).enumerate() {
                if !self.read(base + (k << 16) as u64, chunk) {
                    chunk.fill(0);
                }
            }
            img
        }
    }
}

/// Runs the link until the game's process ends (or `stop` says so). `say` gets a line when something changes.
#[cfg(windows)]
pub fn run(pid: u32, addon_dir: &Path, stop: &dyn Fn() -> bool, say: &dyn Fn(String)) -> Result<(), String> {
    let p = win::Process::open(pid)?;
    let dir: PathBuf = addon_dir.join("aac_replay");
    let request = dir.join(REQUEST);
    let heartbeat = dir.join(HEARTBEAT);
    let mut global: Option<u64> = None;
    let mut looked = Instant::now() - Duration::from_secs(10);
    let mut link = Link::default();
    let mut beat = 0u64;
    let mut beat_at = Instant::now() - Duration::from_secs(10);
    while p.alive() && !stop() {
        if global.is_none() && looked.elapsed() >= Duration::from_secs(3) {
            // x2game is packed on disk: its code is only there to read once the client has unpacked it
            looked = Instant::now();
            if let Some((base, size)) = p.module("x2game.dll") {
                if let Some(g) = controller_global(&p.image(base, size)) {
                    global = Some(base + g);
                    say(format!("camera link: the game's camera found (x2game +{g:#x})"));
                }
            }
        }
        if let Some(g) = global {
            let want = std::fs::read_to_string(&request).ok().and_then(|t| parse_request(&t)).unwrap_or(0);
            let ctl = p.u64_at(g).unwrap_or(0);
            if (0x10000..0x8000_0000_0000).contains(&ctl) {
                if let (Some(owner), Some(target)) = (p.u32_at(ctl + OWNER), p.u32_at(ctl + TARGET)) {
                    let before = link.pointed;
                    if let Some((o, t)) = link.step(owner, target, want) {
                        // the target first: for a moment the camera turns with the new unit, then stands at it
                        p.write(ctl + TARGET, &t.to_le_bytes());
                        p.write(ctl + OWNER, &o.to_le_bytes());
                    }
                    if link.pointed != before {
                        say(if link.pointed == 0 { "camera link: the camera is ours again".into() } else { format!("camera link: the camera follows unit {}", link.pointed) });
                    }
                }
            }
            if beat_at.elapsed() >= Duration::from_millis(500) && dir.is_dir() {
                beat_at = Instant::now();
                beat += 1;
                let _ = std::fs::write(&heartbeat, heartbeat_text(beat, link.pointed));
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_file(&heartbeat);
    Ok(())
}

#[cfg(not(windows))]
pub fn run(_pid: u32, _addon_dir: &Path, _stop: &dyn Fn() -> bool, _say: &dyn Fn(String)) -> Result<(), String> {
    Err("the camera link needs Windows".into())
}

/// Starts the link beside a client the launcher started; it ends with the client.
pub fn start(pid: u32, addon_dir: PathBuf, log: impl Fn(String) + Send + 'static) {
    std::thread::spawn(move || {
        if let Err(e) = run(pid, &addon_dir, &|| false, &log) {
            log(format!("camera link: {e}"));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake image with both patterns pointing at the global `g` (as in the 297 client: code at 0x18660 and
    /// 0x183e0, the global at 0xf69930).
    fn image(g: u64, with_second: bool) -> Vec<u8> {
        let mut img = vec![0xccu8; 0x20000];
        let put = |img: &mut Vec<u8>, at: usize, p: &str, disp: usize, end: usize| {
            for (k, b) in parse_pattern(p).iter().enumerate() {
                img[at + k] = b.unwrap_or(0x11);
            }
            let d = (g as i64 - (at + end) as i64) as i32;
            img[at + disp..at + disp + 4].copy_from_slice(&d.to_le_bytes());
        };
        put(&mut img, 0x8660, SET_ANGLES, 3, 7);
        if with_second {
            put(&mut img, 0x83e0, TARGET_CHANGED, 19, 24);
        }
        img
    }

    #[test]
    fn finds_the_camera_global_by_its_code() {
        assert_eq!(controller_global(&image(0xf69930, true)), Some(0xf69930));
        assert_eq!(controller_global(&image(0xf6a930, false)), Some(0xf6a930));
        assert_eq!(controller_global(&vec![0u8; 4096]), None);
        // a second copy of the pattern: not sure which, so none
        let mut twice = image(0xf69930, false);
        let first: Vec<u8> = twice[0x8660..0x8660 + 22].to_vec();
        twice[0x9000..0x9000 + 22].copy_from_slice(&first);
        assert_eq!(controller_global(&twice), None);
    }

    #[test]
    fn reads_the_addons_request() {
        assert_eq!(parse_request("{\n    unit = 23253,\n}\n"), Some(23253));
        assert_eq!(parse_request("{ unit = 0, }"), Some(0));
        assert_eq!(parse_request("{ }"), None);
        assert_eq!(parse_request("{ unit = \"x\", }"), None);
    }

    #[test]
    fn points_the_camera_and_gives_it_back() {
        let mut l = Link::default();
        assert_eq!(l.step(0, 0, 500), None, "no character yet");
        assert_eq!(l.step(352, 0, 0), None, "nothing asked");
        assert_eq!(l.step(352, 0, 500), Some((500, 500)));
        assert_eq!(l.step(500, 500, 500), None, "already there");
        assert_eq!(l.step(500, 500, 600), Some((600, 600)), "another person");
        assert_eq!(l.step(600, 600, 0), Some((352, 0)), "ours again");
        assert_eq!(l.step(352, 0, 0), None);
    }

    #[test]
    fn a_new_world_while_pointed_keeps_the_clients_own_character() {
        let mut l = Link::default();
        l.step(352, 0, 500);
        // the client entered another world and put its new character there
        assert_eq!(l.step(77, 0, 500), None, "the old unit is not in this world");
        assert_eq!(l.step(77, 0, 500), None);
        assert_eq!(l.step(77, 0, 0), None, "nothing to give back");
        assert_eq!(l.step(77, 0, 900), Some((900, 900)), "a new request is obeyed");
        assert_eq!(l.step(900, 900, 0), Some((77, 0)));
    }

    #[test]
    fn the_heartbeat_reads_back_as_a_lua_table() {
        assert_eq!(heartbeat_text(3, 500), "{\n    beat = 3,\n    unit = 500,\n}\n");
    }
}
