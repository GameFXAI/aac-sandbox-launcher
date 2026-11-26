//! The account API of a server: what the server is, sign-up, and login for a launch ticket.
//!
//! The rules this module keeps:
//! - The password goes only to the server the player typed, and only over HTTPS. Plain HTTP is allowed to this PC
//!   alone (a server on the same machine, or a tunnel that ends on a loopback address).
//! - Redirects are not followed, so a server cannot pass the password on to another address.
//! - Nothing here writes the password or the ticket to a log or to disk.

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, ToSocketAddrs};
use std::time::Duration;

/// What the player typed in the Server field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Server {
    /// A bare address of a development server: no account API, the client gets the password hash as its token.
    Direct(String),
    /// A server with an account API: `url` has no trailing slash, `host` is the name or address in it.
    Api { url: String, host: String },
}

fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host.parse::<IpAddr>().map(|a| a.is_loopback()).unwrap_or(false)
}

impl Server {
    pub fn parse(input: &str) -> Result<Server, String> {
        let s = input.trim().trim_end_matches('/');
        if s.is_empty() {
            return Err("enter the server's address".into());
        }
        let lower = s.to_ascii_lowercase();
        let (https, rest) = if lower.starts_with("https://") {
            (true, &s[8..])
        } else if lower.starts_with("http://") {
            (false, &s[7..])
        } else {
            if s.contains(['/', ' ', '@']) {
                return Err("the server is an address (127.0.0.1) or a link (https://...)".into());
            }
            return Ok(Server::Direct(s.to_string()));
        };
        let authority = rest.split('/').next().unwrap_or("");
        if authority.is_empty() || authority.contains('@') || authority.contains(' ') {
            return Err("the server link is not valid".into());
        }
        // [::1]:1238, 127.0.0.1:1238, arena.example.com
        let host = if let Some(end) = authority.strip_prefix('[').and_then(|a| a.find(']')) {
            authority[1..=end].to_string()
        } else {
            authority.rsplit_once(':').map(|(h, _)| h).unwrap_or(authority).to_string()
        };
        if host.is_empty() {
            return Err("the server link is not valid".into());
        }
        if !https && !is_loopback(&host) {
            return Err("a server on another computer needs a link that starts with https://".into());
        }
        Ok(Server::Api { url: s.to_string(), host })
    }

