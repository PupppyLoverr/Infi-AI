//! cosmos-agents — the Agents app (spec §6.11).
//!
//! One window managing the machine's agents: policy files on the left
//! (`~/.config/cosmos/agents/<name>.toml`), the live audit feed for the
//! selected agent on the right (`~/.local/share/cosmos/agent-audit/`),
//! and a snapper rollback pane — the OS-level undo for agent changes.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cosmos_kit::controls::{button, segmented, ButtonKind};
use cosmos_kit::layout::{
    card, group, sheet, sidebar_item, sidebar_section, status_text, toolbar_title, AppWindow,
    ColAlign, Column, Table,
};
use cosmos_kit::{Icon, Kit};
use cosmos_theme::space;

#[derive(Default, Clone)]
struct AgentPolicy {
    name: String,
    has_policy_file: bool,
    read_roots: Vec<String>,
    write_roots: Vec<String>,
    tools: Vec<String>,
    sensitive: Vec<String>,
    /// Command that starts the agent (`command = "opencode"`).
    command: Option<String>,
}

#[derive(Clone)]
struct AuditRow {
    ts: String,
    tool: String,
    ok: bool,
    detail: String,
}

#[derive(Default)]
struct State {
    agents: Vec<AgentPolicy>,
    selected: Option<String>,
    audit: Vec<AuditRow>,
    audit_agent: String,
    /// (snapper config, number, description).
    snapshots: Vec<(&'static str, u32, String)>,
    /// Agent pane tab: 0 Overview, 1 Activity.
    tab: usize,
    status: String,
    /// Sidebar's System → Rollback pane instead of an agent.
    rollback_pane: bool,
    confirm_rollback: Option<(&'static str, u32)>,
    /// `--onboard` (Ask Cosmos → "Set up an agent…"): the provider sheet.
    onboard: bool,
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into()))
}

fn policies_dir() -> PathBuf {
    home().join(".config/cosmos/agents")
}

fn audit_dir() -> PathBuf {
    home().join(".local/share/cosmos/agent-audit")
}

/// Same permissive TOML-ish reader the agentd uses (key = scalar or
/// [array]); keeps the two parsers consistent without a toml dep.
fn parse_policy(path: &Path, name: &str) -> AgentPolicy {
    let mut p = AgentPolicy {
        name: name.to_string(),
        has_policy_file: true,
        ..Default::default()
    };
    let Ok(text) = fs::read_to_string(path) else {
        return p;
    };
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let val = val.trim();
        let list = |v: &str| -> Vec<String> {
            v.trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .filter_map(|s| {
                    let s = s.trim().trim_matches('"').trim_matches('\'');
                    if s.is_empty() {
                        None
                    } else {
                        Some(s.to_string())
                    }
                })
                .collect()
        };
        match key {
            "read_roots" => p.read_roots = list(val),
            "write_roots" => p.write_roots = list(val),
            "tools" => p.tools = list(val),
            "sensitive" => p.sensitive = list(val),
            "command" => p.command = Some(val.trim_matches('"').to_string()),
            _ => {}
        }
    }
    p
}

fn refresh_agents(st: &mut State) {
    let mut names: HashSet<String> = HashSet::new();
    if let Ok(rd) = fs::read_dir(policies_dir()) {
        for e in rd.flatten() {
            if let Some(n) = e.file_name().to_str() {
                if let Some(stripped) = n.strip_suffix(".toml") {
                    names.insert(stripped.to_string());
                }
            }
        }
    }
    // Agents that only appear in the audit feed still list (no policy file
    // yet → spec defaults apply, flagged in the detail pane).
    if let Ok(rd) = fs::read_dir(audit_dir()) {
        for e in rd.flatten() {
            if let Ok(text) = fs::read_to_string(e.path()) {
                for line in text.lines() {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                        if let Some(a) = v.get("agent").and_then(|a| a.as_str()) {
                            names.insert(a.to_string());
                        }
                    }
                }
            }
        }
    }
    let mut sorted: Vec<String> = names.into_iter().collect();
    sorted.sort();
    st.agents = sorted
        .iter()
        .map(|n| {
            let path = policies_dir().join(format!("{n}.toml"));
            if path.exists() {
                parse_policy(&path, n)
            } else {
                AgentPolicy {
                    name: n.clone(),
                    has_policy_file: false,
                    ..Default::default()
                }
            }
        })
        .collect();
    if st.selected.is_none() {
        st.selected = st.agents.first().map(|a| a.name.clone());
    }
}

