//! Translating other agents' hook calls into panel events.
//!
//! Claude Code runs a command for hook events and sends JSON on stdin. For
//! `PermissionRequest` the command may answer with a decision; this is how an
//! approval can be given from the notch. Nothing here ever approves on its
//! own: with no panel, no answer, or a timeout, the hook prints nothing and
//! Claude falls back to its normal prompt in the terminal.
//!
//! The JSON contracts below follow Claude Code's documented hooks and Codex's
//! `notify` hook. They are not verified against a live install of either CLI
//! in CI, and they can change when those tools change.

use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

use crate::events::{clean_id, now, Answer, Emitter};

const MAX_TEXT: usize = 300;

fn clip(s: &str) -> String {
    let flat: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let flat = flat.trim();
    if flat.chars().count() <= MAX_TEXT {
        flat.to_string()
    } else {
        let mut o: String = flat.chars().take(MAX_TEXT - 1).collect();
        o.push('…');
        o
    }
}

/// The task a hook belongs to: the wrapper's (`SOLYRA_AGENT_ID`) when the CLI
/// runs inside `solyra-agent wrap`, else derived from the CLI's session id.
pub fn task_id(prefix: &str, session: &str, wrapped_id: Option<&str>) -> String {
    match wrapped_id.filter(|s| !s.is_empty()) {
        Some(id) => clean_id(id),
        None => clean_id(&format!("{prefix}-{}", session.chars().take(12).collect::<String>())),
    }
}

/// One line describing what a tool call does, and what it touches.
pub fn describe_tool(tool: &str, input: &Value) -> (String, Option<String>) {
    let s = |k: &str| input.get(k).and_then(Value::as_str).map(clip);
    match tool {
        "Bash" => (
            format!("Run a shell command"),
            s("command"),
        ),
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => (
            format!("{} a file", if tool == "Write" { "Write" } else { "Edit" }),
            s("file_path").or_else(|| s("notebook_path")),
        ),
        "Read" => ("Read a file".into(), s("file_path")),
        "WebFetch" => ("Fetch a web page".into(), s("url")),
        "WebSearch" => ("Search the web".into(), s("query")),
        other => (format!("Use the {} tool", clip(other)), None),
    }
}

/// Whether a tool only reads (so it is known to be reversible), else unknown.
pub fn reversible(tool: &str) -> Option<bool> {
    matches!(tool, "Read" | "Glob" | "Grep" | "WebSearch" | "WebFetch").then_some(true)
}

pub struct ClaudeCtx<'a> {
    pub root: &'a Path,
    pub wrapped_id: Option<&'a str>,
    pub wrapped: bool,
    /// How long to wait for the human before falling back to Claude's own prompt.
    pub wait: Duration,
}

/// Handle one Claude Code hook call. Returns the JSON to print, if any.
pub fn claude(input: &Value, ctx: &ClaudeCtx<'_>) -> Option<Value> {
    let event = input.get("hook_event_name")?.as_str()?;
    let session = input.get("session_id").and_then(Value::as_str).unwrap_or("session");
    let e = Emitter::new(ctx.root.to_path_buf(), &task_id("claude", session, ctx.wrapped_id));

    if !e.exists() {
        let cwd = input.get("cwd").and_then(Value::as_str).unwrap_or("");
        let name = Path::new(cwd).file_name().and_then(|n| n.to_str()).unwrap_or("Claude Code");
        e.emit(json!({
            "type": "started", "name": clip(name), "agent": "claude",
            "interactive": ctx.wrapped,
            "environment": null,
        }))
        .ok()?;
    }

    match event {
        "SessionStart" | "UserPromptSubmit" => {
            e.emit(json!({"type": "status", "state": "working"})).ok()?;
            None
        }
        "PreToolUse" => {
            let tool = input.get("tool_name").and_then(Value::as_str).unwrap_or("tool");
            let (what, res) = describe_tool(tool, input.get("tool_input").unwrap_or(&Value::Null));
            let text = match res {
                Some(r) => format!("{what}: {r}"),
                None => what,
            };
            e.emit(json!({"type": "milestone", "text": text})).ok()?;
            None
        }
        "Notification" => {
            // Permission prompts are handled by PermissionRequest; the other
            // notification is "waiting for your input".
            let msg = input.get("message").and_then(Value::as_str).unwrap_or("");
            if !msg.to_lowercase().contains("permission") {
                e.emit(json!({"type": "status", "state": "idle"})).ok()?;
            }
            None
        }
        "Stop" | "SubagentStop" => {
            e.emit(json!({"type": "status", "state": "idle"})).ok()?;
            None
        }
        "SessionEnd" => {
            e.emit(json!({"type": "finished", "summary": "Session ended"})).ok()?;
            None
        }
        "PermissionRequest" => permission(input, &e, ctx),
        _ => None,
    }
}

