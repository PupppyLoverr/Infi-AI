//! Dynamic island: the black capsule that hugs the menubar centre and
//! opens into a 460×220 card with three icon tabs (Activities, Clipboard,
//! Shelf). `panel.rs` paints the capsule via [`paint_pill`]; this module
//! owns the card surface.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::shell::WaylandSurface;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, glass, ShellState};

pub const CARD_W: u32 = 460;
pub const CARD_H: u32 = 220;
pub const PILL_W: f64 = 120.0;
const PILL_ACTIVE_W: f64 = 200.0;
const PAD: f64 = 12.0;
const TAB: f64 = 28.0;
const BODY_Y: f64 = PAD + TAB + 8.0;
const ROW_H: f64 = 40.0;
const MAX_ROWS: usize = 4;
const MAX_CLIPS: usize = 10;
const CHIP_H: f64 = 26.0;
const CARD_R: f32 = 22.0;

type Rect = (f64, f64, f64, f64);

/// Capsule ↔ card morph length (v5 §2.5).
pub const MORPH: std::time::Duration = std::time::Duration::from_millis(280);

/// Eased morph progress: 0 = capsule, 1 = full card (also when idle).
pub fn morph(state: &ShellState) -> f32 {
    let Some((t0, opening)) = state.island_anim else {
        return 1.0;
    };
    let p = (t0.elapsed().as_secs_f32() / MORPH.as_secs_f32()).min(1.0);
    let p = if opening { p } else { 1.0 - p };
    1.0 - (1.0 - p).powi(3)
}

/// Visible card outline at morph progress `m`: grows from a capsule the
/// pill's size at the top centre to the whole 460×220 card.
pub fn morph_rect(m: f32) -> (f32, f32, f32, f32, f32) {
    let lerp = |a: f32, b: f32| a + (b - a) * m;
    let pw = PILL_W as f32;
    let ph = crate::PANEL_HEIGHT as f32;
    let w = lerp(pw, CARD_W as f32);
    let h = lerp(ph, CARD_H as f32);
    ((CARD_W as f32 - w) / 2.0, 0.0, w, h, lerp(ph / 2.0, CARD_R))
}

fn inside(r: Rect, x: f64, y: f64) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Activities,
    Clipboard,
    Shelf,
}

const TABS: [Tab; 3] = [Tab::Activities, Tab::Clipboard, Tab::Shelf];

/// What the collapsed capsule shows — the most urgent live activity.
#[derive(Debug, Clone, PartialEq)]
pub enum Pill {
    Idle,
    /// An agent waits for a decision: short verb ("Write file").
    Approval(String),
    Agent {
        name: String,
        elapsed: String,
        paused: bool,
        busy: bool,
    },
    Media {
        title: String,
        playing: bool,
    },
}

/// "Claude wants to write — ~/a.txt" → "Write file"; plain-English verb
/// for the compact capsule.
pub fn short_verb(summary: &str) -> String {
    let rest = summary
        .split_once(" wants to ")
        .or_else(|| summary.split_once(" wants "))
        .map(|(_, r)| r)
        .unwrap_or(summary);
    let (phrase, has_target) = match rest.split_once(" — ") {
        Some((p, _)) => (p, true),
        None => (rest, false),
    };
    let mut verb: String = phrase.trim().to_string();
    if has_target && matches!(verb.as_str(), "write" | "read" | "delete" | "move" | "copy") {
        verb.push_str(" file");
    }
    let mut out: String = verb.chars().take(18).collect();
    if verb.chars().count() > 18 {
        out.pop();
        out.push('…');
    }
    let mut c = out.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => "Approve".into(),
    }
}

pub fn pill(state: &ShellState) -> Pill {
    if let Some(n) = crate::activity::pending_approvals(&state.notifications).first() {
        return Pill::Approval(short_verb(&n.summary));
    }
    if let Some(a) = state
        .agents
        .iter()
        .find(|a| a.busy && !a.stopped)
        .or(state.agents.iter().find(|a| !a.stopped))
    {
        return Pill::Agent {
            name: a.agent.clone(),
            elapsed: crate::activity::elapsed(a.started),
            paused: a.paused,
            busy: a.busy,
        };
    }
    if let Some(m) = &state.media {
        return Pill::Media {
            title: m.title.clone(),
            playing: m.playing,
        };
    }
    Pill::Idle
}