fn refresh_audit(st: &mut State, agent: &str) {
    if st.audit_agent == agent {
        return;
    }
    st.audit_agent = agent.to_string();
    st.audit.clear();
    let dir = audit_dir();
    let mut days: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    days.sort();
    days.reverse();
    'outer: for day in days {
        let Ok(text) = fs::read_to_string(&day) else {
            continue;
        };
        for line in text.lines().rev() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if v.get("agent").and_then(|a| a.as_str()) != Some(agent) {
                continue;
            }
            st.audit.push(AuditRow {
                ts: v
                    .get("ts")
                    .and_then(|t| t.as_u64())
                    .map(ago)
                    .unwrap_or_default(),
                tool: v
                    .get("tool")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string(),
                ok: v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false),
                detail: v
                    .get("detail")
                    .and_then(|d| d.as_str())
                    .map(one_line)
                    .unwrap_or_default(),
            });
            if st.audit.len() >= 200 {
                break 'outer;
            }
        }
    }
}

/// agentd stamps audit entries in epoch seconds; show them relative so
/// the table needs no timezone database.
fn ago(ts: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(ts);
    match now.saturating_sub(ts) {
        0..=59 => "Just now".into(),
        s @ 60..=3599 => format!("{} min ago", s / 60),
        s @ 3600..=86_399 => format!("{} h ago", s / 3600),
        s => format!("{} d ago", s / 86_400),
    }
}

/// Audit detail on one row: results are often pretty-printed JSON, so
/// fold every line and run of whitespace into single spaces.
fn one_line(d: &str) -> String {
    let line = d.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = line.chars().take(80).collect();
    if line.chars().count() > 80 {
        out.push('…');
    }
    out
}

fn refresh_snapshots(st: &mut State) {
    st.snapshots.clear();
    for cfg in ["home", "root"] {
        let Ok(out) = Command::new("snapper")
            .args(["-c", cfg, "--machine-readable", "csv", "list"])
            .args(["--columns", "number,description"])
            .output()
        else {
            return; // snapper not installed — UI shows the empty state
        };
        if !out.status.success() {
            continue;
        }
        for line in String::from_utf8_lossy(&out.stdout).lines().skip(1) {
            let Some((n, desc)) = line.split_once(',') else {
                continue;
            };
            if let Ok(num @ 1..) = n.parse::<u32>() {
                let desc = desc.trim_matches('"').replace("\"\"", "\"");
                st.snapshots.push((cfg, num, desc));
            }
        }
    }
}

