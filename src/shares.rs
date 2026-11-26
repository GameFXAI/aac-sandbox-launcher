//! The companion app's sharing, friends and guild, as the server sends them (aaemu35 AccountApi.Shares.cs). Every call
//! signs in like the rest of net.rs (password or remembered sign-in in the body) and the server decides everything:
//! who may see a share, who your friends are, which guild you are in, what is reviewed. The app shows what it last
//! heard and sends requests; people are only ever named by their characters' names, as the server gives them.

use crate::net::{Cred, Server};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Who a share is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Audience {
    Friends,
    Guild,
    Community,
}

impl Audience {
    pub const ALL: [Audience; 3] = [Audience::Friends, Audience::Guild, Audience::Community];

    pub fn wire(self) -> &'static str {
        match self {
            Audience::Friends => "friends",
            Audience::Guild => "guild",
            Audience::Community => "community",
        }
    }

    pub fn parse(s: &str) -> Option<Audience> {
        Audience::ALL.into_iter().find(|a| a.wire() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Audience::Friends => "Friends",
            Audience::Guild => "Guild",
            Audience::Community => "Community",
        }
    }
}

/// Which shares a feed holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    Community,
    Following,
    WithMe,
    Guild,
    Saved,
}

impl Scope {
    pub fn wire(self) -> &'static str {
        match self {
            Scope::Community => "community",
            Scope::Following => "following",
            Scope::WithMe => "withme",
            Scope::Guild => "guild",
            Scope::Saved => "saved",
        }
    }
}

/// A shared moment as the server lists it. `by` is the character the server names the owner by.
#[derive(Clone, Debug, Deserialize, Serialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Share {
    pub id: i64,
    pub n: i32,
    pub audience: String,
    /// the guild's name, for a guild share the server could name
    pub guild: Option<String>,
    pub title: String,
    pub by: String,
    #[serde(rename = "self")]
    pub me: String,
    pub other: String,
    pub kind: String,
    /// when the fight started, Unix milliseconds
    pub when: i64,
    /// seconds, when the title says
    pub length: Option<u32>,
    pub note: String,
    /// when it was shared (ISO 8601, UTC)
    pub created: String,
    pub mine: bool,
    pub saved: bool,
    pub following: bool,
    pub replies: u32,
    pub saves: u32,
    /// the server has a real picture of the recording (taken by the recorder during the fight)
    pub thumb: bool,
}

#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Feed {
    pub shares: Vec<Share>,
    /// pass as `after` for the next page; none at the end
    pub next: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Reply {
    pub id: i64,
    pub name: String,
    pub text: String,
    pub at: String,
    pub mine: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Default, PartialEq)]
#[serde(default)]
pub struct Mark {
    /// seconds into the recording
    pub at: u32,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Note {
    pub name: String,
    pub text: String,
    pub at: String,
    pub marks: Vec<Mark>,
}

#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Reviewed {
    pub name: String,
    pub at: String,
}

/// One share with its discussion, and for a guild share the guild's note and review.
#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Detail {
    pub share: Share,
    pub replies: Vec<Reply>,
    pub note: Option<Note>,
    pub review: Option<Reviewed>,
}

/// A character as the app shows it: in the world or not, and where (a zone number) or when it was last seen.
#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Person {
    pub name: String,
    pub online: bool,
    pub zone: u32,
    pub last_seen: Option<String>,
    pub level: u32,
}

#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Guild {
    pub name: String,
    pub online: u32,
    pub members: Vec<Person>,
}

/// The account's place in the game: the character it is known by, its friends, its guild.
#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Social {
    pub name: String,
    pub friends: Vec<Person>,
    pub guild: Option<Guild>,
}

impl Social {
    pub fn online(&self) -> usize {
        self.friends.iter().filter(|f| f.online).count()
    }
}

#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct ReviewItem {
    pub share: Share,
    pub reviewed: Option<Reviewed>,
}

#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Review {
    pub guild: String,
    pub shares: Vec<ReviewItem>,
}

impl Review {
    pub fn done(&self) -> usize {
        self.shares.iter().filter(|s| s.reviewed.is_some()).count()
    }
}

