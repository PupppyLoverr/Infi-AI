//! Bridge for opencode (`opencode serve`): its sessions appear in the notch,
//! its permission and question requests are answered there, and text typed
//! to the agent in the notch is sent to the session as a prompt.
//!
//! opencode is built around a local HTTP server with an event bus, which is a
//! better contract than screen-scraping: `GET /global/event` is a
//! Server-Sent Events stream of typed events for every project directory and
//! subagent session (`{directory, payload}` frames; requests are scoped back
//! with `?directory=`), and requests are answered by id
//! (`POST /permission/{id}/reply` with `once` / `always` / `reject`,
//! `POST /question/{id}/reply` with one list of labels per question). Text
//! goes to `POST /session/{id}/prompt_async`. Checked against opencode
//! 1.19's OpenAPI document (`GET /doc`).
//!
//! Mapping to the Solyra protocol (docs/agent-protocol.md):
//! session seen → `started`; `session.status` busy/idle → `status`;
//! `todo.updated` → `milestone` (the item in progress);
//! `permission.asked` → `approval_request` (approve once → `once`, approve
//! for the task → `always`, deny → `reject`); `question.asked` → `question`;
//! answered in opencode itself → `approval_resolved` "elsewhere";
//! `session.error` → `milestone`.
//!
//! Only plain `http://host:port` URLs (opencode serves on localhost). With
//! `OPENCODE_SERVER_PASSWORD` set (opencode's HTTP Basic auth, user
//! `opencode`) the bridge sends it. Lessons taken from zeron's native opencode
//! driver: the global bus rather than one directory's, directory-scoped
//! replies, and surfacing `retry` statuses so a failing provider is visible.

use std::collections::HashSet;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use crate::events::{Answer, Emitter};

