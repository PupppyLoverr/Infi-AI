//! `solyra-agent install-hooks claude [--apply]`: add the hooks that report to
//! the Solyra panel to the user's Claude Code settings. Shows what it would
//! change first, backs the file up, and is idempotent.

use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::{json, Value};

const EVENTS: [&str; 7] = [
    "SessionStart", "UserPromptSubmit", "PreToolUse", "Notification", "Stop", "SessionEnd", "PermissionRequest",
];
const COMMAND: &str = "solyra-agent hook claude";

/// `settings` with a Solyra hook added to every event it is missing from.
/// Existing hooks are kept; running it twice changes nothing.
pub fn merge_claude_settings(settings: &Value) -> Value {
    let mut out = settings.clone();
    if !out.is_object() {
        out = json!({});
    }
    let hooks = out
        .as_object_mut()
        .expect("object")
        .entry("hooks")
        .or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    for ev in EVENTS {
        let arr = hooks
            .as_object_mut()
            .expect("object")
            .entry(ev)
            .or_insert_with(|| json!([]));
        let Some(list) = arr.as_array_mut() else { continue };
        let present = list.iter().any(|g| {
            g.get("hooks")
                .and_then(Value::as_array)
                .is_some_and(|hs| hs.iter().any(|h| h.get("command").and_then(Value::as_str) == Some(COMMAND)))
        });
        if !present {
            let timeout = if ev == "PermissionRequest" { 600 } else { 10 };
            list.push(json!({"matcher": "*", "hooks": [{"type": "command", "command": COMMAND, "timeout": timeout}]}));
        }
    }
    out
}

fn settings_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join(".claude/settings.json"))
}

pub fn cmd(args: &[String]) -> ExitCode {
    if args.first().map(String::as_str) != Some("claude") {
        eprintln!("solyra-agent: install-hooks supports: claude");
        return ExitCode::from(2);
    }
    let apply = args.iter().any(|a| a == "--apply");
    let Some(path) = settings_path() else {
        eprintln!("solyra-agent: HOME is not set");
        return ExitCode::from(2);
    };
    let existing: Value = match std::fs::read_to_string(&path) {
        Ok(t) => match serde_json::from_str(&t) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("solyra-agent: {} is not valid JSON, not touching it: {e}", path.display());
                return ExitCode::from(2);
            }
        },
        Err(_) => json!({}),
    };
    let merged = merge_claude_settings(&existing);
    if merged == existing {
        println!("Already installed in {}", path.display());
        return ExitCode::SUCCESS;
    }
    println!("{}", serde_json::to_string_pretty(&merged).unwrap_or_default());
    if !apply {
        println!("\nThis is what {} would contain. Run again with --apply to write it (a .solyra-bak backup is kept).", path.display());
        return ExitCode::SUCCESS;
    }
    if path.exists() {
        let _ = std::fs::copy(&path, path.with_extension("json.solyra-bak"));
    }
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    match std::fs::write(&path, serde_json::to_string_pretty(&merged).unwrap_or_default() + "\n") {
        Ok(()) => {
            println!("Hooks installed in {}.", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("solyra-agent: cannot write {}: {e}", path.display());
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_hooks_and_keeps_existing_settings() {
        let existing = json!({"model":"x","hooks":{"Stop":[{"hooks":[{"type":"command","command":"mine"}]}]}});
        let out = merge_claude_settings(&existing);
        assert_eq!(out["model"], "x");
        let stop = out["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "the user's own hook is kept");
        assert!(out["hooks"]["PermissionRequest"][0]["hooks"][0]["timeout"] == 600);
        for ev in EVENTS {
            assert!(out["hooks"][ev].as_array().is_some_and(|a| !a.is_empty()), "{ev}");
        }
    }

    #[test]
    fn is_idempotent() {
        let once = merge_claude_settings(&json!({}));
        assert_eq!(merge_claude_settings(&once), once);
    }

    #[test]
    fn survives_unexpected_shapes() {
        assert!(merge_claude_settings(&json!(5))["hooks"].is_object());
        assert!(merge_claude_settings(&json!({"hooks": 3}))["hooks"]["Stop"].is_array());
    }
}
