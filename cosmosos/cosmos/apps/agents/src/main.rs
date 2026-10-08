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

use egui::Color32;

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
                ts: v.get("ts").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                tool: v.get("tool").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                ok: v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false),
                detail: v
                    .get("error")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string(),
            });
            if st.audit.len() >= 200 {
                break 'outer;
            }
        }
    }
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
    egui::Frame::NONE
        .inner_margin(egui::Margin::same(6)) // 10px window margin + 6 → 16px padding
        .show(ui, |ui| {
    ui.columns(2, |cols| {
        // ——— Agent list ———
        let left = &mut cols[0];
        left.heading("Agents");
        left.add_space(4.0);
        for a in &st.agents {
            let selected = st.selected.as_deref() == Some(a.name.as_str());
            let label = if a.has_policy_file {
                a.name.clone()
            } else {
                format!("{}  (default policy)", a.name)
            };
            if left.selectable_label(selected, label).clicked() {
                st.selected = Some(a.name.clone());
                st.audit_agent.clear();
            }
        }
        if st.agents.is_empty() {
            left.weak("No agents yet — policies live in\n~/.config/cosmos/agents/<name>.toml");
        }
        left.separator();
        if let Some(sel) = &st.selected {
            if let Some(a) = st.agents.iter().find(|a| &a.name == sel) {
                left.label(egui::RichText::new("POLICY").weak().size(11.0));
                if !a.read_roots.is_empty() {
                    left.monospace(format!("read:  {}", a.read_roots.join(", ")));
                }
                if !a.write_roots.is_empty() {
                    left.monospace(format!("write: {}", a.write_roots.join(", ")));
                }
                if !a.tools.is_empty() {
                    left.monospace(format!("tools: {}", a.tools.join(", ")));
                }
                if !a.sensitive.is_empty() {
                    left.colored_label(
                        Color32::from_rgb(0xE0, 0x8A, 0x2F),
                        format!("sensitive: {}", a.sensitive.join(", ")),
                    );
                }
            }
        }

        // ——— Detail: audit feed + rollback ———
        let right = &mut cols[1];
        right.heading("Activity");
        right.add_space(4.0);
        if let Some(sel) = st.selected.clone() {
            refresh_audit(st, &sel);
            egui::ScrollArea::vertical()
                .id_salt("audit")
                .max_height(180.0)
                .show(right, |ui| {
                    for r in &st.audit {
                        let color = if r.ok {
                            ui.visuals().text_color()
                        } else {
                            Color32::from_rgb(0xE0, 0x56, 0x56)
                        };
                        ui.colored_label(
                            color,
                            format!("{}  {}  {}", &r.ts[..r.ts.len().min(19)], r.tool, r.detail),
                        );
                    }
                    if st.audit.is_empty() {
                        ui.weak("No audited calls yet for this agent.");
                    }
                });
        }

        right.separator();
        right.heading("Rollback");
        right.weak("snapper snapshots (btrfs @)");
        for (num, desc) in st.snapshots.clone() {
            right.horizontal(|ui| {
                ui.monospace(format!("#{num}  {desc}"));
                if ui.button("Rollback").clicked() {
                    st.status = match rollback_to(num) {
                        Ok(m) => m,
                        Err(e) => format!("rollback failed: {e}"),
                    };
                }
            });
        }
        if st.snapshots.is_empty() {
            right.weak("No snapshots — snapper not configured on this image.");
        }
        if !st.status.is_empty() {
            right.separator();
            right.label(&st.status);
        }
    });
        });
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
    if let Err(e) = cosmos_uitk::run("Agents", "cosmos.agents", (640, 480), move |ui| {
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
