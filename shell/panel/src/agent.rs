//! Agent task protocol, panel side.
//!
//! The panel does not run agents. It renders what an agent runtime reports and
//! records the human's decisions for that runtime to read. The contract is a
//! directory of append-only JSON-lines files, documented in
//! `docs/agent-protocol.md`:
//!
//! ```text
//! $XDG_RUNTIME_DIR/solyra-agent/
//!   tasks/<id>.jsonl       events written by the runtime
//!   decisions/<id>.jsonl   approvals and denials written by the panel
//!   requests/<n>.json      draft task requests written by the launcher
//! ```
//!
//! Nothing here is simulated. If the directory does not exist there is no
//! runtime, and the UI says so.
//!
//! A decision recorded here is a message to the runtime, not an enforcement
//! point: whether the runtime honours it, and what isolation the agent runs
//! under, is the runtime's business. The UI must never imply otherwise.

use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// A task that has produced no event for this long is flagged as quiet.
pub const QUIET_AFTER_SECS: u64 = 120;
/// Finished and failed tasks disappear from the list after this long.
pub const TERMINAL_VISIBLE_SECS: u64 = 6 * 3600;
/// Only the tail of a log this large is read; a runaway log cannot stall the
/// shell.
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
/// Milestones kept per task for display.
const MAX_MILESTONES: usize = 40;
/// Upper bound on one text field shown in the UI.
const MAX_FIELD_CHARS: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    /// Alive and waiting for the human's next instruction.
    Idle,
    WaitingApproval,
    Blocked,
    Finished,
    Failed,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Working",
            Self::Idle => "Idle",
            Self::WaitingApproval => "Needs you",
            Self::Blocked => "Blocked",
            Self::Finished => "Finished",
            Self::Failed => "Failed",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Finished | Self::Failed)
    }

    /// Lower sorts first: what needs the human comes before what does not.
    fn urgency(self) -> u8 {
        match self {
            Self::WaitingApproval => 0,
            Self::Blocked => 1,
            Self::Running => 2,
            Self::Idle => 3,
            Self::Failed => 4,
            Self::Finished => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Once,
    Task,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Task => "task",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "once" => Some(Self::Once),
            "task" => Some(Self::Task),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    Deny,
    /// A free-text answer to a question.
    Answer,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Deny => "deny",
            Self::Answer => "answer",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "approve" => Some(Self::Approve),
            "deny" => Some(Self::Deny),
            "answer" => Some(Self::Answer),
            _ => None,
        }
    }
}