/// What one opencode event means for the panel.
#[derive(Debug, PartialEq)]
pub enum Action {
    /// Append this event to the session's task file.
    Emit { session: String, event: Value },
    /// Ask the person; answer opencode with the result.
    Permission { session: String, id: String, event: Value },
    Question { session: String, id: String, questions: Vec<(String, Vec<String>)> },
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn clip(t: &str) -> String {
    let t = t.trim().replace('\n', " ");
    if t.chars().count() > 140 {
        format!("{}…", t.chars().take(139).collect::<String>())
    } else {
        t
    }
}

/// Our task id for an opencode session.
pub fn task_id(session: &str) -> String {
    crate::events::clean_id(&format!("opencode-{session}"))
}

/// Translate one event from opencode's bus. Pure, for tests.
pub fn translate(ev: &Value) -> Vec<Action> {
    let p = ev.get("properties").unwrap_or(&Value::Null);
    let session = s(p, "sessionID").to_string();
    let emit = |event: Value| Action::Emit { session: session.clone(), event };
    match s(ev, "type") {
        "session.status" => {
            let state = match p.pointer("/status/type").and_then(Value::as_str) {
                Some("idle") => "idle",
                Some("retry") => "waiting",
                Some(_) => "working",
                None => return vec![],
            };
            let mut out = vec![emit(json!({"type": "status", "state": state}))];
            if let Some(msg) = p.pointer("/status/message").and_then(Value::as_str) {
                out.push(emit(json!({"type": "milestone", "text": format!("Retrying: {}", clip(msg))})));
            }
            out
        }
        "session.idle" => vec![emit(json!({"type": "status", "state": "idle"}))],
        "todo.updated" => {
            let todos = p.get("todos").and_then(Value::as_array).cloned().unwrap_or_default();
            let doing = todos.iter().find(|t| s(t, "status") == "in_progress");
            let done = todos.iter().filter(|t| s(t, "status") == "completed").count();
            match doing {
                Some(t) => vec![emit(json!({"type": "milestone",
                    "text": format!("{} ({done}/{} done)", clip(s(t, "content")), todos.len())}))],
                None => vec![],
            }
        }
        "session.error" => {
            let msg = p
                .pointer("/error/data/message")
                .or_else(|| p.pointer("/error/message"))
                .and_then(Value::as_str)
                .unwrap_or("an error");
            vec![emit(json!({"type": "milestone", "text": format!("Error: {}", clip(msg))}))]
        }
        "permission.asked" => {
            let id = s(p, "id").to_string();
            let what = s(p, "permission");
            let patterns: Vec<&str> = p
                .get("patterns")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let action = match what {
                "bash" => "Run a shell command".to_string(),
                "edit" | "write" => "Change files".to_string(),
                "webfetch" => "Fetch a web page".to_string(),
                "external_directory" => "Work outside the project folder".to_string(),
                "" => "Use a tool".to_string(),
                other => format!("Use {other}"),
            };
            let mut event = json!({
                "type": "approval_request", "request_id": id,
                "action": action, "scopes": ["once", "task"],
            });
            if !patterns.is_empty() {
                event["resource"] = json!(clip(&patterns.join(", ")));
            }
            if matches!(what, "read" | "list" | "glob" | "grep" | "webfetch") {
                event["reversible"] = json!(true);
            }
            vec![Action::Permission { session, id, event }]
        }
        "permission.replied" | "question.replied" | "question.rejected" => {
            let id = s(p, "requestID").to_string();
            vec![emit(json!({"type": "approval_resolved", "request_id": id, "decision": "elsewhere"}))]
        }
        "question.asked" => {
            let questions = p
                .get("questions")
                .and_then(Value::as_array)
                .map(|qs| {
                    qs.iter()
                        .map(|q| {
                            let labels = q
                                .get("options")
                                .and_then(Value::as_array)
                                .map(|o| o.iter().map(|o| s(o, "label").to_string()).collect())
                                .unwrap_or_default();
                            (s(q, "question").to_string(), labels)
                        })
                        .collect()
                })
                .unwrap_or_default();
            vec![Action::Question { session, id: s(p, "id").to_string(), questions }]
        }
        _ => vec![],
    }
}

/// A global-bus frame is `{directory, payload}`; a per-directory one is the
/// event itself.
pub fn unwrap_global(frame: Value) -> (Value, Option<String>) {
    match frame.get("payload") {
        Some(p) => (
            p.clone(),
            frame.get("directory").and_then(Value::as_str).map(str::to_string),
        ),
        None => (frame, None),
    }
}

/// opencode's reply for a panel answer.
pub fn permission_reply(a: &Answer) -> &'static str {
    match a {
        Answer::Approve { scope_task: true } => "always",
        Answer::Approve { scope_task: false } => "once",
        Answer::Deny | Answer::Text(_) => "reject",
    }
}

#[derive(Clone, Debug)]
pub struct Server {
    host: String,
    auth: Option<String>,
}

fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in input.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `?directory=` for a reply, percent-encoded.
pub fn scoped(path: &str, directory: Option<&str>) -> String {
    match directory.filter(|d| !d.is_empty()) {
        None => path.to_string(),
        Some(d) => {
            let enc: String = d
                .bytes()
                .map(|b| match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                        (b as char).to_string()
                    }
                    _ => format!("%{b:02X}"),
                })
                .collect();
            format!("{path}?directory={enc}")
        }
    }
}

impl Server {
    pub fn parse(url: &str) -> Result<Self, String> {
        let rest = url
            .strip_prefix("http://")
            .ok_or("only http:// URLs (opencode serves on localhost)")?;
        let host = rest.split('/').next().unwrap_or("").to_string();
        if host.is_empty() || !host.contains(':') {
            return Err("give host:port, e.g. http://127.0.0.1:4096".into());
        }
        let auth = std::env::var("OPENCODE_SERVER_PASSWORD")
            .ok()
            .filter(|p| !p.is_empty())
            .map(|p| format!("Authorization: Basic {}\r\n", base64(format!("opencode:{p}").as_bytes())));
        Ok(Self { host, auth })
    }

    fn auth(&self) -> &str {
        self.auth.as_deref().unwrap_or("")
    }

