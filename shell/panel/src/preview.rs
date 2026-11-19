//! Offscreen rendering of every shell surface, test-only.
//!
//! `cargo test preview` always renders the full matrix as a smoke test (no
//! panics, non-empty output). Set `SOLYRA_RENDER_DIR` to also write PNGs for
//! visual review:
//!
//! ```sh
//! SOLYRA_RENDER_DIR=/tmp/solyra-preview cargo test preview
//! ```
//!
//! Limits, so nobody mistakes this for a screenshot of the desktop: surfaces
//! are composited over a flat colour with no compositor backdrop blur, and the
//! shell has no HiDPI support yet, so everything is rendered at scale 1.

use std::collections::HashMap;

use crate::agent::{self, Summary};
use crate::canvas::{Canvas, Fonts, Rgba};
use crate::launcher;
use crate::state::{
    catalog_apps, AppEntry, DockStyle, LaunchTarget, NetworkState, ShellPrefs, Telemetry,
    WindowInfo,
};
use crate::surfaces::*;
use crate::tokens::{Theme, ThemeKind};

fn telem(battery: Option<u8>, network: NetworkState, brightness: bool) -> Telemetry {
    Telemetry {
        mem_used_mib: 412,
        mem_total_mib: 3900,
        battery_pct: battery,
        network,
        brightness_controllable: brightness,
        volume_controllable: true,
    }
}

fn win(id: u64, app: &str, title: &str, focused: bool) -> WindowInfo {
    WindowInfo {
        id,
        app_id: app.into(),
        title: title.into(),
        workspace_id: 1,
        is_focused: focused,
    }
}

fn log(lines: &[&str]) -> String {
    format!("{}\n", lines.join("\n"))
}

fn fixtures() -> Vec<agent::TaskView> {
    let now = 10_000;
    let ask = agent::reduce(
        "tidy",
        &log(&[
            r#"{"ts":9900,"type":"started","name":"Tidy Downloads","objective":"Sort ~/Downloads into folders by file type and remove exact duplicates.","environment":"Sandbox level 1 (dedicated user, no network)"}"#,
            r#"{"ts":9930,"type":"milestone","text":"Listed 214 files"}"#,
            r#"{"ts":9960,"type":"approval_request","request_id":"r1","action":"Delete 12 duplicate files","resource":"~/Downloads/dupes (12 files, 48 MB)","reason":"Byte-identical copies of files kept elsewhere","reversible":false,"scopes":["once","task"]}"#,
        ]),
        "",
    );
    let run = agent::reduce(
        "tests",
        &log(&[
            r#"{"ts":9000,"type":"started","name":"Fix flaky parser test"}"#,
            r#"{"ts":9990,"type":"milestone","text":"Running cargo test"}"#,
        ]),
        "",
    );
    let blocked = agent::reduce(
        "docs",
        &log(&[
            r#"{"ts":9970,"type":"started","name":"Draft release notes","environment":"Runs as your user, no sandbox"}"#,
            r#"{"ts":9980,"type":"blocked","text":"Needs the version number to document"}"#,
        ]),
        "",
    );
    let done = agent::reduce(
        "done",
        &log(&[
            r#"{"ts":9500,"type":"started","name":"Rename photos"}"#,
            r#"{"ts":9600,"type":"finished","summary":"Renamed 58 files by capture date"}"#,
        ]),
        "",
    );
    let failed = agent::reduce(
        "fail",
        &log(&[
            r#"{"ts":9400,"type":"started","name":"Backup configs"}"#,
            r#"{"ts":9450,"type":"failed","error":"Destination is read-only"}"#,
        ]),
        "",
    );
    let _ = now;
    vec![ask, blocked, run, failed, done]
}

struct Shot {
    name: String,
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

/// Straight RGBA over a flat backdrop, from the canvas's premultiplied BGRA.
fn flatten(px: &[u8], w: u32, h: u32, backdrop: Rgba) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for p in px.chunks_exact(4) {
        let a = p[3] as f32 / 255.0;
        let mix = |c: u8, b: u8| (c as f32 + b as f32 * (1.0 - a)).round().min(255.0) as u8;
        out.extend_from_slice(&[
            mix(p[2], backdrop.0),
            mix(p[1], backdrop.1),
            mix(p[0], backdrop.2),
            255,
        ]);
    }
    out
}