fn take_snapshot() -> String {
    let out = Command::new("snapper")
        .args(["-c", "home", "create", "--cleanup-algorithm", "number"])
        .args(["--description", "Manual snapshot", "--print-number"])
        .output();
    match out {
        Ok(o) if o.status.success() => format!(
            "took snapshot #{}",
            String::from_utf8_lossy(&o.stdout).trim()
        ),
        Ok(o) => format!(
            "snapshot failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => format!("snapper unavailable: {e}"),
    }
}

/// Files under /home changed since snapshot `num`, minus hidden paths:
/// a restore brings back documents without rewinding app settings,
/// caches or the agent audit log itself.
fn home_changes(num: u32) -> Result<Vec<String>, String> {
    let out = Command::new("snapper")
        .args(["-c", "home", "status", &format!("{num}..0")])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(visible_changes(&String::from_utf8_lossy(&out.stdout)))
}

fn visible_changes(status: &str) -> Vec<String> {
    status
        .lines()
        .filter_map(|l| l.split_once(' ').map(|(_, p)| p.trim().to_string()))
        .filter(|p| p.starts_with('/') && !p.split('/').any(|c| c.starts_with('.')))
        .collect()
}

fn rollback_to(cfg: &str, num: u32) -> Result<String, String> {
    // pkexec so the app doesn't need to run as root; the image ships a
    // polkit rule for the cosmos user (image-side wiring).
    let mut cmd = Command::new("pkexec");
    if cfg == "home" {
        let files = home_changes(num)?;
        if files.is_empty() {
            return Ok(format!("nothing changed since snapshot #{num}"));
        }
        cmd.args(["snapper", "-c", "home", "undochange", &format!("{num}..0")])
            .args(&files);
    } else {
        cmd.args(["snapper", "-c", "root", "rollback", &num.to_string()]);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(if cfg == "home" {
        format!("restored your files to snapshot #{num}")
    } else {
        format!("the system boots into snapshot #{num} next time")
    })
}

fn avatar_colour(name: &str) -> egui::Color32 {
    let h = name
        .bytes()
        .fold(0x811c_9dc5_u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193));
    egui::ecolor::Hsva::new((h % 360) as f32 / 360.0, 0.55, 0.78, 1.0).into()
}

/// Rounded colour square with a white sparkle: the agent's avatar.
fn paint_avatar(ui: &egui::Ui, centre: egui::Pos2, size: f32, name: &str) {
    let r = egui::Rect::from_center_size(centre, egui::vec2(size, size));
    ui.painter().rect_filled(r, size * 0.28, avatar_colour(name));
    cosmos_kit::icons::paint(ui, Icon::Sparkle, r.shrink(size * 0.22), egui::Color32::WHITE);
}

fn tool_label(tool: &str) -> String {
    match tool {
        "files.read" => "Read file",
        "files.write" => "Write file",
        "files.move" => "Move file",
        "files.search" => "Search files",
        "desktop.windows.list" => "List windows",
        "desktop.windows.focus" => "Focus window",
        "desktop.windows.close" => "Close window",
        "desktop.windows.snap" => "Snap window",
        "desktop.windows.move_workspace" => "Move window",
        "desktop.workspaces.list" => "List workspaces",
        "desktop.workspaces.switch" => "Switch workspace",
        "desktop.launch" => "Launch app",
        "desktop.screenshot" => "Screenshot",
        "system.settings.get" => "Read settings",
        "system.settings.set" => "Change setting",
        other => return other.to_string(),
    }
    .to_string()
}

fn result_label(r: &AuditRow) -> &'static str {
    let d = r.detail.to_lowercase();
    match () {
        _ if r.ok => "Done",
        _ if ["denied", "outside", "stopped", "not allowed"]
            .iter()
            .any(|w| d.contains(w)) =>
        {
            "Denied"
        }
        _ => "Failed",
    }
}

/// agentd's live file for a running agent (`Live::write`).
struct LiveState {
    tool: String,
    busy: bool,
    calls: u64,
    started: u64,
}

fn read_live(agent: &str) -> Option<LiveState> {
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
    let path = home()
        .join(".local/state/cosmos/agent-live")
        .join(format!("{safe}.json"));
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    let pid = v.get("pid")?.as_u64()?;
    if !Path::new(&format!("/proc/{pid}")).exists() {
        return None; // left behind by a killed agent
    }
    Some(LiveState {
        tool: v["tool"].as_str().unwrap_or("").to_string(),
        busy: v["busy"].as_bool().unwrap_or(false),
        calls: v["calls"].as_u64().unwrap_or(0),
        started: v["started"].as_u64().unwrap_or(0),
    })
}

fn expand_home(s: &str) -> PathBuf {
    match s.strip_prefix('~') {
        Some("") => home(),
        Some(rest) if rest.starts_with('/') => home().join(&rest[1..]),
        _ => PathBuf::from(s),
    }
}

fn start_agent(a: &AgentPolicy, cmd: &str) -> String {
    let dir = a
        .read_roots
        .first()
        .map(|r| expand_home(r))
        .filter(|d| d.is_dir())
        .unwrap_or_else(home);
    match Command::new("cosmos-terminal")
        .args(["-e", cmd])
        .current_dir(dir)
        .spawn()
    {
        Ok(_) => format!("started {} in Terminal", a.name),
        Err(e) => format!("can't open Terminal: {e}"),
    }
}

