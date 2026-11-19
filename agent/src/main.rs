//! solyra-agent: hooks, terminal wrapper and level-1 sandbox for AI agents.
//!
//!   solyra-agent hook claude              (stdin: Claude Code hook JSON)
//!   solyra-agent hook codex '<json>'      (Codex `notify` argument)
//!   solyra-agent wrap [--name N] -- CMD ARGS...
//!   solyra-agent run --policy FILE -- CMD ARGS...
//!   solyra-agent undo <run-id>            restore the workspace of a run
//!   solyra-agent runs                     list runs that can be undone
//!   solyra-agent install-hooks claude [--apply]

mod events;
mod hook;
mod installer;
mod sandbox;
mod wrap;

use std::io::Read;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::time::Duration;

use events::{now, Emitter};
use serde_json::json;

fn die(msg: impl AsRef<str>) -> ExitCode {
    eprintln!("solyra-agent: {}", msg.as_ref());
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("hook") => cmd_hook(&args[1..]),
        Some("wrap") => cmd_wrap(&args[1..]),
        Some("run") => cmd_run(&args[1..]),
        Some("undo") => cmd_undo(&args[1..]),
        Some("runs") => cmd_runs(),
        Some("install-hooks") => installer::cmd(&args[1..]),
        _ => {
            eprintln!("{}", include_str!("main.rs").lines().take_while(|l| l.starts_with("//!")).map(|l| l.trim_start_matches("//!").trim_start_matches(' ')).collect::<Vec<_>>().join("\n"));
            ExitCode::from(2)
        }
    }
}

fn wait_secs() -> Duration {
    Duration::from_secs(
        std::env::var("SOLYRA_AGENT_WAIT_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(120),
    )
}

/// Hooks must never break the agent: any problem means "print nothing", which
/// makes the agent fall back to its own behaviour.
fn cmd_hook(args: &[String]) -> ExitCode {
    let Some(root) = events::root_dir() else { return ExitCode::SUCCESS };
    let wrapped_id = std::env::var("SOLYRA_AGENT_ID").ok();
    match args.first().map(String::as_str) {
        Some("claude") => {
            let mut raw = String::new();
            if std::io::stdin().read_to_string(&mut raw).is_err() {
                return ExitCode::SUCCESS;
            }
            let Ok(input) = serde_json::from_str(&raw) else { return ExitCode::SUCCESS };
            let ctx = hook::ClaudeCtx {
                root: &root,
                wrapped_id: wrapped_id.as_deref(),
                wrapped: std::env::var_os("SOLYRA_AGENT_WRAPPED").is_some(),
                wait: wait_secs(),
            };
            if let Some(out) = hook::claude(&input, &ctx) {
                println!("{out}");
            }
            ExitCode::SUCCESS
        }
        Some("codex") => {
            if let Some(arg) = args.get(1) {
                let _ = hook::codex(arg, &root, wrapped_id.as_deref());
            }
            ExitCode::SUCCESS
        }
        _ => die("hook needs `claude` or `codex`"),
    }
}

fn split_dashdash(args: &[String]) -> (&[String], &[String]) {
    match args.iter().position(|a| a == "--") {
        Some(i) => (&args[..i], &args[i + 1..]),
        None => (args, &[]),
    }
}

fn flag<'a>(opts: &'a [String], name: &str) -> Option<&'a str> {
    opts.iter().position(|a| a == name).and_then(|i| opts.get(i + 1)).map(String::as_str)
}

fn cmd_wrap(args: &[String]) -> ExitCode {
    let (opts, command) = split_dashdash(args);
    if command.is_empty() {
        return die("usage: solyra-agent wrap [--name N] -- CMD ARGS...");
    }
    let base = std::path::Path::new(&command[0])
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("agent")
        .to_string();
    let agent = flag(opts, "--name").unwrap_or(&base).to_string();
    let Some(root) = events::root_dir() else {
        // No runtime directory: run the program untouched.
        let err = exec(command);
        return die(format!("{}: {err}", command[0]));
    };
    let id = format!("{}-{}-{}", base, std::process::id(), now() % 100000);
    let cfg = wrap::WrapConfig {
        emitter: Emitter::new(root, &id),
        command: command.to_vec(),
        agent,
        idle_after: Duration::from_millis(2500),
    };
    match wrap::run(&cfg, std::io::stdin().as_raw_fd(), std::io::stdout().as_raw_fd()) {
        Ok(code) => ExitCode::from(code.clamp(0, 255) as u8),
        Err(e) => die(e.to_string()),
    }
}

