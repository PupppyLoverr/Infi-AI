//! The runtime side of the panel protocol (docs/agent-protocol.md): append
//! events to `tasks/<id>.jsonl`, read the panel's answers from
//! `decisions/<id>.jsonl` and messages from `messages/<id>.jsonl`.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// `$XDG_RUNTIME_DIR/solyra-agent`, or `SOLYRA_AGENT_DIR` (tests, containers).
pub fn root_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("SOLYRA_AGENT_DIR") {
        return Some(PathBuf::from(d));
    }
    Some(PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?).join("solyra-agent"))
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Task ids become file names: letters, digits, `-` and `_`, at most 64.
pub fn clean_id(raw: &str) -> String {
    let id: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .take(64)
        .collect();
    if id.is_empty() { "task".into() } else { id }
}

#[derive(Clone, Debug)]
pub struct Emitter {
    root: PathBuf,
    pub id: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    Approve { scope_task: bool },
    Deny,
    Text(String),
}

impl Emitter {
    pub fn new(root: PathBuf, id: &str) -> Self {
        Self { root, id: clean_id(id) }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, dir: &str) -> PathBuf {
        self.root.join(dir).join(format!("{}.jsonl", self.id))
    }

    pub fn exists(&self) -> bool {
        self.path("tasks").exists()
    }

    /// Append one event. The runtime creates the directory: its presence is
    /// what tells the panel an agent runtime exists.
    pub fn emit(&self, event: Value) -> io::Result<()> {
        let mut event = event;
        if let Some(o) = event.as_object_mut() {
            o.entry("ts").or_insert(json!(now()));
        }
        let p = self.path("tasks");
        fs::create_dir_all(p.parent().expect("has parent"))?;
        let mut f = fs::OpenOptions::new().create(true).append(true).open(p)?;
        f.write_all(format!("{event}\n").as_bytes())
    }

    /// Block until the panel answers `request_id`, up to `timeout`.
    pub fn wait_answer(&self, request_id: &str, timeout: Duration) -> Option<Answer> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(a) = self.read_answer(request_id) {
                return Some(a);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn read_answer(&self, request_id: &str) -> Option<Answer> {
        let text = fs::read_to_string(self.path("decisions")).ok()?;
        text.lines().rev().find_map(|line| {
            let v: Value = serde_json::from_str(line).ok()?;
            if v.get("request_id")?.as_str()? != request_id {
                return None;
            }
            match v.get("decision")?.as_str()? {
                "approve" => Some(Answer::Approve {
                    scope_task: v.get("scope").and_then(Value::as_str) == Some("task"),
                }),
                "deny" => Some(Answer::Deny),
                "answer" => Some(Answer::Text(v.get("text")?.as_str()?.to_string())),
                _ => None,
            }
        })
    }

    /// Messages the panel wrote after byte offset `from`; returns them and the
    /// new offset. Complete lines only.
    pub fn read_messages(&self, from: u64) -> (Vec<String>, u64) {
        let Ok(bytes) = fs::read(self.path("messages")) else {
            return (Vec::new(), from);
        };
        let from = (from as usize).min(bytes.len());
        let tail = &bytes[from..];
        let Some(end) = tail.iter().rposition(|b| *b == b'\n') else {
            return (Vec::new(), from as u64);
        };
        let msgs = String::from_utf8_lossy(&tail[..=end])
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|v| v.get("text").and_then(Value::as_str).map(str::to_string))
            .collect();
        (msgs, (from + end + 1) as u64)
    }
}

#[cfg(test)]
pub fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("solyra-agent-{tag}-{}-{}", std::process::id(), now()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_clean() {
        assert_eq!(clean_id("../a b"), "---a-b");
        assert_eq!(clean_id(""), "task");
        assert_eq!(clean_id(&"x".repeat(100)).len(), 64);
    }

    #[test]
    fn emit_creates_the_runtime_dir_and_stamps_ts() {
        let root = scratch("emit");
        let e = Emitter::new(root.join("rt"), "t1");
        e.emit(json!({"type":"started","name":"x"})).unwrap();
        let text = fs::read_to_string(root.join("rt/tasks/t1.jsonl")).unwrap();
        let v: Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(v["type"], "started");
        assert!(v["ts"].as_u64().unwrap() > 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn answers_are_read_back_latest_wins() {
        let root = scratch("ans");
        let e = Emitter::new(root.clone(), "t1");
        fs::create_dir_all(root.join("decisions")).unwrap();
        fs::write(
            root.join("decisions/t1.jsonl"),
            "{\"request_id\":\"r1\",\"decision\":\"deny\"}\n{\"request_id\":\"r2\",\"decision\":\"approve\",\"scope\":\"task\"}\n{\"request_id\":\"r1\",\"decision\":\"approve\"}\n{\"request_id\":\"q\",\"decision\":\"answer\",\"text\":\"main\"}\n",
        )
        .unwrap();
        assert_eq!(e.read_answer("r1"), Some(Answer::Approve { scope_task: false }));
        assert_eq!(e.read_answer("r2"), Some(Answer::Approve { scope_task: true }));
        assert_eq!(e.read_answer("q"), Some(Answer::Text("main".into())));
        assert_eq!(e.read_answer("zzz"), None);
        assert_eq!(e.wait_answer("zzz", Duration::from_millis(150)), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn messages_are_read_incrementally_and_partial_lines_wait() {
        let root = scratch("msg");
        let e = Emitter::new(root.clone(), "t1");
        fs::create_dir_all(root.join("messages")).unwrap();
        let p = root.join("messages/t1.jsonl");
        fs::write(&p, "{\"text\":\"one\"}\n{\"text\":\"tw").unwrap();
        let (m, off) = e.read_messages(0);
        assert_eq!(m, vec!["one"]);
        let (m2, off2) = e.read_messages(off);
        assert!(m2.is_empty());
        assert_eq!(off, off2);
        fs::write(&p, "{\"text\":\"one\"}\n{\"text\":\"two\"}\n").unwrap();
        let (m3, _) = e.read_messages(off);
        assert_eq!(m3, vec!["two"]);
        let _ = fs::remove_dir_all(&root);
    }
}
