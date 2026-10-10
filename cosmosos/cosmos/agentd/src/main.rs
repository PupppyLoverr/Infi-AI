//! cosmos-agentd — the CosmosOS agent runtime.
//!
//! A Unix-socket MCP server that exposes the OS to agents as typed tools
//! (spec §6.2). Wire format is newline-delimited JSON-RPC: `initialize`,
//! `notifications/initialized`, `ping`, `tools/list`, `tools/call` — the
//! same method names MCP uses, so any MCP-capable agent can speak to it.
//!
//! Tool backends:
//!  - `desktop.*` proxies the compositor IPC socket (semantic first —
//!    real window/workspace state and actions, never screen scraping).
//!  - `files.*` are real filesystem ops scoped by the agent's policy
//!    file (`~/.config/cosmos/agents/<name>.toml`); the policy carries
//!    `read_roots`, `write_roots`, and a `tools` allowlist. Missing
//!    policy = spec default: read ~, write ~/Agents/<name>/, all tools.
//!  - `system.settings.*` proxied to the compositor's persisted config.
//!  - Every call is appended to `~/.local/share/cosmos/agent-audit/`
//!    as JSONL (spec §6.8).
//!
//! Approvals (spec §6.6): a tool listed in the policy's `sensitive`
//! array pauses the call and posts a Deny / Allow once / Always allow
//! card over org.freedesktop.Notifications; Allow-once is per call and
//! Always is remembered for this agentd's lifetime. Sandboxed agent
//! identities (§6.3) are a later slice.

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use cosmos_ipc::{Event, Request};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

fn runtime_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from("/tmp")
}

#[derive(PartialEq)]
enum Perm {
    Allowed,
    Sensitive,
}

#[derive(PartialEq)]
enum Decision {
    Once,
    Always,
    Denied,
}

fn socket_path() -> PathBuf {
    runtime_dir().join("cosmos-agentd.sock")
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into()))
}

/// Per-agent policy (`~/.config/cosmos/agents/<name>.toml`).
/// All fields optional — an absent file means the spec default.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct Policy {
    /// Directories the agent may read. Default: $HOME.
    read_roots: Vec<PathBuf>,
    /// Directories the agent may create/modify. Default: ~/Agents/<name>.
    write_roots: Vec<PathBuf>,
    /// Tool allowlist; empty = all tools.
    tools: Vec<String>,
    /// Tools that must be approval-gated (spec §6.6): each call pauses
    /// until the user clicks Allow/Deny on the notification card.
    sensitive: Vec<String>,
    /// Tools approved "always" this session (not persisted to disk).
    #[serde(skip)]
    approved: HashSet<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            read_roots: Vec::new(),
            write_roots: Vec::new(),
            tools: Vec::new(),
            sensitive: Vec::new(),
            approved: HashSet::new(),
        }
    }
}

impl Policy {
    fn load(agent: &str) -> Self {
        let path = home()
            .join(".config/cosmos/agents")
            .join(format!("{agent}.toml"));
        let mut policy: Policy = fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml_like_parse(&s).ok())
            .unwrap_or_default();
        if policy.read_roots.is_empty() {
            policy.read_roots = vec![home()];
        }
        if policy.write_roots.is_empty() {
            policy.write_roots = vec![home().join("Agents").join(agent)];
        }
        if policy.sensitive.is_empty() && !path.exists() {
            // No policy file: default-gate the consequential tools —
            // screenshots and writes always ask (spec §6.6). An explicit
            // `sensitive = []` in a TOML opts out.
            policy.sensitive = vec![
                "desktop.screenshot".into(),
                "files.write".into(),
                "files.move".into(),
            ];
        }
        policy
    }

    fn tool_perm(&self, name: &str) -> Result<Perm> {
        if self.approved.contains(name) {
            return Ok(Perm::Allowed);
        }
        if self.sensitive.iter().any(|t| t == name) {
            return Ok(Perm::Sensitive);
        }
        if !self.tools.is_empty() && !self.tools.iter().any(|t| t == name) {
            bail!("tool `{name}` is not in this agent's policy allowlist");
        }
        Ok(Perm::Allowed)
    }

    /// `dir` must live under one of `roots` (after canonicalizing the
    /// longest existing prefix so `..` can't escape).
    fn path_in(&self, path: &Path, roots: &[PathBuf]) -> bool {
        let resolved = canonicalize_lenient(path);
        roots.iter().any(|root| {
            let r = canonicalize_lenient(root);
            resolved == r || resolved.starts_with(&r)
        })
    }
}