fn exec(command: &[String]) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    Command::new(&command[0]).args(&command[1..]).exec()
}

fn cmd_run(args: &[String]) -> ExitCode {
    let (opts, command) = split_dashdash(args);
    let Some(policy_path) = flag(opts, "--policy") else {
        return die("usage: solyra-agent run --policy FILE -- CMD ARGS...");
    };
    if command.is_empty() {
        return die("no command to run");
    }
    let text = match std::fs::read_to_string(policy_path) {
        Ok(t) => t,
        Err(e) => return die(format!("{policy_path}: {e}")),
    };
    let policy = match sandbox::parse_policy(&text) {
        Ok(p) => p,
        Err(e) => return die(e),
    };
    let ws = match sandbox::check_workspace(&policy.workspace) {
        Ok(w) => w,
        Err(e) => return die(e),
    };
    let Some(snaps) = sandbox::snapshots_dir() else { return die("HOME is not set") };
    let run_id = format!("run-{}-{}", now(), std::process::id());
    let snap_dir = snaps.join(&run_id);
    let reflink = match sandbox::snapshot(&ws, &snap_dir.join("files")) {
        Ok(r) => r,
        Err(e) => return die(format!("snapshot failed, nothing was run: {e}")),
    };
    let _ = std::fs::write(snap_dir.join("workspace"), ws.to_string_lossy().as_bytes());
    eprintln!(
        "solyra-agent: snapshot {run_id} taken ({}). Undo with: solyra-agent undo {run_id}",
        if reflink { "instant, copy-on-write" } else { "full copy" }
    );
    audit(json!({"event":"run_started","run":run_id,"workspace":ws,"network":policy.network,"command":command}));

    let bw = sandbox::bwrap_args(&policy, &ws, command);
    let status = Command::new("bwrap").args(&bw).status();
    let code = match status {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => return die(format!("could not start bubblewrap (is it installed?): {e}")),
    };
    let changes = sandbox::diff(&ws, &snap_dir.join("files")).ok();
    if let Some(c) = &changes {
        eprintln!(
            "solyra-agent: {} added, {} changed, {} removed in {}",
            c.added.len(), c.changed.len(), c.removed.len(), ws.display()
        );
    }
    audit(json!({"event":"run_finished","run":run_id,"exit":code,
        "added": changes.as_ref().map(|c| c.added.len()), "changed": changes.as_ref().map(|c| c.changed.len()),
        "removed": changes.as_ref().map(|c| c.removed.len())}));
    ExitCode::from(code.clamp(0, 255) as u8)
}

fn audit(entry: serde_json::Value) {
    let Some(dir) = sandbox::snapshots_dir().and_then(|d| d.parent().map(PathBuf::from)) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let mut e = entry;
    e["ts"] = json!(now());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("agent-audit.jsonl")) {
        use std::io::Write;
        let _ = writeln!(f, "{e}");
    }
}

fn cmd_undo(args: &[String]) -> ExitCode {
    let Some(run) = args.first() else { return die("usage: solyra-agent undo <run-id>") };
    if !run.starts_with("run-") || run.contains('/') || run.contains("..") {
        return die("not a run id");
    }
    let Some(snaps) = sandbox::snapshots_dir() else { return die("HOME is not set") };
    let dir = snaps.join(run);
    let Ok(ws) = std::fs::read_to_string(dir.join("workspace")) else { return die(format!("no such run: {run}")) };
    let ws = PathBuf::from(ws.trim());
    // The same guard as at run time: undo never writes into a broad folder.
    if let Err(e) = sandbox::check_workspace(&ws) {
        return die(e);
    }
    match sandbox::restore(&ws, &dir.join("files")) {
        Ok(c) => {
            eprintln!(
                "solyra-agent: restored {}: removed {} added, restored {} changed, restored {} deleted",
                ws.display(), c.added.len(), c.changed.len(), c.removed.len()
            );
            audit(json!({"event":"undo","run":run,"workspace":ws}));
            ExitCode::SUCCESS
        }
        Err(e) => die(format!("undo failed: {e}")),
    }
}

fn cmd_runs() -> ExitCode {
    let Some(snaps) = sandbox::snapshots_dir() else { return die("HOME is not set") };
    let mut runs: Vec<_> = std::fs::read_dir(snaps).into_iter().flatten().flatten().collect();
    runs.sort_by_key(|e| e.file_name());
    for r in runs {
        let ws = std::fs::read_to_string(r.path().join("workspace")).unwrap_or_default();
        println!("{}\t{}", r.file_name().to_string_lossy(), ws.trim());
    }
    ExitCode::SUCCESS
}