/// True while the capsule animates (approval pulse, working agent ring).
pub fn pulsing(state: &ShellState) -> bool {
    !crate::LITE.with(|l| l.get())
        && matches!(
            pill(state),
            Pill::Approval(_)
                | Pill::Agent {
                    busy: true,
                    paused: false,
                    ..
                }
        )
}

pub fn pill_width(state: &ShellState) -> f64 {
    match pill(state) {
        Pill::Idle => PILL_W,
        _ => PILL_ACTIVE_W,
    }
}

/// Capsule rect inside the panel surface: centred, full bar height.
pub fn pill_rect(state: &ShellState) -> Rect {
    let (w, h) = state.panel_size;
    let pw = pill_width(state);
    ((w as f64 - pw) / 2.0, 0.0, pw, h as f64)
}

/// Restrained per-agent colour for its squircle avatar.
pub fn agent_colour(name: &str) -> Color {
    const P: [(u8, u8, u8); 6] = [
        (0x5E, 0x81, 0xF4),
        (0x3F, 0xA7, 0x8B),
        (0xD9, 0x7A, 0x4A),
        (0xB0, 0x6A, 0xD9),
        (0xD4, 0x5D, 0x79),
        (0x4A, 0x9F, 0xC8),
    ];
    let h = name
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let (r, g, b) = P[h as usize % P.len()];
    Color::from_rgba8(r, g, b, 0xFF)
}

const WARN: (u8, u8, u8) = (0xFF, 0x9F, 0x0A);
const WHITE: CtColor = CtColor::rgba(0xF4, 0xF4, 0xF6, 0xFF);
const WHITE_DIM: CtColor = CtColor::rgba(0xB4, 0xB4, 0xBC, 0xFF);

fn pulse_phase() -> f32 {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    (ms % 1400) as f32 / 1400.0
}

fn dot(pm: &mut PixmapMut<'_>, cx: f32, cy: f32, r: f32, c: Color) {
    draw::fill_round_rect(pm, cx - r, cy - r, r * 2.0, r * 2.0, r, c);
}

/// Paint the capsule into the menubar pixmap. Black in both themes.
pub fn paint_pill(
    pm: &mut PixmapMut<'_>,
    p: &Pill,
    (x, y, w, h): Rect,
    hover: bool,
    open: bool,
    dark: bool,
) {
    let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
    let a = if hover || open { 0xFF } else { 0xF2 };
    let bg = Color::from_rgba8(0x06, 0x06, 0x08, a);
    // Square top edge (it hangs off the screen edge), round bottom.
    draw::fill_round_rect(pm, x, y, w, h, h / 2.0, bg);
    draw::fill_round_rect(pm, x, y, w, h / 2.0, 0.0, bg);
    let lite = crate::LITE.with(|l| l.get());
    let phase = if lite { 0.0 } else { pulse_phase() };
    let breathe = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
    let cy = y + h / 2.0;
    match p {
        Pill::Idle => {
            dot(pm, x + w - 16.0, cy, 3.0, draw::accent(true));
        }
        Pill::Approval(verb) => {
            let (r, g, b) = WARN;
            if !lite {
                let ring = 4.0 + 5.0 * breathe;
                let al = (0xA0 as f32 * (1.0 - breathe)) as u8;
                draw::stroke_round_rect(
                    pm,
                    x + 16.0 - ring,
                    cy - ring,
                    ring * 2.0,
                    ring * 2.0,
                    ring,
                    1.5,
                    Color::from_rgba8(r, g, b, al),
                );
            }
            dot(pm, x + 16.0, cy, 4.0, Color::from_rgba8(r, g, b, 0xFF));
            draw::text(pm, x + 30.0, cy - 8.0, w - 40.0, 16.0, 12.0, verb, WHITE);
        }
        Pill::Agent {
            name,
            elapsed,
            paused,
            busy,
        } => {
            draw::fill_round_rect(pm, x + 8.0, cy - 9.0, 18.0, 18.0, 6.0, agent_colour(name));
            let initial: String = name.chars().take(1).collect::<String>().to_uppercase();
            draw::text_centered(pm, x + 8.0, cy - 7.5, 18.0, 15.0, 10.5, &initial, WHITE);
            let label: String = name.chars().take(14).collect();
            draw::text(pm, x + 32.0, cy - 8.0, w - 96.0, 16.0, 12.0, &label, WHITE);
            let right = if *paused {
                "Paused".to_string()
            } else {
                elapsed.clone()
            };
            let tw = draw::text_width(11.5, &right);
            let mut rx = x + w - 14.0 - tw;
            if *busy && !*paused && !lite {
                let al = (0x60 as f32 + 0x9F as f32 * breathe) as u8;
                let c = draw::accent(true);
                draw::stroke_round_rect(
                    pm,
                    rx - 16.0,
                    cy - 5.0,
                    10.0,
                    10.0,
                    5.0,
                    1.5,
                    Color::from_rgba8(
                        (c.red() * 255.0) as u8,
                        (c.green() * 255.0) as u8,
                        (c.blue() * 255.0) as u8,
                        al,
                    ),
                );
                rx = rx.max(x + 32.0);
            }
            draw::text(pm, rx, cy - 7.5, tw + 4.0, 16.0, 11.5, &right, WHITE_DIM);
        }
        Pill::Media { title, playing } => {
            let c = if *playing {
                draw::accent(true)
            } else {
                Color::from_rgba8(0x8A, 0x8A, 0x92, 0xFF)
            };
            dot(pm, x + 16.0, cy, 3.5, c);
            let t: String = title.chars().take(24).collect();
            draw::text(pm, x + 28.0, cy - 8.0, w - 40.0, 16.0, 12.0, &t, WHITE);
        }
    }
    let _ = dark;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Tab(Tab),
    /// (approval index, action index into its `actions`).
    Approve(usize, usize),
    AgentPause(usize),
    AgentStop(usize),
    Media,
    Clip(usize),
    Chip(usize),
    Backdrop,
}