/// Canonicalize as much of `path` as exists, then re-append the tail —
/// resolves `.`/`..`/symlinks without requiring the target to exist.
fn canonicalize_lenient(path: &Path) -> PathBuf {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        home().join(path)
    };
    let mut probe = path.clone();
    let mut tail = Vec::new();
    loop {
        match probe.canonicalize() {
            Ok(c) => {
                let mut out = c;
                for part in tail.iter().rev() {
                    out.push(part);
                }
                return out;
            }
            Err(_) => match (probe.file_name(), probe.parent()) {
                (Some(name), Some(parent)) => {
                    tail.push(name.to_os_string());
                    probe = parent.to_path_buf();
                }
                _ => return path,
            },
        }
    }
}

/// Minimal TOML subset for the policy file: `key = ["a", "b"]` or
/// `key = "v"` lines. Keeps agentd dependency-light (no toml crate);
/// policy files are written by Settings/onboarding in a later slice.
/// `~` / `~/x` in policy roots resolve against $HOME, so one policy file
/// in /etc/skel works for every user.
fn expand_home(s: &str) -> PathBuf {
    match s.strip_prefix('~') {
        Some("") => home(),
        Some(rest) if rest.starts_with('/') => home().join(&rest[1..]),
        _ => PathBuf::from(s),
    }
}