/// What the agent is asking the human for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskKind {
    /// Permission to do something: approve or deny.
    Permission,
    /// A question: answered with text (or one of the offered choices).
    Question,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Approval {
    pub kind: AskKind,
    /// Answers the agent offered for a question; empty means free text.
    pub choices: Vec<String>,
    pub request_id: String,
    pub action: String,
    pub resource: Option<String>,
    pub reason: Option<String>,
    /// `None` means the agent did not say. The UI must not read that as "yes".
    pub reversible: Option<bool>,
    pub scopes: Vec<Scope>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Milestone {
    pub ts: u64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskView {
    pub id: String,
    pub name: String,
    pub objective: Option<String>,
    /// The program's own name for itself (`claude`, `codex`), as reported.
    pub agent: Option<String>,
    /// The agent runs under `solyra-agent wrap`, so text sent from the panel
    /// reaches it. Only ever true if the runtime said so.
    pub interactive: bool,
    /// Where the agent says it runs, verbatim. Never inferred.
    pub environment: Option<String>,
    pub status: Status,
    pub milestones: Vec<Milestone>,
    pub pending: Option<Approval>,
    /// A decision the panel has recorded that the runtime has not yet
    /// acknowledged with `approval_resolved`.
    pub decision_sent: Option<(Verdict, Scope)>,
    pub last_ts: u64,
    /// Failure text, blocker text, or completion summary, as reported.
    pub detail: Option<String>,
    /// Lines that were not valid JSON (a half-written last line is normal and
    /// is not counted).
    pub malformed_lines: usize,
    /// Valid events whose `type` this panel does not know.
    pub unknown_events: usize,
}

impl TaskView {
    pub fn is_quiet(&self, now: u64) -> bool {
        !self.status.is_terminal() && now.saturating_sub(self.last_ts) > QUIET_AFTER_SECS
    }

    pub fn latest_milestone(&self) -> Option<&Milestone> {
        self.milestones.last()
    }
}

fn clip(raw: &str) -> String {
    let flat: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.trim();
    if flat.chars().count() <= MAX_FIELD_CHARS {
        flat.to_string()
    } else {
        let mut out: String = flat.chars().take(MAX_FIELD_CHARS - 1).collect();
        out.push('…');
        out
    }
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(clip)
        .filter(|s| !s.is_empty())
}

/// Task ids come from file names and end up in paths: keep them boring.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Fold one task's event log and decision log into what the UI shows.
///
/// `events` and `decisions` are the raw JSON-lines text. The last line of
/// `events` may be a partial write; it is ignored rather than reported.
pub fn reduce(id: &str, events: &str, decisions: &str) -> TaskView {
    let mut view = TaskView {
        id: id.to_string(),
        name: id.to_string(),
        objective: None,
        agent: None,
        interactive: false,
        environment: None,
        status: Status::Running,
        milestones: Vec::new(),
        pending: None,
        decision_sent: None,
        last_ts: 0,
        detail: None,
        malformed_lines: 0,
        unknown_events: 0,
    };

    let ends_with_newline = events.ends_with('\n');
    let lines: Vec<&str> = events.lines().collect();
    let last_index = lines.len().saturating_sub(1);

    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(ev) = serde_json::from_str::<Value>(line) else {
            // An unterminated final line is a write in progress.
            if !(i == last_index && !ends_with_newline) {
                view.malformed_lines += 1;
            }
            continue;
        };
        if let Some(ts) = ev.get("ts").and_then(Value::as_u64) {
            view.last_ts = view.last_ts.max(ts);
        }
        let ts = ev.get("ts").and_then(Value::as_u64).unwrap_or(0);
        match ev.get("type").and_then(Value::as_str).unwrap_or("") {
            "started" => {
                if let Some(name) = str_field(&ev, "name") {
                    view.name = name;
                }
                view.objective = str_field(&ev, "objective");
                view.agent = str_field(&ev, "agent");
                view.interactive = ev
                    .get("interactive")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                view.environment = str_field(&ev, "environment");
                view.status = Status::Running;
            }
            "milestone" => {
                if let Some(text) = str_field(&ev, "text") {
                    view.milestones.push(Milestone { ts, text });
                    if view.milestones.len() > MAX_MILESTONES {
                        view.milestones.remove(0);
                    }
                }
                if matches!(view.status, Status::Blocked) {
                    view.status = Status::Running;
                    view.detail = None;
                }
            }
            "approval_request" => {
                let (Some(request_id), Some(action)) =
                    (str_field(&ev, "request_id"), str_field(&ev, "action"))
                else {
                    view.malformed_lines += 1;
                    continue;
                };
                let mut scopes: Vec<Scope> = ev
                    .get("scopes")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .filter_map(Scope::parse)
                            .collect()
                    })
                    .unwrap_or_default();
                if scopes.is_empty() {
                    scopes.push(Scope::Once);
                }
                view.pending = Some(Approval {
                    kind: AskKind::Permission,
                    choices: Vec::new(),
                    request_id,
                    action,
                    resource: str_field(&ev, "resource"),
                    reason: str_field(&ev, "reason"),
                    reversible: ev.get("reversible").and_then(Value::as_bool),
                    scopes,
                });
                view.status = Status::WaitingApproval;
            }
            "question" => {
                let (Some(request_id), Some(text)) =
                    (str_field(&ev, "request_id"), str_field(&ev, "text"))
                else {
                    view.malformed_lines += 1;
                    continue;
                };
                let choices = ev
                    .get("choices")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(clip)
                            .take(8)
                            .collect()
                    })
                    .unwrap_or_default();
                view.pending = Some(Approval {
                    kind: AskKind::Question,
                    choices,
                    request_id,
                    action: text,
                    resource: None,
                    reason: None,
                    reversible: None,
                    scopes: vec![Scope::Once],
                });
                view.status = Status::WaitingApproval;
            }
            "approval_resolved" | "question_resolved" => {
                let resolved = str_field(&ev, "request_id");
                if view.pending.as_ref().map(|p| &p.request_id) == resolved.as_ref() {
                    view.pending = None;
                    if view.status == Status::WaitingApproval {
                        view.status = Status::Running;
                    }
                }
            }
            "status" => {
                // Working or idle. Never overrides a question or a terminal state.
                let next = match ev.get("state").and_then(Value::as_str) {
                    Some("working") => Some(Status::Running),
                    Some("idle") => Some(Status::Idle),
                    _ => None,
                };
                match next {
                    Some(n)
                        if !matches!(
                            view.status,
                            Status::WaitingApproval
                                | Status::Finished
                                | Status::Failed
                                | Status::Blocked
                        ) =>
                    {
                        view.status = n
                    }
                    Some(_) => {}
                    None => view.unknown_events += 1,
                }
            }
            "blocked" => {
                view.detail = str_field(&ev, "text");
                view.status = Status::Blocked;
            }
            "finished" => {
                view.detail = str_field(&ev, "summary");
                view.pending = None;
                view.status = Status::Finished;
            }
            "failed" => {
                view.detail = str_field(&ev, "error");
                view.pending = None;
                view.status = Status::Failed;
            }
            _ => view.unknown_events += 1,
        }
    }

    // A recorded decision the runtime has not yet acknowledged.
    if let Some(pending) = &view.pending {
        for line in decisions.lines() {
            let Ok(d) = serde_json::from_str::<Value>(line.trim()) else {
                continue;
            };
            if d.get("request_id").and_then(Value::as_str) != Some(pending.request_id.as_str()) {
                continue;
            }
            let verdict = d
                .get("decision")
                .and_then(Value::as_str)
                .and_then(Verdict::parse);
            let scope = d
                .get("scope")
                .and_then(Value::as_str)
                .and_then(Scope::parse)
                .unwrap_or(Scope::Once);
            if let Some(verdict) = verdict {
                view.decision_sent = Some((verdict, scope));
            }
        }
    }

    view
}