    /// The address the game client is given (it takes addresses, not names).
    pub fn client_address(&self) -> Result<String, String> {
        let host = match self {
            Server::Direct(h) => h,
            Server::Api { host, .. } => host,
        };
        if host.parse::<IpAddr>().is_ok() {
            return Ok(host.clone());
        }
        let mut found: Vec<IpAddr> = (host.as_str(), 0u16).to_socket_addrs().map_err(|e| format!("cannot find {host}: {e}"))?.map(|a| a.ip()).collect();
        found.sort_by_key(|a| !a.is_ipv4()); // the client is IPv4 only
        match found.first() {
            Some(a) if a.is_ipv4() => Ok(a.to_string()),
            _ => Err(format!("{host} has no IPv4 address")),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Info {
    pub name: String,
    pub builds: Vec<String>,
    /// the client builds (vault build ids) this server accepts; empty = not stated
    pub build_ids: Vec<String>,
    pub addon_pack: String,
    /// "invite" or "open"
    pub signup: String,
    pub tickets_only: bool,
}

/// A launch ticket: starts the client once, within a couple of minutes. Never log it.
#[derive(Clone, Deserialize)]
pub struct Ticket {
    pub username: String,
    pub ticket: String,
    /// with "remember me": the remembered sign-in, sent later in place of the password. Never log it either.
    #[serde(default)]
    pub session: Option<String>,
}

/// How the player proves the account: the password typed now, or a remembered sign-in.
#[derive(Clone, Copy)]
pub enum Cred<'a> {
    Password(&'a str),
    Session(&'a str),
}

impl Cred<'_> {
    /// The fields a request carries for it.
    pub(crate) fn put(&self, body: &mut serde_json::Value) {
        match self {
            Cred::Password(p) => body["password"] = (*p).into(),
            Cred::Session(s) => body["session"] = (*s).into(),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Failure {
    error: String,
}

/// The server's error codes in plain words.
pub fn explain(code: &str) -> String {
    match code {
        "BadUsername" => "account name: 3 to 24 letters, digits or _".into(),
        "BadPassword" => "password: 8 to 128 characters".into(),
        "UsernameTaken" => "that account name is taken".into(),
        "BadInvite" => "that invite code is not valid or was already used".into(),
        "BadCredentials" => "wrong account name or password".into(),
        "Banned" => "this account is banned".into(),
        "TooManyAttempts" => "too many tries: wait ten minutes".into(),
        "GameServerOffline" => "the game server is not running right now: try again in a minute".into(),
        "BadTransfer" => "the server could not use what was sent".into(),
        "BadSession" => "your saved sign-in has run out: enter your password once more".into(),
        "BadReport" => "the report was empty or too large".into(),
        "ReportsFull" => "the server takes no more reports right now".into(),
        "BadReplay" => "the server could not read that recording as a fight".into(),
        "NoSuchReplay" => "that replay is not on the server any more".into(),
        "NoSuchShare" => "that share is gone, or no longer shared with you".into(),
        "" => "the server refused the request".into(),
        other => format!("the server refused the request ({other})"),
    }
}

pub(crate) fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).redirects(0).build()
}

fn failed(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(404, _) => "this server has no account API".into(),
        ureq::Error::Status(_, r) => explain(&r.into_json::<Failure>().unwrap_or_default().error),
        // the transport's own text names the address but never the request body
        ureq::Error::Transport(t) => format!("cannot reach the server: {t}"),
    }
}

fn api_url(server: &Server) -> Result<&str, String> {
    match server {
        Server::Api { url, .. } => Ok(url),
        Server::Direct(_) => Err("accounts are made on a server link (https://...), not on a bare address".into()),
    }
}

pub fn info(server: &Server) -> Result<Info, String> {
    let r = agent().get(&format!("{}/api/info", api_url(server)?)).call().map_err(failed)?;
    r.into_json().map_err(|e| format!("the server's answer was not understood: {e}"))
}

/// Downloads the server's addon pack with this id (addons.rs checks it before anything is written).
pub fn pack(server: &Server, id: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let r = ureq::AgentBuilder::new().timeout(Duration::from_secs(120)).redirects(0).build().get(&format!("{}/api/packs/{id}", api_url(server)?)).call().map_err(|e| match e {
        ureq::Error::Status(404, _) => "the server does not have the addon pack it names".to_string(),
        e => failed(e),
    })?;
    let mut out = Vec::new();
    r.into_reader().take(crate::addons::MAX_PACK + 1).read_to_end(&mut out).map_err(|e| format!("the addon pack download broke off: {e}"))?;
    if out.len() as u64 > crate::addons::MAX_PACK {
        return Err("the addon pack is larger than an addon pack can be".into());
    }
    Ok(out)
}

/// Makes the account; the answer is the name as the server stored it.
pub fn signup(server: &Server, username: &str, password: &str, invite: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Made {
        username: String,
    }
    let body = serde_json::json!({ "username": username, "password": password, "invite": invite.trim() });
    let r = agent().post(&format!("{}/api/signup", api_url(server)?)).send_json(body).map_err(failed)?;
    r.into_json::<Made>().map(|m| m.username).map_err(|e| format!("the server's answer was not understood: {e}"))
}

/// Signs in for a launch ticket. With `remember` and a password, the server also gives a remembered sign-in.
pub fn login(server: &Server, username: &str, cred: Cred, remember: bool) -> Result<Ticket, String> {
    let device = std::env::var("COMPUTERNAME").unwrap_or_default();
    let mut body = serde_json::json!({ "username": username, "remember": remember, "device": device });
    cred.put(&mut body);
    let r = agent().post(&format!("{}/api/login", api_url(server)?)).send_json(body).map_err(failed)?;
    let t: Ticket = r.into_json().map_err(|e| format!("the server's answer was not understood: {e}"))?;
    if !crate::core::valid_token(&t.ticket) {
        return Err("the server's ticket is not in the expected form".into());
    }
    if t.session.as_deref().is_some_and(|s| !crate::core::valid_token(s)) {
        return Err("the server's sign-in is not in the expected form".into());
    }
    Ok(t)
}

/// Ends a remembered sign-in on the server ("sign out").
pub fn logout(server: &Server, session: &str) -> Result<(), String> {
    agent().post(&format!("{}/api/logout", api_url(server)?)).send_json(serde_json::json!({ "session": session })).map_err(failed)?;
    Ok(())
}

/// "Report a problem": the player's words and the two logs, kept on the server for its owner. The answer is the
/// report's id. The sign-in, when there is one, only marks the account name as checked.
pub fn report(server: &Server, username: &str, session: Option<&str>, text: &str, launcher_log: &str, client_log: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Saved {
        id: String,
    }
    let body = serde_json::json!({
        "username": username, "session": session.unwrap_or(""), "text": text,
        "launcherLog": launcher_log, "clientLog": client_log, "version": crate::update::VERSION,
    });
    let r = ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).redirects(0).build()
        .post(&format!("{}/api/report", api_url(server)?)).send_json(body).map_err(|e| match e {
            ureq::Error::Status(404, _) => "this server takes no reports".to_string(),
            e => failed(e),
        })?;
    r.into_json::<Saved>().map(|s| s.id).map_err(|e| format!("the server's answer was not understood: {e}"))
}

/// What the server took from a transfer: the parts it will put on the character at the next world entry.
#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct Accepted {
    pub accepted: Vec<String>,
}

/// Sends a transfer from AA Classic (see transfer.rs for what is in it) with the account's name and its password or
/// remembered sign-in. The server keeps it for the account and applies it when one of its characters next enters the world.
pub fn transfer(server: &Server, username: &str, cred: Cred, payload: &serde_json::Value) -> Result<Accepted, String> {
    let mut body = payload.clone();
    body["username"] = username.into();
    cred.put(&mut body);
    let r = agent().post(&format!("{}/api/transfer", api_url(server)?)).send_json(body).map_err(|e| match e {
        ureq::Error::Status(404, _) => "this server does not take transfers yet".to_string(),
        e => failed(e),
    })?;
    r.into_json().map_err(|e| format!("the server's answer was not understood: {e}"))
}

/// Sends one of the player's own AA Classic fights (recorder.rs) to the account's replays. The fight's names are
/// "You" and "Opponent" already: no live player's name leaves the PC.
pub fn upload_replay(server: &Server, username: &str, cred: Cred, fight: &[u8]) -> Result<(), String> {
    use base64::Engine;
    let mut body = serde_json::json!({ "username": username, "fight": base64::engine::general_purpose::STANDARD.encode(fight) });
    cred.put(&mut body);
    ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).redirects(0).build()
        .post(&format!("{}/api/replays", api_url(server)?)).send_json(body).map_err(|e| match e {
            ureq::Error::Status(404, _) => "this server does not take recordings yet".to_string(),
            e => failed(e),
        })?;
    Ok(())
}