fn draw(ui: &mut egui::Ui, st: &mut State) {
    let kit = Kit::get(ui.ctx());
    if let Some(sel) = st.selected.clone() {
        if !st.rollback_pane {
            refresh_audit(st, &sel);
        }
    }
    let title = if st.rollback_pane {
        "Rollback".to_string()
    } else {
        st.selected.clone().unwrap_or_else(|| "Agents".into())
    };
    let cell = std::cell::RefCell::new(&mut *st);
    AppWindow::new()
        .toolbar(|ui| toolbar_title(ui, &title))
        .sidebar(|ui| {
            let mut st = cell.borrow_mut();
            let st = &mut **st;
            sidebar_section(ui, "Agents");
            let mut pick = None;
            for a in &st.agents {
                let on = !st.rollback_pane && st.selected.as_deref() == Some(a.name.as_str());
                let r = sidebar_item(ui, Icon::Sparkle, &a.name, on);
                paint_avatar(
                    ui,
                    egui::pos2(r.rect.left() + 18.0, r.rect.center().y),
                    18.0,
                    &a.name,
                );
                let r = if a.has_policy_file {
                    r
                } else {
                    r.on_hover_text("No policy file — default policy applies")
                };
                if r.clicked() {
                    pick = Some(a.name.clone());
                }
            }
            if st.agents.is_empty() {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("No agents yet")
                        .size(12.0)
                        .color(kit.text3()),
                );
            }
            if let Some(n) = pick {
                st.selected = Some(n);
                st.rollback_pane = false;
                st.audit_agent.clear();
            }
            sidebar_section(ui, "System");
            if sidebar_item(ui, Icon::Rollback, "Rollback", st.rollback_pane).clicked() {
                st.rollback_pane = true;
                refresh_snapshots(st);
            }
        })
        .status(|ui| {
            let st = cell.borrow();
            let n = st.agents.len();
            status_text(ui, &format!("{n} agent{}", if n == 1 { "" } else { "s" }));
            if !st.status.is_empty() {
                status_text(ui, &format!("· {}", st.status));
            }
        })
        .show(ui, |ui| {
            let mut st = cell.borrow_mut();
            let st = &mut **st;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(24, 20))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            if st.rollback_pane {
                                rollback_pane(ui, &kit, st);
                            } else {
                                agent_pane(ui, &kit, st);
                            }
                        });
                });
        });

    if st.onboard {
        let ctx = ui.ctx().clone();
        let mut done = false;
        let ready = cosmos_kit::ask::provider_configured();
        let open = sheet(
            &ctx,
            egui::Id::new("agents-onboard"),
            "Set Up an Agent",
            |ui| {
                ui.label(
                    egui::RichText::new(
                        "Ask Cosmos runs opencode with your own model provider. Sign in once and \
                     Summarize, Explain and Rewrite work in Files, the Editor and Search.",
                    )
                    .color(kit.text2()),
                );
                ui.add_space(space::S12);
                let (glyph, line) = if ready {
                    (Icon::Check, "A model provider is connected.")
                } else {
                    (Icon::Info, "No model provider yet.")
                };
                ui.horizontal(|ui| {
                    cosmos_kit::icons::show(ui, glyph, 16.0, kit.text());
                    ui.label(egui::RichText::new(line).color(kit.text()));
                });
                ui.add_space(space::S16);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if button(ui, ButtonKind::Primary, "Done").clicked() {
                        done = true;
                    }
                    if !ready && button(ui, ButtonKind::Secondary, "Sign In…").clicked() {
                        st.status = match Command::new("cosmos-terminal")
                            .args(["-e", "opencode auth login"])
                            .spawn()
                        {
                            Ok(_) => "opened opencode sign-in in Terminal".into(),
                            Err(e) => format!("can't open Terminal: {e}"),
                        };
                    }
                });
            },
        );
        if !open || done {
            st.onboard = false;
        }
    }

    if let Some((cfg, num)) = st.confirm_rollback {
        let mut done = false;
        let ctx = ui.ctx().clone();
        let open = sheet(
            &ctx,
            egui::Id::new("agents-rollback"),
            if cfg == "home" {
                "Restore Files?"
            } else {
                "Roll Back System?"
            },
            |ui| {
                ui.label(
                egui::RichText::new(if cfg == "home" {
                    format!("Files in your home folder go back to how they were at snapshot #{num}: deleted files come back and newer files are removed. Hidden settings folders are left alone.")
                } else {
                    format!("The system will return to snapshot #{num} on the next boot. Changes made since then are kept in a new snapshot.")
                })
                .color(kit.text2()),
            );
                ui.add_space(space::S16);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if button(ui, ButtonKind::Destructive, "Roll Back").clicked() {
                        st.status = match rollback_to(cfg, num) {
                            Ok(m) => m,
                            Err(e) => format!("rollback failed: {e}"),
                        };
                        refresh_snapshots(st);
                        done = true;
                    }
                    if button(ui, ButtonKind::Secondary, "Cancel").clicked() {
                        done = true;
                    }
                });
            },
        );
        if !open || done {
            st.confirm_rollback = None;
        }
    }
}