// ------------------------------------------------------------ filesystem

/// What the shell chrome needs to know at a glance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub runtime_present: bool,
    /// Tasks not yet finished or failed.
    pub live: usize,
    pub waiting: usize,
    pub blocked: usize,
}

impl Summary {
    pub fn of(runtime_present: bool, tasks: &[TaskView]) -> Self {
        Self {
            runtime_present,
            live: tasks.iter().filter(|t| !t.status.is_terminal()).count(),
            waiting: tasks
                .iter()
                .filter(|t| t.status == Status::WaitingApproval && t.decision_sent.is_none())
                .count(),
            blocked: tasks.iter().filter(|t| t.status == Status::Blocked).count(),
        }
    }

    pub fn needs_attention(&self) -> bool {
        self.waiting + self.blocked > 0
    }
}

pub fn root_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(PathBuf::from(base).join("solyra-agent"))
}

/// True when an agent runtime has set up its directory. Absence is the honest
/// "no runtime" signal; the panel never creates this directory itself.
pub fn runtime_present(root: &Path) -> bool {
    root.join("tasks").is_dir()
}

fn read_tail(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    let mut skipped_to_mid_line = false;
    if len > MAX_LOG_BYTES {
        file.seek(SeekFrom::Start(len - MAX_LOG_BYTES))?;
        skipped_to_mid_line = true;
    }
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if skipped_to_mid_line {
        // Drop the cut-off first line.
        if let Some(nl) = text.find('\n') {
            text.drain(..=nl);
        }
    }
    Ok(text)
}

pub struct Snapshot {
    pub runtime_present: bool,
    pub tasks: Vec<TaskView>,
    /// Task files that could not be read, with the reason.
    pub read_errors: Vec<String>,
}