    fn connect(&self) -> io::Result<TcpStream> {
        let s = TcpStream::connect(&self.host)?;
        s.set_write_timeout(Some(Duration::from_secs(5)))?;
        Ok(s)
    }

    /// POST JSON; returns the status code.
    pub fn post(&self, path: &str, body: &Value) -> io::Result<u16> {
        let mut s = self.connect()?;
        s.set_read_timeout(Some(Duration::from_secs(15)))?;
        let body = body.to_string();
        write!(
            s,
            "POST {path} HTTP/1.1\r\nHost: {}\r\n{}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.host,
            self.auth(),
            body.len()
        )?;
        let mut resp = String::new();
        let _ = s.read_to_string(&mut resp);
        resp.split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .ok_or_else(|| io::Error::other("no HTTP status"))
    }

    /// Open the global event stream; yields `(event, directory)`.
    pub fn events(&self) -> io::Result<impl Iterator<Item = (Value, Option<String>)>> {
        let mut s = self.connect()?;
        write!(
            s,
            "GET /global/event HTTP/1.1\r\nHost: {}\r\n{}Accept: text/event-stream\r\n\r\n",
            self.host,
            self.auth()
        )?;
        let reader = BufReader::new(s);
        Ok(reader
            .lines()
            .map_while(Result::ok)
            .filter_map(|l| l.strip_prefix("data:").map(|d| d.trim().to_string()))
            .filter_map(|d| serde_json::from_str::<Value>(&d).ok())
            .map(unwrap_global))
    }
}