fn agent_pane(ui: &mut egui::Ui, kit: &Kit, st: &mut State) {
    let Some(a) = st
        .selected
        .as_ref()
        .and_then(|sel| st.agents.iter().find(|a| &a.name == sel))
        .cloned()
    else {
        let c = ui.max_rect().center_top() + egui::vec2(0.0, 120.0);
        cosmos_kit::icons::paint(
            ui,
            Icon::Sparkle,
            egui::Rect::from_center_size(c, egui::vec2(40.0, 40.0)),
            kit.text3(),
        );
        let msg = if st.agents.is_empty() {
            "Agents appear here once they connect to cosmos-agentd. Policies live in ~/.config/cosmos/agents/<name>.toml."
        } else {
            "Select an agent to see its permissions and activity."
        };
        ui.painter().text(
            c + egui::vec2(0.0, 40.0),
            egui::Align2::CENTER_CENTER,
            msg,
            egui::FontId::proportional(13.0),
            kit.text2(),
        );
        return;
    };
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
        paint_avatar(ui, r.center(), 40.0, &a.name);
        ui.add_space(space::S8);
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(&a.name)
                    .strong()
                    .size(15.0)
                    .color(kit.text()),
            );
            let policy = if a.has_policy_file {
                "Custom policy"
            } else {
                "Default policy — screenshots and file writes ask first"
            };
            ui.label(egui::RichText::new(policy).size(12.0).color(kit.text2()));
        });
    });
    ui.add_space(space::S12);
    segmented(ui, &mut st.tab, &["Overview", "Activity"]);
    ui.add_space(space::S16);
    if st.tab == 0 {
        overview(ui, kit, st, &a);
    } else {
        activity(ui, kit, st);
    }
}

fn overview(ui: &mut egui::Ui, kit: &Kit, st: &mut State, a: &AgentPolicy) {
    let live = read_live(&a.name);
    let (task, detail) = match &live {
        Some(l) => (
            if l.busy {
                format!("Running: {}", tool_label(&l.tool))
            } else {
                "Connected, waiting for its next step".to_string()
            },
            format!(
                "Started {} · {} call{}",
                ago(l.started),
                l.calls,
                if l.calls == 1 { "" } else { "s" }
            ),
        ),
        None => ("Not running".to_string(), String::new()),
    };
    group(ui, Some("Current Task"), |g| {
        g.row_detail(&task, (!detail.is_empty()).then_some(detail.as_str()), |ui| {
            if let (None, Some(cmd)) = (&live, &a.command) {
                if button(ui, ButtonKind::Secondary, "Start").clicked() {
                    st.status = start_agent(a, cmd);
                }
            }
        });
    });
    ui.add_space(space::S20);

    ui.label(egui::RichText::new("Live Log").strong().color(kit.text()));
    ui.add_space(space::S8);
    card(ui, |ui| {
        if st.audit.is_empty() {
            ui.label(egui::RichText::new("No calls yet.").color(kit.text3()));
            return;
        }
        for r in st.audit.iter().take(8) {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [72.0, 18.0],
                    egui::Label::new(egui::RichText::new(&r.ts).size(11.0).color(kit.text3()))
                        .truncate(),
                );
                ui.add_sized(
                    [104.0, 18.0],
                    egui::Label::new(egui::RichText::new(tool_label(&r.tool)).color(kit.text()))
                        .truncate(),
                );
                let res = result_label(r);
                let line = if r.detail.is_empty() {
                    res.to_string()
                } else {
                    format!("{res} · {}", r.detail)
                };
                ui.add(egui::Label::new(egui::RichText::new(line).color(kit.text2())).truncate());
            });
        }
    });
    ui.add_space(space::S20);

    let home = home().display().to_string();
    let tidy = |v: &[String], dflt: &str| {
        if v.is_empty() {
            dflt.to_string()
        } else {
            v.join(", ").replace(&home, "~")
        }
    };
    let labels = |v: &[String]| v.iter().map(|t| tool_label(t)).collect::<Vec<_>>().join(", ");
    let rows = [
        ("Can read", tidy(&a.read_roots, "~ (home folder)")),
        (
            "Can write",
            tidy(&a.write_roots, &format!("~/Agents/{}", a.name)),
        ),
        (
            "Tools",
            if a.tools.is_empty() {
                "All tools".to_string()
            } else {
                labels(&a.tools)
            },
        ),
        (
            "Asks first",
            match (a.sensitive.is_empty(), a.has_policy_file) {
                (false, _) => labels(&a.sensitive),
                (true, true) => "Nothing".to_string(),
                (true, false) => "Screenshot, Write file, Move file".to_string(),
            },
        ),
    ];
    group(ui, Some("Permissions"), |g| {
        for (label, value) in &rows {
            g.row(label, |ui| {
                ui.label(egui::RichText::new(value).color(kit.text2()));
            });
        }
    });
}

