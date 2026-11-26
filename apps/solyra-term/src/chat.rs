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
use std::sync::{Arc, Mutex};

use zgx::reactive::{Runtime, Signal};

#[derive(Clone, Debug, PartialEq)]
pub struct Bubble {
    pub role: &'static str,
    pub text: String,
}

type Update = (Vec<Bubble>, String);

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
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let mut child = cmd.spawn().ok()?;
        let stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;

        let bubbles = rt.signal(Vec::new());
        let status = rt.signal("connecting".to_string());
        let rev = rt.signal(0);
        let input = rt.signal(String::new());

        let (tx, mut rx) = zgx::task::latest::<Update>();
        // The conversation is shared: the reader appends assistant frames,
        // send() appends the user's own messages, and every shipped snapshot
        // carries both. A reader-owned copy would drop user bubbles.
        let convo_shared = Arc::new(Mutex::new(Vec::<Bubble>::new()));
        let convo_reader = convo_shared.clone();
        std::thread::spawn(move || {
            let convo = convo_reader;
            let mut state = "connecting".to_string();
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match kind {
                    "system" => state = "ready".into(),
                    "assistant" => {
                        // Complete assistant frames only; partials are off.
                        if let Some(content) = v
                            .get("message")
                            .and_then(|m| m.get("content"))
                            .and_then(|c| c.as_array())
                        {
                            let mut text = String::new();
                            for block in content {
                                if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                                    if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                                        text.push_str(t);
                                    }
                                }
                            }
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
                        state = "done — send another message".into();
                        if let Some(cost) = v.get("total_cost_usd").and_then(|c| c.as_f64()) {
                            state = format!("done — ${cost:.4} this session");
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
                if !tx.send((snapshot, state.clone())) {
                    break;
                }
            }
            let snapshot = convo.lock().map(|c| c.clone()).unwrap_or_default();
            tx.send((snapshot, "session ended".into()));
        });

        {
            let (bubbles, status, rev) = (bubbles.clone(), status.clone(), rev.clone());
            tasks.spawn(async move {
                while let Some((convo, state)) = rx.recv().await {
                    bubbles.set(convo);
                    status.set(state);
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
            self.status.set("working".into());
            self.rev.update(|n| *n += 1);
        }
        ok
    }
}