pub fn load(root: &Path, now: u64) -> Snapshot {
    let tasks_dir = root.join("tasks");
    let mut snap = Snapshot {
        runtime_present: runtime_present(root),
        tasks: Vec::new(),
        read_errors: Vec::new(),
    };
    if !snap.runtime_present {
        return snap;
    }
    let entries = match fs::read_dir(&tasks_dir) {
        Ok(e) => e,
        Err(err) => {
            snap.read_errors
                .push(format!("{}: {err}", tasks_dir.display()));
            return snap;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        if !is_valid_id(&id) {
            snap.read_errors
                .push(format!("ignored task file with unsafe name: {id}"));
            continue;
        }
        let events = match read_tail(&path) {
            Ok(t) => t,
            Err(err) => {
                snap.read_errors.push(format!("{id}: {err}"));
                continue;
            }
        };
        let decisions =
            read_tail(&root.join("decisions").join(format!("{id}.jsonl"))).unwrap_or_default();
        let view = reduce(&id, &events, &decisions);
        if view.status.is_terminal() && now.saturating_sub(view.last_ts) > TERMINAL_VISIBLE_SECS {
            continue;
        }
        snap.tasks.push(view);
    }
    snap.tasks
        .sort_by_key(|t| (t.status.urgency(), std::cmp::Reverse(t.last_ts)));
    snap
}

/// Append one decision for the runtime to read. Returns the error to the
/// caller: a decision that was not recorded must not look recorded.
pub fn record_decision(
    root: &Path,
    task_id: &str,
    request_id: &str,
    verdict: Verdict,
    scope: Scope,
    now: u64,
) -> io::Result<()> {
    if !is_valid_id(task_id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "task id contains unsafe characters",
        ));
    }
    if !runtime_present(root) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no agent runtime directory",
        ));
    }
    let dir = root.join("decisions");
    fs::create_dir_all(&dir)?;
    let line = serde_json::json!({
        "ts": now,
        "request_id": request_id,
        "decision": verdict.as_str(),
        "scope": scope.as_str(),
        "source": "solyra-panel",
    });
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{task_id}.jsonl")))?;
    // One write call so a concurrent reader never sees half a line.
    file.write_all(format!("{line}\n").as_bytes())?;
    file.sync_data()
}

/// Longest reply the panel will send.
pub const MAX_REPLY_CHARS: usize = 2000;

/// Text typed by the human, made safe to hand to a program or a pty: control
/// characters (escape sequences, newlines, NUL) are dropped, length is capped.
/// The receiver adds the Enter itself.
pub fn sanitize_reply(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .take(MAX_REPLY_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Answer a question: appended to the decisions log as `answer`.
pub fn record_answer(
    root: &Path,
    task_id: &str,
    request_id: &str,
    text: &str,
    now: u64,
) -> io::Result<()> {
    let text = sanitize_reply(text);
    if text.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty answer"));
    }
    append_line(
        root,
        "decisions",
        task_id,
        serde_json::json!({
            "ts": now, "request_id": request_id, "decision": "answer",
            "text": text, "source": "solyra-panel",
        }),
    )
}

/// Send a message to a running agent: appended to `messages/<id>.jsonl`, which
/// `solyra-agent wrap` types into the agent's terminal. Only meaningful for a
/// task that reported `interactive`.
pub fn send_message(root: &Path, task_id: &str, text: &str, now: u64) -> io::Result<()> {
    let text = sanitize_reply(text);
    if text.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty message"));
    }
    append_line(
        root,
        "messages",
        task_id,
        serde_json::json!({"ts": now, "text": text, "source": "solyra-panel"}),
    )
}

fn append_line(root: &Path, dir: &str, task_id: &str, line: Value) -> io::Result<()> {
    if !is_valid_id(task_id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "task id contains unsafe characters",
        ));
    }
    if !runtime_present(root) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no agent runtime directory",
        ));
    }
    let dir = root.join(dir);
    fs::create_dir_all(&dir)?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{task_id}.jsonl")))?;
    file.write_all(format!("{line}\n").as_bytes())?;
    file.sync_data()
}

