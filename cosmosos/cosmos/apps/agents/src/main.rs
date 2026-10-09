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

use cosmos_kit::controls::{button, ButtonKind};
use cosmos_kit::layout::{
    card, group, sheet, sidebar_item, sidebar_section, status_text, toolbar_title, AppWindow,
    ColAlign, Column, Table,
};
use cosmos_kit::{Icon, Kit};
use cosmos_theme::space;

#[derive(Default)]
struct AgentPolicy {
    name: String,
    has_policy_file: bool,
    read_roots: Vec<String>,
    write_roots: Vec<String>,
    tools: Vec<String>,
    sensitive: Vec<String>,
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
    snapshots: Vec<(u32, String)>,
    status: String,
    /// Sidebar's System → Rollback pane instead of an agent.
    rollback_pane: bool,
    confirm_rollback: Option<u32>,
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
    let out = Command::new("snapper")
        .args(["-c", "root", "--machine-readable", "csv", "list"])
        .output();
    let Ok(out) = out else {
        return; // snapper not installed — UI shows the empty state
    };
    if !out.status.success() {
        return;
    }
    for line in String::from_utf8_lossy(&out.stdout).lines().skip(1) {
        let mut f = line.split(',');
        if let (Some(n), Some(desc)) = (f.next(), f.next().or(f.next())) {
            if let Ok(num) = n.parse::<u32>() {
                st.snapshots.push((num, desc.to_string()));
            }
        }
    }
}

fn rollback_to(num: u32) -> Result<String, String> {
    // pkexec so the app doesn't need to run as root; the image ships a
    // polkit rule for the cosmos user (image-side wiring).
    let out = Command::new("pkexec")
        .args(["snapper", "-c", "root", "rollback", &num.to_string()])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(format!("Rolled back to snapshot {num}"))
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
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

    if let Some(num) = st.confirm_rollback {
        let mut done = false;
        let ctx = ui.ctx().clone();
        let open = sheet(
            &ctx,
            egui::Id::new("agents-rollback"),
            "Roll Back System?",
            |ui| {
                ui.label(
                egui::RichText::new(format!(
                    "The system will return to snapshot #{num} on the next boot. Changes made since then are kept in a new snapshot."
                ))
                .color(kit.text2()),
            );
                ui.add_space(space::S16);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if button(ui, ButtonKind::Destructive, "Roll Back").clicked() {
                        st.status = match rollback_to(num) {
                            Ok(m) => m,
                            Err(e) => format!("rollback failed: {e}"),
                        };
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
    let policy = if a.has_policy_file {
        "Custom policy"
    } else {
        "Default policy — screenshots and file writes ask first"
    };
    ui.label(egui::RichText::new(policy).size(12.0).color(kit.text2()));
    ui.add_space(space::S12);
    let fields: [(&str, &Vec<String>); 4] = [
        ("Can read", &a.read_roots),
        ("Can write", &a.write_roots),
        ("Allowed tools", &a.tools),
        ("Asks first", &a.sensitive),
    ];
    if fields.iter().any(|(_, v)| !v.is_empty()) {
        group(ui, Some("Permissions"), |g| {
            for (label, v) in fields {
                if !v.is_empty() {
                    g.row(label, |ui| {
                        ui.label(egui::RichText::new(v.join(", ")).color(kit.text2()));
                    });
                }
            }
        });
        ui.add_space(space::S20);
    }
    ui.label(egui::RichText::new("Activity").strong().color(kit.text()));
    ui.add_space(space::S8);
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
                width: 148.0,
                align: ColAlign::Left,
            },
            Column {
                title: "Tool",
                width: 160.0,
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
        for r in st.audit.iter().rev() {
            let ts = r.ts.get(..19).unwrap_or(&r.ts).replace('T', " ");
            let res = if r.ok { "Allowed" } else { "Denied" };
            t.row(ui, false, None, &[&ts, &r.tool, res, &r.detail]);
        }
    });
}

fn rollback_pane(ui: &mut egui::Ui, kit: &Kit, st: &mut State) {
    ui.label(
        egui::RichText::new("Btrfs snapshots of the system volume, taken by snapper before agent and package changes.")
            .size(12.0)
            .color(kit.text2()),
    );
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
    group(ui, Some("Snapshots"), |g| {
        for (num, desc) in st.snapshots.iter().rev() {
            let label = format!("#{num}");
            let desc = if desc.is_empty() {
                "—"
            } else {
                desc.as_str()
            };
            g.row_detail(&label, Some(desc), |ui| {
                if button(ui, ButtonKind::Secondary, "Roll Back…").clicked() {
                    pick = Some(*num);
                }
            });
        }
    });
    if pick.is_some() {
        st.confirm_rollback = pick;
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let mut st = State::default();
    refresh_agents(&mut st);
    refresh_snapshots(&mut st);
    let mut last_tick = std::time::Instant::now() - std::time::Duration::from_secs(4);
    if let Err(e) = cosmos_uitk::run("Agents", "cosmos.agents", (760, 520), move |ui| {
        if last_tick.elapsed() >= std::time::Duration::from_secs(3) {
            refresh_agents(&mut st);
            if let Some(sel) = st.selected.clone() {
                st.audit_agent.clear();
                refresh_audit(&mut st, &sel);
            }
            last_tick = std::time::Instant::now();
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(1500));
        draw(ui, &mut st);
    }) {
        tracing::error!("cosmos-agents fatal: {e}");
        std::process::exit(1);
    }
}
