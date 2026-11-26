//! Chat mode for a pane: Claude Code over its stream-json stdio protocol
//! (the recipe verified in zeron's harness: --print --input-format
//! stream-json --output-format stream-json --verbose). User text goes in as
//! user frames; assistant text frames render as bubbles. This is the real
//! harness, not a mock: with no `claude` binary the pane says so.
//!
//! zgx signals are UI-thread-only, so the reader thread accumulates the
//! conversation and ships the whole vector through `zgx::task::latest` —
//! latest-wins is exactly right for cumulative state — and a UI task applies
//! it to the signals.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use zgx::reactive::{Runtime, Signal};

#[derive(Clone, Debug, PartialEq)]
pub struct Bubble {
    pub role: &'static str,
    pub text: String,
}

/// (conversation, phase, session id, cost in USD, model reported).
type Update = (Vec<Bubble>, String, String, f64, String);

/// How a chat session runs: the composer chips (permission mode, model,
/// effort) and the session it resumes. Changing a chip respawns the harness
/// on the same session id, so the conversation carries on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Opts {
    pub mode: String,
    pub model: String,
    pub effort: String,
    pub resume: Option<String>,
}

pub const MODES: [(&str, &str); 4] = [("auto", "Auto"), ("acceptEdits", "Accept edits"), ("plan", "Plan"), ("default", "Ask")];
pub const MODELS: [(&str, &str); 4] = [("", "Default"), ("opus", "Opus"), ("sonnet", "Sonnet"), ("haiku", "Haiku")];
pub const EFFORTS: [(&str, &str); 4] = [("low", "Low"), ("medium", "Medium"), ("high", "High"), ("max", "Max")];

/// The next value in a chip's cycle.
pub fn cycle(list: &[(&str, &str)], cur: &str) -> String {
    let i = list.iter().position(|(k, _)| *k == cur).map_or(0, |i| (i + 1) % list.len());
    list[i].0.to_string()
}

/// A chip's label.
pub fn label(list: &[(&str, &str)], cur: &str) -> String {
    list.iter().find(|(k, _)| *k == cur).map_or(cur, |(_, l)| *l).to_string()
}

/// "claude-opus-5-5" -> "Opus 5.5"; empty -> "".
pub fn pretty_model(m: &str) -> String {
    for fam in ["opus", "sonnet", "haiku", "fable"] {
        if let Some(i) = m.find(fam) {
            let rest: Vec<&str> = m[i + fam.len()..].trim_start_matches('-').split('-').filter(|p| p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() && p.len() < 3).collect();
            let mut name = format!("{}{}", fam[..1].to_uppercase(), &fam[1..]);
            if !rest.is_empty() {
                name.push(' ');
                name.push_str(&rest.join("."));
            }
            return name;
        }
    }
    m.to_string()
}

