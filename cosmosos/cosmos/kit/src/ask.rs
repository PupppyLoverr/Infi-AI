//! Ask Cosmos from app context menus: runs `opencode run <prompt>` (the
//! image ships opencode; the model provider is the user's own) and
//! streams its stdout into a glass result sheet.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use egui::Ui;

use crate::controls::{button, ButtonKind};
use crate::layout::sheet;
use crate::Kit;
use cosmos_theme::space;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Summarize,
    Explain,
    Rewrite,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Summarize => "Summarize",
            Action::Explain => "Explain",
            Action::Rewrite => "Rewrite",
        }
    }
}

/// What the action is about: a file/folder on disk, or selected text.
#[derive(Clone, Debug)]
pub enum Subject {
    Path(PathBuf),
    Text(String),
}

pub fn prompt(action: Action, subject: &Subject) -> String {
    match subject {
        Subject::Path(p) => {
            let what = if p.is_dir() { "folder" } else { "file" };
            let p = p.display();
            match action {
                Action::Summarize => {
                    format!("Summarize the {what} at {p} in a few short sentences.")
                }
                Action::Explain | Action::Rewrite => format!(
                    "Explain what the {what} at {p} is and what it does, in plain language."
                ),
            }
        }
        Subject::Text(t) => match action {
            Action::Summarize => {
                format!("Summarize the following text in a few short sentences:\n\n{t}")
            }
            Action::Explain => format!("Explain the following text in plain language:\n\n{t}"),
            Action::Rewrite => format!(
                "Rewrite the following text to be clearer and more concise. \
                 Reply with only the rewritten text:\n\n{t}"
            ),
        },
    }
}

fn env_dir(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(fallback))
}

const PROVIDER_ENV: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_GENERATIVE_AI_API_KEY",
    "OPENROUTER_API_KEY",
    "GROQ_API_KEY",
    "XAI_API_KEY",
    "DEEPSEEK_API_KEY",
    "MISTRAL_API_KEY",
];

/// Whether opencode has a model provider: a provider API key in the
/// environment, credentials from `opencode auth login`, or a `provider`
/// block in the user's opencode config.
pub fn provider_configured() -> bool {
    provider_configured_in(
        &env_dir("XDG_DATA_HOME", ".local/share").join("opencode"),
        &env_dir("XDG_CONFIG_HOME", ".config").join("opencode"),
        |k| std::env::var(k).ok(),
    )
}

fn provider_configured_in(
    data: &Path,
    config: &Path,
    env: impl Fn(&str) -> Option<String>,
) -> bool {
    if PROVIDER_ENV
        .iter()
        .any(|k| env(k).is_some_and(|v| !v.trim().is_empty()))
    {
        return true;
    }
    let auth = std::fs::read_to_string(data.join("auth.json")).unwrap_or_default();
    let auth: String = auth.chars().filter(|c| !c.is_whitespace()).collect();
    if auth.starts_with("{\"") {
        return true;
    }
    ["opencode.json", "opencode.jsonc"]
        .iter()
        .any(|f| std::fs::read_to_string(config.join(f)).is_ok_and(|s| s.contains("\"provider\"")))
}

/// POSIX single-quote `s` for a `sh -c` command line.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// "Open in Agent": an interactive opencode session in a terminal, in
/// the folder of `path`.
pub fn open_in_agent(path: &Path) -> std::io::Result<()> {
    let dir = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    let cmd = format!("cd {} && opencode", shell_quote(&dir.to_string_lossy()));
    Command::new("cosmos-terminal")
        .args(["-e", &cmd])
        .spawn()
        .map(|_| ())
}

/// "Set up an agent…": the Agents app's onboarding sheet.
pub fn open_onboarding() -> std::io::Result<()> {
    Command::new("cosmos-agents")
        .arg("--onboard")
        .spawn()
        .map(|_| ())
}

#[derive(Default)]
struct Shared {
    text: String,
    done: Option<Result<(), String>>,
    child: Option<Child>,
}

/// A running (or finished) Ask Cosmos request.
pub struct Job {
    pub title: String,
    pub action: Action,
    shared: Arc<Mutex<Shared>>,
}

impl Job {
    pub fn start(ctx: &egui::Context, action: Action, subject: Subject) -> Job {
        let title = match &subject {
            Subject::Path(p) => format!(
                "{} {}",
                action.label(),
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.display().to_string())
            ),
            Subject::Text(_) => format!("{} Selection", action.label()),
        };
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (sh, ctx, q) = (shared.clone(), ctx.clone(), prompt(action, &subject));
        std::thread::spawn(move || run(&q, &sh, &ctx));
        Job {
            title,
            action,
            shared,
        }
    }

    pub fn text(&self) -> String {
        self.shared
            .lock()
            .map(|s| s.text.clone())
            .unwrap_or_default()
    }

    pub fn done(&self) -> Option<Result<(), String>> {
        self.shared.lock().ok().and_then(|s| s.done.clone())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if let Ok(mut s) = self.shared.lock() {
            if let Some(c) = s.child.as_mut() {
                let _ = c.kill();
            }
        }
    }
}

