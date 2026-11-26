//! Terminal backend: real PTYs for shell panes and agent TUIs, and the
//! agent harnesses' own structured streams for the chat view.
//!
//! Chat mode never scrapes a TUI: it runs the harness headless with its
//! documented JSON stream (`claude -p --output-format stream-json`,
//! `codex exec --json`) and resumes the same session id, so switching to the
//! TUI view (`claude --resume <id>`) continues the same conversation.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

struct Pty {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
}

#[derive(Default, Clone)]
pub struct Ptys(Arc<Mutex<HashMap<u32, Pty>>>);

#[derive(Serialize, Clone)]
struct PtyOutput {
    id: u32,
    data: String,
}

#[derive(Serialize, Clone)]
struct PtyExit {
    id: u32,
}

/// Split `buf` into the longest valid UTF-8 prefix and the bytes of an
/// incomplete trailing character, which wait for the next read.
fn take_utf8(buf: &mut Vec<u8>) -> String {
    match std::str::from_utf8(buf) {
        Ok(s) => {
            let out = s.to_string();
            buf.clear();
            out
        }
        Err(e) => {
            let valid = e.valid_up_to();
            if e.error_len().is_some() {
                // Truly invalid bytes: replace them rather than wait forever.
                let out = String::from_utf8_lossy(buf).into_owned();
                buf.clear();
                return out;
            }
            let out = String::from_utf8_lossy(&buf[..valid]).into_owned();
            buf.drain(..valid);
            out
        }
    }
}

impl Ptys {
    pub fn spawn(&self, app: AppHandle, id: u32, cmd: Option<String>, cwd: Option<String>, cols: u16, rows: u16) -> Result<(), String> {
        let pair = native_pty_system()
            .openpty(PtySize { rows: rows.max(2), cols: cols.max(10), pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let mut builder = match cmd.as_deref().filter(|c| !c.trim().is_empty()) {
            Some(c) => {
                let mut b = CommandBuilder::new("sh");
                b.args(["-c", c]);
                b
            }
            None => CommandBuilder::new(shell),
        };
        builder.env("TERM", "xterm-256color");
        builder.env("COLORTERM", "truecolor");
        let dir = cwd
            .map(|c| crate::sys::expand(&c))
            .unwrap_or_else(|| crate::sys::expand("~"));
        builder.cwd(dir);
        let mut child = pair.slave.spawn_command(builder).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        self.0.lock().unwrap().insert(id, Pty { writer, master: pair.master });
        let map = self.0.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut pending = Vec::new();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        pending.extend_from_slice(&buf[..n]);
                        let data = take_utf8(&mut pending);
                        if !data.is_empty() {
                            let _ = app.emit("pty-output", PtyOutput { id, data });
                        }
                    }
                }
            }
            let _ = child.wait();
            map.lock().unwrap().remove(&id);
            let _ = app.emit("pty-exit", PtyExit { id });
        });
        Ok(())
    }

    pub fn write(&self, id: u32, data: &str) {
        if let Some(p) = self.0.lock().unwrap().get_mut(&id) {
            let _ = p.writer.write_all(data.as_bytes());
            let _ = p.writer.flush();
        }
    }

    pub fn resize(&self, id: u32, cols: u16, rows: u16) {
        if let Some(p) = self.0.lock().unwrap().get(&id) {
            let _ = p.master.resize(PtySize { rows: rows.max(2), cols: cols.max(10), pixel_width: 0, pixel_height: 0 });
        }
    }

    pub fn kill(&self, id: u32) {
        // Dropping the master closes the PTY; the child gets SIGHUP.
        self.0.lock().unwrap().remove(&id);
    }
}

/// Agent CLIs installed on this machine.
pub fn harnesses() -> Vec<&'static str> {
    ["claude", "codex", "gemini", "opencode"]
        .into_iter()
        .filter(|b| crate::sys::on_path(b))
        .collect()
}

#[derive(Serialize, Clone)]
struct AgentLine {
    key: String,
    line: String,
}

#[derive(Serialize, Clone)]
struct AgentDone {
    key: String,
    code: i32,
}

/// Options for one chat turn.
#[derive(Default, serde::Deserialize, Clone)]
pub struct AgentOpts {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub mode: Option<String>,
    /// The conversation id. On the first turn it is created with
    /// `--session-id`, later turns `--resume` it, and the TUI view opens it
    /// with `claude --resume <id>`.
    pub session: Option<String>,
    #[serde(default)]
    pub fresh: bool,
    pub cwd: Option<String>,
}