/// What the server says when it refuses: its error code, put in words.
fn refused(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(404, r) => {
            let code = r.into_json::<serde_json::Value>().ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or_default();
            match code.as_str() {
                "" => "this server has no shares yet".into(),
                c => explain(c),
            }
        }
        ureq::Error::Status(_, r) => explain(&r.into_json::<serde_json::Value>().ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or_default()),
        ureq::Error::Transport(t) => format!("cannot reach the server: {t}"),
    }
}

/// The server's error codes for these calls; the rest are the account API's own (net::explain).
pub fn explain(code: &str) -> String {
    match code {
        "NoSuchShare" => "that share is gone, or no longer shared with you".into(),
        "NoSuchReplay" => "that capture is not on the server any more".into(),
        "NoGuild" => "your character is not in a guild".into(),
        "NoCharacter" => "make a character in ArcheAge first: people here are known by their characters' names".into(),
        "BadTitle" => "a name of up to 64 characters".into(),
        "BadNote" => "the note is too long".into(),
        "BadReply" => "a reply of 1 to 500 characters".into(),
        "BadAudience" | "BadScope" | "BadFilter" => "the server did not take that choice".into(),
        "OwnShare" => "that's your own share".into(),
        other => crate::net::explain(other),
    }
}

/// One call: the credentials and `fields` in the body, the answer read as `T`.
fn call<T: DeserializeOwned>(server: &Server, path: &str, username: &str, cred: Cred, fields: serde_json::Value) -> Result<T, String> {
    let url = match server {
        Server::Api { url, .. } => url,
        Server::Direct(_) => return Err("shares live on servers with an account API (a server link)".into()),
    };
    let mut body = serde_json::json!({ "username": username });
    if let (Some(b), Some(f)) = (body.as_object_mut(), fields.as_object()) {
        for (k, v) in f {
            b.insert(k.clone(), v.clone());
        }
    }
    cred.put(&mut body);
    let r = crate::net::agent().post(&format!("{url}/api{path}")).send_json(body).map_err(refused)?;
    r.into_json().map_err(|e| format!("the server's answer was not understood: {e}"))
}

pub fn rename(server: &Server, user: &str, cred: Cred, n: i32, title: &str) -> Result<Option<String>, String> {
    #[derive(Deserialize)]
    struct A {
        name: Option<String>,
    }
    call::<A>(server, "/replays/rename", user, cred, serde_json::json!({ "n": n, "title": title })).map(|a| a.name)
}

pub fn share(server: &Server, user: &str, cred: Cred, n: i32, audience: Audience, note: &str) -> Result<i64, String> {
    #[derive(Deserialize)]
    struct A {
        share: i64,
    }
    call::<A>(server, "/replays/share", user, cred, serde_json::json!({ "n": n, "audience": audience.wire(), "note": note })).map(|a| a.share)
}

pub fn unshare(server: &Server, user: &str, cred: Cred, share: i64) -> Result<(), String> {
    call::<serde_json::Value>(server, "/replays/unshare", user, cred, serde_json::json!({ "share": share })).map(|_| ())
}

pub fn feed(server: &Server, user: &str, cred: Cred, scope: Scope, after: Option<i64>) -> Result<Feed, String> {
    call(server, "/shares/feed", user, cred, serde_json::json!({ "scope": scope.wire(), "after": after, "limit": 50 }))
}

pub fn get(server: &Server, user: &str, cred: Cred, share: i64) -> Result<Detail, String> {
    call(server, "/shares/get", user, cred, serde_json::json!({ "share": share }))
}

pub fn reply(server: &Server, user: &str, cred: Cred, share: i64, text: &str) -> Result<(), String> {
    call::<serde_json::Value>(server, "/shares/reply", user, cred, serde_json::json!({ "share": share, "text": text })).map(|_| ())
}

pub fn save(server: &Server, user: &str, cred: Cred, share: i64, on: bool) -> Result<(), String> {
    call::<serde_json::Value>(server, if on { "/shares/save" } else { "/shares/unsave" }, user, cred, serde_json::json!({ "share": share })).map(|_| ())
}