/// Sends the real picture of a recording the account uploaded (a JPEG the recorder took during the fight), keyed by the
/// fight file's sha-256. Ok(false): the server has not listed that fight yet (the game server takes it in a moment
/// after the upload); try again later.
pub fn upload_picture(server: &Server, username: &str, cred: Cred, sha: &str, jpeg: &[u8]) -> Result<bool, String> {
    use base64::Engine;
    let mut body = serde_json::json!({ "username": username, "sha": sha, "jpeg": base64::engine::general_purpose::STANDARD.encode(jpeg) });
    cred.put(&mut body);
    match agent().post(&format!("{}/api/replays/thumb", api_url(server)?)).send_json(body) {
        Ok(_) => Ok(true),
        Err(ureq::Error::Status(404, _)) => Ok(false),
        Err(e) => Err(failed(e)),
    }
}

/// A picture the server keeps: `path` with these fields, signed in. Ok(None): there is none (or it may not be seen).
pub fn picture(server: &Server, path: &str, username: &str, cred: Cred, fields: serde_json::Value) -> Result<Option<Vec<u8>>, String> {
    use std::io::Read;
    let mut body = serde_json::json!({ "username": username });
    if let (Some(b), Some(f)) = (body.as_object_mut(), fields.as_object()) {
        for (k, v) in f {
            b.insert(k.clone(), v.clone());
        }
    }
    cred.put(&mut body);
    match agent().post(&format!("{}/api{path}", api_url(server)?)).send_json(body) {
        Ok(r) => {
            let mut b = Vec::new();
            r.into_reader().take(256 * 1024).read_to_end(&mut b).map_err(|e| format!("the picture did not arrive: {e}"))?;
            Ok(Some(b))
        }
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(e) => Err(failed(e)),
    }
}