fn shoot(
    name: &str,
    kind: ThemeKind,
    w: u32,
    h: u32,
    draw: impl FnOnce(&mut Canvas<'_>, &Theme),
) -> Shot {
    let th = Theme::for_kind(kind);
    let mut buf = vec![0u8; (w * h * 4) as usize];
    {
        let mut cv = Canvas::new(&mut buf, w, h);
        draw(&mut cv, &th);
    }
    assert!(
        buf.chunks_exact(4).any(|p| p[3] > 0),
        "{name}: drew nothing"
    );
    let backdrop = match kind {
        ThemeKind::Dark => Rgba::rgb(0x2b, 0x1d, 0x4a),
        ThemeKind::Light => Rgba::rgb(0xc9, 0xc3, 0xdd),
    };
    Shot {
        name: format!("{name}-{}", kind.as_str()),
        w,
        h,
        rgba: flatten(&buf, w, h, backdrop),
    }
}

fn write_png(dir: &std::path::Path, shot: &Shot) {
    let file = std::fs::File::create(dir.join(format!("{}.png", shot.name))).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), shot.w, shot.h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&shot.rgba)
        .unwrap();
}

fn launcher_shot(
    name: &str,
    kind: ThemeKind,
    query: &str,
    selected: usize,
    runtime: bool,
    fonts: &Fonts,
    cat: &[AppEntry],
) -> Shot {
    let wins = [
        win(1, "foot", "foot — terminal notes", true),
        win(2, "firefox", "Release notes", false),
    ];
    let items = launcher::search(query, cat, &wins, runtime);
    shoot(name, kind, LAUNCHER_W, LAUNCHER_H, |cv, th| {
        let mut icons = HashMap::new();
        draw_launcher(cv, th, fonts, query, &items, selected, &mut icons, None);
    })
}

fn sample_catalog() -> Vec<AppEntry> {
    // The real catalogue includes only what is installed on this machine, so
    // previews use a fixed list that exercises every group.
    let mut cat: Vec<AppEntry> = catalog_apps()
        .into_iter()
        .filter(|a| matches!(a.command, LaunchTarget::Internal(_) | LaunchTarget::Ipc(_)))
        .collect();
    for (title, id, sub) in [
        ("Files", "thunar", "Browse local and mounted storage"),
        ("Terminal", "foot", "Command line shell"),
        ("Firefox", "firefox", "Browse the web"),
        ("Text Editor", "gedit", "Edit text files"),
    ] {
        cat.insert(
            0,
            AppEntry {
                window: None,
                window_count: 0,
                is_focused: false,
                app_id: id.into(),
                role: id.into(),
                title: title.into(),
                subtitle: sub.into(),
                command: LaunchTarget::Shell(id.into()),
            },
        );
    }
    cat
}