pub fn follow(server: &Server, user: &str, cred: Cred, share: i64, on: bool) -> Result<(), String> {
    call::<serde_json::Value>(server, if on { "/shares/follow" } else { "/shares/unfollow" }, user, cred, serde_json::json!({ "share": share })).map(|_| ())
}

pub fn social(server: &Server, user: &str, cred: Cred) -> Result<Social, String> {
    call(server, "/social", user, cred, serde_json::json!({}))
}

pub fn review(server: &Server, user: &str, cred: Cred) -> Result<Review, String> {
    call(server, "/guild/review", user, cred, serde_json::json!({ "filter": "all" }))
}

pub fn reviewed(server: &Server, user: &str, cred: Cred, share: i64, done: bool) -> Result<(), String> {
    call::<serde_json::Value>(server, "/guild/reviewed", user, cred, serde_json::json!({ "share": share, "done": done })).map(|_| ())
}

pub fn note(server: &Server, user: &str, cred: Cred, share: i64, text: &str, marks: &[Mark]) -> Result<(), String> {
    call::<serde_json::Value>(server, "/guild/note", user, cred, serde_json::json!({ "share": share, "text": text, "marks": marks })).map(|_| ())
}

/// "00:42" or "1:02:03" or "42" -> seconds; for a guild note's marks.
pub fn mark_seconds(s: &str) -> Option<u32> {
    let t = s.trim();
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit() || c == ':') {
        return None;
    }
    t.split(':').try_fold(0u32, |acc, part| part.parse::<u32>().ok().map(|v| acc * 60 + v))
}

/// 42 -> "00:42", 3723 -> "1:02:03".
pub fn mark_words(secs: u32) -> String {
    if secs >= 3600 { format!("{}:{:02}:{:02}", secs / 3600, secs % 3600 / 60, secs % 60) } else { format!("{:02}:{:02}", secs / 60, secs % 60) }
}

