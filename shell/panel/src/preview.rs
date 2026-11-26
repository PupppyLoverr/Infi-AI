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

fn ask_fixtures() -> Vec<agent::TaskView> {
    let typed = agent::reduce(
        "claude-1",
        &log(&[
            r#"{"ts":9900,"type":"started","name":"Refactor parser","agent":"claude","interactive":true,"environment":"Runs as your user, no sandbox"}"#,
            r#"{"ts":9990,"type":"question","request_id":"q1","text":"The config has two possible formats. Which one should the new loader read first?"}"#,
        ]),
        "",
    );
    let choices = agent::reduce(
        "codex-2",
        &log(&[
            r#"{"ts":9900,"type":"started","name":"Update CI","agent":"codex"}"#,
            r#"{"ts":9990,"type":"question","request_id":"q2","text":"Which branch should I push to?","choices":["main","dev","a new branch"]}"#,
        ]),
        "",
    );
    let working = agent::reduce(
        "claude-3",
        &log(&[
            r#"{"ts":9900,"type":"started","name":"Write tests","agent":"claude","interactive":true}"#,
            r#"{"ts":9990,"type":"milestone","text":"Running cargo test"}"#,
        ]),
        "",
    );
    vec![typed, choices, working]
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
    shoot_scaled(name, kind, w, h, 1, draw)
}