fn permission(input: &Value, e: &Emitter, ctx: &ClaudeCtx<'_>) -> Option<Value> {
    let tool = input.get("tool_name").and_then(Value::as_str).unwrap_or("tool");
    let tool_input = input.get("tool_input").unwrap_or(&Value::Null);
    let (action, resource) = describe_tool(tool, tool_input);

    // "Approve for this task" earlier: allow the same tool again, visibly.
    if granted(e, tool) {
        e.emit(json!({"type":"milestone","text": format!("Allowed (approved for this task): {action}")})).ok()?;
        return Some(allow());
    }

    // Unique per request: a reused id would pick up an older answer.
    let request_id = unique_request_id();
    let mut ev = json!({
        "type": "approval_request", "request_id": request_id,
        "action": action, "scopes": ["once", "task"],
    });
    if let Some(r) = resource {
        ev["resource"] = json!(r);
    }
    if let Some(rev) = reversible(tool) {
        ev["reversible"] = json!(rev);
    }
    e.emit(ev).ok()?;

    let resolved = |decision: &str| {
        let _ = e.emit(json!({"type":"approval_resolved","request_id": request_id,"decision": decision}));
    };
    match e.wait_answer(&request_id, ctx.wait) {
        Some(Answer::Approve { scope_task }) => {
            if scope_task {
                grant(e, tool);
            }
            resolved("approve");
            Some(allow())
        }
        Some(Answer::Deny) => {
            resolved("deny");
            Some(deny("Denied from the Solyra panel"))
        }
        Some(Answer::Text(_)) | None => {
            // No decision in time: leave it to Claude's own prompt.
            resolved("timeout");
            None
        }
    }
}

fn unique_request_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed))
}

fn allow() -> Value {
    json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "allow"}}})
}

fn deny(message: &str) -> Value {
    json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest",
        "decision": {"behavior": "deny", "message": message}}})
}

fn grants_path(e: &Emitter) -> std::path::PathBuf {
    e.root().join("grants").join(format!("{}.txt", e.id))
}

fn granted(e: &Emitter, tool: &str) -> bool {
    std::fs::read_to_string(grants_path(e)).is_ok_and(|t| t.lines().any(|l| l == tool))
}

fn grant(e: &Emitter, tool: &str) {
    let p = grants_path(e);
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        use std::io::Write;
        let _ = writeln!(f, "{tool}");
    }
}

