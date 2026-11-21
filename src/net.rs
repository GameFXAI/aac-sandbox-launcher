//! The account API of a server: what the server is, sign-up, and login for a launch ticket.
//!
//! The rules this module keeps:
//! - The password goes only to the server the player typed, and only over HTTPS. Plain HTTP is allowed to this PC
//!   alone (a server on the same machine, or a tunnel that ends on a loopback address).
//! - Redirects are not followed, so a server cannot pass the password on to another address.
//! - Nothing here writes the password or the ticket to a log or to disk.

use serde::Deserialize;
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
        "" => "the server refused the request".into(),
        other => format!("the server refused the request ({other})"),
    }
}

fn agent() -> ureq::Agent {
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

pub fn login(server: &Server, username: &str, password: &str) -> Result<Ticket, String> {
    let body = serde_json::json!({ "username": username, "password": password });
    let r = agent().post(&format!("{}/api/login", api_url(server)?)).send_json(body).map_err(failed)?;
    let t: Ticket = r.into_json().map_err(|e| format!("the server's answer was not understood: {e}"))?;
    if !crate::core::valid_token(&t.ticket) {
        return Err("the server's ticket is not in the expected form".into());
    }
    Ok(t)
}

/// What the server took from a transfer: the parts it will put on the character at the next world entry.
#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct Accepted {
    pub accepted: Vec<String>,
}

/// Sends a transfer from AA Classic (see transfer.rs for what is in it) with the account's own name and password.
/// The server keeps it for the account and applies it when one of its characters next enters the world.
pub fn transfer(server: &Server, username: &str, password: &str, payload: &serde_json::Value) -> Result<Accepted, String> {
    let mut body = payload.clone();
    body["username"] = username.into();
    body["password"] = password.into();
    let r = agent().post(&format!("{}/api/transfer", api_url(server)?)).send_json(body).map_err(|e| match e {
        ureq::Error::Status(404, _) => "this server does not take transfers yet".to_string(),
        e => failed(e),
    })?;
    r.into_json().map_err(|e| format!("the server's answer was not understood: {e}"))
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
        assert!(login(&s, "a", "b").is_err());
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
        let t = login(&s, "friend", "long enough").unwrap();
        assert_eq!(t.username, "Friend");
        assert_eq!(t.ticket.len(), 64);
        let sent = h.join().unwrap();
        assert!(sent.starts_with("POST /api/login "));
        assert!(sent.contains(r#""username":"friend""#) && sent.contains(r#""password":"long enough""#));
    }

    #[test]
    fn login_refusals_are_explained() {
        let (s, h) = serve_once("401 Unauthorized", r#"{"error":"BadCredentials"}"#);
        assert_eq!(login(&s, "friend", "wrong").err(), Some("wrong account name or password".into()));
        h.join().unwrap();
        let (s, h) = serve_once("429 Too Many Requests", r#"{"error":"TooManyAttempts"}"#);
        assert_eq!(login(&s, "friend", "wrong").err(), Some("too many tries: wait ten minutes".into()));
        h.join().unwrap();
        let (s, h) = serve_once("404 Not Found", "");
        assert_eq!(login(&s, "friend", "wrong").err(), Some("this server has no account API".into()));
        h.join().unwrap();
    }

    #[test]
    fn a_ticket_in_another_form_is_refused() {
        let (s, h) = serve_once("200 OK", r#"{"username":"friend","ticket":"-L injected"}"#);
        assert!(login(&s, "friend", "long enough").is_err());
        h.join().unwrap();
    }

    #[test]
    fn a_redirect_is_not_followed() {
        let (s, h) = serve_once("307 Temporary Redirect\r\nLocation: http://127.0.0.1:9/elsewhere", "");
        assert!(login(&s, "friend", "long enough").is_err());
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
        let a = transfer(&s, "friend", "long enough", &serde_json::json!({"gear":"oqHS4M","uiData":{"2":"x"}})).unwrap();
        assert_eq!(a.accepted, vec!["gear", "keybinds"]);
        let sent = h.join().unwrap();
        assert!(sent.starts_with("POST /api/transfer "));
        assert!(sent.contains(r#""gear":"oqHS4M""#) && sent.contains(r#""username":"friend""#));
        let (s, h) = serve_once("404 Not Found", "");
        assert_eq!(transfer(&s, "a", "b", &serde_json::json!({})).err(), Some("this server does not take transfers yet".into()));
        h.join().unwrap();
    }

}