enum Row {
    Approval {
        text: String,
        actions: Vec<String>,
    },
    Agent {
        name: String,
        doing: String,
        right: String,
        paused: bool,
        stopped: bool,
    },
    Media {
        text: String,
        playing: bool,
    },
}

fn rows(state: &ShellState) -> Vec<Row> {
    let mut out = Vec::new();
    for n in crate::activity::pending_approvals(&state.notifications) {
        out.push(Row::Approval {
            text: n.summary.clone(),
            actions: n.actions.iter().take(3).map(|(_, l)| l.clone()).collect(),
        });
    }
    for a in &state.agents {
        let doing = if a.stopped {
            "Stopped".to_string()
        } else if a.paused {
            "Paused".to_string()
        } else if a.busy && !a.tool.is_empty() {
            a.tool.clone()
        } else {
            format!("Idle · {} calls", a.calls)
        };
        out.push(Row::Agent {
            name: a.agent.clone(),
            doing,
            right: crate::activity::elapsed(a.started),
            paused: a.paused,
            stopped: a.stopped,
        });
    }
    if let Some(m) = &state.media {
        out.push(Row::Media {
            text: if m.artist.is_empty() {
                m.title.clone()
            } else {
                format!("{} — {}", m.title, m.artist)
            },
            playing: m.playing,
        });
    }
    out.truncate(MAX_ROWS);
    out
}

fn tab_rect(i: usize) -> Rect {
    (PAD + i as f64 * (TAB + 6.0), PAD, TAB, TAB)
}

fn row_y(i: usize) -> f64 {
    BODY_Y + i as f64 * ROW_H
}

/// Approval action buttons, right-aligned (labels from the caller).
fn action_rects(actions: &[String], y: f64) -> Vec<Rect> {
    let mut rx = CARD_W as f64 - PAD;
    let mut out = Vec::new();
    for l in actions.iter().rev() {
        let bw = draw::text_width(11.0, l) as f64 + 18.0;
        rx -= bw;
        out.push((rx, y + 8.0, bw, ROW_H - 16.0));
        rx -= 6.0;
    }
    out.reverse();
    out
}

/// Circular 28px icon buttons at the right of an agent / media row.
fn icon_btn(slot: usize, y: f64) -> Rect {
    let x = CARD_W as f64 - PAD - TAB - slot as f64 * (TAB + 6.0);
    (x, y + (ROW_H - TAB) / 2.0, TAB, TAB)
}