/// One of the account's own recordings, as the server lists it. `n` is its stable number (it never changes while the
/// recording exists; list positions do). `over_cap`: beyond the plan's cap since a downgrade, so the next recording
/// that comes in drops it.
#[derive(Clone, Debug, Deserialize, Serialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Replay {
    pub n: i32,
    pub title: String,
    /// when the fight started, Unix milliseconds
    pub when: i64,
    #[serde(rename = "self")]
    pub me: String,
    pub other: String,
    pub over_cap: bool,
    /// the name the player gave it (Rename); none: the fight's own title
    pub name: Option<String>,
    /// who it is shared with, one entry per share; empty: private
    pub shares: Vec<ReplayShare>,
    /// the server has a real picture of it (taken by the recorder during the fight)
    pub thumb: bool,
}

/// One share of a recording, as the library lists it.
#[derive(Clone, Debug, Deserialize, Serialize, Default, PartialEq)]
#[serde(default)]
pub struct ReplayShare {
    pub id: i64,
    /// "friends", "guild" or "community"
    pub audience: String,
}

/// The Library: the account's plan as the server has it, how many recordings that plan keeps, and the recordings,
/// newest first. Every number here is the server's: the launcher shows it and never works one out itself.
#[derive(Clone, Debug, Deserialize, Serialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Library {
    /// "free" or "sub"
    pub plan: String,
    /// when a sub ends (UTC, ISO 8601); none on the free plan
    pub plan_expires_at: Option<String>,
    pub cap: u32,
    /// where "Upgrade" points; empty: no button
    pub upgrade_url: String,
    pub replays: Vec<Replay>,
}

impl Library {
    pub fn is_sub(&self) -> bool {
        self.plan == "sub"
    }
}

/// The account's recordings and plan (POST /api/replays/list): the credentials go in the body, never in the address.
pub fn list_replays(server: &Server, username: &str, cred: Cred) -> Result<Library, String> {
    let mut body = serde_json::json!({ "username": username });
    cred.put(&mut body);
    let r = agent().post(&format!("{}/api/replays/list", api_url(server)?)).send_json(body).map_err(|e| match e {
        ureq::Error::Status(404, _) => "this server has no replay library yet".to_string(),
        e => failed(e),
    })?;
    r.into_json().map_err(|e| format!("the server's answer was not understood: {e}"))
}

/// Deletes one of the account's recordings by its stable number (POST /api/replays/delete). The server checks it is
/// this account's own; a number it does not find is "not on the server any more".
pub fn delete_replay(server: &Server, username: &str, cred: Cred, n: i32) -> Result<(), String> {
    let mut body = serde_json::json!({ "username": username, "n": n });
    cred.put(&mut body);
    agent().post(&format!("{}/api/replays/delete", api_url(server)?)).send_json(body).map_err(|e| match e {
        ureq::Error::Status(404, r) => match r.into_json::<Failure>().unwrap_or_default().error.as_str() {
            "NoSuchReplay" => explain("NoSuchReplay"),
            _ => "this server has no replay library yet".to_string(),
        },
        e => failed(e),
    })?;
    Ok(())
}

/// Watch a recording another player shared (POST /api/shares/watch): the server checks this account may see it and
/// lets the game server play it for this account once. The game's replay player then starts it by itself, in a game
/// already open or once the game is in the world; the answer is how many seconds the grant lasts. Nothing is written
/// for the game: the grant is the server's, bound to this account.
pub fn watch_share(server: &Server, username: &str, cred: Cred, share: i64) -> Result<u32, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Granted {
        expires_in: u32,
    }
    let mut body = serde_json::json!({ "username": username, "share": share });
    cred.put(&mut body);
    let r = agent().post(&format!("{}/api/shares/watch", api_url(server)?)).send_json(body).map_err(|e| match e {
        ureq::Error::Status(404, r) => match r.into_json::<Failure>().unwrap_or_default().error.as_str() {
            "NoSuchShare" => explain("NoSuchShare"),
            _ => "this server has no shares yet".to_string(),
        },
        e => failed(e),
    })?;
    let g: Granted = r.into_json().map_err(|e| format!("the server's answer was not understood: {e}"))?;
    Ok(g.expires_in)
}