fn some(v: &Option<String>) -> Option<&str> {
    v.as_deref().filter(|s| !s.is_empty())
}

/// The headless command line for one chat turn.
pub fn agent_command(harness: &str, prompt: &str, o: &AgentOpts) -> Option<Vec<String>> {
    let mut v: Vec<String> = match harness {
        "claude" => {
            let mut v: Vec<String> = ["claude", "-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages"]
                .map(String::from)
                .to_vec();
            if let Some(m) = some(&o.model) {
                v.extend(["--model".into(), m.into()]);
            }
            if let Some(e) = some(&o.effort) {
                v.extend(["--effort".into(), e.into()]);
            }
            if let Some(m) = some(&o.mode) {
                v.extend(["--permission-mode".into(), m.into()]);
            }
            if let Some(id) = some(&o.session) {
                v.extend([if o.fresh { "--session-id" } else { "--resume" }.into(), id.into()]);
            }
            v
        }
        "codex" => {
            let mut v: Vec<String> = vec!["codex".into(), "exec".into()];
            if let (Some(id), false) = (some(&o.session), o.fresh) {
                v.extend(["resume".into(), id.into()]);
            }
            v.push("--json".into());
            if let Some(m) = some(&o.model) {
                v.extend(["-m".into(), m.into()]);
            }
            v
        }
        _ => return None,
    };
    v.push(prompt.into());
    Some(v)
}

/// Running chat turns by key, so the composer's stop button can end one.
#[derive(Default, Clone)]
pub struct Agents(Arc<Mutex<HashMap<String, u32>>>);

impl Agents {
    pub fn stop(&self, key: &str) {
        if let Some(pid) = self.0.lock().unwrap().remove(key) {
            let _ = std::process::Command::new("kill").args(["-INT", &pid.to_string()]).status();
        }
    }
}

pub fn agent_send(app: AppHandle, agents: &Agents, key: String, harness: String, prompt: String, o: AgentOpts) -> Result<(), String> {
    let argv = agent_command(&harness, &prompt, &o).ok_or_else(|| format!("{harness} has no chat mode"))?;
    if !crate::sys::on_path(&argv[0]) {
        return Err(format!("{} is not installed", argv[0]));
    }
    let mut child = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(o.cwd.as_deref().map(crate::sys::expand).unwrap_or_else(|| crate::sys::expand("~")))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    agents.0.lock().unwrap().insert(key.clone(), child.id());
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (app2, key2) = (app.clone(), key.clone());
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = app2.emit("agent-line", AgentLine { key: key2.clone(), line: format!("{{\"type\":\"stderr\",\"text\":{}}}", serde_json::to_string(&line).unwrap_or_default()) });
        }
    });
    let map = agents.0.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = app.emit("agent-line", AgentLine { key: key.clone(), line });
        }
        let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
        map.lock().unwrap().remove(&key);
        let _ = app.emit("agent-done", AgentDone { key, code });
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_is_reassembled_across_reads() {
        let mut buf = "héllo".as_bytes()[..2].to_vec(); // 'h' + first byte of 'é'
        assert_eq!(take_utf8(&mut buf), "h");
        buf.extend_from_slice(&"héllo".as_bytes()[2..]);
        assert_eq!(take_utf8(&mut buf), "éllo");
        assert!(buf.is_empty());
    }

    #[test]
    fn chat_turns_use_the_harness_json_streams() {
        let mut o = AgentOpts { model: Some("haiku".into()), session: Some("abc".into()), fresh: true, mode: Some("auto".into()), ..Default::default() };
        let c = agent_command("claude", "hi", &o).unwrap();
        assert_eq!(c[..5], ["claude", "-p", "--output-format", "stream-json", "--verbose"]);
        assert!(c.windows(2).any(|w| w == ["--session-id", "abc"]));
        assert!(c.windows(2).any(|w| w == ["--permission-mode", "auto"]));
        assert_eq!(c.last().unwrap(), "hi");
        o.fresh = false;
        assert!(agent_command("claude", "hi", &o).unwrap().windows(2).any(|w| w == ["--resume", "abc"]));
        let x = agent_command("codex", "hi", &AgentOpts::default()).unwrap();
        assert_eq!(x[..3], ["codex", "exec", "--json"]);
        assert!(agent_command("codex", "hi", &o).unwrap().windows(2).any(|w| w == ["resume", "abc"]));
        assert!(agent_command("unknown", "hi", &o).is_none());
    }
}