/// A note's marks from the text a player types: "00:42 Hold the gangplank" per line; lines without a time are left
/// out (the server checks them again).
pub fn parse_marks(text: &str) -> Vec<Mark> {
    text.lines()
        .filter_map(|l| {
            let (t, rest) = l.trim().split_once(' ').unwrap_or((l.trim(), ""));
            mark_seconds(t).map(|at| Mark { at, text: rest.trim().to_string() })
        })
        .filter(|m| !m.text.is_empty())
        .take(20)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_feed_reads_as_the_server_sends_it() {
        let j = r#"{"shares":[{"id":3,"n":2,"audience":"guild","guild":"Ember Watch","title":"beat Brannoc 0:42","by":"Ayla",
            "self":"Ayla","other":"Brannoc","kind":"duel","when":1700000000000,"length":42,"note":"watch the stun",
            "created":"2026-11-26T12:00:00Z","mine":false,"saved":true,"following":false,"replies":2,"saves":1}],"next":null}"#;
        let f: Feed = serde_json::from_str(j).unwrap();
        assert_eq!(f.shares[0].by, "Ayla");
        assert_eq!(f.shares[0].me, "Ayla");
        assert_eq!(f.shares[0].length, Some(42));
        assert_eq!(f.shares[0].guild.as_deref(), Some("Ember Watch"));
        assert!(f.next.is_none());
        assert_eq!(Audience::parse(&f.shares[0].audience), Some(Audience::Guild));
    }

    #[test]
    fn social_and_review_read_and_count() {
        let s: Social = serde_json::from_str(r#"{"name":"Ayla","friends":[{"name":"Brannoc","online":true,"zone":179,"lastSeen":null,"level":55},
            {"name":"Corvin","online":false,"zone":0,"lastSeen":"2026-11-24T00:00:00Z","level":40}],"guild":{"name":"Ember Watch","online":1,"members":[]}}"#).unwrap();
        assert_eq!(s.online(), 1);
        assert_eq!(s.friends[1].last_seen.as_deref(), Some("2026-11-24T00:00:00Z"));
        let r: Review = serde_json::from_str(r#"{"guild":"Ember Watch","shares":[{"share":{"id":1},"reviewed":{"name":"Corvin","at":"x"}},{"share":{"id":2},"reviewed":null}]}"#).unwrap();
        assert_eq!(r.done(), 1);
    }

    #[test]
    fn marks_are_read_from_lines_with_a_time() {
        assert_eq!(mark_seconds("00:42"), Some(42));
        assert_eq!(mark_seconds("1:02:03"), Some(3723));
        assert_eq!(mark_seconds("x1"), None);
        assert_eq!(mark_words(42), "00:42");
        assert_eq!(mark_words(3723), "1:02:03");
        let m = parse_marks("00:42 Hold the gangplank\nno time here\n0:47 Wait for the pull\n1:00");
        assert_eq!(m, vec![Mark { at: 42, text: "Hold the gangplank".into() }, Mark { at: 47, text: "Wait for the pull".into() }]);
    }

    /// Every call against a real server: AAEMU_SHARES_LIVE names a JSON file {"server", "username", "session"} of a
    /// signed-in throwaway account with recordings 1 and 2 and a guild (a local test server only). Run with
    /// `cargo test live_round_trip -- --ignored`.
    #[test]
    #[ignore]
    fn live_round_trip() {
        let path = std::env::var("AAEMU_SHARES_LIVE").expect("AAEMU_SHARES_LIVE");
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let server = Server::parse(v["server"].as_str().unwrap()).unwrap();
        let (user, session) = (v["username"].as_str().unwrap(), v["session"].as_str().unwrap());
        let c = || Cred::Session(session);

        let s = social(&server, user, c()).unwrap();
        assert!(!s.name.is_empty() && s.guild.is_some(), "{s:?}");
        let with = feed(&server, user, c(), Scope::WithMe, None).unwrap();
        assert!(!with.shares.is_empty());
        let other = with.shares.iter().find(|x| !x.mine).unwrap().clone();

        assert_eq!(rename(&server, user, c(), 2, "Live round trip").unwrap().as_deref(), Some("Live round trip"));
        let id = share(&server, user, c(), 2, Audience::Community, "from the launcher's test").unwrap();
        assert_eq!(share(&server, user, c(), 2, Audience::Community, "").unwrap(), id);
        let lib = crate::net::list_replays(&server, user, c()).unwrap();
        let r = lib.replays.iter().find(|r| r.n == 2).unwrap();
        assert_eq!(r.name.as_deref(), Some("Live round trip"));
        assert!(r.shares.iter().any(|x| x.id == id && x.audience == "community"));

        reply(&server, user, c(), other.id, "seen from the launcher").unwrap();
        let d = get(&server, user, c(), other.id).unwrap();
        assert!(d.replies.iter().any(|x| x.mine && x.text == "seen from the launcher"));
        save(&server, user, c(), other.id, true).unwrap();
        assert!(feed(&server, user, c(), Scope::Saved, None).unwrap().shares.iter().any(|x| x.id == other.id));
        save(&server, user, c(), other.id, false).unwrap();
        follow(&server, user, c(), other.id, true).unwrap();
        assert!(feed(&server, user, c(), Scope::Following, None).unwrap().shares.iter().any(|x| x.id == other.id));
        follow(&server, user, c(), other.id, false).unwrap();

        let q = review(&server, user, c()).unwrap();
        let g = q.shares.iter().find(|i| i.reviewed.is_none()).unwrap().share.id;
        reviewed(&server, user, c(), g, true).unwrap();
        note(&server, user, c(), g, "from the launcher", &parse_marks("00:42 the gangplank")).unwrap();
        let d = get(&server, user, c(), g).unwrap();
        assert_eq!(d.note.as_ref().map(|n| (n.text.as_str(), n.marks.len())), Some(("from the launcher", 1)));
        assert!(d.review.is_some());
        reviewed(&server, user, c(), g, false).unwrap();

        assert!(crate::net::watch_share(&server, user, c(), other.id).unwrap() > 0);
        unshare(&server, user, c(), id).unwrap();
        // a removed share is gone for everyone, its owner too
        assert!(get(&server, user, c(), id).unwrap_err().contains("gone"));
        assert_eq!(rename(&server, user, c(), 2, "").unwrap(), None);
    }

    #[test]
    fn refusals_are_put_in_words() {
        assert!(explain("NoGuild").contains("guild"));
        assert!(explain("NoSuchShare").contains("gone"));
        assert!(explain("BadCredentials").contains("password"));
    }
}