/// Render at `scale`: the image is `scale` times the logical size.
fn shoot_scaled(
    name: &str,
    kind: ThemeKind,
    w: u32,
    h: u32,
    scale: u32,
    draw: impl FnOnce(&mut Canvas<'_>, &Theme),
) -> Shot {
    let th = Theme::for_kind(kind);
    let (pw, ph) = (w * scale, h * scale);
    let mut buf = vec![0u8; (pw * ph * 4) as usize];
    {
        let mut cv = Canvas::new(&mut buf, w, h, scale as i32);
        draw(&mut cv, &th);
    }
    assert!(
        buf.chunks_exact(4).any(|p| p[3] > 0),
        "{name}: drew nothing"
    );
    // Neutral graphite — never purple — so glass overlays are judged fairly.
    let backdrop = match kind {
        ThemeKind::Dark => Rgba::rgb(0x1c, 0x1c, 0x1e),
        ThemeKind::Light => Rgba::rgb(0xe8, 0xe8, 0xed),
    };
    Shot {
        name: format!("{name}-{}", kind.as_str()),
        w: pw,
        h: ph,
        rgba: flatten(&buf, pw, ph, backdrop),
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
    use crate::material::Material;
    use solyra_conf::look::NotchStyle;
    let wins = [
        win(1, "foot", "foot — terminal notes", true),
        win(2, "firefox", "Release notes", false),
    ];
    let items = launcher::search(query, cat, &wins, runtime);
    let mut scene = scene_pixels(
        fonts,
        kind,
        "beach",
        Material::Glass,
        Material::Glass,
        Material::Solid,
        DockStyle::Macos,
        NotchStyle::Island,
        false,
    );
    let lay = spot_layout(query, &items);
    let at = (
        (scene.w as i32 - lay.w as i32) / 2,
        (scene.h as f32 * 0.14) as i32 - 20,
    );
    let mut buf = vec![0u8; (lay.w * lay.h * 4) as usize];
    {
        let th = Theme::for_kind(kind);
        let mut cv = Canvas::new(&mut buf, lay.w, lay.h, 1);
        let mut icons = HashMap::new();
        draw_launcher(
            &mut cv, &th, fonts, query, &items, selected, &mut icons, None,
        );
    }
    let rects: Vec<_> = lay
        .shapes()
        .iter()
        .flat_map(|(r, radius)| crate::material::blur_rects(r.x, r.y, r.w, r.h, *radius))
        .collect();
    compositor_backdrop(
        &scene.sharp,
        &scene.soft,
        scene.w,
        Material::Glass,
        &rects,
        at,
        &mut scene.out,
    );
    over(&mut scene.out, scene.w, &buf, lay.w, lay.h, at);
    Shot {
        name: format!("{name}-{}", kind.as_str()),
        w: scene.w,
        h: scene.h,
        rgba: flatten(&scene.out, scene.w, scene.h, Rgba::rgb(0, 0, 0)),
    }
}

/// Spotlight grown out of the island / notch, at morph progress `t`.
#[allow(clippy::too_many_arguments)]
fn island_spot_shot(
    name: &str,
    query: &str,
    selected: usize,
    notch: bool,
    t: f32,
    fonts: &Fonts,
    cat: &[AppEntry],
) -> Shot {
    use crate::material::Material;
    use solyra_conf::look::NotchStyle;
    let wins = [win(1, "foot", "foot — terminal notes", true)];
    let items = launcher::search(query, cat, &wins, true);
    let mut scene = scene_pixels(
        fonts,
        ThemeKind::Light,
        "silk-light",
        Material::Glass,
        Material::Glass,
        Material::Solid,
        DockStyle::Macos,
        if notch {
            NotchStyle::Notch
        } else {
            NotchStyle::Island
        },
        false,
    );
    let lay = crate::surfaces::island_spot_layout(query, &items, notch);
    let at = ((scene.w as i32 - lay.w as i32) / 2, 0);
    if let (Some(c), true) = (lay.card, t >= 0.62) {
        compositor_lens(
            &scene.soft,
            scene.w,
            scene.h,
            (c.x + at.0, c.y + at.1, c.w, c.h),
            crate::surfaces::SPOT_CARD_R,
            12.0,
            1.35,
            &mut scene.out,
        );
    }
    let mut buf = vec![0u8; (lay.w * lay.h * 4) as usize];
    {
        let mut cv = Canvas::new(&mut buf, lay.w, lay.h, 1);
        let mut icons = HashMap::new();
        crate::surfaces::draw_island_spotlight(
            &mut cv,
            fonts,
            crate::tokens::Accent::Blue,
            query,
            &items,
            selected,
            &mut icons,
            None,
            notch,
            true,
            t,
        );
    }
    over(&mut scene.out, scene.w, &buf, lay.w, lay.h, at);
    Shot {
        name: name.into(),
        w: scene.w,
        h: scene.h,
        rgba: flatten(&scene.out, scene.w, scene.h, Rgba::rgb(0, 0, 0)),
    }
}

#[test]
fn island_spotlight_renders() {
    let fonts = Fonts::load();
    let cat = sample_catalog();
    let shots = [
        island_spot_shot("spot-island-morph", "", 0, false, 0.45, &fonts, &cat),
        island_spot_shot("spot-island-empty", "", 0, false, 1.0, &fonts, &cat),
        island_spot_shot("spot-island-query", "te", 1, false, 1.0, &fonts, &cat),
        island_spot_shot("spot-notch-query", "te", 0, true, 1.0, &fonts, &cat),
        island_spot_shot(
            "spot-notch-agent",
            "sort my downloads",
            0,
            true,
            1.0,
            &fonts,
            &cat,
        ),
    ];
    // The finished shape is smoked glass at the top centre: dark enough for
    // white text, but not opaque black.
    for s in &shots[1..] {
        let i = ((10 * s.w + s.w / 2) * 4) as usize;
        let l = s.rgba[i] as u32 + s.rgba[i + 1] as u32 + s.rgba[i + 2] as u32;
        assert!(l < 3 * 110, "{}: too light behind the field ({l})", s.name);
    }
    if let Some(dir) = std::env::var_os("SOLYRA_RENDER_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for s in &shots {
            write_png(&dir, s);
        }
    }
}

fn desk_shot(name: &str, kind: ThemeKind, fonts: &Fonts, named: bool) -> Shot {
    use crate::desk::{self, Glance};
    use crate::material::Material;
    use solyra_conf::look::NotchStyle;
    let named_lines = ["Infi-AI", "shell/panel"];
    let lines: &[&str] = if named {
        &named_lines
    } else {
        &desk::PROJECT_PLACEHOLDER
    };
    let mut scene = scene_pixels(
        fonts,
        kind,
        "beach",
        Material::Glass,
        Material::Glass,
        Material::Solid,
        DockStyle::Macos,
        NotchStyle::Island,
        false,
    );
    let (cw, ch) = (desk::W as u32, desk::H as u32);
    let at = (scene.w as i32 - desk::W - 36, BAR_HEIGHT as i32 + 36);
    {
        let mut cv = Canvas::new_keep(&mut scene.out, scene.w, scene.h, 1);
        cv.round_rect_shadow(at.0, at.1, desk::W, desk::H, desk::RADIUS, 12.0, 30.0, 70);
    }
    let mut card_buf = vec![0u8; (cw * ch * 4) as usize];
    {
        let th = Theme::for_kind(kind);
        let mut cv = Canvas::new(&mut card_buf, cw, ch, 1);
        desk::paint(
            &mut cv,
            &th,
            fonts,
            &Glance {
                clock: "9:41",
                date: "Tuesday 17 November",
                project_lines: lines,
                environment: 1,
                agents_running: if named { Some(2) } else { None },
            },
            Some(Rgba::rgb(0x3b, 0x9e, 0xd8)),
            None,
            0,
            0,
        );
    }
    over(&mut scene.out, scene.w, &card_buf, cw, ch, at);
    Shot {
        name: format!("{name}-{}", kind.as_str()),
        w: scene.w,
        h: scene.h,
        rgba: flatten(&scene.out, scene.w, scene.h, Rgba::rgb(0, 0, 0)),
    }
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
        shots.push(desk_shot("desk-project", kind, &fonts, true));
        shots.push(desk_shot("desk-placeholder", kind, &fonts, false));

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
                    reply: "",
                    typing: false,
                    desk: true,
                    clock: "09:41",
                    date: "TUE 17 NOV",
                    project_lines: &[],
                    environment: 1,
                    agents_running: None,
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
                reply: "",
                typing: false,
                desk: false,
                clock: "09:41",
                date: "TUE 17 NOV",
                project_lines: &[],
                environment: 1,
                agents_running: None,
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
                    reply: "",
                    typing: false,
                    desk: false,
                    clock: "09:41",
                    date: "TUE 17 NOV",
                    project_lines: &[],
                    environment: 1,
                    agents_running: None,
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
                    reply: "",
                    typing: false,
                    desk: false,
                    clock: "09:41",
                    date: "TUE 17 NOV",
                    project_lines: &[],
                    environment: 1,
                    agents_running: None,
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
                    reply: "",
                    typing: false,
                    desk: false,
                    clock: "09:41",
                    date: "TUE 17 NOV",
                    project_lines: &[],
                    environment: 1,
                    agents_running: None,
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
                    reply: "",
                    typing: false,
                    desk: false,
                    clock: "09:41",
                    date: "TUE 17 NOV",
                    project_lines: &[],
                    environment: 1,
                    agents_running: None,
                };
                draw_agents(cv, th, &fonts, &v, None);
            }));
        }

        let asks = ask_fixtures();
        for (name, sel, reply, typing) in [
            (
                "agents-question-typing",
                0usize,
                "Read the TOML one first",
                true,
            ),
            ("agents-question-choices", 1, "", false),
            ("agents-message-agent", 2, "", false),
        ] {
            shots.push(shoot(name, kind, AGENTS_W, AGENTS_H, |cv, th| {
                let v = AgentsView {
                    runtime_present: true,
                    tasks: &asks,
                    selected: sel,
                    now: 10_000,
                    root_hint: "",
                    error: None,
                    reply,
                    typing,
                    desk: false,
                    clock: "09:41",
                    date: "TUE 17 NOV",
                    project_lines: &[],
                    environment: 1,
                    agents_running: None,
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
            let activity = None;
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
                    activity,
                    &prefs,
                    &t,
                    None,
                );
            }));
        }
        shots.push(shoot("bar-activity", kind, 1280, BAR_HEIGHT, |cv, th| {
            let t = telem(Some(57), NetworkState::Wireless, true);
            draw_bar(
                cv,
                th,
                &fonts,
                &[(1, true), (2, false)],
                &[win(1, "foot", "x", true)],
                None,
                None,
                Some("claude · Running cargo test"),
                &prefs,
                &t,
                None,
            );
        }));
        let summary = Summary::of(true, &tasks);
        let dock_wins = [win(1, "foot", "x", true), win(2, "thunar", "y", false)];
        for (name, style, magnify, hover) in [
            ("dock-solyra", DockStyle::Solyra, false, None),
            ("dock-floating", DockStyle::Macos, false, None),
            (
                "dock-floating-magnify",
                DockStyle::Macos,
                true,
                Some(0.5f64),
            ),
            ("dock-minimal", DockStyle::Minimal, false, None),
            ("dock-taskbar", DockStyle::Windows, false, None),
        ] {
            let spec = crate::dock::spec(style, &dock_wins, magnify, false);
            let w = if spec.full_width { 1280 } else { spec.width };
            shots.push(shoot(name, kind, w, spec.height, |cv, th| {
                let t = telem(Some(64), NetworkState::Wireless, false);
                let live = Live {
                    agents: summary,
                    telem: &t,
                    prefs: &prefs,
                };
                let mut icons = HashMap::new();
                let hover = hover.map(|f| (w as f64 * f, spec.height as f64 - 20.0));
                crate::dock::draw(
                    cv,
                    th,
                    &fonts,
                    style,
                    &dock_wins,
                    &mut icons,
                    hover,
                    &live,
                    magnify,
                    false,
                    &[],
                );
            }));
        }
    }
    shots
}