fn clip_rect(i: usize) -> Rect {
    let cols = 5.0;
    let gap = 8.0;
    let cw = (CARD_W as f64 - PAD * 2.0 - gap * (cols - 1.0)) / cols;
    let ch = (CARD_H as f64 - BODY_Y - PAD - gap) / 2.0;
    let (c, r) = ((i % 5) as f64, (i / 5) as f64);
    (PAD + c * (cw + gap), BODY_Y + r * (ch + gap), cw, ch)
}

fn chip_rects(files: &[String]) -> Vec<Rect> {
    let (mut x, mut y) = (PAD + 8.0, BODY_Y + 8.0);
    let right = CARD_W as f64 - PAD - 8.0;
    let mut out = Vec::new();
    for f in files {
        let name = f.rsplit('/').next().unwrap_or(f);
        let w = (draw::text_width(11.5, name) as f64 + 22.0).min(right - PAD - 8.0);
        if x + w > right {
            x = PAD + 8.0;
            y += CHIP_H + 6.0;
        }
        if y + CHIP_H > CARD_H as f64 - PAD - 8.0 {
            break;
        }
        out.push((x, y, w, CHIP_H));
        x += w + 6.0;
    }
    out
}

pub fn card_height(_state: &ShellState) -> u32 {
    CARD_H
}

pub fn hit_test(state: &ShellState, x: f64, y: f64) -> Hit {
    if x < 0.0 || y < 0.0 || x >= CARD_W as f64 || y >= CARD_H as f64 {
        return Hit::Backdrop;
    }
    for (i, t) in TABS.iter().enumerate() {
        if inside(tab_rect(i), x, y) {
            return Hit::Tab(*t);
        }
    }
    match state.island_tab {
        Tab::Activities => {
            let mut appr = 0;
            let mut agent = 0;
            for (i, r) in rows(state).iter().enumerate() {
                let ry = row_y(i);
                match r {
                    Row::Approval { actions, .. } => {
                        for (k, b) in action_rects(actions, ry).into_iter().enumerate() {
                            if inside(b, x, y) {
                                return Hit::Approve(appr, k);
                            }
                        }
                        appr += 1;
                    }
                    Row::Agent { stopped, .. } => {
                        if !stopped {
                            if inside(icon_btn(0, ry), x, y) {
                                return Hit::AgentStop(agent);
                            }
                            if inside(icon_btn(1, ry), x, y) {
                                return Hit::AgentPause(agent);
                            }
                        }
                        agent += 1;
                    }
                    Row::Media { .. } => {
                        if inside(icon_btn(0, ry), x, y) {
                            return Hit::Media;
                        }
                    }
                }
            }
        }
        Tab::Clipboard => {
            for i in 0..state.clip_history.len().min(MAX_CLIPS) {
                if inside(clip_rect(i), x, y) {
                    return Hit::Clip(i);
                }
            }
        }
        Tab::Shelf => {
            for (i, r) in chip_rects(&state.staged_files).into_iter().enumerate() {
                if inside(r, x, y) {
                    return Hit::Chip(i);
                }
            }
        }
    }
    Hit::Backdrop
}

fn tab_glyph(pm: &mut PixmapMut<'_>, t: Tab, r: Rect, c: Color) {
    let (x, y) = (r.0 as f32 + 7.0, r.1 as f32 + 7.0);
    match t {
        Tab::Activities => {
            // Three activity bars.
            for (i, hgt) in [6.0f32, 12.0, 9.0].iter().enumerate() {
                let bx = x + 1.5 + i as f32 * 5.0;
                draw::fill_round_rect(pm, bx, y + 13.0 - hgt, 3.0, *hgt, 1.5, c);
            }
        }
        Tab::Clipboard => crate::icons::icon(pm, "clipboard", x - 1.0, y - 1.0, 16.0, c),
        Tab::Shelf => {
            // Tray: open box outline.
            draw::stroke_round_rect(pm, x, y + 5.0, 14.0, 9.0, 2.5, 1.5, c);
            draw::fill_round_rect(pm, x + 4.0, y + 1.0, 6.0, 1.5, 0.75, c);
        }
    }
}