/// A DaruGear build as the launcher shows it before sending: the server fetches and maps it itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Build {
    pub code: String,
    pub title: String,
    pub class: String,
    /// worn pieces: (slot name, item name, grade name)
    pub pieces: Vec<(String, String, String)>,
}

/// The fixed host DaruGear's page reads builds from; only a code is ever put in the address.
pub const DARU_API: &str = "https://154smechk6.execute-api.ap-southeast-2.amazonaws.com/default/gear/";

/// The share code of a DaruGear link (https://darugear.com/?s=code) or a bare code, as the server reads it.
pub fn daru_code(text: &str) -> Option<String> {
    let t = text.trim().trim_matches(|c| c == '"' || c == '<' || c == '>');
    let ok = |c: &str| (3..=32).contains(&c.len()) && c.chars().all(|x| x.is_ascii_alphanumeric() || x == '_' || x == '-');
    if let Some(i) = t.find("?s=").or_else(|| t.find("&s=")) {
        let rest = &t[i + 3..];
        let code = rest.split(['&', '#']).next().unwrap_or("");
        let host_ok = t.starts_with("https://darugear.com/") || t.starts_with("http://darugear.com/") || t.starts_with("https://www.darugear.com/") || t.starts_with("darugear.com/");
        return (host_ok && ok(code)).then(|| code.to_string());
    }
    ok(t).then(|| t.to_string())
}

/// Reads DaruGear's JSON for a build.
pub fn parse_build(code: &str, json: &serde_json::Value) -> Result<Build, String> {
    let slots = json.get("slots").and_then(|s| s.as_array()).ok_or("DaruGear has no build with that code")?;
    let s = |v: Option<&serde_json::Value>| v.and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    let mut b = Build { code: code.into(), title: s(json.get("title")), class: s(json.pointer("/player/playerclass/name")), pieces: vec![] };
    // DaruGear writes numbers as floats (8000924.0) and a grade as its id
    let num = |v: Option<&serde_json::Value>| v.and_then(|x| x.as_f64()).unwrap_or(0.0);
    const GRADES: [&str; 13] = ["Basic", "Crude", "Grand", "Rare", "Arcane", "Heroic", "Unique", "Celestial", "Divine", "Epic", "Legendary", "Mythic", "Eternal"];
    for slot in slots {
        let eq = slot.pointer("/item/equipment");
        if num(eq.and_then(|e| e.get("id"))) < 1.0 {
            continue;
        }
        let mut grade = s(slot.pointer("/item/grade/name"));
        if grade.is_empty() {
            let g = num(slot.pointer("/item/grade/id"));
            if g >= 0.0 && (g as usize) < GRADES.len() && slot.pointer("/item/grade/id").is_some() {
                grade = GRADES[g as usize].to_string();
            }
        }
        b.pieces.push((s(slot.get("name")), s(eq.and_then(|e| e.get("name"))), grade));
    }
    if b.title.is_empty() {
        b.title = code.into();
    }
    Ok(b)
}