/// Write a draft task request. The runtime decides what to do with it; the
/// panel only carries the user's words. Returns the file written.
pub fn submit_request(root: &Path, objective: &str, now: u64) -> io::Result<PathBuf> {
    let objective = objective.trim();
    if objective.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty objective",
        ));
    }
    if !runtime_present(root) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no agent runtime directory",
        ));
    }
    let dir = root.join("requests");
    fs::create_dir_all(&dir)?;
    let body = serde_json::json!({
        "ts": now,
        "objective": objective,
        "source": "launcher",
    });
    // `create_new` so two requests in the same second never overwrite.
    for n in 0..1000u32 {
        let path = dir.join(format!("{now}-{n}.json"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                f.write_all(format!("{body}\n").as_bytes())?;
                f.sync_data()?;
                return Ok(path);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "too many requests in one second",
    ))
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(lines: &[&str]) -> String {
        let mut s = lines.join("\n");
        s.push('\n');
        s
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "solyra-agent-test-{tag}-{}-{}",
            std::process::id(),
            now_secs()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("tasks")).unwrap();
        dir
    }

    #[test]
    fn started_and_milestones() {
        let log = ev(&[
            r#"{"ts":100,"type":"started","name":"Tidy Downloads","objective":"Sort by type","environment":"level-1 sandbox"}"#,
            r#"{"ts":105,"type":"milestone","text":"Reading directory"}"#,
            r#"{"ts":110,"type":"milestone","text":"Preparing moves"}"#,
        ]);
        let v = reduce("t1", &log, "");
        assert_eq!(v.name, "Tidy Downloads");
        assert_eq!(v.objective.as_deref(), Some("Sort by type"));
        assert_eq!(v.environment.as_deref(), Some("level-1 sandbox"));
        assert_eq!(v.status, Status::Running);
        assert_eq!(v.milestones.len(), 2);
        assert_eq!(v.latest_milestone().unwrap().text, "Preparing moves");
        assert_eq!(v.last_ts, 110);
    }

    #[test]
    fn environment_is_never_invented() {
        let v = reduce("t", &ev(&[r#"{"ts":1,"type":"started","name":"x"}"#]), "");
        assert_eq!(v.environment, None);
    }

    #[test]
    fn approval_pending_then_resolved() {
        let req = r#"{"ts":120,"type":"approval_request","request_id":"r1","action":"Delete 3 files","resource":"~/Downloads/old","reversible":false,"scopes":["once","task"]}"#;
        let v = reduce(
            "t",
            &ev(&[r#"{"ts":100,"type":"started","name":"x"}"#, req]),
            "",
        );
        assert_eq!(v.status, Status::WaitingApproval);
        let p = v.pending.expect("pending approval");
        assert_eq!(p.action, "Delete 3 files");
        assert_eq!(p.reversible, Some(false));
        assert_eq!(p.scopes, vec![Scope::Once, Scope::Task]);

        let resolved =
            r#"{"ts":130,"type":"approval_resolved","request_id":"r1","decision":"approve"}"#;
        let v = reduce("t", &ev(&[req, resolved]), "");
        assert_eq!(v.status, Status::Running);
        assert!(v.pending.is_none());
    }

    #[test]
    fn missing_reversibility_stays_unknown() {
        let req = r#"{"ts":1,"type":"approval_request","request_id":"r","action":"Run script"}"#;
        let p = reduce("t", &ev(&[req]), "").pending.unwrap();
        assert_eq!(p.reversible, None);
        assert_eq!(p.scopes, vec![Scope::Once]);
        assert_eq!(p.resource, None);
    }

    #[test]
    fn resolving_a_different_request_leaves_pending() {
        let log = ev(&[
            r#"{"ts":1,"type":"approval_request","request_id":"r2","action":"A"}"#,
            r#"{"ts":2,"type":"approval_resolved","request_id":"r1"}"#,
        ]);
        let v = reduce("t", &log, "");
        assert_eq!(v.status, Status::WaitingApproval);
        assert_eq!(v.pending.unwrap().request_id, "r2");
    }

    #[test]
    fn recorded_decision_is_shown_until_acknowledged() {
        let req = r#"{"ts":1,"type":"approval_request","request_id":"r1","action":"A"}"#;
        let dec = r#"{"ts":2,"request_id":"r1","decision":"deny","scope":"once"}"#;
        let v = reduce("t", &ev(&[req]), &ev(&[dec]));
        assert_eq!(v.decision_sent, Some((Verdict::Deny, Scope::Once)));
        assert_eq!(v.status, Status::WaitingApproval, "runtime has not acked");
    }

    #[test]
    fn decision_for_another_request_is_ignored() {
        let req = r#"{"ts":1,"type":"approval_request","request_id":"r2","action":"A"}"#;
        let dec = r#"{"ts":2,"request_id":"r1","decision":"approve"}"#;
        assert_eq!(reduce("t", &ev(&[req]), &ev(&[dec])).decision_sent, None);
    }

    #[test]
    fn blocked_clears_on_progress() {
        let log = ev(&[
            r#"{"ts":1,"type":"blocked","text":"Need a path"}"#,
            r#"{"ts":2,"type":"milestone","text":"Resumed"}"#,
        ]);
        let v = reduce("t", &log, "");
        assert_eq!(v.status, Status::Running);
        assert_eq!(v.detail, None);
        let v = reduce(
            "t",
            &ev(&[r#"{"ts":1,"type":"blocked","text":"Need a path"}"#]),
            "",
        );
        assert_eq!(v.status, Status::Blocked);
        assert_eq!(v.detail.as_deref(), Some("Need a path"));
    }

    #[test]
    fn terminal_states_clear_pending() {
        let log = ev(&[
            r#"{"ts":1,"type":"approval_request","request_id":"r","action":"A"}"#,
            r#"{"ts":2,"type":"failed","error":"sandbox exited"}"#,
        ]);
        let v = reduce("t", &log, "");
        assert_eq!(v.status, Status::Failed);
        assert!(v.pending.is_none());
        assert_eq!(v.detail.as_deref(), Some("sandbox exited"));
    }

    #[test]
    fn partial_last_line_is_not_an_error() {
        let log = format!(
            "{}{}",
            ev(&[r#"{"ts":1,"type":"started","name":"x"}"#]),
            r#"{"ts":2,"type":"mile"#
        );
        let v = reduce("t", &log, "");
        assert_eq!(v.malformed_lines, 0);
        assert_eq!(v.name, "x");
    }

    #[test]
    fn corrupt_middle_line_is_counted_and_skipped() {
        let log = ev(&[
            r#"{"ts":1,"type":"started","name":"x"}"#,
            "not json at all",
            r#"{"ts":3,"type":"milestone","text":"after"}"#,
        ]);
        let v = reduce("t", &log, "");
        assert_eq!(v.malformed_lines, 1);
        assert_eq!(v.milestones.len(), 1);
    }

    #[test]
    fn unknown_event_types_are_counted_not_dropped_silently() {
        let log = ev(&[r#"{"ts":1,"type":"telepathy"}"#]);
        assert_eq!(reduce("t", &log, "").unknown_events, 1);
    }

    #[test]
    fn approval_missing_required_fields_is_malformed() {
        let log = ev(&[r#"{"ts":1,"type":"approval_request","request_id":"r"}"#]);
        let v = reduce("t", &log, "");
        assert_eq!(v.malformed_lines, 1);
        assert!(v.pending.is_none());
        assert_eq!(v.status, Status::Running);
    }

    #[test]
    fn control_characters_and_length_are_clipped() {
        let long = "a".repeat(2000);
        let log =
            format!("{{\"ts\":1,\"type\":\"milestone\",\"text\":\"x\\u001b[31m\\ny{long}\"}}\n");
        let v = reduce("t", &log, "");
        let text = &v.milestones[0].text;
        assert!(!text.chars().any(char::is_control));
        assert!(text.chars().count() <= MAX_FIELD_CHARS);
    }

    #[test]
    fn quiet_flag_applies_only_to_live_tasks() {
        let v = reduce("t", &ev(&[r#"{"ts":100,"type":"started","name":"x"}"#]), "");
        assert!(!v.is_quiet(100 + QUIET_AFTER_SECS));
        assert!(v.is_quiet(100 + QUIET_AFTER_SECS + 1));
        let done = reduce("t", &ev(&[r#"{"ts":100,"type":"finished"}"#]), "");
        assert!(!done.is_quiet(1_000_000));
    }

    #[test]
    fn summary_counts_only_what_is_real() {
        assert_eq!(Summary::of(false, &[]), Summary::default());
        let req = r#"{"ts":1,"type":"approval_request","request_id":"r","action":"A"}"#;
        let waiting = reduce("a", &ev(&[req]), "");
        let answered = reduce(
            "b",
            &ev(&[req]),
            &ev(&[r#"{"ts":2,"request_id":"r","decision":"deny"}"#]),
        );
        let done = reduce("c", &ev(&[r#"{"ts":1,"type":"finished"}"#]), "");
        let s = Summary::of(true, &[waiting, answered, done]);
        assert_eq!((s.live, s.waiting, s.blocked), (2, 1, 0));
        assert!(s.needs_attention());
    }

    #[test]
    fn question_becomes_a_pending_ask_with_choices() {
        let q = r#"{"ts":1,"type":"question","request_id":"q1","text":"Which branch?","choices":["main","dev"]}"#;
        let v = reduce("t", &ev(&[q]), "");
        let p = v.pending.expect("pending");
        assert_eq!(p.kind, AskKind::Question);
        assert_eq!(p.action, "Which branch?");
        assert_eq!(p.choices, vec!["main", "dev"]);
        assert_eq!(v.status, Status::WaitingApproval);
        let done = r#"{"ts":2,"type":"question_resolved","request_id":"q1"}"#;
        assert!(reduce("t", &ev(&[q, done]), "").pending.is_none());
    }

    #[test]
    fn started_reports_agent_and_interactive_only_when_stated() {
        let v = reduce(
            "t",
            &ev(&[r#"{"ts":1,"type":"started","name":"x","agent":"claude","interactive":true}"#]),
            "",
        );
        assert_eq!(v.agent.as_deref(), Some("claude"));
        assert!(v.interactive);
        let v = reduce("t", &ev(&[r#"{"ts":1,"type":"started","name":"x"}"#]), "");
        assert!(!v.interactive && v.agent.is_none());
    }

    #[test]
    fn replies_are_sanitised() {
        assert_eq!(sanitize_reply("  hi\x1b[31m there\n\r"), "hi[31m there");
        assert_eq!(sanitize_reply("\x07\x00"), "");
        assert_eq!(
            sanitize_reply(&"a".repeat(5000)).chars().count(),
            MAX_REPLY_CHARS
        );
    }

    #[test]
    fn answers_and_messages_round_trip() {
        let root = scratch("ans");
        let q = r#"{"ts":1,"type":"question","request_id":"q1","text":"Name?"}"#;
        fs::write(root.join("tasks/t1.jsonl"), ev(&[q])).unwrap();
        record_answer(&root, "t1", "q1", "  Ada\n", 5).unwrap();
        let snap = load(&root, 10);
        assert_eq!(
            snap.tasks[0].decision_sent,
            Some((Verdict::Answer, Scope::Once))
        );
        assert!(record_answer(&root, "t1", "q1", "\x1b", 5).is_err());
        send_message(&root, "t1", "keep going", 6).unwrap();
        let raw = fs::read_to_string(root.join("messages/t1.jsonl")).unwrap();
        assert!(raw.contains("keep going") && raw.contains("solyra-panel"));
        assert!(send_message(&root, "../x", "hi", 6).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn status_events_switch_between_working_and_idle_but_not_over_a_question() {
        let s = |st: &str| format!(r#"{{"ts":2,"type":"status","state":"{st}"}}"#);
        let start = r#"{"ts":1,"type":"started","name":"x"}"#;
        assert_eq!(
            reduce("t", &ev(&[start, &s("idle")]), "").status,
            Status::Idle
        );
        assert_eq!(
            reduce("t", &ev(&[start, &s("idle"), &s("working")]), "").status,
            Status::Running
        );
        let q = r#"{"ts":2,"type":"question","request_id":"q","text":"?"}"#;
        assert_eq!(
            reduce("t", &ev(&[start, q, &s("idle")]), "").status,
            Status::WaitingApproval
        );
        assert_eq!(
            reduce("t", &ev(&[start, &s("sideways")]), "").unknown_events,
            1
        );
    }

    #[test]
    fn id_validation() {
        assert!(is_valid_id("task-1_a"));
        assert!(!is_valid_id(""));
        assert!(!is_valid_id("../etc"));
        assert!(!is_valid_id("a/b"));
        assert!(!is_valid_id(&"x".repeat(65)));
    }

    #[test]
    fn no_runtime_means_empty_snapshot_and_no_directories_created() {
        let root = std::env::temp_dir().join(format!("solyra-agent-none-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let snap = load(&root, 0);
        assert!(!snap.runtime_present);
        assert!(snap.tasks.is_empty());
        assert!(!root.exists(), "the panel must not create the runtime dir");
        assert!(record_decision(&root, "t", "r", Verdict::Approve, Scope::Once, 1).is_err());
        assert!(submit_request(&root, "do a thing", 1).is_err());
        assert!(!root.exists());
    }

    #[test]
    fn load_sorts_by_urgency_and_hides_old_terminal_tasks() {
        let root = scratch("load");
        let w = |name: &str, body: &str| {
            fs::write(root.join("tasks").join(name), body).unwrap();
        };
        let now = 100_000;
        w(
            "run.jsonl",
            &ev(&[r#"{"ts":99990,"type":"started","name":"run"}"#]),
        );
        w(
            "ask.jsonl",
            &ev(&[
                r#"{"ts":99000,"type":"started","name":"ask"}"#,
                r#"{"ts":99001,"type":"approval_request","request_id":"r","action":"A"}"#,
            ]),
        );
        w("old.jsonl", &ev(&[r#"{"ts":1,"type":"finished"}"#]));
        w("new.jsonl", &ev(&[r#"{"ts":99999,"type":"finished"}"#]));
        w("bad name.jsonl", "x");
        let snap = load(&root, now);
        let ids: Vec<&str> = snap.tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["ask", "run", "new"]);
        assert_eq!(
            snap.read_errors.len(),
            1,
            "unsafe name reported: {:?}",
            snap.read_errors
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn decision_round_trip_through_the_filesystem() {
        let root = scratch("decide");
        let req = r#"{"ts":1,"type":"approval_request","request_id":"r1","action":"A","scopes":["once","task"]}"#;
        fs::write(root.join("tasks/t1.jsonl"), ev(&[req])).unwrap();
        record_decision(&root, "t1", "r1", Verdict::Approve, Scope::Task, 5).unwrap();
        let snap = load(&root, 10);
        assert_eq!(
            snap.tasks[0].decision_sent,
            Some((Verdict::Approve, Scope::Task))
        );
        let raw = fs::read_to_string(root.join("decisions/t1.jsonl")).unwrap();
        assert_eq!(raw.lines().count(), 1);
        assert!(raw.contains("\"source\":\"solyra-panel\""));
        assert!(record_decision(&root, "../x", "r", Verdict::Deny, Scope::Once, 1).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn requests_never_overwrite_each_other() {
        let root = scratch("req");
        let a = submit_request(&root, "first", 7).unwrap();
        let b = submit_request(&root, "second", 7).unwrap();
        assert_ne!(a, b);
        assert!(submit_request(&root, "   ", 7).is_err());
        assert!(fs::read_to_string(a).unwrap().contains("first"));
        let _ = fs::remove_dir_all(&root);
    }
}