fn btn_glyph(pm: &mut PixmapMut<'_>, kind: &str, r: Rect, c: Color) {
    let (cx, cy) = ((r.0 + r.2 / 2.0) as f32, (r.1 + r.3 / 2.0) as f32);
    match kind {
        "pause" => {
            draw::fill_round_rect(pm, cx - 4.5, cy - 5.0, 3.0, 10.0, 1.0, c);
            draw::fill_round_rect(pm, cx + 1.5, cy - 5.0, 3.0, 10.0, 1.0, c);
        }
        "stop" => draw::fill_round_rect(pm, cx - 4.5, cy - 4.5, 9.0, 9.0, 2.0, c),
        _ => {
            let mut pb = PathBuilder::new();
            pb.move_to(cx - 3.5, cy - 5.5);
            pb.line_to(cx + 5.5, cy);
            pb.line_to(cx - 3.5, cy + 5.5);
            pb.close();
            if let Some(path) = pb.finish() {
                let mut paint = Paint::default();
                paint.set_color(c);
                paint.anti_alias = true;
                pm.fill_path(&path, &paint, FillRule::Winding, draw::xf(), None);
            }
        }
    }
}

fn ellipsize(s: &str, w: f64, size: f32) -> String {
    let flat = s.replace('\n', " ");
    if draw::text_width(size, &flat) as f64 <= w {
        return flat;
    }
    let mut out = String::new();
    for ch in flat.chars() {
        out.push(ch);
        if draw::text_width(size, &out) as f64 > w - 10.0 {
            out.pop();
            break;
        }
    }
    out.push('…');
    out
}

