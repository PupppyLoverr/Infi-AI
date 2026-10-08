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
//! Approvals (spec §6.6) and sandboxed agent identities (§6.3) are
//! later slices — a tool listed in the policy's `sensitive` array is
//! denied outright until the approval flow lands.

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use cosmos_ipc::{Event, Request};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

fn runtime_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from("/tmp")
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
    /// Tools that must be approval-gated. Until approvals land these
    /// are denied with a clear reason (fail closed, spec §6.6).
    sensitive: Vec<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            read_roots: Vec::new(),
            write_roots: Vec::new(),
            tools: Vec::new(),
            sensitive: Vec::new(),
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
        policy
    }

    fn tool_allowed(&self, name: &str) -> Result<()> {
        if self.sensitive.iter().any(|t| t == name) {
            bail!("tool `{name}` is marked sensitive and requires approval flow (not yet implemented)");
        }
        if !self.tools.is_empty() && !self.tools.iter().any(|t| t == name) {
            bail!("tool `{name}` is not in this agent's policy allowlist");
        }
        Ok(())
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
            "read_roots" => policy.read_roots = list.iter().map(PathBuf::from).collect(),
            "write_roots" => policy.write_roots = list.iter().map(PathBuf::from).collect(),
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
}

impl Agentd {
    /// Round-trip for requests that always reply (lists, config, screenshot).
    fn ipc(&self, req: &Request) -> Result<Event> {
        let stream = UnixStream::connect(cosmos_ipc::socket_path())
            .context("connect cosmos-ipc socket — is the compositor running?")?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
        cosmos_ipc::write_message(&mut &stream, req)?;
        let mut reader = BufReader::new(&stream);
        cosmos_ipc::read_message(&mut reader)?
            .context("compositor closed the IPC connection")
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

    fn call(&mut self, name: &str, args: &Map<String, Value>) -> Result<Value> {
        self.policy.tool_allowed(name)?;
        let get = |key: &str| -> Result<&Value> {
            args.get(key).with_context(|| format!("missing argument `{key}`"))
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
                    zone: get("zone")?.as_str().context("`zone` must be a string")?.into(),
                })?;
                "ok".to_string()
            }
            "desktop.windows.move_workspace" => {
                self.ipc_send(&Request::MoveWindowToWorkspace {
                    id: get("id")?.as_u64().context("`id` must be a u64")?,
                    workspace: get("workspace")?.as_u64().context("`workspace` must be a u64")? as u8,
                })?;
                "ok".to_string()
            }
            "desktop.workspaces.switch" => {
                self.ipc_send(&Request::SwitchWorkspace {
                    workspace: get("workspace")?.as_u64().context("`workspace` must be a u64")? as u8,
                })?;
                "ok".to_string()
            }
            "desktop.launch" => {
                let cmd = get("command")?.as_str().context("`command` must be a string")?;
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
                    if let Ok(key) = get("key").and_then(|v| {
                        v.as_str().context("`key` must be a string")
                    }) {
                        serde_json::to_string_pretty(
                            cfg.get(key).unwrap_or(&Value::Null),
                        )?
                    } else {
                        serde_json::to_string_pretty(&cfg)?
                    }
                }
                other => bail!("unexpected reply: {other:?}"),
            },
            "system.settings.set" => {
                let key = get("key")?.as_str().context("`key` must be a string")?.to_string();
                let raw = get("value")?;
                let value: Value =
                    serde_json::from_str(raw.as_str().unwrap_or("")).unwrap_or_else(|_| raw.clone());
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
                let content = get("content")?.as_str().context("`content` must be a string")?;
                fs::write(&path, content).with_context(|| format!("write {path:?}"))?;
                format!("wrote {path:?}")
            }
            "files.search" => {
                let root = PathBuf::from(get("root")?.as_str().context("`root` must be a string")?);
                if !self.policy.path_in(&root, &self.policy.read_roots) {
                    bail!("{root:?} is outside this agent's read roots");
                }
                let query = get("query")?.as_str().context("`query` must be a string")?.to_lowercase();
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
                        if path.file_name()
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
                    bail!("move must stay inside read roots (source) and write roots (destination)");
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

#[derive(Debug, Serialize)]
struct AuditEntry {
    ts: u64,
    agent: String,
    tool: String,
    ok: bool,
    detail: String,
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
            Ok(v) => v.to_string().chars().take(400).collect(),
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

fn handle(stream: UnixStream) -> Result<()> {
    let reader = BufReader::new(stream.try_clone()?);
    let mut writer = &stream;
    let mut agent: Option<Agentd> = None;
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
                let name = rpc
                    .params
                    .get("clientInfo")
                    .and_then(|c| c.get("name"))
                    .and_then(|n| n.as_str())
                    .or_else(|| rpc.params.get("agent").and_then(|a| a.as_str()))
                    .unwrap_or("default")
                    .to_string();
                agent = Some(Agentd {
                    policy: Policy::load(&name),
                    agent: name.clone(),
                });
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
                let outcome = state.call(&tool, &args);
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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
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
                    if let Err(e) = handle(stream) {
                        tracing::debug!("client disconnected: {e:#}");
                    }
                });
            }
            Err(e) => tracing::warn!("accept failed: {e:#}"),
        }
    }
    Ok(())
}