/// HiDPI: the same painter at 2x makes a 2x image whose hit regions are
/// identical (they stay in logical pixels), and text is redrawn at the real
/// resolution rather than stretched.
#[test]
fn two_x_renders_are_sharp_and_keep_logical_hit_regions() {
    let fonts = Fonts::load();
    let cat = sample_catalog();
    let items = launcher::search("te", &cat, &[], true);
    let mut regions = Vec::new();
    let mut shots = Vec::new();
    for scale in [1u32, 2] {
        let s = shoot_scaled(
            "launcher-hidpi",
            ThemeKind::Dark,
            spot_layout("te", &items).w,
            spot_layout("te", &items).h,
            scale,
            |cv, th| {
                let mut icons = HashMap::new();
                regions.push(draw_launcher(
                    cv, th, &fonts, "te", &items, 0, &mut icons, None,
                ));
            },
        );
        shots.push(s);
    }
    assert_eq!((shots[1].w, shots[1].h), (shots[0].w * 2, shots[0].h * 2));
    let key = |r: &Vec<Region>| r.iter().map(|r| (r.x, r.y, r.w, r.h)).collect::<Vec<_>>();
    assert_eq!(
        key(&regions[0]),
        key(&regions[1]),
        "hit regions must not depend on scale"
    );
    // Sharpness: compare the real 2x render with the 1x render enlarged by
    // pixel doubling. The layout is the same, so most pixels match; text and
    // curves must differ, because the 2x render has real extra detail.
    let (w1, w2) = (shots[0].w as usize, shots[1].w as usize);
    let (h2, mut differing, mut total) = (shots[1].h as usize, 0usize, 0usize);
    for y in 0..h2 {
        for x in 0..w2 {
            let a = &shots[0].rgba[((y / 2) * w1 + x / 2) * 4..][..4];
            let b = &shots[1].rgba[(y * w2 + x) * 4..][..4];
            total += 1;
            let d: i32 = a
                .iter()
                .zip(b)
                .map(|(p, q)| (*p as i32 - *q as i32).abs())
                .sum();
            if d > 24 {
                differing += 1;
            }
        }
    }
    let frac = differing as f64 / total as f64;
    assert!(
        frac > 0.0005,
        "2x render has no extra detail (looks stretched): {frac:.4}"
    );
    assert!(frac < 0.25, "2x layout differs from 1x: {frac:.4}");
    if let Some(dir) = std::env::var_os("SOLYRA_RENDER_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_png(&dir, &shots[1]);
    }
}

fn island_shots(fonts: &Fonts) -> Vec<Shot> {
    use crate::island::{self, Anim, Ask, Content, Extra, Glance};
    use solyra_conf::look::NotchStyle;
    let glance = Glance {
        battery: Some(82),
        network: "Wi-Fi · connected".into(),
        agents_running: 1,
        windows: 3,
    };
    let ask = Ask {
        task: 0,
        who: "claude".into(),
        kind: agent::AskKind::Permission,
        text: "Run a shell command".into(),
        resource: Some("cargo test --workspace".into()),
        reversible: None,
        task_scope: true,
        choices: vec![],
        sent: false,
    };
    let question = Ask {
        kind: agent::AskKind::Question,
        text: "Which branch should I push to?".into(),
        choices: vec!["main".into(), "dev".into()],
        resource: None,
        ..ask.clone()
    };
    let media = crate::media::Media {
        bus: "x".into(),
        title: "Clair de Lune".into(),
        artist: "Debussy".into(),
        playing: true,
        position_us: Some(15_000_000),
        length_us: Some(203_000_000),
    };
    let cases: Vec<(&str, Content, bool)> = vec![
        ("idle", Content::Idle, false),
        ("idle-expanded", Content::Idle, true),
        (
            "activity",
            Content::Activity {
                who: "claude".into(),
                text: "Running cargo test".into(),
            },
            false,
        ),
        ("attention", Content::Attention(ask.clone()), false),
        ("attention-expanded", Content::Attention(ask), true),
        ("question-expanded", Content::Attention(question), true),
        ("media", Content::Media(media.clone()), false),
        ("media-expanded", Content::Media(media), true),
        (
            "message-expanded",
            Content::Message("Firefox · Download finished — report.pdf".into()),
            true,
        ),
        ("shelf-empty", Content::Shelf(vec![]), true),
        (
            "hud-volume",
            Content::Hud(crate::hud::Hud {
                kind: crate::hud::HudKind::Volume,
                level: Some(62),
                on: false,
            }),
            false,
        ),
        (
            "hud-charging",
            Content::Hud(crate::hud::Hud {
                kind: crate::hud::HudKind::Charging,
                level: Some(84),
                on: true,
            }),
            false,
        ),
        (
            "hud-unlocked",
            Content::Hud(crate::hud::Hud {
                kind: crate::hud::HudKind::Unlocked,
                level: None,
                on: true,
            }),
            false,
        ),
        (
            "shelf",
            Content::Shelf(
                [
                    "/home/me/Q3 report.pdf",
                    "/home/me/Pictures/beach.jpg",
                    "/home/me/notes.md",
                    "/home/me/song.flac",
                    "/home/me/build.tar.zst",
                ]
                .map(std::path::PathBuf::from)
                .to_vec(),
            ),
            true,
        ),
    ];
    let (w, h) = (island::SURF_W, island::SURF_H);
    let beach_px = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/wallpapers/beach-1920x1080.png"
    ))
    .ok()
    .and_then(|b| crate::canvas::decode_png(&b));
    let beach = crate::lock::backdrop(beach_px.as_ref(), w, h);
    let soft = crate::lock::blur(&crate::lock::blur(&beach, w, h), w, h);
    let mut out = Vec::new();
    for style in [NotchStyle::Notch, NotchStyle::Island] {
        for (name, content, expanded) in &cases {
            let anim = Anim::new(island::target(style, content, *expanded));
            let extra = Extra {
                glance: &glance,
                clock: "21:47",
                date: "Thursday 19 November",
                reply: "",
                typing: false,
                glass: true,
                badge: None,
            };
            let tag = format!("island-{}-{name}", style.as_str());
            let mut buf = beach.clone();
            let rects = island::glass_rects(style, &anim.cur);
            compositor_backdrop(
                &beach,
                &soft,
                w,
                crate::material::Material::Glass,
                &rects,
                (0, 0),
                &mut buf,
            );
            {
                let mut cv = crate::canvas::Canvas::new_keep(&mut buf, w, h, 1);
                island::draw(
                    &mut cv, fonts, style, &anim, content, *expanded, &extra, None,
                );
            }
            out.push(Shot {
                name: tag,
                w,
                h,
                rgba: flatten(&buf, w, h, Rgba::rgb(0, 0, 0)),
            });
        }
    }
    out
}