fn render_all() -> Vec<Shot> {
    let fonts = Fonts::load();
    let cat = sample_catalog();
    let tasks = fixtures();
    let mut shots = Vec::new();

    for kind in [ThemeKind::Dark, ThemeKind::Light] {
        shots.push(launcher_shot(
            "launcher-empty",
            kind,
            "",
            0,
            true,
            &fonts,
            &cat,
        ));
        shots.push(launcher_shot(
            "launcher-query",
            kind,
            "te",
            1,
            true,
            &fonts,
            &cat,
        ));
        shots.push(launcher_shot(
            "launcher-no-runtime",
            kind,
            "sort my downloads",
            0,
            false,
            &fonts,
            &cat,
        ));
        shots.push(launcher_shot(
            "launcher-agent-only",
            kind,
            "qwertyuiop",
            0,
            true,
            &fonts,
            &cat,
        ));
        shots.push(launcher_shot(
            "launcher-no-match",
            kind,
            "zz",
            0,
            true,
            &fonts,
            &cat,
        ));

        let mut prefs = ShellPrefs::default();
        prefs.theme = kind;
        shots.push(shoot(
            "control-laptop",
            kind,
            CONTROL_W,
            CONTROL_H,
            |cv, th| {
                let t = telem(Some(57), NetworkState::Wireless, true);
                draw_control(cv, th, &fonts, &prefs, &t, Summary::of(true, &tasks), None);
            },
        ));
        shots.push(shoot(
            "control-desktop-offline",
            kind,
            CONTROL_W,
            CONTROL_H,
            |cv, th| {
                let t = telem(None, NetworkState::Offline, false);
                draw_control(cv, th, &fonts, &prefs, &t, Summary::default(), None);
            },
        ));

        shots.push(shoot(
            "agents-no-runtime",
            kind,
            AGENTS_W,
            AGENTS_H,
            |cv, th| {
                let v = AgentsView {
                    runtime_present: false,
                    tasks: &[],
                    selected: 0,
                    now: 10_000,
                    root_hint: "/run/user/1000/solyra-agent",
                    error: None,
                };
                draw_agents(cv, th, &fonts, &v, None);
            },
        ));
        shots.push(shoot("agents-empty", kind, AGENTS_W, AGENTS_H, |cv, th| {
            let v = AgentsView {
                runtime_present: true,
                tasks: &[],
                selected: 0,
                now: 10_000,
                root_hint: "",
                error: None,
            };
            draw_agents(cv, th, &fonts, &v, None);
        }));
        shots.push(shoot(
            "agents-approval",
            kind,
            AGENTS_W,
            AGENTS_H,
            |cv, th| {
                let v = AgentsView {
                    runtime_present: true,
                    tasks: &tasks,
                    selected: 0,
                    now: 10_000,
                    root_hint: "",
                    error: None,
                };
                draw_agents(cv, th, &fonts, &v, None);
            },
        ));
        shots.push(shoot(
            "agents-approval-error",
            kind,
            AGENTS_W,
            AGENTS_H,
            |cv, th| {
                let v = AgentsView {
                    runtime_present: true,
                    tasks: &tasks,
                    selected: 0,
                    now: 10_000,
                    root_hint: "",
                    error: Some("Decision not recorded: permission denied"),
                };
                draw_agents(cv, th, &fonts, &v, None);
            },
        ));
        let mut answered = tasks.clone();
        answered[0] = agent::reduce(
            "tidy",
            &log(&[
                r#"{"ts":9960,"type":"approval_request","request_id":"r1","action":"Delete 12 duplicate files"}"#,
            ]),
            &log(&[r#"{"ts":9961,"request_id":"r1","decision":"deny","scope":"once"}"#]),
        );
        shots.push(shoot(
            "agents-decision-sent",
            kind,
            AGENTS_W,
            AGENTS_H,
            |cv, th| {
                let v = AgentsView {
                    runtime_present: true,
                    tasks: &answered,
                    selected: 0,
                    now: 10_000,
                    root_hint: "",
                    error: None,
                };
                draw_agents(cv, th, &fonts, &v, None);
            },
        ));
        for (name, sel) in [
            ("agents-blocked", 1),
            ("agents-running", 2),
            ("agents-failed", 3),
            ("agents-finished", 4),
        ] {
            shots.push(shoot(name, kind, AGENTS_W, AGENTS_H, |cv, th| {
                let v = AgentsView {
                    runtime_present: true,
                    tasks: &tasks,
                    selected: sel,
                    now: 10_000,
                    root_hint: "",
                    error: None,
                };
                draw_agents(cv, th, &fonts, &v, None);
            }));
        }

        shots.push(shoot(
            "island-card",
            kind,
            ISLAND_CARD_W,
            ISLAND_CARD_H,
            |cv, th| {
                let t = telem(None, NetworkState::Wired, false);
                draw_island_card(
                    cv,
                    th,
                    &fonts,
                    None,
                    &[win(1, "foot", "x", true)],
                    &t,
                    Summary::of(true, &tasks),
                );
            },
        ));

        for (name, width, attention) in [
            (
                "bar-attention",
                1280u32,
                Some("Approval needed · Tidy Downloads"),
            ),
            ("bar-quiet", 1280, None),
        ] {
            shots.push(shoot(name, kind, width, BAR_HEIGHT, |cv, th| {
                let t = telem(Some(57), NetworkState::Wireless, true);
                draw_bar(
                    cv,
                    th,
                    &fonts,
                    &[(1, true), (2, false), (3, false)],
                    &[win(1, "foot", "x", true)],
                    None,
                    attention,
                    &prefs,
                    &t,
                    None,
                );
            }));
        }
        let summary = Summary::of(true, &tasks);
        shots.push(shoot(
            "dock-solyra",
            kind,
            340,
            DOCK_FLOAT_HEIGHT,
            |cv, th| {
                let t = telem(None, NetworkState::Wired, false);
                let live = Live {
                    agents: summary,
                    telem: &t,
                    prefs: &prefs,
                };
                let mut icons = HashMap::new();
                draw_dock(
                    cv,
                    th,
                    &fonts,
                    DockStyle::Solyra,
                    &[win(1, "foot", "x", true)],
                    &mut icons,
                    None,
                    &live,
                );
            },
        ));
    }
    shots
}

#[test]
fn preview_matrix_renders_without_panicking() {
    let shots = render_all();
    assert!(shots.len() >= 40, "only {} shots", shots.len());
    if let Some(dir) = std::env::var_os("SOLYRA_RENDER_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for s in &shots {
            write_png(&dir, s);
        }
        eprintln!("wrote {} previews to {}", shots.len(), dir.display());
    }
}
