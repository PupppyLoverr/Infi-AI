//! ADE side panel: the agent/harness surface inside the terminal, like the
//! panel in claude/codex IDE integrations. Reads the Solyra agent protocol
//! ($XDG_RUNTIME_DIR/solyra-agent/tasks/*.jsonl) and writes decisions back
//! (decisions/<id>.jsonl) — the same contract the panel shell implements.

use std::fs;
use std::path::PathBuf;

/// One agent task reduced to what the panel shows.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRow {
    pub id: String,
    pub name: String,
    pub status: String,
    pub last_text: String,
    /// approval_request awaiting a decision: (action, resource).
    pub pending: Option<(String, String)>,
}

fn root_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(PathBuf::from(base).join("solyra-agent"))
}

/// Extract a json string field without a parser: the events are written one
/// per line by runtimes we control, always flat and never with escaped quotes
/// in practice; fall back to empty when absent.
fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\":\"");
    let start = line.find(&pat)? + pat.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// Scan the runtime directory. Empty when no runtime is connected.
pub fn scan() -> Vec<TaskRow> {
    let Some(root) = root_dir() else { return Vec::new() };
    let tasks = root.join("tasks");
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&tasks) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let mut row = TaskRow {
            id: id.clone(),
            name: id.clone(),
            status: "working".into(),
            last_text: String::new(),
            pending: None,
        };
        for line in text.lines().rev().take(40) {
            if line.contains("\"finished\"") || line.contains("\"done\"") {
                row.status = "finished".into();
            } else if line.contains("\"failed\"") {
                row.status = "failed".into();
            }
            if row.name == id {
                if let Some(n) = field(line, "name").or_else(|| field(line, "agent")) {
                    row.name = n.to_string();
                }
            }
            if row.last_text.is_empty() {
                if let Some(t) = field(line, "text").or_else(|| field(line, "title")) {
                    row.last_text = t.to_string();
                }
            }
        }
        // A pending approval is an approval_request with no later resolution
        // event and no decision line yet.
        let has_req = text.contains("approval_request");
        let resolved = text.contains("approval_resolved") || text.contains("question_resolved");
        if has_req && !resolved {
            let decided = root
                .join("decisions")
                .join(format!("{id}.jsonl"))
                .exists();
            if !decided {
                let mut action = String::new();
                let mut resource = String::new();
                for line in text.lines() {
                    if line.contains("approval_request") {
                        action = field(line, "action").unwrap_or("").to_string();
                        resource = field(line, "resource").unwrap_or("").to_string();
                    }
                }
                row.pending = Some((action, resource));
                row.status = "waiting".into();
            }
        }
        out.push(row);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Write a decision for a task's pending approval. Returns false when the
/// directory is missing (no runtime).
pub fn decide(id: &str, decision: &str) -> bool {
    let Some(root) = root_dir() else { return false };
    let dir = root.join("decisions");
    if !dir.is_dir() {
        return false;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!("{{\"ts\":{ts},\"decision\":\"{decision}\"}}\n");
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{id}.jsonl")))
        .and_then(|mut f| use_write(&mut f, line.as_bytes()))
        .is_ok()
}

fn use_write(f: &mut fs::File, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    f.write_all(bytes)
}