#[test]
fn menu_bar_and_menus_render() {
    use crate::menu::{self, AppCtx, MenuId, Title};
    let fonts = Fonts::load();
    let mut shots = Vec::new();
    for kind in [ThemeKind::Dark, ThemeKind::Light] {
        let mut prefs = ShellPrefs::default();
        prefs.theme = kind;
        let mut regions = Vec::new();
        shots.push(shoot("bar-menubar", kind, 1280, BAR_HEIGHT, |cv, th| {
            let t = telem(Some(57), NetworkState::Wireless, true);
            regions = draw_bar(
                cv,
                th,
                &fonts,
                &[(1, true), (2, false), (3, false)],
                &[win(1, "foot", "x", true)],
                None,
                None,
                None,
                &prefs,
                &t,
                None,
            );
        }));
        let titles: Vec<Title> = regions
            .iter()
            .filter_map(|r| match r.action {
                HitAction::OpenMenu(id) => Some(Title {
                    id,
                    x: r.x + 6,
                    w: r.w - 12,
                }),
                _ => None,
            })
            .collect();
        assert!(
            titles.iter().any(|t| t.id == MenuId::Window),
            "menu titles are clickable"
        );
        let app = AppCtx {
            name: "Terminal",
            launch: Some("solyra-open terminal".into()),
            has_window: true,
        };
        for (id, armed) in [(MenuId::Solyra, Some(7)), (MenuId::Window, None)] {
            let title = titles.iter().copied().find(|t| t.id == id).unwrap();
            let entries = menu::entries(id, &app, kind == ThemeKind::Dark);
            shots.push(shoot(
                &format!("menu-{}", id.title().to_lowercase()),
                kind,
                640,
                420,
                |cv, th| {
                    menu::draw(
                        cv,
                        th,
                        &fonts,
                        title,
                        &titles,
                        &entries,
                        BAR_HEIGHT as i32,
                        armed,
                        Some((80.0, 120.0)),
                    );
                },
            ));
        }
        let mut compact = ShellPrefs::default();
        compact.theme = kind;
        compact.look.bar = solyra_conf::look::BarStyle::Compact;
        compact.look.clock_24h = false;
        shots.push(shoot("bar-compact", kind, 1280, BAR_HEIGHT, |cv, th| {
            let t = telem(None, NetworkState::Wired, false);
            draw_bar(
                cv,
                th,
                &fonts,
                &[(1, true), (2, false)],
                &[],
                None,
                None,
                None,
                &compact,
                &t,
                None,
            );
        }));
    }
    if let Some(dir) = std::env::var_os("SOLYRA_RENDER_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for s in &shots {
            write_png(&dir, s);
        }
    }
}

#[test]
fn lock_screens_render() {
    use crate::lock::{backdrop, blur, paint, View};
    use solyra_conf::look::LockStyle;
    let fonts = Fonts::load();
    let wp = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/wallpapers/beach-1920x1080.png"
    ))
    .ok()
    .and_then(|b| crate::canvas::decode_png(&b));
    let (w, h) = (960u32, 600u32);
    let sharp = backdrop(wp.as_ref(), w, h);
    let mut soft = blur(&sharp, w, h);
    crate::lock::vivid(&mut soft, 1.25);
    let th = Theme::with(ThemeKind::Dark, crate::tokens::Accent::Blue);
    let mut shots = Vec::new();
    for (name, style, card, dots, msg) in [
        ("lock-solyra", LockStyle::Solyra, false, 6, None),
        ("lock-centred", LockStyle::Classic, false, 0, None),
        ("lock-corner", LockStyle::Corner, false, 0, None),
        (
            "lock-corner-signin",
            LockStyle::Corner,
            true,
            3,
            Some("Wrong password"),
        ),
        ("lock-dashboard", LockStyle::Dashboard, false, 0, None),
        (
            "lock-dashboard-glance",
            LockStyle::Dashboard,
            false,
            4,
            None,
        ),
        ("lock-centred-media", LockStyle::Classic, false, 0, None),
        ("lock-centred-unlocking", LockStyle::Classic, false, 0, None),
    ] {
        let mut buf = if crate::lock::sharp_backdrop(style, card) {
            sharp.clone()
        } else {
            soft.clone()
        };
        {
            let mut cv = crate::canvas::Canvas::new_keep(&mut buf, w, h, 1);
            let dashboard = style == LockStyle::Dashboard;
            let glance = name == "lock-dashboard-glance";
            let with_media = glance || name == "lock-centred-media";
            let track = crate::media::Media {
                bus: "org.mpris.MediaPlayer2.preview".into(),
                title: "Clair de Lune".into(),
                artist: "Claude Debussy".into(),
                playing: true,
                position_us: Some(83_000_000),
                length_us: Some(301_000_000),
            };
            let v = View {
                theme: &th,
                fonts: &fonts,
                style,
                clock: if dashboard {
                    "20:46".into()
                } else {
                    "09:41".into()
                },
                date: if dashboard {
                    "SAT 17 NOV".into()
                } else {
                    "Friday 20 November".into()
                },
                name: "Ada Lovelace",
                dots,
                caps: name == "lock-solyra",
                checking: false,
                message: msg,
                card,
                agents_running: if glance { 2 } else { 0 },
                media_title: if glance { Some("Clair de Lune") } else { None },
                media_artist: if glance { Some("Debussy") } else { None },
                weather: if dashboard { Some("25°") } else { None },
                media: with_media.then_some(&track),
                unlock: (name == "lock-centred-unlocking").then_some(0.8),
                phase: 1.3,
            };
            let hits = paint(&mut cv, &v);
            assert_eq!(
                hits.is_empty(),
                !with_media,
                "{name}: controls only with media"
            );
        }
        shots.push(Shot {
            name: name.into(),
            w,
            h,
            rgba: flatten(&buf, w, h, Rgba::rgb(0, 0, 0)),
        });
    }
    if let Some(dir) = std::env::var_os("SOLYRA_RENDER_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for s in &shots {
            write_png(&dir, s);
        }
    }
}

#[test]
fn island_previews_render() {
    let fonts = Fonts::load();
    let shots = island_shots(&fonts);
    assert_eq!(shots.len(), 28, "14 states x 2 styles");
    if let Some(dir) = std::env::var_os("SOLYRA_RENDER_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for s in &shots {
            write_png(&dir, s);
        }
    }
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

/// What the compositor does behind a blurring material, approximated for
/// previews: blurred backdrop, a saturation lift, and (Acrylic) fine grain.
/// Values mirror the `.glass` / `.acrylic` layer rules in solyra-config.kdl.
/// The compositor's Liquid Glass for previews: the blurred backdrop, bent at
/// the rim exactly as `refract_surface.frag` does (sample pulled from beyond
/// the edge over a band 2.6 x the strength, falloff (1 + d/band)^2, red
/// 1.25x and blue 0.75x for the fringe), with the glass saturation.
#[allow(clippy::too_many_arguments)]
fn compositor_lens(
    soft: &[u8],
    w: u32,
    h: u32,
    (gx, gy, gw, gh): (i32, i32, i32, i32),
    r: f32,
    strength: f32,
    sat: f32,
    out: &mut [u8],
) {
    let (hw, hh) = (gw as f32 / 2.0, gh as f32 / 2.0);
    let r = r.min(hw).min(hh);
    let (cx, cy) = (gx as f32 + hw, gy as f32 + hh);
    let sdf = |qx: f32, qy: f32| {
        let ax = qx.abs() - hw + r;
        let ay = qy.abs() - hh + r;
        ax.max(ay).min(0.0) + (ax.max(0.0).powi(2) + ay.max(0.0).powi(2)).sqrt() - r
    };
    let band = (strength * 2.6).max(6.0);
    let sample = |x: f32, y: f32, c: usize| -> f32 {
        let xi = (x.round() as i32).clamp(0, w as i32 - 1) as u32;
        let yi = (y.round() as i32).clamp(0, h as i32 - 1) as u32;
        soft[((yi * w + xi) * 4) as usize + c] as f32
    };
    for py in gy.max(0)..(gy + gh).min(h as i32) {
        for px in gx.max(0)..(gx + gw).min(w as i32) {
            let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
            let (qx, qy) = (fx - cx, fy - cy);
            let d = sdf(qx, qy);
            if d > 0.5 {
                continue;
            }
            let f = (1.0 + d / band).clamp(0.0, 1.0).powi(2);
            let gxv = sdf(qx + 1.0, qy) - sdf(qx - 1.0, qy);
            let gyv = sdf(qx, qy + 1.0) - sdf(qx, qy - 1.0);
            let l = (gxv * gxv + gyv * gyv).sqrt().max(1e-5);
            let (ox, oy) = (gxv / l * f * strength, gyv / l * f * strength);
            let rr = sample(fx + ox * 1.25, fy + oy * 1.25, 0);
            let gg = sample(fx + ox, fy + oy, 1);
            let bb = sample(fx + ox * 0.75, fy + oy * 0.75, 2);
            let lum = 0.299 * rr + 0.587 * gg + 0.114 * bb;
            let i = ((py as u32 * w + px as u32) * 4) as usize;
            for (c, v) in [(0usize, rr), (1, gg), (2, bb)] {
                out[i + c] = (lum + (v - lum) * sat).clamp(0.0, 255.0) as u8;
            }
            out[i + 3] = 255;
        }
    }
}