pub fn draw(state: &mut ShellState) {
    let (w, h) = (CARD_W, CARD_H);
    let Some(layer) = state.island_surface.clone() else {
        return;
    };
    let tab = state.island_tab;
    let hover = state.island_hover;
    let rows = rows(state);
    let clips: Vec<String> = state.clip_history.iter().take(MAX_CLIPS).cloned().collect();
    let files = state.staged_files.clone();
    let chips = chip_rects(&files);
    let dnd = state.dnd_on_island;
    let screen_x = ((state.panel_size.0 as f32 - w as f32) / 2.0).max(0.0);
    let m = morph(state);

    let (pw, ph) = draw::phys(w, h);
    let Ok((buffer, canvas)) = state.pool.create_buffer(
        pw as i32,
        ph as i32,
        pw as i32 * 4,
        wl_shm::Format::Abgr8888,
    ) else {
        tracing::warn!("island: pool create_buffer failed");
        return;
    };
    let Some(mut pm) = PixmapMut::from_bytes(canvas, pw, ph) else {
        return;
    };
    pm.fill(Color::TRANSPARENT);

    let (wf, hf) = (w as f32, h as f32);
    draw::shadow(&mut pm, 0.0, 0.0, wf, hf, CARD_R);
    glass::fill_glass(
        &mut pm,
        &crate::ShellState::wallpaper_name(),
        0.0,
        0.0,
        wf,
        hf,
        CARD_R,
        screen_x,
        crate::PANEL_HEIGHT as f32 + 4.0,
        true,
        Color::from_rgba8(0x08, 0x08, 0x0B, 0xE6),
    );
    // Black glass: deepen the dark tint so it reads as the island, not a panel.
    draw::fill_round_rect(
        &mut pm,
        0.0,
        0.0,
        wf,
        hf,
        CARD_R,
        Color::from_rgba8(0, 0, 0, 0x8C),
    );
    draw::stroke_round_rect(
        &mut pm,
        0.5,
        0.5,
        wf - 1.0,
        hf - 1.0,
        CARD_R,
        1.0,
        Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x1A),
    );

    let glyph_on = Color::from_rgba8(0xF4, 0xF4, 0xF6, 0xFF);
    let glyph_off = Color::from_rgba8(0xA0, 0xA0, 0xA8, 0xFF);
    let raised = Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x1F);
    let hovered = Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x12);
    for (i, t) in TABS.iter().enumerate() {
        let r = tab_rect(i);
        let sel = *t == tab;
        if sel || hover == Some(Hit::Tab(*t)) {
            draw::fill_round_rect(
                &mut pm,
                r.0 as f32,
                r.1 as f32,
                r.2 as f32,
                r.3 as f32,
                (TAB / 2.0) as f32,
                if sel { raised } else { hovered },
            );
        }
        tab_glyph(&mut pm, *t, r, if sel { glyph_on } else { glyph_off });
    }

    let body_w = CARD_W as f64 - PAD * 2.0;
    let empty = |pm: &mut PixmapMut<'_>, msg: &str| {
        draw::text_centered(
            pm,
            PAD as f32,
            (BODY_Y + 58.0) as f32,
            body_w as f32,
            18.0,
            12.5,
            msg,
            WHITE_DIM,
        );
    };
    match tab {
        Tab::Activities => {
            if rows.is_empty() {
                empty(&mut pm, "No live activities");
            }
            let (mut appr, mut agent) = (0, 0);
            for (i, r) in rows.iter().enumerate() {
                let ry = row_y(i);
                let cy = (ry + ROW_H / 2.0) as f32;
                if i > 0 {
                    draw::fill_round_rect(
                        &mut pm,
                        PAD as f32,
                        ry as f32,
                        body_w as f32,
                        1.0,
                        0.0,
                        Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x14),
                    );
                }
                match r {
                    Row::Approval { text, actions } => {
                        let (cr, cg, cb) = WARN;
                        dot(
                            &mut pm,
                            PAD as f32 + 9.0,
                            cy,
                            4.0,
                            Color::from_rgba8(cr, cg, cb, 0xFF),
                        );
                        let rects = action_rects(actions, ry);
                        let text_w =
                            rects.first().map(|b| b.0).unwrap_or(CARD_W as f64) - PAD - 30.0;
                        draw::text(
                            &mut pm,
                            (PAD + 24.0) as f32,
                            cy - 8.0,
                            text_w as f32,
                            16.0,
                            12.0,
                            &ellipsize(text, text_w, 12.0),
                            WHITE,
                        );
                        for (k, (b, l)) in rects.iter().zip(actions).enumerate() {
                            let primary = k == actions.len() - 2 || actions.len() == 1;
                            let hov = hover == Some(Hit::Approve(appr, k));
                            let bg = if primary {
                                draw::accent(true)
                            } else if hov {
                                Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x30)
                            } else {
                                raised
                            };
                            draw::fill_round_rect(
                                &mut pm,
                                b.0 as f32,
                                b.1 as f32,
                                b.2 as f32,
                                b.3 as f32,
                                (b.3 / 2.0) as f32,
                                bg,
                            );
                            draw::text_centered(
                                &mut pm,
                                b.0 as f32,
                                (b.1 + b.3 / 2.0 - 7.5) as f32,
                                b.2 as f32,
                                15.0,
                                11.0,
                                l,
                                WHITE,
                            );
                        }
                        appr += 1;
                    }
                    Row::Agent {
                        name,
                        doing,
                        right,
                        paused,
                        stopped,
                    } => {
                        draw::fill_round_rect(
                            &mut pm,
                            PAD as f32,
                            cy - 11.0,
                            22.0,
                            22.0,
                            7.0,
                            agent_colour(name),
                        );
                        let initial = name.chars().take(1).collect::<String>().to_uppercase();
                        draw::text_centered(
                            &mut pm,
                            PAD as f32,
                            cy - 8.0,
                            22.0,
                            16.0,
                            11.5,
                            &initial,
                            WHITE,
                        );
                        let btns = if *stopped { 0.0 } else { 2.0 * (TAB + 6.0) };
                        let right_w = draw::text_width(11.5, right) as f64 + 12.0;
                        let text_w = body_w - 32.0 - btns - right_w;
                        draw::text_bold(
                            &mut pm,
                            (PAD + 32.0) as f32,
                            cy - 15.0,
                            text_w as f32,
                            16.0,
                            12.0,
                            &ellipsize(name, text_w, 12.0),
                            WHITE,
                        );
                        draw::text(
                            &mut pm,
                            (PAD + 32.0) as f32,
                            cy + 0.5,
                            text_w as f32,
                            15.0,
                            11.0,
                            &ellipsize(doing, text_w, 11.0),
                            WHITE_DIM,
                        );
                        let rx = CARD_W as f64 - PAD - btns - right_w;
                        draw::text(
                            &mut pm,
                            rx as f32,
                            cy - 7.5,
                            right_w as f32,
                            15.0,
                            11.5,
                            right,
                            WHITE_DIM,
                        );
                        if !stopped {
                            for (slot, kind, hit) in [
                                (0, "stop", Hit::AgentStop(agent)),
                                (
                                    1,
                                    if *paused { "play" } else { "pause" },
                                    Hit::AgentPause(agent),
                                ),
                            ] {
                                let b = icon_btn(slot, ry);
                                let bg = if hover == Some(hit) {
                                    Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x30)
                                } else {
                                    raised
                                };
                                draw::fill_round_rect(
                                    &mut pm,
                                    b.0 as f32,
                                    b.1 as f32,
                                    b.2 as f32,
                                    b.3 as f32,
                                    (TAB / 2.0) as f32,
                                    bg,
                                );
                                btn_glyph(&mut pm, kind, b, glyph_on);
                            }
                        }
                        agent += 1;
                    }
                    Row::Media { text, playing } => {
                        dot(&mut pm, PAD as f32 + 9.0, cy, 3.5, draw::accent(true));
                        let text_w = body_w - 32.0 - TAB - 6.0;
                        draw::text(
                            &mut pm,
                            (PAD + 24.0) as f32,
                            cy - 8.0,
                            text_w as f32,
                            16.0,
                            12.0,
                            &ellipsize(text, text_w, 12.0),
                            WHITE,
                        );
                        let b = icon_btn(0, ry);
                        let bg = if hover == Some(Hit::Media) {
                            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x30)
                        } else {
                            raised
                        };
                        draw::fill_round_rect(
                            &mut pm,
                            b.0 as f32,
                            b.1 as f32,
                            b.2 as f32,
                            b.3 as f32,
                            (TAB / 2.0) as f32,
                            bg,
                        );
                        btn_glyph(
                            &mut pm,
                            if *playing { "pause" } else { "play" },
                            b,
                            glyph_on,
                        );
                    }
                }
            }
        }
        Tab::Clipboard => {
            if clips.is_empty() {
                empty(&mut pm, "Copied text appears here");
            }
            for (i, t) in clips.iter().enumerate() {
                let r = clip_rect(i);
                let bg = if hover == Some(Hit::Clip(i)) {
                    Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x2A)
                } else {
                    Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x14)
                };
                draw::fill_round_rect(
                    &mut pm, r.0 as f32, r.1 as f32, r.2 as f32, r.3 as f32, 10.0, bg,
                );
                // Up to three lines of the snippet, word-agnostic wrap.
                let flat: String = t.replace('\n', " ").trim().chars().take(120).collect();
                let max_w = r.2 - 16.0;
                let mut lines: Vec<String> = Vec::new();
                let mut cur = String::new();
                for ch in flat.chars() {
                    cur.push(ch);
                    if draw::text_width(11.0, &cur) as f64 > max_w {
                        cur.pop();
                        lines.push(std::mem::take(&mut cur));
                        cur.push(ch);
                        if lines.len() == 3 {
                            break;
                        }
                    }
                }
                if lines.len() < 3 && !cur.is_empty() {
                    lines.push(cur);
                } else if lines.len() == 3 {
                    if let Some(last) = lines.last_mut() {
                        *last = ellipsize(&format!("{last}…"), max_w, 11.0);
                    }
                }
                for (li, l) in lines.iter().enumerate() {
                    draw::text(
                        &mut pm,
                        (r.0 + 8.0) as f32,
                        (r.1 + 8.0 + li as f64 * 15.0) as f32,
                        max_w as f32,
                        15.0,
                        11.0,
                        l,
                        if li == 0 { WHITE } else { WHITE_DIM },
                    );
                }
            }
        }
        Tab::Shelf => {
            let zone = (PAD, BODY_Y, body_w, CARD_H as f64 - BODY_Y - PAD);
            if dnd {
                draw::fill_round_rect(
                    &mut pm,
                    zone.0 as f32,
                    zone.1 as f32,
                    zone.2 as f32,
                    zone.3 as f32,
                    14.0,
                    Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x1A),
                );
            }
            draw::stroke_round_rect(
                &mut pm,
                zone.0 as f32,
                zone.1 as f32,
                zone.2 as f32,
                zone.3 as f32,
                14.0,
                1.0,
                Color::from_rgba8(0xFF, 0xFF, 0xFF, if dnd { 0x60 } else { 0x26 }),
            );
            if files.is_empty() {
                empty(&mut pm, "Drop files here to keep them handy");
            }
            for (i, r) in chips.iter().enumerate() {
                let bg = if hover == Some(Hit::Chip(i)) {
                    Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x30)
                } else {
                    raised
                };
                draw::fill_round_rect(
                    &mut pm,
                    r.0 as f32,
                    r.1 as f32,
                    r.2 as f32,
                    r.3 as f32,
                    (CHIP_H / 2.0) as f32,
                    bg,
                );
                let name = files[i].rsplit('/').next().unwrap_or(&files[i]);
                draw::text(
                    &mut pm,
                    (r.0 + 11.0) as f32,
                    (r.1 + 5.0) as f32,
                    (r.2 - 22.0) as f32,
                    16.0,
                    11.5,
                    &ellipsize(name, r.2 - 22.0, 11.5),
                    WHITE,
                );
            }
        }
    }

    if m < 1.0 {
        // Mid-morph: clip the painted card to the growing capsule outline.
        let (mx, my, mw, mh, mr) = morph_rect(m);
        if let (Some(mut mask), Some(path)) = (
            tiny_skia::Mask::new(pw, ph),
            draw::round_rect_path(mx, my, mw, mh, mr),
        ) {
            mask.fill_path(&path, FillRule::Winding, true, draw::xf());
            pm.apply_mask(&mask);
        }
    }

    state.set_viewport(&layer.wl_surface().clone(), w, h);
    buffer.attach_to(layer.wl_surface()).ok();
    layer.wl_surface().damage_buffer(0, 0, pw as i32, ph as i32);
    layer.wl_surface().commit();
}