fn activity(ui: &mut egui::Ui, kit: &Kit, st: &State) {
    card(ui, |ui| {
        if st.audit.is_empty() {
            ui.label(
                egui::RichText::new("No audited calls yet for this agent.").color(kit.text3()),
            );
            return;
        }
        let cols = [
            Column {
                title: "Time",
                width: 96.0,
                align: ColAlign::Left,
            },
            Column {
                title: "Tool",
                width: 132.0,
                align: ColAlign::Left,
            },
            Column {
                title: "Result",
                width: 72.0,
                align: ColAlign::Left,
            },
            Column {
                title: "Detail",
                width: 0.0,
                align: ColAlign::Left,
            },
        ];
        let t = Table { cols: &cols };
        t.header(ui);
        ui.spacing_mut().item_spacing.y = 0.0;
        for r in &st.audit {
            let tool = tool_label(&r.tool);
            t.row(ui, false, None, &[&r.ts, &tool, result_label(r), &r.detail]);
        }
    });
}

fn rollback_pane(ui: &mut egui::Ui, kit: &Kit, st: &mut State) {
    ui.label(
        egui::RichText::new("Snapper snapshots. Your home folder is snapshotted before an agent's first change in a session; restoring one takes effect at once. System snapshots apply on the next boot.")
            .size(12.0)
            .color(kit.text2()),
    );
    ui.add_space(space::S12);
    if button(ui, ButtonKind::Secondary, "Take Snapshot").clicked() {
        st.status = take_snapshot();
        refresh_snapshots(st);
    }
    ui.add_space(space::S12);
    if st.snapshots.is_empty() {
        group(ui, None, |g| {
            g.row("No snapshots", |ui| {
                ui.label(
                    egui::RichText::new("snapper isn't configured on this system")
                        .color(kit.text3()),
                );
            });
        });
        return;
    }
    let mut pick = None;
    for (cfg, title) in [("home", "Home Folder"), ("root", "System")] {
        let rows: Vec<&(&str, u32, String)> =
            st.snapshots.iter().filter(|s| s.0 == cfg).collect();
        if rows.is_empty() {
            continue;
        }
        group(ui, Some(title), |g| {
            for s in rows.iter().rev() {
                let num = s.1;
                let desc = if s.2.is_empty() { "—" } else { s.2.as_str() };
                g.row_detail(&format!("#{num}"), Some(desc), |ui| {
                    let label = if cfg == "home" { "Restore…" } else { "Roll Back…" };
                    if button(ui, ButtonKind::Secondary, label).clicked() {
                        pick = Some((cfg, num));
                    }
                });
            }
        });
        ui.add_space(space::S16);
    }
    if pick.is_some() {
        st.confirm_rollback = pick;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_skips_hidden_paths() {
        let status = "-..... /home/cosmos/Documents/plan.md\n\
                      c..... /home/cosmos/.config/cosmos/agents/x.toml\n\
                      +..... /home/cosmos/Documents/new.txt\n\
                      c..... /home/cosmos/.local/share/cosmos/agent-audit/day-1.jsonl\n";
        assert_eq!(
            visible_changes(status),
            ["/home/cosmos/Documents/plan.md", "/home/cosmos/Documents/new.txt"]
        );
    }

    #[test]
    fn tool_labels_fall_back_to_the_raw_name() {
        assert_eq!(tool_label("files.write"), "Write file");
        assert_eq!(tool_label("custom.tool"), "custom.tool");
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let mut st = State {
        onboard: std::env::args().any(|a| a == "--onboard"),
        ..State::default()
    };
    refresh_agents(&mut st);
    refresh_snapshots(&mut st);
    let mut last_tick = std::time::Instant::now() - std::time::Duration::from_secs(4);
    if let Err(e) = cosmos_uitk::run("Agents", "cosmos.agents", (760, 520), move |ui| {
        if last_tick.elapsed() >= std::time::Duration::from_secs(1) {
            refresh_agents(&mut st);
            if let Some(sel) = st.selected.clone() {
                st.audit_agent.clear();
                refresh_audit(&mut st, &sel);
            }
            last_tick = std::time::Instant::now();
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(1000));
        draw(ui, &mut st);
    }) {
        tracing::error!("cosmos-agents fatal: {e}");
        std::process::exit(1);
    }
}