fn compositor_backdrop(
    sharp: &[u8],
    soft: &[u8],
    w: u32,
    m: crate::material::Material,
    rects: &[(i32, i32, i32, i32)],
    offset: (i32, i32),
    out: &mut [u8],
) {
    use crate::material::Material;
    let (sat, noise) = match m {
        Material::Glass => (1.1f32, 0.0f32),
        Material::Acrylic => (1.12, 0.05),
        _ => return,
    };
    let _ = sharp;
    for &(rx, ry, rw, rh) in rects {
        for y in ry..ry + rh {
            for x in rx..rx + rw {
                let (gx, gy) = (x + offset.0, y + offset.1);
                if gx < 0 || gy < 0 || gx >= w as i32 {
                    continue;
                }
                let i = ((gy as u32 * w + gx as u32) * 4) as usize;
                if i + 3 >= out.len() {
                    continue;
                }
                let px = &soft[i..i + 3];
                let l = 0.114 * px[0] as f32 + 0.587 * px[1] as f32 + 0.299 * px[2] as f32;
                // Deterministic grain: a hash of the position.
                let hash = ((gx as u32).wrapping_mul(73_856_093)
                    ^ (gy as u32).wrapping_mul(19_349_663))
                    % 1000;
                let g = (hash as f32 / 1000.0 - 0.5) * 2.0 * noise * 255.0;
                for c in 0..3 {
                    let v = l + (px[c] as f32 - l) * sat + g;
                    out[i + c] = v.clamp(0.0, 255.0) as u8;
                }
                out[i + 3] = 255;
            }
        }
    }
}

/// Draw `surface` (premultiplied BGRA, `sw`×`sh`) over `out` at `at`.
fn over(out: &mut [u8], w: u32, surface: &[u8], sw: u32, sh: u32, at: (i32, i32)) {
    for y in 0..sh as i32 {
        for x in 0..sw as i32 {
            let (gx, gy) = (x + at.0, y + at.1);
            if gx < 0 || gy < 0 || gx >= w as i32 {
                continue;
            }
            let si = ((y as u32 * sw + x as u32) * 4) as usize;
            let di = ((gy as u32 * w + gx as u32) * 4) as usize;
            if di + 3 >= out.len() {
                continue;
            }
            let a = surface[si + 3] as f32 / 255.0;
            for c in 0..3 {
                out[di + c] = (surface[si + c] as f32 + out[di + c] as f32 * (1.0 - a))
                    .round()
                    .min(255.0) as u8;
            }
        }
    }
}

/// A desktop scene (wallpaper, menu bar, notch or island, dock) in one
/// material, so each material can be judged as a whole.
fn material_scene(
    fonts: &Fonts,
    name: &str,
    kind: ThemeKind,
    wallpaper: &str,
    bar_m: crate::material::Material,
    dock_m: crate::material::Material,
    notch_m: crate::material::Material,
    dock_style: DockStyle,
    notch: solyra_conf::look::NotchStyle,
) -> Shot {
    let scene = scene_pixels(
        fonts, kind, wallpaper, bar_m, dock_m, notch_m, dock_style, notch, true,
    );
    Shot {
        name: format!("{name}-{}", kind.as_str()),
        w: scene.w,
        h: scene.h,
        rgba: flatten(&scene.out, scene.w, scene.h, Rgba::rgb(0, 0, 0)),
    }
}

/// A composed desktop: the sharp wallpaper, its blurred twin (what a blurring
/// surface shows), and the scene with bar, dock and island drawn.
struct Scene {
    sharp: Vec<u8>,
    soft: Vec<u8>,
    out: Vec<u8>,
    w: u32,
    h: u32,
}

#[allow(clippy::too_many_arguments)]
fn scene_pixels(
    fonts: &Fonts,
    kind: ThemeKind,
    wallpaper: &str,
    bar_m: crate::material::Material,
    dock_m: crate::material::Material,
    notch_m: crate::material::Material,
    dock_style: DockStyle,
    notch: solyra_conf::look::NotchStyle,
    media: bool,
) -> Scene {
    scene_pixels_with(
        fonts,
        kind,
        wallpaper,
        bar_m,
        dock_m,
        notch_m,
        dock_style,
        notch,
        media,
        |_, _, _| {},
    )
}