/// Snapshot the home volume before an agent's first change of a session,
/// so Agents → Rollback can undo it. Best effort: no snapper, no snapshot.
fn pre_change_snapshot(agent: &str, tool: &str) {
    let desc = format!("Before {agent}: {tool}");
    match std::process::Command::new("snapper")
        .args(["-c", "home", "create", "--cleanup-algorithm", "number"])
        .args(["--description", &desc])
        .output()
    {
        Ok(o) if o.status.success() => tracing::info!("snapshot taken: {desc}"),
        Ok(o) => tracing::warn!(
            "snapper create failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => tracing::debug!("snapper unavailable: {e}"),
    }
}

fn toml_like_parse(text: &str) -> Result<Policy> {
    let mut policy = Policy::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('[') {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let val = val.trim();
        let list: Vec<String> = if val.starts_with('[') {
            val.trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(|s| s.trim().trim_matches('"').to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            vec![val.trim_matches('"').to_string()]
        };
        match key {
            "read_roots" => policy.read_roots = list.iter().map(|s| expand_home(s)).collect(),
            "write_roots" => policy.write_roots = list.iter().map(|s| expand_home(s)).collect(),
            "tools" => policy.tools = list,
            "sensitive" => policy.sensitive = list,
            _ => {}
        }
    }
    Ok(policy)
}

/// MCP tool descriptor emitted by `tools/list`.
fn tool_defs() -> Value {
    let obj = |props: &[(&str, &str)]| {
        json!({
            "type": "object",
            "properties": props.iter().map(|(k, v)| (k.to_string(), json!({"type": v}))).collect::<Map<_,_>>(),
        })
    };
    json!([
        {"name": "desktop.windows.list", "description": "List all windows: id, title, app_id, workspace, geometry, state.", "inputSchema": obj(&[])},
        {"name": "desktop.windows.focus", "description": "Focus a window by id.", "inputSchema": obj(&[("id", "integer")])},
        {"name": "desktop.windows.close", "description": "Politely close a window by id.", "inputSchema": obj(&[("id", "integer")])},
        {"name": "desktop.windows.snap", "description": "Snap a window into a named zone (left/right/top/bottom/quarters/…).", "inputSchema": obj(&[("id", "integer"), ("zone", "string")])},
        {"name": "desktop.windows.move_workspace", "description": "Move a window to a 0-based workspace index.", "inputSchema": obj(&[("id", "integer"), ("workspace", "integer")])},
        {"name": "desktop.workspaces.list", "description": "List workspaces: index, focused flag, window count.", "inputSchema": obj(&[])},
        {"name": "desktop.workspaces.switch", "description": "Switch the active workspace.", "inputSchema": obj(&[("workspace", "integer")])},
        {"name": "desktop.launch", "description": "Launch a command on the desktop (app binary or shell line).", "inputSchema": obj(&[("command", "string")])},
        {"name": "desktop.screenshot", "description": "Capture the primary output to a PNG under ~/Agents/<agent>/shots/. Permission-gated.", "inputSchema": obj(&[("name", "string")])},
        {"name": "system.settings.get", "description": "Read all CosmosOS settings (or pass key to read one).", "inputSchema": obj(&[("key", "string")])},
        {"name": "system.settings.set", "description": "Write a CosmosOS setting (persisted by the compositor).", "inputSchema": obj(&[("key", "string"), ("value", "string")])},
        {"name": "files.read", "description": "Read a file under the agent's read roots.", "inputSchema": obj(&[("path", "string")])},
        {"name": "files.write", "description": "Write a file under the agent's write roots.", "inputSchema": obj(&[("path", "string"), ("content", "string")])},
        {"name": "files.search", "description": "Search file names under a read root (substring match).", "inputSchema": obj(&[("root", "string"), ("query", "string")])},
        {"name": "files.move", "description": "Move/rename a file; destination must stay inside write roots.", "inputSchema": obj(&[("from", "string"), ("to", "string")])},
    ])
}

struct Agentd {
    agent: String,
    policy: Policy,
    /// A pre-change home snapshot was taken this session.
    snapped: bool,
}

impl Agentd {
    /// Round-trip for requests that always reply (lists, config, screenshot).
    fn ipc(&self, req: &Request) -> Result<Event> {
        let stream = UnixStream::connect(cosmos_ipc::socket_path())
            .context("connect cosmos-ipc socket — is the compositor running?")?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
        cosmos_ipc::write_message(&mut &stream, req)?;
        let mut reader = BufReader::new(&stream);
        cosmos_ipc::read_message(&mut reader)?.context("compositor closed the IPC connection")
    }

    /// Fire-and-verify for action verbs: the compositor replies only on
    /// failure (`Event::Error`) — wait briefly to catch it, else assume
    /// applied. A 300ms grace covers the dispatch without stalling calls.
    fn ipc_send(&self, req: &Request) -> Result<()> {
        let stream = UnixStream::connect(cosmos_ipc::socket_path())
            .context("connect cosmos-ipc socket — is the compositor running?")?;
        stream.set_read_timeout(Some(std::time::Duration::from_millis(300)))?;
        cosmos_ipc::write_message(&mut &stream, req)?;
        let mut reader = BufReader::new(&stream);
        match cosmos_ipc::read_message(&mut reader) {
            Ok(Some(Event::Error { message })) => bail!("compositor: {message}"),
            _ => Ok(()),
        }
    }

    /// Post a persistent, urgency=critical notification card with
    /// Deny / Allow once / Always allow buttons and block up to 120s for
    /// the user's ActionInvoked (or NotificationClosed = Deny). spec §6.6.
    fn approve(&self, tool: &str, args: &Map<String, Value>) -> Result<Decision> {
        use zbus::blocking::{Connection, Proxy};
        let conn =
            Connection::session().context("no session bus — cannot show the approval card")?;
        let proxy = Proxy::new(
            &conn,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )?;
        // Subscribe BEFORE Notify so the reply can't race past us.
        let mut invoked = proxy.receive_signal("ActionInvoked")?;
        let mut closed = proxy.receive_signal("NotificationClosed")?;

        // Human-readable sentence, never raw JSON — the args go behind
        // the card's Details affordance (daemon renders the body text).
        let mut summary = match tool {
            "files.write" | "files.move" | "files.read" => {
                let verb = match tool {
                    "files.write" => "write a file",
                    "files.move" => "move a file",
                    _ => "read a file",
                };
                let p = args
                    .get("path")
                    .or_else(|| args.get("src"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let disp = p.replace(home().to_string_lossy().as_ref(), "~");
                format!("{} wants to {} — {}", self.agent, verb, disp)
            }
            "desktop.screenshot" => format!("{} wants a screenshot of the desktop", self.agent),
            "desktop.launch" => {
                let cmd = args.get("cmd").and_then(|v| v.as_str()).unwrap_or("?");
                format!("{} wants to launch — {}", self.agent, cmd)
            }
            t if t.starts_with("system.settings.") => {
                let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("?");
                format!("{} wants to change setting {}", self.agent, key)
            }
            _ => format!("{} wants {}", self.agent, tool),
        };
        if summary.len() > 80 {
            summary.truncate(77);
            summary.push('…');
        }
        let mut body = if args.is_empty() {
            String::new()
        } else {
            serde_json::to_string_pretty(args).unwrap_or_default()
        };
        if body.len() > 400 {
            body.truncate(397);
            body.push('…');
        }
        let actions = vec![
            "deny".to_string(),
            "Deny".to_string(),
            "allow".to_string(),
            "Allow once".to_string(),
            "always".to_string(),
            "Always allow".to_string(),
        ];
        let mut hints = std::collections::HashMap::new();
        hints.insert("urgency", zbus::zvariant::Value::U8(2));
        let want_id: u32 = proxy.call(
            "Notify",
            &(
                "Cosmos Agents",
                0u32,
                "",
                summary.as_str(),
                body.as_str(),
                actions,
                hints,
                -1i32,
            ),
        )?;

        let (tx, rx) = std::sync::mpsc::channel::<(u32, Option<String>)>();
        {
            let tx = tx.clone();
            std::thread::spawn(move || {
                while let Some(sig) = invoked.next() {
                    if let Ok((id, key)) = sig.body().deserialize::<(u32, String)>() {
                        let _ = tx.send((id, Some(key)));
                    }
                }
            });
        }
        std::thread::spawn(move || {
            while let Some(sig) = closed.next() {
                if let Ok((id, _reason)) = sig.body().deserialize::<(u32, u32)>() {
                    let _ = tx.send((id, None));
                }
            }
        });

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            let left = deadline
                .checked_duration_since(std::time::Instant::now())
                .context("approval timed out — treated as Deny")?;
            match rx.recv_timeout(left) {
                Ok((id, key)) if id == want_id => {
                    return Ok(match key.as_deref() {
                        Some("allow") => Decision::Once,
                        Some("always") => Decision::Always,
                        _ => Decision::Denied,
                    });
                }
                Ok(_) => continue, // another client's notification — keep waiting
                Err(_) => bail!("approval timed out — treated as Deny"),
            }
        }
    }

    /// Policy denials that don't need a user's click — run BEFORE the
    /// approval card so we never ask the user to approve a call the
    /// policy would deny anyway (gate ordering, drive defect #3).
    fn precheck_scope(&self, name: &str, args: &Map<String, Value>) -> Result<()> {
        let path = |key: &str| -> Result<PathBuf> {
            args.get(key)
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .with_context(|| format!("missing argument `{key}`"))
        };
        match name {
            "files.read" => {
                if !self.policy.path_in(&path("path")?, &self.policy.read_roots) {
                    bail!("path is outside the agent's read roots");
                }
            }
            "files.write" => {
                if !self
                    .policy
                    .path_in(&path("path")?, &self.policy.write_roots)
                {
                    bail!("path is outside the agent's write roots");
                }
            }
            "files.move" => {
                if !self.policy.path_in(&path("to")?, &self.policy.write_roots) {
                    bail!("destination is outside the agent's write roots");
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn call(&mut self, name: &str, args: &Map<String, Value>) -> Result<Value> {
        if self.policy.tool_perm(name)? == Perm::Sensitive {
            self.precheck_scope(name, args)?;
            match self.approve(name, args) {
                Ok(Decision::Always) => {
                    self.policy.approved.insert(name.to_string());
                }
                Ok(Decision::Once) => {}
                Ok(Decision::Denied) => bail!("tool `{name}` denied by user"),
                Err(e) => bail!("tool `{name}` needs approval but no decision arrived: {e:#}"),
            }
        }
        if matches!(name, "files.write" | "files.move")
            && !self.snapped
            && self.precheck_scope(name, args).is_ok()
        {
            self.snapped = true;
            pre_change_snapshot(&self.agent, name);
        }
        let get = |key: &str| -> Result<&Value> {
            args.get(key)
                .with_context(|| format!("missing argument `{key}`"))
        };
        let text = match name {
            "desktop.windows.list" => match self.ipc(&Request::ListWindows)? {
                Event::Windows { windows } => serde_json::to_string_pretty(&windows)?,
                other => bail!("unexpected reply: {other:?}"),
            },
            "desktop.workspaces.list" => match self.ipc(&Request::ListWorkspaces)? {
                Event::Workspaces { workspaces } => serde_json::to_string_pretty(&workspaces)?,
                other => bail!("unexpected reply: {other:?}"),
            },
            "desktop.windows.focus" => {
                self.ipc_send(&Request::FocusWindow {
                    id: get("id")?.as_u64().context("`id` must be a u64")?,
                })?;
                "ok".to_string()
            }
            "desktop.windows.close" => {
                self.ipc_send(&Request::CloseWindow {
                    id: get("id")?.as_u64().context("`id` must be a u64")?,
                })?;
                "ok".to_string()
            }
            "desktop.windows.snap" => {
                self.ipc_send(&Request::SnapToZone {
                    id: get("id")?.as_u64().context("`id` must be a u64")?,
                    zone: get("zone")?
                        .as_str()
                        .context("`zone` must be a string")?
                        .into(),
                })?;
                "ok".to_string()
            }
            "desktop.windows.move_workspace" => {
                self.ipc_send(&Request::MoveWindowToWorkspace {
                    id: get("id")?.as_u64().context("`id` must be a u64")?,
                    workspace: get("workspace")?
                        .as_u64()
                        .context("`workspace` must be a u64")? as u8,
                })?;
                "ok".to_string()
            }
            "desktop.workspaces.switch" => {
                self.ipc_send(&Request::SwitchWorkspace {
                    workspace: get("workspace")?
                        .as_u64()
                        .context("`workspace` must be a u64")? as u8,
                })?;
                "ok".to_string()
            }
            "desktop.launch" => {
                let cmd = get("command")?
                    .as_str()
                    .context("`command` must be a string")?;
                Command::new("sh")
                    .arg("-c")
                    .arg(cmd)
                    .env("XDG_CURRENT_DESKTOP", "cosmos")
                    .spawn()
                    .with_context(|| format!("spawn `{cmd}`"))?;
                format!("launched: {cmd}")
            }
            "desktop.screenshot" => {
                let dir = self.policy.write_roots[0].join("shots");
                fs::create_dir_all(&dir)?;
                let name = get("name")
                    .ok()
                    .and_then(|v| v.as_str())
                    .unwrap_or("shot")
                    .replace(['/', '\\'], "_");
                let path = dir.join(format!("{name}.png"));
                match self.ipc(&Request::Screenshot {
                    path: path.to_string_lossy().into(),
                })? {
                    Event::Screenshot { path: p } => p,
                    Event::Error { message } => bail!("compositor: {message}"),
                    other => bail!("unexpected reply: {other:?}"),
                }
            }
            "system.settings.get" => match self.ipc(&Request::GetConfig)? {
                Event::Config(cfg) => {
                    if let Ok(key) =
                        get("key").and_then(|v| v.as_str().context("`key` must be a string"))
                    {
                        serde_json::to_string_pretty(cfg.get(key).unwrap_or(&Value::Null))?
                    } else {
                        serde_json::to_string_pretty(&cfg)?
                    }
                }
                other => bail!("unexpected reply: {other:?}"),
            },
            "system.settings.set" => {
                let key = get("key")?
                    .as_str()
                    .context("`key` must be a string")?
                    .to_string();
                let raw = get("value")?;
                let value: Value = serde_json::from_str(raw.as_str().unwrap_or(""))
                    .unwrap_or_else(|_| raw.clone());
                self.ipc_send(&Request::SetConfig { key, value })?;
                "ok".to_string()
            }
            "files.read" => {
                let path = PathBuf::from(get("path")?.as_str().context("`path` must be a string")?);
                if !self.policy.path_in(&path, &self.policy.read_roots) {
                    bail!("{path:?} is outside this agent's read roots");
                }
                fs::read_to_string(&path).with_context(|| format!("read {path:?}"))?
            }
            "files.write" => {
                let path = PathBuf::from(get("path")?.as_str().context("`path` must be a string")?);
                if !self.policy.path_in(&path, &self.policy.write_roots) {
                    bail!("{path:?} is outside this agent's write roots");
                }
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let content = get("content")?
                    .as_str()
                    .context("`content` must be a string")?;
                fs::write(&path, content).with_context(|| format!("write {path:?}"))?;
                format!("wrote {path:?}")
            }
            "files.search" => {
                let root = PathBuf::from(get("root")?.as_str().context("`root` must be a string")?);
                if !self.policy.path_in(&root, &self.policy.read_roots) {
                    bail!("{root:?} is outside this agent's read roots");
                }
                let query = get("query")?
                    .as_str()
                    .context("`query` must be a string")?
                    .to_lowercase();
                let mut hits = Vec::new();
                let mut stack = vec![root];
                let mut visited = 0usize;
                while let Some(dir) = stack.pop() {
                    if visited >= 5000 || hits.len() >= 200 {
                        break;
                    }
                    let Ok(entries) = fs::read_dir(&dir) else {
                        continue;
                    };
                    for entry in entries.flatten() {
                        visited += 1;
                        let path = entry.path();
                        if path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_lowercase().contains(&query))
                            .unwrap_or(false)
                        {
                            hits.push(path.to_string_lossy().into_owned());
                        }
                        if path.is_dir() {
                            stack.push(path);
                        }
                    }
                }
                serde_json::to_string_pretty(&hits)?
            }
            "files.move" => {
                let from = PathBuf::from(get("from")?.as_str().context("`from` must be a string")?);
                let to = PathBuf::from(get("to")?.as_str().context("`to` must be a string")?);
                if !self.policy.path_in(&from, &self.policy.read_roots)
                    || !self.policy.path_in(&to, &self.policy.write_roots)
                {
                    bail!(
                        "move must stay inside read roots (source) and write roots (destination)"
                    );
                }
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::rename(&from, &to).with_context(|| format!("rename {from:?} → {to:?}"))?;
                format!("moved {from:?} → {to:?}")
            }
            _ => bail!("unknown tool `{name}`"),
        };
        Ok(json!([{"type": "text", "text": text}]))
    }
}

#[derive(Debug, Deserialize)]
struct Rpc {
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Map<String, Value>,
}

/// Live-activity record the shell's Dynamic Island reads: one JSON file per
/// connected agent under `~/.local/state/cosmos/agent-live/`. A sibling
/// `<agent>.paused` file (written by the island's Pause button) holds the
/// agent's next tool call until it is removed; `<agent>.stopped` (the
/// island's Stop button) refuses every further call. All three files go
/// away when the connection ends.
struct Live {
    path: PathBuf,
    paused: PathBuf,
    stopped: PathBuf,
    agent: String,
    started: u64,
    calls: u64,
}

fn live_dir() -> PathBuf {
    home().join(".local/state/cosmos/agent-live")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Live {
    fn new(agent: &str) -> Self {
        let dir = live_dir();
        let _ = fs::create_dir_all(&dir);
        let safe: String = agent
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let live = Live {
            path: dir.join(format!("{safe}.json")),
            paused: dir.join(format!("{safe}.paused")),
            stopped: dir.join(format!("{safe}.stopped")),
            agent: agent.into(),
            started: now_secs(),
            calls: 0,
        };
        live.write("", false);
        live
    }

    fn write(&self, tool: &str, busy: bool) {
        let v = json!({
            "agent": self.agent,
            "pid": std::process::id(),
            "started": self.started,
            "tool": tool,
            "busy": busy,
            "calls": self.calls,
        });
        let tmp = self.path.with_extension("tmp");
        if fs::write(&tmp, v.to_string()).is_ok() {
            let _ = fs::rename(&tmp, &self.path);
        }
    }

    /// Blocks while the user has the agent paused, then marks it busy.
    /// `false` when the user stopped the agent: the call must be refused.
    fn begin(&mut self, tool: &str) -> bool {
        if self.paused.exists() {
            tracing::info!(agent = %self.agent, "paused — holding tool call `{tool}`");
            while self.paused.exists() && !self.stopped.exists() {
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
        if self.stopped.exists() {
            tracing::info!(agent = %self.agent, "stopped — refusing tool call `{tool}`");
            return false;
        }
        self.calls += 1;
        self.write(tool, true);
        true
    }

    fn end(&self, tool: &str) {
        self.write(tool, false);
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(&self.paused);
        let _ = fs::remove_file(&self.stopped);
    }
}

#[derive(Debug, Serialize)]
struct AuditEntry {
    ts: u64,
    agent: String,
    tool: String,
    ok: bool,
    detail: String,
}

/// Tool results are MCP content arrays (`[{"type":"text","text":..}]`);
/// audit the text itself so the Agents table reads as prose, not JSON.
fn audit_text(v: &Value) -> String {
    let items = v
        .as_array()
        .or_else(|| v.get("content").and_then(|c| c.as_array()));
    let text: Vec<&str> = items
        .into_iter()
        .flatten()
        .filter_map(|i| i.get("text").and_then(|t| t.as_str()))
        .collect();
    if text.is_empty() {
        v.to_string()
    } else {
        text.join(" ")
    }
}

fn audit(agent: &str, tool: &str, result: &Result<Value>) {
    let dir = home().join(".local/share/cosmos/agent-audit");
    let _ = fs::create_dir_all(&dir);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = AuditEntry {
        ts: now,
        agent: agent.into(),
        tool: tool.into(),
        ok: result.is_ok(),
        detail: match result {
            Ok(v) => audit_text(v).chars().take(400).collect(),
            Err(e) => format!("{e:#}").chars().take(400).collect(),
        },
    };
    let day = {
        // days→date via the lock client's civil-from-days math would be
        // ideal; a flat epoch-second filename still groups entries fine.
        format!("{}", now / 86400)
    };
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("day-{day}.jsonl")))
    {
        let _ = serde_json::to_writer(&mut f, &entry);
        use std::io::Write;
        let _ = f.write_all(b"\n");
    }
}

fn handle<R, W>(reader: R, mut writer: W) -> Result<()>
where
    R: BufRead,
    W: std::io::Write,
{
    let mut agent: Option<Agentd> = None;
    let mut live: Option<Live> = None;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let rpc: Rpc = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                cosmos_ipc::write_message(
                    &mut writer,
                    &json!({"jsonrpc": "2.0", "id": Value::Null,
                            "error": {"code": -32700, "message": e.to_string()}}),
                )?;
                continue;
            }
        };
        let id = rpc.id.clone();
        let reply = match rpc.method.as_str() {
            "initialize" => {
                // COSMOS_AGENT (set by the spawning agent's MCP config in
                // --stdio mode) names the agent when its client id is generic.
                let name = std::env::var("COSMOS_AGENT")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| {
                        rpc.params
                            .get("clientInfo")
                            .and_then(|c| c.get("name"))
                            .and_then(|n| n.as_str())
                            .or_else(|| rpc.params.get("agent").and_then(|a| a.as_str()))
                            .unwrap_or("default")
                            .to_string()
                    });
                agent = Some(Agentd {
                    policy: Policy::load(&name),
                    agent: name.clone(),
                    snapped: false,
                });
                live = Some(Live::new(&name));
                json!({"jsonrpc": "2.0", "id": id, "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "cosmos-agentd", "version": env!("CARGO_PKG_VERSION")},
                    "agent": name,
                }})
            }
            "notifications/initialized" | "notifications/cancelled" => continue,
            "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
            "tools/list" => json!({"jsonrpc": "2.0", "id": id, "result": {"tools": tool_defs()}}),
            "tools/call" => {
                let tool = rpc
                    .params
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = rpc
                    .params
                    .get("arguments")
                    .and_then(|a| a.as_object())
                    .cloned()
                    .unwrap_or_default();
                let Some(state) = agent.as_mut() else {
                    cosmos_ipc::write_message(
                        &mut writer,
                        &json!({"jsonrpc": "2.0", "id": id, "error":
                                {"code": -32002, "message": "call initialize first"}}),
                    )?;
                    continue;
                };
                let allowed = live.as_mut().map(|l| l.begin(&tool)).unwrap_or(true);
                let outcome = if allowed {
                    state.call(&tool, &args)
                } else {
                    Err(anyhow::anyhow!(
                        "stopped by the user from the Dynamic Island"
                    ))
                };
                if let Some(l) = live.as_ref() {
                    l.end(&tool);
                }
                audit(&state.agent, &tool, &outcome);
                match outcome {
                    Ok(content) => json!({"jsonrpc": "2.0", "id": id,
                                          "result": {"content": content, "isError": false}}),
                    Err(e) => json!({"jsonrpc": "2.0", "id": id, "result": {
                        "content": [{"type": "text", "text": format!("{e:#}")}],
                        "isError": true,
                    }}),
                }
            }
            other => json!({"jsonrpc": "2.0", "id": id, "error":
                            {"code": -32601, "message": format!("method `{other}` not found")}}),
        };
        cosmos_ipc::write_message(&mut writer, &reply)?;
    }
    Ok(())
}

fn main() -> Result<()> {
    // stdio mode: agents like opencode/Claude Code spawn a command and
    // speak MCP over stdin/stdout — same handler, no socket. Logs must
    // stay off stdout, so tracing writes to stderr there anyway.
    if std::env::args().any(|a| a == "--stdio") {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        return handle(BufReader::new(stdin.lock()), stdout.lock());
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let path = socket_path();
    if path.exists() {
        fs::remove_file(&path)?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let listener = UnixListener::bind(&path)?;
    // Agent connections must not be able to claim each other's sockets;
    // the daemon runs in the user's session so 0700 on the file is the
    // right baseline until slice B adds peer-cred policy.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o700));
    }
    tracing::info!("cosmos-agentd listening on {}", path.display());
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                std::thread::spawn(move || {
                    let reader = match stream.try_clone() {
                        Ok(s) => BufReader::new(s),
                        Err(e) => {
                            tracing::warn!("client setup failed: {e:#}");
                            return;
                        }
                    };
                    if let Err(e) = handle(reader, &stream) {
                        tracing::debug!("client disconnected: {e:#}");
                    }
                });
            }
            Err(e) => tracing::warn!("accept failed: {e:#}"),
        }
    }
    Ok(())
}