fn run(q: &str, sh: &Mutex<Shared>, ctx: &egui::Context) {
    let finish = |r: Result<(), String>| {
        if let Ok(mut s) = sh.lock() {
            s.done = Some(r);
        }
        ctx.request_repaint();
    };
    let mut child = match Command::new("opencode")
        .args(["run", q])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return finish(Err("opencode is not installed".into()))
        }
        Err(e) => return finish(Err(format!("could not start opencode: {e}"))),
    };
    let (mut out, err) = (child.stdout.take(), child.stderr.take());
    if let Ok(mut s) = sh.lock() {
        s.child = Some(child);
    }
    let err_thread = std::thread::spawn(move || {
        let mut e = String::new();
        if let Some(mut s) = err {
            let _ = s.read_to_string(&mut e);
        }
        e
    });
    let (mut buf, mut pending) = ([0u8; 1024], Vec::new());
    while let Some(Ok(n @ 1..)) = out.as_mut().map(|o| o.read(&mut buf)) {
        pending.extend_from_slice(&buf[..n]);
        let valid = match std::str::from_utf8(&pending) {
            Ok(_) => pending.len(),
            Err(e) => e.valid_up_to(),
        };
        let text = strip_ansi(&String::from_utf8_lossy(&pending[..valid]));
        pending.drain(..valid);
        if let Ok(mut s) = sh.lock() {
            s.text.push_str(&text);
        }
        ctx.request_repaint();
    }
    let status = sh
        .lock()
        .ok()
        .and_then(|mut s| s.child.as_mut().map(|c| c.wait()));
    let stderr = err_thread.join().unwrap_or_default();
    match status {
        Some(Ok(st)) if st.success() => finish(Ok(())),
        _ => {
            let msg = strip_ansi(stderr.trim());
            let msg = msg.lines().take(3).collect::<Vec<_>>().join("\n");
            finish(Err(if msg.is_empty() {
                "opencode exited without an answer".into()
            } else {
                msg
            }))
        }
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            if it.peek() == Some(&'[') {
                it.next();
                for d in it.by_ref() {
                    if ('@'..='~').contains(&d) {
                        break;
                    }
                }
            }
            continue;
        }
        if c != '\r' {
            out.push(c);
        }
    }
    out
}

#[derive(Debug, PartialEq, Eq)]
pub enum SheetResult {
    Open,
    Closed,
    /// The user chose Replace on a finished Rewrite.
    Replace(String),
}

/// The result sheet for `job`. A finished Rewrite offers Replace when
/// `replaceable` (the caller still has the selection to swap).
pub fn result_sheet(ctx: &egui::Context, job: &Job, replaceable: bool) -> SheetResult {
    let kit = Kit::get(ctx);
    let mut close = false;
    let mut replace = None;
    let open = sheet(
        ctx,
        egui::Id::new(("ask-cosmos", &job.title)),
        &job.title,
        |ui: &mut Ui| {
            let text = job.text();
            let done = job.done();
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if !text.trim().is_empty() {
                        ui.add(
                            egui::Label::new(egui::RichText::new(text.trim()).color(kit.text()))
                                .selectable(true),
                        );
                    } else if done.is_none() {
                        ui.label(egui::RichText::new("Thinking…").color(kit.text2()));
                    }
                    if let Some(Err(e)) = &done {
                        ui.label(
                            egui::RichText::new("Couldn't answer")
                                .strong()
                                .color(kit.text()),
                        );
                        ui.label(egui::RichText::new(e).color(kit.text2()));
                        ui.label(
                            egui::RichText::new(
                                "Set up a model provider in Terminal: opencode auth login",
                            )
                            .color(kit.text2()),
                        );
                    }
                });
            ui.add_space(space::S16);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let finished = matches!(done, Some(Ok(())));
                let can_replace = finished && replaceable && job.action == Action::Rewrite;
                if can_replace && button(ui, ButtonKind::Primary, "Replace").clicked() {
                    replace = Some(text.trim().to_string());
                }
                let done_kind = if can_replace {
                    ButtonKind::Secondary
                } else {
                    ButtonKind::Primary
                };
                if button(ui, done_kind, "Done").clicked() {
                    close = true;
                }
                if finished && button(ui, ButtonKind::Secondary, "Copy").clicked() {
                    ui.ctx().copy_text(text.trim().to_string());
                }
            });
        },
    );
    match replace {
        Some(t) => SheetResult::Replace(t),
        None if open && !close => SheetResult::Open,
        None => SheetResult::Closed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_detection() {
        let tmp = std::env::temp_dir().join(format!("cosmos-ask-{}", std::process::id()));
        let (data, config) = (tmp.join("data"), tmp.join("config"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        let none = |_: &str| None;
        assert!(!provider_configured_in(&data, &config, none));
        std::fs::write(data.join("auth.json"), "{ }").unwrap();
        assert!(!provider_configured_in(&data, &config, none));
        assert!(provider_configured_in(&data, &config, |k| {
            (k == "OPENROUTER_API_KEY").then(|| "sk-x".to_string())
        }));
        std::fs::write(
            data.join("auth.json"),
            "{\n \"anthropic\": {\"type\": \"api\"}}",
        )
        .unwrap();
        assert!(provider_configured_in(&data, &config, none));
        std::fs::remove_file(data.join("auth.json")).unwrap();
        std::fs::write(
            config.join("opencode.json"),
            r#"{"provider": {"ollama": {}}}"#,
        )
        .unwrap();
        assert!(provider_configured_in(&data, &config, none));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn prompts_and_quoting() {
        let t = Subject::Text("hello".into());
        assert!(prompt(Action::Rewrite, &t).ends_with("\n\nhello"));
        assert!(prompt(Action::Summarize, &Subject::Path("/tmp".into())).contains("folder at /tmp"));
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(strip_ansi("\x1b[1mhi\x1b[0m\r\n"), "hi\n");
    }
}