/// [`scene_pixels`], with `under` painting onto the desktop before the shell
/// surfaces go on top (a window behind the dock, to show the glass).
#[allow(clippy::too_many_arguments)]
fn scene_pixels_with(
    fonts: &Fonts,
    kind: ThemeKind,
    wallpaper: &str,
    bar_m: crate::material::Material,
    dock_m: crate::material::Material,
    notch_m: crate::material::Material,
    dock_style: DockStyle,
    notch: solyra_conf::look::NotchStyle,
    media: bool,
    under: impl FnOnce(&mut [u8], u32, u32),
) -> Scene {
    use crate::island::{self, Anim, Content, Extra, Glance};
    use crate::lock::{backdrop, blur};
    let (w, h) = (1280u32, 800u32);
    let wp = std::fs::read(format!(
        "{}/../../assets/wallpapers/{wallpaper}-1920x1080.png",
        env!("CARGO_MANIFEST_DIR")
    ))
    .ok()
    .and_then(|b| crate::canvas::decode_png(&b));
    let mut sharp = backdrop(wp.as_ref(), w, h);
    under(&mut sharp, w, h);
    let soft = blur(&blur(&sharp, w, h), w, h);
    let mut out = sharp.clone();

    let mut prefs = ShellPrefs::default();
    prefs.theme = kind;
    prefs.dock_style = dock_style;
    prefs.look.bar_material = bar_m;
    prefs.look.dock_material = dock_m;
    prefs.look.notch_material = notch_m;
    prefs.look.notch = notch;
    prefs.wall_tint = wp.as_ref().and_then(crate::material::average);
    let th = Theme::for_kind(kind);

    // Bar.
    let bh = BAR_HEIGHT;
    let mut buf = vec![0u8; (w * bh * 4) as usize];
    {
        let mut cv = Canvas::new(&mut buf, w, bh, 1);
        let t = telem(Some(64), NetworkState::Wireless, false);
        draw_bar(
            &mut cv,
            &th,
            fonts,
            &[(1, true), (2, false), (3, false)],
            &[win(1, "firefox", "Solyra — Firefox", true)],
            None,
            None,
            None,
            &prefs,
            &t,
            None,
        );
    }
    if bar_m == crate::material::Material::Acrylic {
        compositor_backdrop(
            &sharp,
            &soft,
            w,
            bar_m,
            &[(0, 0, w as i32, bh as i32)],
            (0, 0),
            &mut out,
        );
    }
    over(&mut out, w, &buf, w, bh, (0, 0));

    // Dock.
    let wins = [
        win(1, "firefox", "Firefox", true),
        win(2, "foot", "Terminal", false),
        win(3, "thunar", "Files", false),
    ];
    let spec = crate::dock::spec(dock_style, &wins, false, false);
    let dw = if spec.full_width { w } else { spec.width };
    let mut buf = vec![0u8; (dw * spec.height * 4) as usize];
    let body;
    {
        let mut cv = Canvas::new(&mut buf, dw, spec.height, 1);
        let t = telem(Some(64), NetworkState::Wireless, false);
        let live = Live {
            agents: Summary::of(true, &[]),
            telem: &t,
            prefs: &prefs,
        };
        let mut icons = HashMap::new();
        body = crate::dock::draw(
            &mut cv,
            &th,
            fonts,
            dock_style,
            &wins,
            &mut icons,
            None,
            &live,
            false,
            false,
            &[],
        )
        .1;
    }
    let at = (
        (w as i32 - dw as i32) / 2,
        h as i32 - spec.height as i32 - spec.bottom_margin,
    );
    if let Some(b) = body {
        // The dock draws its own elevation shadow (dock.rs); none is added here.
        if dock_m == crate::material::Material::Glass {
            compositor_lens(
                &soft,
                w,
                h,
                (b.x + at.0, b.y + at.1, b.w, b.h),
                b.r,
                10.0,
                1.35,
                &mut out,
            );
        } else if crate::material::blurs(dock_m) {
            let rects = crate::material::blur_rects(b.x, b.y, b.w, b.h, b.r);
            compositor_backdrop(&sharp, &soft, w, dock_m, &rects, at, &mut out);
        }
    }
    over(&mut out, w, &buf, dw, spec.height, at);

    // Notch / island, showing what is playing.
    if notch != solyra_conf::look::NotchStyle::Off {
        let content = if !media {
            Content::Idle
        } else {
            Content::Media(crate::media::Media {
                bus: "x".into(),
                title: "Clair de Lune".into(),
                artist: "Claude Debussy".into(),
                playing: true,
                position_us: Some(15_000_000),
                length_us: Some(203_000_000),
            })
        };
        let anim = Anim::new(island::target(notch, &content, false));
        let glance = Glance {
            battery: Some(64),
            network: "Wi-Fi".into(),
            agents_running: 0,
            windows: 3,
        };
        let extra = Extra {
            glance: &glance,
            clock: "09:41",
            date: "Thursday 19 November",
            reply: "",
            typing: false,
            glass: crate::material::blurs(notch_m),
            badge: None,
        };
        let (sw, sh) = (island::SURF_W, island::SURF_H);
        let mut buf = vec![0u8; (sw * sh * 4) as usize];
        {
            let mut cv = Canvas::new(&mut buf, sw, sh, 1);
            island::draw(&mut cv, fonts, notch, &anim, &content, false, &extra, None);
        }
        let at = ((w as i32 - sw as i32) / 2, 0);
        if crate::material::blurs(notch_m) {
            let rects = island::glass_rects(notch, &anim.cur);
            compositor_backdrop(&sharp, &soft, w, notch_m, &rects, at, &mut out);
        }
        over(&mut out, w, &buf, sw, sh, at);
    }

    Scene {
        sharp,
        soft,
        out,
        w,
        h,
    }
}

#[test]
fn material_scenes_render() {
    use crate::material::Material;
    use solyra_conf::look::NotchStyle;
    let fonts = Fonts::load();
    let mut shots = Vec::new();
    for kind in [ThemeKind::Dark, ThemeKind::Light] {
        // Dark scenes over a dark wallpaper, so a light-tinted "dark" theme shows up.
        let wp = if kind == ThemeKind::Dark {
            "silk"
        } else {
            "beach"
        };
        for m in Material::ALL {
            shots.push(material_scene(
                &fonts,
                &format!("material-{}", m.as_str()),
                kind,
                wp,
                *m,
                *m,
                *m,
                DockStyle::Macos,
                NotchStyle::Notch,
            ));
        }
        // Product default: Glass bar + dock over Beach, pure-black Dynamic Island.
        for (tag, dock) in [
            ("solyra", DockStyle::Solyra),
            ("taskbar", DockStyle::Windows),
            ("minimal", DockStyle::Minimal),
        ] {
            shots.push(material_scene(
                &fonts,
                &format!("material-scene-{tag}"),
                kind,
                wp,
                Material::Glass,
                Material::Glass,
                Material::Solid,
                dock,
                NotchStyle::Island,
            ));
        }
    }
    // Glass must differ from solid where the dock is: the backdrop shows.
    let solid = &shots[0];
    let glass = &shots[2];
    assert_ne!(solid.rgba, glass.rgba);
    if let Some(dir) = std::env::var_os("SOLYRA_RENDER_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for s in &shots {
            write_png(&dir, s);
        }
    }
}

/// The out-of-box desktop, straight from the defaults (`Look::default`):
/// what someone sees on first boot. Compared against pearOS by eye.
#[test]
fn default_desktop_renders() {
    let fonts = Fonts::load();
    let d = ShellPrefs::default();
    let mut shots = Vec::new();
    for (kind, wp) in [(ThemeKind::Dark, "silk"), (ThemeKind::Light, "silk-light")] {
        let mut scene = scene_pixels(
            &fonts,
            kind,
            wp,
            d.look.bar_material,
            d.look.dock_material,
            d.look.notch_material,
            d.dock_style,
            d.look.notch,
            false,
        );
        // Desktop widgets at the top left, under the menu bar.
        {
            use crate::widgets;
            let (ww, wh) = (widgets::W as u32, widgets::H as u32);
            let mut buf = vec![0u8; (ww * wh * 4) as usize];
            {
                let mut cv = Canvas::new(&mut buf, ww, wh, 1);
                let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 25).unwrap();
                let sample = widgets::Weather {
                    city: "Phoenix".into(),
                    temp: 16.0,
                    code: 2,
                    hi: 21.0,
                    lo: 9.0,
                };
                let dark = kind == ThemeKind::Dark;
                widgets::paint(&mut cv, &fonts, dark, today, dark.then_some(&sample), false);
            }
            let at = (14, BAR_HEIGHT as i32 + 14);
            let rects: Vec<_> = [(0, 0), (widgets::CARD + widgets::GAP, 0)]
                .iter()
                .flat_map(|&(x, y)| {
                    crate::material::blur_rects(x, y, widgets::CARD, widgets::CARD, widgets::RADIUS)
                })
                .collect();
            compositor_backdrop(
                &scene.sharp,
                &scene.soft,
                scene.w,
                crate::material::Material::Glass,
                &rects,
                at,
                &mut scene.out,
            );
            over(&mut scene.out, scene.w, &buf, ww, wh, at);
        }
        shots.push(Shot {
            name: format!("desktop-default-{}", kind.as_str()),
            w: scene.w,
            h: scene.h,
            rgba: flatten(&scene.out, scene.w, scene.h, Rgba::rgb(0, 0, 0)),
        });
    }
    // Dark and light must look clearly different (the gap report found 3.7 %).
    let diff = shots[0]
        .rgba
        .chunks_exact(4)
        .zip(shots[1].rgba.chunks_exact(4))
        .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 24))
        .count();
    let frac = diff as f64 / (shots[0].w * shots[0].h) as f64;
    assert!(
        frac > 0.25,
        "dark and light defaults differ in only {:.1} % of pixels",
        frac * 100.0
    );
    if let Some(dir) = std::env::var_os("SOLYRA_RENDER_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for s in &shots {
            write_png(&dir, s);
        }
    }
}