/// Asks DaruGear for the build behind a code (the same request the darugear.com page makes).
pub fn daru(code: &str) -> Result<Build, String> {
    use std::io::Read;
    if daru_code(code).as_deref() != Some(code) {
        return Err("that is not a DaruGear code".into());
    }
    let r = ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).redirects(0).build().get(&format!("{DARU_API}{code}")).call().map_err(|e| match e {
        ureq::Error::Status(c, _) => format!("DaruGear answered {c}"),
        ureq::Error::Transport(t) => format!("cannot reach DaruGear: {t}"),
    })?;
    let mut out = Vec::new();
    r.into_reader().take(1 << 20).read_to_end(&mut out).map_err(|e| format!("DaruGear's answer broke off: {e}"))?;
    let v: serde_json::Value = serde_json::from_slice(&out).map_err(|_| "DaruGear's answer was not understood".to_string())?;
    parse_build(code, &v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn server_field_rules() {
        assert_eq!(Server::parse(" 127.0.0.1 "), Ok(Server::Direct("127.0.0.1".into())));
        assert_eq!(Server::parse("https://arena.example.com/"), Ok(Server::Api { url: "https://arena.example.com".into(), host: "arena.example.com".into() }));
        assert_eq!(Server::parse("HTTP://127.0.0.1:1238"), Ok(Server::Api { url: "HTTP://127.0.0.1:1238".into(), host: "127.0.0.1".into() }));
        assert_eq!(Server::parse("http://localhost:1238").map(|s| matches!(s, Server::Api { .. })), Ok(true));
        assert_eq!(Server::parse("http://[::1]:1238"), Ok(Server::Api { url: "http://[::1]:1238".into(), host: "::1".into() }));
        // the password must not cross the network in the clear
        assert!(Server::parse("http://arena.example.com").is_err());
        assert!(Server::parse("http://192.168.1.20:1238").is_err());
        // a name that only looks local
        assert!(Server::parse("http://127.0.0.1.example.com").is_err());
        assert!(Server::parse("http://localhost@example.com").is_err());
        assert!(Server::parse("https://user@example.com").is_err());
        assert!(Server::parse("").is_err());
        assert!(Server::parse("ftp://x/y").is_err());
    }

    #[test]
    fn client_gets_an_address() {
        assert_eq!(Server::parse("10.0.0.5").unwrap().client_address(), Ok("10.0.0.5".into()));
        assert_eq!(Server::parse("http://127.0.0.1:1238").unwrap().client_address(), Ok("127.0.0.1".into()));
        assert_eq!(Server::parse("http://localhost:1238").unwrap().client_address(), Ok("127.0.0.1".into()));
    }

    #[test]
    fn a_bare_address_has_no_account_api() {
        let s = Server::parse("127.0.0.1").unwrap();
        assert!(login(&s, "a", Cred::Password("b"), false).is_err());
        assert!(signup(&s, "a", "b", "c").is_err());
    }

    /// A one-request HTTP server on a loopback port; gives back what it was sent.
    fn serve_once(status: &'static str, body: &'static str) -> (Server, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = c.read(&mut buf).unwrap();
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                if let Some(i) = text.find("\r\n\r\n") {
                    let len = text.to_ascii_lowercase().split("content-length:").nth(1).and_then(|s| s.lines().next()).and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(0);
                    if got.len() >= i + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let _ = write!(c, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            String::from_utf8_lossy(&got).to_string()
        });
        (Server::parse(&format!("http://127.0.0.1:{port}")).unwrap(), h)
    }

    #[test]
    fn login_gives_the_ticket() {
        let (s, h) = serve_once("200 OK", r#"{"username":"Friend","ticket":"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff","expiresIn":120}"#);
        let t = login(&s, "friend", Cred::Password("long enough"), false).unwrap();
        assert!(t.session.is_none());
        assert_eq!(t.username, "Friend");
        assert_eq!(t.ticket.len(), 64);
        let sent = h.join().unwrap();
        assert!(sent.starts_with("POST /api/login "));
        assert!(sent.contains(r#""username":"friend""#) && sent.contains(r#""password":"long enough""#));
    }

    #[test]
    fn remember_gives_a_session_and_a_session_signs_in_without_the_password() {
        let (s, h) = serve_once("200 OK", r#"{"username":"Friend","ticket":"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff","session":"ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100","sessionExpiresIn":2592000}"#);
        let t = login(&s, "friend", Cred::Password("long enough"), true).unwrap();
        assert_eq!(t.session.as_deref().map(str::len), Some(64));
        let sent = h.join().unwrap();
        assert!(sent.contains(r#""remember":true"#) && sent.contains(r#""password":"long enough""#));
        let (s, h) = serve_once("200 OK", r#"{"username":"Friend","ticket":"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"}"#);
        login(&s, "friend", Cred::Session("ffee"), true).unwrap();
        let sent = h.join().unwrap();
        assert!(sent.contains(r#""session":"ffee""#) && !sent.contains("password"));
        let (s, h) = serve_once("401 Unauthorized", r#"{"error":"BadSession"}"#);
        assert!(login(&s, "friend", Cred::Session("ffee"), true).err().unwrap().contains("password"));
        h.join().unwrap();
        // a sign-in in another form is refused like a bad ticket
        let (s, h) = serve_once("200 OK", r#"{"username":"Friend","ticket":"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff","session":"-x"}"#);
        assert!(login(&s, "friend", Cred::Password("long enough"), true).is_err());
        h.join().unwrap();
    }

    #[test]
    fn a_report_carries_the_words_and_both_logs() {
        let (s, h) = serve_once("200 OK", r#"{"id":"20261122-1"}"#);
        assert_eq!(report(&s, "friend", None, "it broke", "launcher lines", "client lines").unwrap(), "20261122-1");
        let sent = h.join().unwrap();
        assert!(sent.starts_with("POST /api/report "));
        assert!(sent.contains(r#""text":"it broke""#) && sent.contains(r#""clientLog":"client lines""#) && sent.contains(r#""launcherLog":"launcher lines""#));
    }

    #[test]
    fn login_refusals_are_explained() {
        let (s, h) = serve_once("401 Unauthorized", r#"{"error":"BadCredentials"}"#);
        assert_eq!(login(&s, "friend", Cred::Password("wrong"), false).err(), Some("wrong account name or password".into()));
        h.join().unwrap();
        let (s, h) = serve_once("429 Too Many Requests", r#"{"error":"TooManyAttempts"}"#);
        assert_eq!(login(&s, "friend", Cred::Password("wrong"), false).err(), Some("too many tries: wait ten minutes".into()));
        h.join().unwrap();
        let (s, h) = serve_once("404 Not Found", "");
        assert_eq!(login(&s, "friend", Cred::Password("wrong"), false).err(), Some("this server has no account API".into()));
        h.join().unwrap();
    }

    #[test]
    fn the_library_comes_with_the_plan_and_the_credentials_go_in_the_body() {
        let (s, h) = serve_once("200 OK", r#"{"plan":"sub","planExpiresAt":"2027-01-01T00:00:00Z","cap":50,"upgradeUrl":"https://x.invalid/up","replays":[{"n":5,"title":"beat Opponent 0:42","when":1764000000005,"self":"You","other":"Opponent","overCap":false},{"n":4,"title":"lost to Opponent 1:10","when":1764000000004,"self":"You","other":"","overCap":true}]}"#);
        let l = list_replays(&s, "friend", Cred::Session("ffee")).unwrap();
        assert!(l.is_sub());
        assert_eq!((l.cap, l.plan_expires_at.as_deref(), l.upgrade_url.as_str()), (50, Some("2027-01-01T00:00:00Z"), "https://x.invalid/up"));
        assert_eq!(l.replays.len(), 2);
        assert_eq!((l.replays[0].n, l.replays[0].me.as_str(), l.replays[0].other.as_str()), (5, "You", "Opponent"));
        assert!(l.replays[1].over_cap && !l.replays[0].over_cap);
        let sent = h.join().unwrap();
        assert!(sent.starts_with("POST /api/replays/list "));
        assert!(sent.contains(r#""session":"ffee""#) && sent.contains(r#""username":"friend""#));
        // a free plan has no expiry; a server from before the library answers 404
        let (s, h) = serve_once("200 OK", r#"{"plan":"free","planExpiresAt":null,"cap":3,"upgradeUrl":"","replays":[]}"#);
        let l = list_replays(&s, "friend", Cred::Password("long enough")).unwrap();
        assert!(!l.is_sub() && l.plan_expires_at.is_none() && l.replays.is_empty());
        h.join().unwrap();
        let (s, h) = serve_once("404 Not Found", "");
        assert_eq!(list_replays(&s, "friend", Cred::Password("x")).err(), Some("this server has no replay library yet".into()));
        h.join().unwrap();
    }

    #[test]
    fn a_delete_names_the_stable_number_and_a_gone_one_is_explained() {
        let (s, h) = serve_once("200 OK", r#"{"deleted":4}"#);
        delete_replay(&s, "friend", Cred::Session("ffee"), 4).unwrap();
        let sent = h.join().unwrap();
        assert!(sent.starts_with("POST /api/replays/delete "));
        assert!(sent.contains(r#""n":4"#) && sent.contains(r#""session":"ffee""#));
        let (s, h) = serve_once("404 Not Found", r#"{"error":"NoSuchReplay"}"#);
        assert_eq!(delete_replay(&s, "friend", Cred::Session("ffee"), 9).err(), Some("that replay is not on the server any more".into()));
        h.join().unwrap();
        let (s, h) = serve_once("503 Service Unavailable", r#"{"error":"GameServerOffline"}"#);
        assert!(delete_replay(&s, "friend", Cred::Session("ffee"), 4).err().unwrap().contains("game server"));
        h.join().unwrap();
    }

    #[test]
    fn a_ticket_in_another_form_is_refused() {
        let (s, h) = serve_once("200 OK", r#"{"username":"friend","ticket":"-L injected"}"#);
        assert!(login(&s, "friend", Cred::Password("long enough"), false).is_err());
        h.join().unwrap();
    }

    #[test]
    fn a_redirect_is_not_followed() {
        let (s, h) = serve_once("307 Temporary Redirect\r\nLocation: http://127.0.0.1:9/elsewhere", "");
        assert!(login(&s, "friend", Cred::Password("long enough"), false).is_err());
        h.join().unwrap();
    }

    #[test]
    fn signup_sends_the_invite_and_explains_a_refusal() {
        let (s, h) = serve_once("200 OK", r#"{"username":"friend"}"#);
        assert_eq!(signup(&s, "friend", "long enough", " ABCD ").unwrap(), "friend");
        assert!(h.join().unwrap().contains(r#""invite":"ABCD""#));
        let (s, h) = serve_once("400 Bad Request", r#"{"error":"BadInvite"}"#);
        assert_eq!(signup(&s, "friend", "long enough", "x").err(), Some("that invite code is not valid or was already used".into()));
        h.join().unwrap();
    }

    #[test]
    fn info_is_read() {
        let (s, h) = serve_once("200 OK", r#"{"name":"Arena","builds":["297"],"addonPack":"","signup":"invite","ticketsOnly":true}"#);
        let i = info(&s).unwrap();
        assert_eq!((i.name.as_str(), i.builds.len(), i.signup.as_str(), i.tickets_only), ("Arena", 1, "invite", true));
        h.join().unwrap();
    }
    #[test]
    fn daru_codes_from_links_and_bare_codes() {
        assert_eq!(daru_code("https://darugear.com/?s=oqHS4M").as_deref(), Some("oqHS4M"));
        assert_eq!(daru_code("  oqHS4M ").as_deref(), Some("oqHS4M"));
        assert_eq!(daru_code("https://darugear.com/?x=1&s=ab_c-9#top").as_deref(), Some("ab_c-9"));
        assert_eq!(daru_code("https://evil.example/?s=abc"), None);
        assert_eq!(daru_code("https://darugear.com/?s=../x"), None);
        assert_eq!(daru_code(""), None);
        assert_eq!(daru_code("two words"), None);
    }

    #[test]
    fn a_daru_build_is_read() {
        let v: serde_json::Value = serde_json::from_str(r#"{"title":"Starter Healer","player":{"playerclass":{"name":"Cleric"}},"slots":[
            {"name":"Head","item":{"equipment":{"id":1,"name":"Hat"},"grade":{"name":"Mythic"}}},
            {"name":"Chest","item":{"equipment":{"id":0}}},
            {"name":"Main Hand","item":{"equipment":{"id":2,"name":"Staff"}}},
            {"name":"Costume","item":{"equipment":{"id":8000924.0,"name":"Melee Basic Costume"},"grade":{"id":11.0}}}]}"#).unwrap();
        let b = parse_build("xh1GuZ", &v).unwrap();
        assert_eq!((b.title.as_str(), b.class.as_str(), b.pieces.len()), ("Starter Healer", "Cleric", 3));
        assert_eq!(b.pieces[2], ("Costume".into(), "Melee Basic Costume".into(), "Mythic".into()));
        assert_eq!(b.pieces[0], ("Head".into(), "Hat".into(), "Mythic".into()));
        assert!(parse_build("x", &serde_json::json!({})).is_err());
    }

    #[test]
    fn a_transfer_carries_the_account_and_the_parts() {
        let (s, h) = serve_once("200 OK", r#"{"accepted":["gear","keybinds"]}"#);
        let a = transfer(&s, "friend", Cred::Password("long enough"), &serde_json::json!({"gear":"oqHS4M","uiData":{"2":"x"}})).unwrap();
        assert_eq!(a.accepted, vec!["gear", "keybinds"]);
        let sent = h.join().unwrap();
        assert!(sent.starts_with("POST /api/transfer "));
        assert!(sent.contains(r#""gear":"oqHS4M""#) && sent.contains(r#""username":"friend""#));
        let (s, h) = serve_once("404 Not Found", "");
        assert_eq!(transfer(&s, "a", Cred::Password("b"), &serde_json::json!({})).err(), Some("this server does not take transfers yet".into()));
        h.join().unwrap();
    }

}