/// Codex's `notify` hook passes one JSON argument. Status only: Codex offers
/// no approval channel through it.
pub fn codex(arg: &str, root: &Path, wrapped_id: Option<&str>) -> Option<()> {
    let v: Value = serde_json::from_str(arg).ok()?;
    let turn = v.get("thread-id").or_else(|| v.get("turn-id")).and_then(Value::as_str).unwrap_or("session");
    let e = Emitter::new(root.to_path_buf(), &task_id("codex", turn, wrapped_id));
    if !e.exists() {
        let cwd = v.get("cwd").and_then(Value::as_str).unwrap_or("");
        let name = Path::new(cwd).file_name().and_then(|n| n.to_str()).unwrap_or("Codex");
        e.emit(json!({"type":"started","name":clip(name),"agent":"codex","interactive": wrapped_id.is_some()})).ok()?;
    }
    if v.get("type").and_then(Value::as_str) == Some("agent-turn-complete") {
        if let Some(msg) = v.get("last-assistant-message").and_then(Value::as_str) {
            e.emit(json!({"type":"milestone","text": format!("Turn finished: {}", clip(msg))})).ok()?;
        }
        e.emit(json!({"type":"status","state":"idle"})).ok()?;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::scratch;
    use std::fs;

    fn ctx<'a>(root: &'a Path) -> ClaudeCtx<'a> {
        ClaudeCtx { root, wrapped_id: None, wrapped: false, wait: Duration::from_millis(300) }
    }

    fn input(event: &str, extra: Value) -> Value {
        let mut v = json!({"hook_event_name": event, "session_id": "abcdef123456789", "cwd": "/home/u/project"});
        for (k, val) in extra.as_object().unwrap() {
            v[k] = val.clone();
        }
        v
    }

    fn tasks(root: &Path, id: &str) -> Vec<Value> {
        fs::read_to_string(root.join(format!("tasks/{id}.jsonl")))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn first_hook_starts_the_task_with_its_project_name() {
        let root = scratch("h1");
        assert!(claude(&input("UserPromptSubmit", json!({})), &ctx(&root)).is_none());
        let ev = tasks(&root, "claude-abcdef123456");
        assert_eq!(ev[0]["type"], "started");
        assert_eq!(ev[0]["name"], "project");
        assert_eq!(ev[0]["agent"], "claude");
        assert_eq!(ev[0]["interactive"], false);
        assert_eq!(ev[1]["state"], "working");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn permission_with_no_answer_never_approves() {
        let root = scratch("h2");
        let inp = input("PermissionRequest", json!({"tool_name":"Bash","tool_input":{"command":"rm -rf build"}}));
        assert!(claude(&inp, &ctx(&root)).is_none(), "timeout must fall back to Claude's own prompt");
        let ev = tasks(&root, "claude-abcdef123456");
        let req = ev.iter().find(|e| e["type"] == "approval_request").unwrap();
        assert_eq!(req["action"], "Run a shell command");
        assert_eq!(req["resource"], "rm -rf build");
        assert!(req.get("reversible").is_none(), "reversibility is unknown, not assumed");
        assert!(ev.iter().any(|e| e["type"] == "approval_resolved" && e["decision"] == "timeout"));
        let _ = fs::remove_dir_all(&root);
    }

    fn answer_in_background(root: &Path, id: &str, decision: &'static str, scope: &'static str) {
        let (root, id) = (root.to_path_buf(), id.to_string());
        std::thread::spawn(move || {
            for _ in 0..50 {
                std::thread::sleep(Duration::from_millis(40));
                let Ok(text) = fs::read_to_string(root.join(format!("tasks/{id}.jsonl"))) else { continue };
                let Some(req) = text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find(|e| e["type"] == "approval_request") else { continue };
                fs::create_dir_all(root.join("decisions")).unwrap();
                let line = json!({"request_id": req["request_id"], "decision": decision, "scope": scope});
                fs::write(root.join(format!("decisions/{id}.jsonl")), format!("{line}\n")).unwrap();
                return;
            }
        });
    }

    #[test]
    fn approve_and_deny_map_to_claude_decisions() {
        for (decision, behavior) in [("approve", "allow"), ("deny", "deny")] {
            let root = scratch(&format!("h3{decision}"));
            let mut c = ctx(&root);
            c.wait = Duration::from_secs(3);
            answer_in_background(&root, "claude-abcdef123456", decision, "once");
            let inp = input("PermissionRequest", json!({"tool_name":"Edit","tool_input":{"file_path":"/p/a.rs"}}));
            let out = claude(&inp, &c).expect("a decision");
            assert_eq!(out["hookSpecificOutput"]["decision"]["behavior"], behavior);
            assert_eq!(out["hookSpecificOutput"]["hookEventName"], "PermissionRequest");
            let _ = fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn approve_for_task_allows_the_same_tool_next_time_visibly() {
        let root = scratch("h4");
        let mut c = ctx(&root);
        c.wait = Duration::from_secs(3);
        answer_in_background(&root, "claude-abcdef123456", "approve", "task");
        let inp = input("PermissionRequest", json!({"tool_name":"Bash","tool_input":{"command":"ls"}}));
        assert!(claude(&inp, &c).is_some());
        c.wait = Duration::from_millis(100);
        let again = claude(&inp, &c).expect("auto-allowed");
        assert_eq!(again["hookSpecificOutput"]["decision"]["behavior"], "allow");
        let ev = tasks(&root, "claude-abcdef123456");
        assert!(ev.iter().any(|e| e["type"] == "milestone" && e["text"].as_str().unwrap().starts_with("Allowed (approved for this task)")));
        // A different tool is not covered.
        let other = input("PermissionRequest", json!({"tool_name":"Write","tool_input":{"file_path":"/x"}}));
        assert!(claude(&other, &c).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn wrapped_hooks_join_the_wrappers_task() {
        let root = scratch("h5");
        let mut c = ctx(&root);
        c.wrapped_id = Some("claude-wrap-1");
        c.wrapped = true;
        claude(&input("Stop", json!({})), &c);
        let ev = tasks(&root, "claude-wrap-1");
        assert_eq!(ev[0]["interactive"], true);
        assert_eq!(ev.last().unwrap()["state"], "idle");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_events_and_garbage_are_ignored() {
        let root = scratch("h6");
        assert!(claude(&input("SomethingNew", json!({})), &ctx(&root)).is_none());
        assert!(claude(&json!({"no":"event"}), &ctx(&root)).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn codex_turn_complete_is_status_only() {
        let root = scratch("h7");
        codex(r#"{"type":"agent-turn-complete","thread-id":"t-1","cwd":"/w/app","last-assistant-message":"Done."}"#, &root, None).unwrap();
        let ev = tasks(&root, "codex-t-1");
        assert_eq!(ev[0]["agent"], "codex");
        assert!(ev.iter().any(|e| e["state"] == "idle"));
        assert!(codex("not json", &root, None).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn tool_descriptions_name_the_resource_and_flag_only_readers_as_reversible() {
        let (a, r) = describe_tool("Write", &json!({"file_path":"/a/b.txt"}));
        assert_eq!((a.as_str(), r.as_deref()), ("Write a file", Some("/a/b.txt")));
        assert_eq!(reversible("Read"), Some(true));
        assert_eq!(reversible("Bash"), None);
        assert_eq!(reversible("Write"), None);
    }
}