/// The dock over a window, the way it is used: the window's colours show
/// through the glass, blurred.
#[test]
fn dock_glass_over_a_window_renders() {
    let fonts = Fonts::load();
    let d = ShellPrefs::default();
    let mut shots = Vec::new();
    for (kind, wp) in [(ThemeKind::Dark, "silk"), (ThemeKind::Light, "silk-light")] {
        let scene = scene_pixels_with(
            &fonts,
            kind,
            wp,
            d.look.bar_material,
            d.look.dock_material,
            d.look.notch_material,
            d.dock_style,
            d.look.notch,
            false,
            |px, w, h| {
                let mut cv = Canvas::new_keep(px, w, h, 1);
                // A photo-viewer-like window reaching under the dock.
                cv.round_rect(250, 420, 780, 420, 16.0, Rgba::rgb(250, 250, 252));
                let cols = [
                    Rgba::rgb(0xff, 0x6b, 0x6b),
                    Rgba::rgb(0xff, 0xc1, 0x45),
                    Rgba::rgb(0x4c, 0xd9, 0x7b),
                    Rgba::rgb(0x3b, 0x9b, 0xf2),
                    Rgba::rgb(0xa8, 0x6b, 0xf7),
                    Rgba::rgb(0xff, 0x8a, 0xc6),
                ];
                for i in 0..12 {
                    let (c, r) = (i % 6, i / 6);
                    cv.round_rect(
                        274 + c * 124,
                        470 + r * 150,
                        112,
                        136,
                        10.0,
                        cols[(i as usize + r as usize) % 6],
                    );
                }
                cv.text(
                    fonts.semibold.as_ref(),
                    "Pictures",
                    274,
                    450,
                    15.0,
                    Rgba::rgb(30, 30, 34),
                );
            },
        );
        shots.push(Shot {
            name: format!("dock-glass-{}", kind.as_str()),
            w: scene.w,
            h: scene.h,
            rgba: flatten(&scene.out, scene.w, scene.h, Rgba::rgb(0, 0, 0)),
        });
    }
    if let Some(dir) = std::env::var_os("SOLYRA_RENDER_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for s in &shots {
            write_png(&dir, s);
        }
    }
}

#[test]
fn side_docks_render() {
    use crate::material::Material;
    use solyra_conf::look::{DockPosition, NotchStyle};
    let fonts = Fonts::load();
    let mut shots = Vec::new();
    for (name, side) in [
        ("dock-left", DockPosition::Left),
        ("dock-right", DockPosition::Right),
    ] {
        let mut scene = scene_pixels(
            &fonts,
            ThemeKind::Light,
            "silk-light",
            Material::Glass,
            Material::Glass,
            Material::Solid,
            DockStyle::Macos,
            NotchStyle::Island,
            false,
        );
        let wins = [win(1, "foot", "Terminal", true)];
        let spec = crate::dock::orient(
            crate::dock::spec(DockStyle::Macos, &wins, false, false),
            side,
        );
        assert_eq!(
            spec.width,
            crate::dock::CARD_H as u32,
            "a side dock is one card wide"
        );
        assert!(spec.height > spec.width, "and stands on its end");
        let mut prefs = ShellPrefs::default();
        prefs.theme = ThemeKind::Light;
        prefs.dock_style = DockStyle::Macos;
        prefs.look.dock_position = side;
        let th = Theme::for_kind(ThemeKind::Light);
        let mut buf = vec![0u8; (spec.width * spec.height * 4) as usize];
        let body;
        {
            let mut cv = Canvas::new(&mut buf, spec.width, spec.height, 1);
            let t = telem(Some(64), NetworkState::Wireless, false);
            let live = Live {
                agents: Summary::of(true, &[]),
                telem: &t,
                prefs: &prefs,
            };
            let mut icons = HashMap::new();
            body = crate::dock::draw(
                &mut cv,
                &th,
                &fonts,
                DockStyle::Macos,
                &wins,
                &mut icons,
                None,
                &live,
                false,
                false,
                &[],
            )
            .1
            .expect("a side dock has a body");
        }
        let x = if side == DockPosition::Left {
            12
        } else {
            scene.w as i32 - 12 - spec.width as i32
        };
        let at = (x, (scene.h as i32 - spec.height as i32) / 2);
        compositor_lens(
            &scene.soft,
            scene.w,
            scene.h,
            (body.x + at.0, body.y + at.1, body.w, body.h),
            body.r,
            10.0,
            1.35,
            &mut scene.out,
        );
        over(&mut scene.out, scene.w, &buf, spec.width, spec.height, at);
        shots.push(Shot {
            name: name.into(),
            w: scene.w,
            h: scene.h,
            rgba: flatten(&scene.out, scene.w, scene.h, Rgba::rgb(0, 0, 0)),
        });
    }
    if let Some(dir) = std::env::var_os("SOLYRA_RENDER_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for s in &shots {
            write_png(&dir, s);
        }
    }
}