/// Run the bridge until the event stream ends.
pub fn run(server: Server, root: std::path::PathBuf, wait: Duration) -> io::Result<()> {
    let seen: Arc<Mutex<HashSet<String>>> = Arc::default();
    let dirs: Arc<Mutex<std::collections::HashMap<String, String>>> = Arc::default();
    // Talk to the agent: forward text the panel wrote for any session we know.
    {
        let (server, root, seen, dirs) = (server.clone(), root.clone(), seen.clone(), dirs.clone());
        std::thread::spawn(move || {
            let mut offsets = std::collections::HashMap::<String, u64>::new();
            loop {
                let sessions: Vec<String> = seen.lock().map(|s| s.iter().cloned().collect()).unwrap_or_default();
                for sid in sessions {
                    let e = Emitter::new(root.clone(), &task_id(&sid));
                    let from = offsets.get(&sid).copied().unwrap_or_else(|| e.read_messages(0).1);
                    let (msgs, next) = e.read_messages(from);
                    offsets.insert(sid.clone(), next);
                    for text in msgs {
                        let body = json!({"parts": [{"type": "text", "text": text}]});
                        let dir = dirs.lock().ok().and_then(|m| m.get(&sid).cloned());
                        let path = scoped(&format!("/session/{sid}/prompt_async"), dir.as_deref());
                        match server.post(&path, &body) {
                            Ok(c) if c < 300 => {
                                let _ = e.emit(json!({"type":"milestone","text": format!("You: {}", clip(&text))}));
                            }
                            other => eprintln!("solyra-agent: opencode prompt: {other:?}"),
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(400));
            }
        });
    }

    // opencode repeats a status; the panel only needs changes.
    let mut last_state = std::collections::HashMap::<String, String>::new();
    for (ev, directory) in server.events()? {
        for action in translate(&ev) {
            if let Action::Emit { session, event } = &action {
                if event["type"] == "status" {
                    let st = event["state"].as_str().unwrap_or("").to_string();
                    if last_state.get(session) == Some(&st) {
                        continue;
                    }
                    last_state.insert(session.clone(), st);
                }
            }
            let session = match &action {
                Action::Emit { session, .. }
                | Action::Permission { session, .. }
                | Action::Question { session, .. } => session.clone(),
            };
            if session.is_empty() {
                continue;
            }
            if let (Some(d), Ok(mut map)) = (&directory, dirs.lock()) {
                map.insert(session.clone(), d.clone());
            }
            let dir = dirs.lock().ok().and_then(|m| m.get(&session).cloned());
            let e = Emitter::new(root.clone(), &task_id(&session));
            if seen.lock().map(|mut s| s.insert(session.clone())).unwrap_or(false) && !e.exists() {
                let _ = e.emit(json!({"type": "started", "name": "opencode session",
                    "agent": "opencode", "interactive": true}));
            }
            match action {
                Action::Emit { event, .. } => {
                    let _ = e.emit(event);
                }
                Action::Permission { id, event, .. } => {
                    let _ = e.emit(event);
                    let server = server.clone();
                    std::thread::spawn(move || {
                        let Some(a) = e.wait_answer(&id, wait) else {
                            let _ = e.emit(json!({"type":"approval_resolved","request_id": id,"decision":"timeout"}));
                            return;
                        };
                        let reply = permission_reply(&a);
                        let _ = server.post(&scoped(&format!("/permission/{id}/reply"), dir.as_deref()), &json!({"reply": reply}));
                        let decision = if reply == "reject" { "deny" } else { "approve" };
                        let _ = e.emit(json!({"type":"approval_resolved","request_id": id,"decision": decision}));
                    });
                }
                Action::Question { id, questions, .. } => {
                    let server = server.clone();
                    std::thread::spawn(move || {
                        let mut answers = Vec::new();
                        for (i, (text, choices)) in questions.iter().enumerate() {
                            let rid = format!("{id}-{i}");
                            let _ = e.emit(json!({"type":"question","request_id": rid,"text": text,"choices": choices}));
                            match e.wait_answer(&rid, wait) {
                                Some(Answer::Text(t)) => answers.push(vec![t]),
                                Some(Answer::Approve { .. }) if !choices.is_empty() => {
                                    answers.push(vec![choices[0].clone()])
                                }
                                _ => {
                                    let _ = server.post(&scoped(&format!("/question/{id}/reject"), dir.as_deref()), &json!({}));
                                    let _ = e.emit(json!({"type":"approval_resolved","request_id": rid,"decision":"deny"}));
                                    return;
                                }
                            }
                            let _ = e.emit(json!({"type":"approval_resolved","request_id": rid,"decision":"answer"}));
                        }
                        let _ = server.post(&scoped(&format!("/question/{id}/reply"), dir.as_deref()), &json!({"answers": answers}));
                    });
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn ev(t: &str, p: Value) -> Value {
        json!({"id": "evt_1", "type": t, "properties": p})
    }

    #[test]
    fn status_and_todos_become_status_and_milestones() {
        let a = translate(&ev("session.status", json!({"sessionID":"ses_1","status":{"type":"busy"}})));
        assert_eq!(a, vec![Action::Emit { session: "ses_1".into(), event: json!({"type":"status","state":"working"}) }]);
        let a = translate(&ev("session.status", json!({"sessionID":"ses_1","status":{"type":"idle"}})));
        assert!(matches!(&a[0], Action::Emit { event, .. } if event["state"] == "idle"));
        let a = translate(&ev("todo.updated", json!({"sessionID":"ses_1","todos":[
            {"content":"Write tests","status":"completed","priority":"high"},
            {"content":"Fix the parser","status":"in_progress","priority":"high"},
            {"content":"Docs","status":"pending","priority":"low"}]})));
        assert!(matches!(&a[0], Action::Emit { event, .. } if event["text"] == "Fix the parser (1/3 done)"));
        assert!(translate(&ev("server.connected", json!({}))).is_empty());
    }

    #[test]
    fn permission_requests_carry_what_and_where() {
        let a = translate(&ev("permission.asked", json!({"id":"per_9","sessionID":"ses_1",
            "permission":"bash","patterns":["cargo test","git push"],"metadata":{},"always":[]})));
        let Action::Permission { session, id, event } = &a[0] else { panic!("{a:?}") };
        assert_eq!((session.as_str(), id.as_str()), ("ses_1", "per_9"));
        assert_eq!(event["action"], "Run a shell command");
        assert_eq!(event["resource"], "cargo test, git push");
        assert_eq!(event["request_id"], "per_9");
        assert!(event.get("reversible").is_none(), "a shell command is not marked reversible");
    }

    #[test]
    fn answers_map_to_opencode_replies() {
        assert_eq!(permission_reply(&Answer::Approve { scope_task: false }), "once");
        assert_eq!(permission_reply(&Answer::Approve { scope_task: true }), "always");
        assert_eq!(permission_reply(&Answer::Deny), "reject");
    }

    #[test]
    fn questions_keep_their_choices() {
        let a = translate(&ev("question.asked", json!({"id":"que_1","sessionID":"ses_1","questions":[
            {"question":"Which branch?","header":"Branch","options":[
                {"label":"main","description":""},{"label":"dev","description":""}]}]})));
        assert_eq!(a, vec![Action::Question { session: "ses_1".into(), id: "que_1".into(),
            questions: vec![("Which branch?".into(), vec!["main".into(), "dev".into()])] }]);
    }

    #[test]
    fn base64_and_scoping() {
        assert_eq!(base64(b"opencode:pw"), "b3BlbmNvZGU6cHc=");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(scoped("/x", None), "/x");
        assert_eq!(scoped("/x", Some("/a b")), "/x?directory=/a%20b");
        let (ev, dir) = unwrap_global(json!({"directory":"/w","payload":{"type":"session.idle"}}));
        assert_eq!((ev["type"].as_str(), dir.as_deref()), (Some("session.idle"), Some("/w")));
    }

    #[test]
    fn urls_are_checked() {
        assert!(Server::parse("http://127.0.0.1:4096").is_ok());
        assert!(Server::parse("https://x:1").is_err());
        assert!(Server::parse("http://localhost").is_err());
    }

    /// End to end against a fake opencode: one permission request arrives on
    /// the event stream, the panel approves it for the task, and the bridge
    /// POSTs `always` to the right endpoint.
    #[test]
    fn bridge_answers_a_permission_through_the_panel_files() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let posted: Arc<Mutex<Vec<String>>> = Arc::default();
        {
            let posted = posted.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let mut s = stream.unwrap();
                    let mut buf = [0u8; 4096];
                    let n = s.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    if req.starts_with("GET /global/event") {
                        let ev = json!({"directory": "/home/me/my app", "payload": {"type":"permission.asked",
                            "properties":{"id":"per_7","sessionID":"ses_a",
                            "permission":"edit","patterns":["src/main.rs"],"metadata":{},"always":[]}}});
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\ndata: {ev}\n\n");
                        // Keep the stream open long enough for the answer.
                        std::thread::sleep(Duration::from_secs(3));
                    } else {
                        posted.lock().unwrap().push(req);
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\ntrue");
                    }
                }
            });
        }
        let root = crate::events::scratch("opencode");
        let r2 = root.clone();
        std::thread::spawn(move || {
            let _ = run(Server::parse(&format!("http://127.0.0.1:{port}")).unwrap(), r2, Duration::from_secs(5));
        });
        // The panel side: wait for the request, then approve for the task.
        let task = root.join("tasks").join(format!("{}.jsonl", task_id("ses_a")));
        let mut ok = false;
        for _ in 0..50 {
            if std::fs::read_to_string(&task).is_ok_and(|t| t.contains("approval_request")) {
                ok = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(ok, "the request reached the task file");
        let text = std::fs::read_to_string(&task).unwrap();
        assert!(text.contains("\"agent\":\"opencode\""), "started first: {text}");
        let d = root.join("decisions");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join(format!("{}.jsonl", task_id("ses_a"))),
            "{\"request_id\":\"per_7\",\"decision\":\"approve\",\"scope\":\"task\"}\n",
        )
        .unwrap();
        let mut sent = None;
        for _ in 0..60 {
            if let Some(r) = posted.lock().unwrap().iter().find(|r| r.starts_with("POST /permission/per_7/reply?directory=/home/me/my%20app ")) {
                sent = Some(r.clone());
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let sent = sent.expect("the bridge replied to opencode");
        assert!(sent.ends_with("{\"reply\":\"always\"}"), "{sent}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