/// Pointer motion → hovered control changes.
pub fn hover(state: &mut ShellState, x: f64, y: f64) -> bool {
    let h = match hit_test(state, x, y) {
        Hit::Backdrop => None,
        h => Some(h),
    };
    if h != state.island_hover {
        state.island_hover = h;
        true
    } else {
        false
    }
}

/// Esc closes the card; Left/Right switch tabs.
pub fn key_press(state: &mut ShellState, event: KeyEvent) {
    let i = TABS
        .iter()
        .position(|t| *t == state.island_tab)
        .unwrap_or(0);
    match event.keysym {
        Keysym::Escape => state.close_island(),
        Keysym::Left => {
            state.island_tab = TABS[(i + 2) % 3];
            state.island_dirty = true;
        }
        Keysym::Right => {
            state.island_tab = TABS[(i + 1) % 3];
            state.island_dirty = true;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbs_are_short_plain_english() {
        assert_eq!(
            short_verb("claude wants to write — ~/notes.txt"),
            "Write file"
        );
        assert_eq!(short_verb("claude wants to launch — firefox"), "Launch");
        assert_eq!(
            short_verb("claude wants a screenshot of the desktop"),
            "A screenshot of t…"
        );
        assert_eq!(
            short_verb("claude wants to change setting dark"),
            "Change setting da…"
        );
    }

    #[test]
    fn rows_and_controls_fit_the_card() {
        for i in 0..MAX_ROWS {
            assert!(row_y(i) + ROW_H <= CARD_H as f64 - PAD + 1.0);
        }
        let labels: Vec<String> = ["Deny", "Allow once", "Always allow"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let r = action_rects(&labels, row_y(0));
        assert!(
            r[0].0 > CARD_W as f64 / 3.0,
            "approval text keeps a third of the row"
        );
        assert!(r[2].0 + r[2].2 <= CARD_W as f64 - PAD + 0.01);
        assert!(r[0].0 + r[0].2 < r[1].0 && r[1].0 + r[1].2 < r[2].0);
        let last = clip_rect(MAX_CLIPS - 1);
        assert!(last.1 + last.3 <= CARD_H as f64 - PAD + 0.01);
        assert!(last.0 + last.2 <= CARD_W as f64 - PAD + 0.01);
        assert!(tab_rect(2).0 + TAB < CARD_W as f64 / 2.0);
    }

    #[test]
    fn morph_runs_from_pill_to_card() {
        let (x, _, w, h, r) = morph_rect(0.0);
        assert_eq!((w, h), (PILL_W as f32, crate::PANEL_HEIGHT as f32));
        assert_eq!(x, (CARD_W as f32 - PILL_W as f32) / 2.0);
        assert_eq!(r, h / 2.0);
        assert_eq!(
            morph_rect(1.0),
            (0.0, 0.0, CARD_W as f32, CARD_H as f32, CARD_R)
        );
    }

    #[test]
    fn agent_colours_are_stable() {
        assert_eq!(agent_colour("claude"), agent_colour("claude"));
    }
}