impl Drop for Chat {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

pub struct Chat {
    stdin: Arc<Mutex<ChildStdin>>,
    #[allow(dead_code)]
    child: Arc<Mutex<Child>>,
    convo: Arc<Mutex<Vec<Bubble>>>,
    pub bubbles: Signal<Vec<Bubble>>,
    pub input: Signal<String>,
    pub status: Signal<String>,
    /// Bumped on every applied update so the view re-renders.
    pub rev: Signal<u32>,
    /// The harness's session id (fixed from the first turn).
    pub session: Signal<String>,
    /// Session cost in USD, from the harness's result frames.
    pub cost: Signal<f64>,
    /// The model the harness reported at start.
    pub model: Signal<String>,
    /// When the current "Writing" phase began (for "Writing… Ns").
    pub since: Rc<Cell<std::time::Instant>>,
    pub opts: Opts,
}

/// A random v4 UUID from the kernel's RNG.
fn new_uuid() -> String {
    let mut b = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut b);
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

impl Chat {
    /// Conversation so far (to seed a respawn).
    pub fn history(&self) -> Vec<Bubble> {
        self.convo.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// Interrupt the current turn (the composer's stop button / Esc).
    pub fn stop(&self) {
        if let Ok(c) = self.child.lock() {
            let _ = std::process::Command::new("kill").args(["-INT", &c.id().to_string()]).status();
        }
        self.status.set(String::new());
    }
}

fn claude_bin() -> Option<String> {
    for cand in ["claude", "claude-code"] {
        if let Ok(out) = Command::new("which").arg(cand).output() {
            if out.status.success() {
                return Some(cand.into());
            }
        }
    }
    None
}

impl Chat {
    /// Spawn the harness in `cwd`. None when claude is not installed.
    pub fn spawn(
        rt: &Runtime,
        tasks: &zgx_desktop::TaskSpawner,
        cwd: Option<std::path::PathBuf>,
        opts: Opts,
        seed: Vec<Bubble>,
    ) -> Option<Self> {
        let exe = claude_bin()?;
        let mut cmd = Command::new(exe);
        cmd.args([
            "--print",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
        ]);
        for (flag, v) in [("--permission-mode", &opts.mode), ("--model", &opts.model), ("--effort", &opts.effort)] {
            if !v.is_empty() {
                cmd.args([flag, v.as_str()]);
            }
        }
        // A fixed id from the first turn, so the TUI (`claude --resume`)
        // opens this very conversation.
        let session_id = opts.resume.clone().unwrap_or_else(new_uuid);
        if opts.resume.is_some() {
            cmd.args(["--resume", &session_id]);
        } else {
            cmd.args(["--session-id", &session_id]);
        }
        cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let mut child = cmd.spawn().ok()?;
        let stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;

        let bubbles = rt.signal(seed.clone());
        let status = rt.signal(String::new());
        let session = rt.signal(session_id.clone());
        let cost = rt.signal(0.0f64);
        let model = rt.signal(String::new());
        let rev = rt.signal(0);
        let input = rt.signal(String::new());

        let since = Rc::new(Cell::new(std::time::Instant::now()));
        let (tx, mut rx) = zgx::task::latest::<Update>();
        // The conversation is shared: the reader appends assistant frames,
        // send() appends the user's own messages, and every shipped snapshot
        // carries both. A reader-owned copy would drop user bubbles.
        let convo_shared = Arc::new(Mutex::new(seed));
        let convo_reader = convo_shared.clone();
        std::thread::spawn(move || {
            let convo = convo_reader;
            let mut state = String::new();
            let (mut sid, mut usd, mut mdl) = (session_id, 0.0f64, String::new());
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match kind {
                    "system" => {
                        if let Some(id) = v.get("session_id").and_then(|x| x.as_str()) {
                            sid = id.to_string();
                        }
                        if let Some(m) = v.get("model").and_then(|x| x.as_str()) {
                            mdl = m.to_string();
                        }
                    }
                    "assistant" => {
                        // Complete assistant frames only; partials are off.
                        if let Some(content) = v
                            .get("message")
                            .and_then(|m| m.get("content"))
                            .and_then(|c| c.as_array())
                        {
                            let mut text = String::new();
                            for block in content {
                                match block.get("type").and_then(|t| t.as_str()) {
                                    Some("text") => {
                                        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                                            text.push_str(t);
                                        }
                                    }
                                    // Tool calls show as one-line rows (name + target).
                                    Some("tool_use") => {
                                        let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("Tool");
                                        let input = block.get("input");
                                        let detail = ["command", "file_path", "path", "pattern", "url", "description"]
                                            .iter()
                                            .find_map(|k| input.and_then(|i| i.get(*k)).and_then(|v| v.as_str()))
                                            .unwrap_or("")
                                            .chars()
                                            .take(120)
                                            .collect::<String>();
                                        if let Ok(mut c) = convo.lock() {
                                            c.push(Bubble { role: "tool", text: format!("{name}\u{1f}{detail}") });
                                        }
                                        state = "Working".into();
                                    }
                                    _ => {}
                                }
                            }
                            state = "Writing".into();
                            if !text.trim().is_empty()
                                && let Ok(mut c) = convo.lock()
                            {
                                c.push(Bubble {
                                    role: "assistant",
                                    text,
                                });
                            }
                        }
                    }
"result" => {
                        state = String::new();
                        if let Some(c) = v.get("total_cost_usd").and_then(|c| c.as_f64()) {
                            usd = c;
                        }
                        if v.get("is_error").and_then(|e| e.as_bool()) == Some(true)
                            && let Some(r) = v.get("result").and_then(|r| r.as_str())
                            && let Ok(mut c) = convo.lock()
                        {
                            c.push(Bubble { role: "error", text: r.to_string() });
                        }
                        // A desktop notification: the shell's notification
                        // centre banners it even when the terminal is behind.
                        let _ = std::process::Command::new("notify-send")
                            .args(["solyra-term", "agent turn finished"])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();
                    }
                    _ => {}
                }
                let snapshot = convo.lock().map(|c| c.clone()).unwrap_or_default();
                if !tx.send((snapshot, state.clone(), sid.clone(), usd, mdl.clone())) {
                    break;
                }
            }
            let snapshot = convo.lock().map(|c| c.clone()).unwrap_or_default();
            tx.send((snapshot, "Session ended".into(), sid, usd, mdl));
        });

        {
            let (bubbles, status, rev) = (bubbles.clone(), status.clone(), rev.clone());
            let (session, cost, model, since) = (session.clone(), cost.clone(), model.clone(), since.clone());
            tasks.spawn(async move {
                while let Some((convo, state, sid, usd, mdl)) = rx.recv().await {
                    bubbles.set(convo);
                    if state == "Writing" && status.get() != "Writing" {
                        since.set(std::time::Instant::now());
                    }
                    status.set(state);
                    session.set(sid);
                    cost.set(usd);
                    if !mdl.is_empty() {
                        model.set(mdl);
                    }
                    rev.update(|n| *n += 1);
                }
            });
        }

        Some(Self {
            stdin: Arc::new(Mutex::new(stdin)),
            child: Arc::new(Mutex::new(child)),
            convo: convo_shared,
            bubbles,
            input,
            status,
            rev,
            session,
            cost,
            model,
            since,
            opts,
        })
    }

    /// Send the input box as a user message. Returns false when empty.
    pub fn send(&self) -> bool {
        let text = self.input.get().trim().to_string();
        if text.is_empty() {
            return false;
        }
        let frame = serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": [{"type": "text", "text": text}]}
        });
        let ok = self
            .stdin
            .lock()
            .ok()
            .is_some_and(|mut w| writeln!(w, "{frame}").and_then(|_| w.flush()).is_ok());
        if ok {
            let bubble = Bubble {
                role: "user",
                text,
            };
            if let Ok(mut c) = self.convo.lock() {
                c.push(bubble.clone());
            }
            self.bubbles.update(|b| b.push(bubble));
            self.input.set(String::new());
            self.status.set("Thinking".into());
            self.rev.update(|n| *n += 1);
        }
        ok
    }
}
