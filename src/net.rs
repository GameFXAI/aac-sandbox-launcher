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
}