/// Full visual showcase of all macOS 27 Golden Gate + Droppy surfaces for
/// side-by-side verification against our reference images.
#[test]
fn macos27_droppy_showcase() {
    use crate::island::{self, Anim, Content, Extra, Glance};
    use crate::material::Material;
    use solyra_conf::look::{LockStyle, NotchStyle};

    let fonts = Fonts::load();
    let mut shots = Vec::new();
    let extra = Extra {
        glance: &Glance {
            battery: Some(86),
            network: "Wi-Fi".into(),
            agents_running: 2,
            windows: 4,
        },
        clock: "08:21",
        date: "Sun, 22 Nov",
        reply: "",
        typing: false,
        glass: false,
        badge: None,
    };

    // 1. macOS 27 Desktop + Menu Bar + Dock + Control Center
    {
        let mut scene = scene_pixels(
            &fonts,
            ThemeKind::Dark,
            "silk",
            Material::Glass,
            Material::Glass,
            Material::Solid,
            DockStyle::Macos,
            NotchStyle::Notch,
            false,
        );
        let mut prefs = ShellPrefs::default();
        prefs.theme = ThemeKind::Dark;
        prefs.dock_style = DockStyle::Macos;
        prefs.look.glass_tint = 35;
        prefs.brightness = 78;
        prefs.volume = 64;
        let th = Theme::for_kind(ThemeKind::Dark);
        let t = telem(Some(86), NetworkState::Wireless, true);
        let mut cbuf = vec![0u8; (CONTROL_W * CONTROL_H * 4) as usize];
        {
            let mut cv = Canvas::new(&mut cbuf, CONTROL_W, CONTROL_H, 1);
            draw_control(&mut cv, &th, &fonts, &prefs, &t, Summary::of(true, &fixtures()), None);
        }
        let at = (scene.w as i32 - CONTROL_W as i32 - 14, BAR_HEIGHT as i32 + 8);
        compositor_lens(
            &scene.soft,
            scene.w,
            scene.h,
            (at.0, at.1, CONTROL_W as i32, CONTROL_H as i32),
            crate::tokens::R_CARD,
            10.0,
            1.35,
            &mut scene.out,
        );
        over(&mut scene.out, scene.w, &cbuf, CONTROL_W, CONTROL_H, at);
        shots.push(Shot {
            name: "showcase-macos27-desktop-control".into(),
            w: scene.w,
            h: scene.h,
            rgba: flatten(&scene.out, scene.w, scene.h, Rgba::rgb(0, 0, 0)),
        });
    }

    // 2. macOS 27 Spotlight Search (Liquid Glass + Calculator Top Hit)
    {
        let mut scene = scene_pixels(
            &fonts,
            ThemeKind::Dark,
            "silk",
            Material::Glass,
            Material::Glass,
            Material::Solid,
            DockStyle::Macos,
            NotchStyle::Notch,
            false,
        );
        let th = Theme::for_kind(ThemeKind::Dark);
        let cat = catalog_apps();
        let q = "24 * 18";
        let items = launcher::search(q, &cat, &[], true);
        let lay = spot_layout(q, &items);
        let mut sbuf = vec![0u8; (lay.w * lay.h * 4) as usize];
        let mut icons = HashMap::new();
        {
            let mut cv = Canvas::new(&mut sbuf, lay.w, lay.h, 1);
            draw_launcher(&mut cv, &th, &fonts, q, &items, 0, &mut icons, None);
        }
        let at = ((scene.w as i32 - lay.w as i32) / 2, 130);
        for (r, rad) in lay.shapes() {
            compositor_lens(
                &scene.soft,
                scene.w,
                scene.h,
                (at.0 + r.x, at.1 + r.y, r.w, r.h),
                rad,
                10.0,
                1.35,
                &mut scene.out,
            );
        }
        over(&mut scene.out, scene.w, &sbuf, lay.w, lay.h, at);
        shots.push(Shot {
            name: "showcase-macos27-spotlight".into(),
            w: scene.w,
            h: scene.h,
            rgba: flatten(&scene.out, scene.w, scene.h, Rgba::rgb(0, 0, 0)),
        });
    }

    // Helper to composite an expanded Droppy Notch onto a crisp desktop top banner
    let mut render_notch_shot = |name: &str, content: Content| {
        let mut scene = scene_pixels(
            &fonts,
            ThemeKind::Dark,
            "ocean",
            Material::Glass,
            Material::Glass,
            Material::Solid,
            DockStyle::Macos,
            NotchStyle::Notch,
            false,
        );
        let shape = island::target(NotchStyle::Notch, &content, true);
        let anim = Anim::new(shape);
        let mut nbuf = vec![0u8; (island::SURF_W * island::SURF_H * 4) as usize];
        {
            let mut cv = Canvas::new(&mut nbuf, island::SURF_W, island::SURF_H, 1);
            island::draw(&mut cv, &fonts, NotchStyle::Notch, &anim, &content, true, &extra, None);
        }
        let at = ((scene.w as i32 - island::SURF_W as i32) / 2, 0);
        over(&mut scene.out, scene.w, &nbuf, island::SURF_W, island::SURF_H, at);
        shots.push(Shot {
            name: name.into(),
            w: scene.w,
            h: scene.h,
            rgba: flatten(&scene.out, scene.w, scene.h, Rgba::rgb(0, 0, 0)),
        });
    };

    // 3. Droppy Compact Expanded Media Notch (Reference 3)
    render_notch_shot(
        "showcase-droppy-notch-media",
        Content::Media(crate::media::Media {
            bus: "org.mpris.MediaPlayer2.spotify".into(),
            title: "Harbour Lights".into(),
            artist: "Solyra Sessions".into(),
            playing: false,
            position_us: Some(66_000_000),
            length_us: Some(204_000_000),
        }),
    );

    // 4. Droppy Expanded Notch with Live Synced Lyrics (Reference 2)
    let moonlit = crate::media::Media {
        bus: "org.mpris.MediaPlayer2.spotify".into(),
        title: "Moonlit Water".into(),
        artist: "Clear Sky".into(),
        playing: true,
        position_us: Some(6_000_000),
        length_us: Some(212_000_000),
    };
    let moon_lyr = crate::lyrics::sample()
        .window(6_000, 1, 2)
        .into_iter()
        .map(|(o, s)| (o, s.to_string()))
        .collect();
    render_notch_shot(
        "showcase-droppy-notch-lyrics",
        Content::MediaLyrics {
            media: moonlit,
            lyrics: moon_lyr,
        },
    );

    // 5. Droppy Notch Clipboard Manager + Context Menu (Reference 4)
    render_notch_shot(
        "showcase-droppy-notch-clipboard",
        Content::Clipboard {
            items: crate::clipboard::demo_items(),
            filter: crate::clipboard::ClipCategory::All,
            context_menu: Some(2),
        },
    );

    // 6. Droppy Fullscreen Ambient Lyrics Lock Screen (Reference 1)
    {
        let (w, h) = (1280u32, 800u32);
        let wp = std::fs::read("assets/wallpapers/sunset-1920x1080.png")
            .ok()
            .and_then(|b| crate::canvas::decode_png(&b));
        let sharp = crate::lock::backdrop(wp.as_ref(), w, h);
        let mut bg = crate::lock::blur(&sharp, w, h);
        let th = Theme::for_kind(ThemeKind::Dark);
        let media = crate::media::Media {
            bus: "org.mpris.MediaPlayer2.spotify".into(),
            title: "Harbour Lights".into(),
            artist: "Solyra Sessions".into(),
            playing: true,
            position_us: Some(51_000_000),
            length_us: Some(203_000_000),
        };
        let view = crate::lock::View {
            theme: &th,
            fonts: &fonts,
            style: LockStyle::Media,
            clock: "08:21".into(),
            date: "Sun, 22 Nov".into(),
            name: "Solyra",
            dots: 0,
            caps: false,
            checking: false,
            message: None,
            card: false,
            agents_running: 1,
            media_title: Some("Harbour Lights"),
            media_artist: Some("Solyra Sessions"),
            weather: Some("22°"),
            media: Some(&media),
            unlock: None,
            phase: 1.4,
        };
        {
            let mut cv = Canvas::new_keep(&mut bg, w, h, 1);
            crate::lock::paint(&mut cv, &view);
        }
        shots.push(Shot {
            name: "showcase-droppy-lock-ambient".into(),
            w,
            h,
            rgba: flatten(&bg, w, h, Rgba::rgb(0, 0, 0)),
        });
    }

    if let Some(dir) = std::env::var_os("SOLYRA_RENDER_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for s in &shots {
            write_png(&dir, s);
        }
    }
}
