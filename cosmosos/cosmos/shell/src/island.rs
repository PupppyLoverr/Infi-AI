//! Dynamic island — the droppy-style card that expands from the menubar
//! pill: live activities (approvals, agents, now playing), clipboard
//! history and staged files (drag & drop tray). The pill itself is drawn inside the menubar by
//! `panel.rs`; this module owns the expanded card surface.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::shell::WaylandSurface;
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, glass, ShellState};

const CARD_W: f64 = 380.0;
const PAD: f64 = 12.0;
const HEAD_H: f64 = 26.0;
const ROW_H: f64 = 34.0;
const SEC_H: f64 = 22.0;
const CARD_R: f32 = 14.0;
const MAX_CLIP_ROWS: usize = 8;
const MAX_FILE_ROWS: usize = 6;

/// What the idle pill shows — the most urgent live activity, if any.
pub struct PillActivity {
    pub label: String,
    /// Accent dot (approval waiting, agent working, media playing) vs dim.
    pub live: bool,
}

pub fn pill_activity(state: &ShellState) -> Option<PillActivity> {
    let approvals = crate::activity::pending_approvals(&state.notifications);
    if let Some(n) = approvals.first() {
        let label = if approvals.len() > 1 {
            format!("{} approvals waiting", approvals.len())
        } else {
            n.summary.chars().take(30).collect()
        };
        return Some(PillActivity { label, live: true });
    }
    if let Some(a) = state
        .agents
        .iter()
        .find(|a| a.busy)
        .or(state.agents.first())
    {
        let state_txt = if a.paused {
            " · paused".to_string()
        } else {
            String::new()
        };
        return Some(PillActivity {
            label: format!(
                "{} · {}{state_txt}",
                a.agent,
                crate::activity::elapsed(a.started)
            ),
            live: !a.paused,
        });
    }
    if let Some(m) = &state.media {
        return Some(PillActivity {
            label: m.title.chars().take(26).collect(),
            live: m.playing,
        });
    }
    None
}

/// Width of the menubar pill (idle island) — centred in the panel.
pub fn pill_width(state: &ShellState) -> f64 {
    if let Some(a) = pill_activity(state) {
        return (30.0 + a.label.chars().count() as f64 * 6.6).clamp(56.0, 280.0);
    }
    let snip = state
        .clip_history
        .front()
        .map(|t| t.chars().take(22).collect::<String>())
        .unwrap_or_default();
    let staged = state.staged_files.len();
    let base = 56.0
        + if snip.is_empty() {
            0.0
        } else {
            snip.len() as f64 * 7.0
        };
    (base + if staged > 0 { 26.0 } else { 0.0 }).clamp(56.0, 260.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ActKind {
    Approval(usize),
    Agent(usize),
    Media,
}

/// One LIVE ACTIVITY row in the card.
struct ActRow {
    kind: ActKind,
    text: String,
    right: String,
    button: Option<&'static str>,
    live: bool,
}

impl ActRow {
    fn hit(&self) -> Hit {
        match self.kind {
            ActKind::Approval(i) => Hit::ApprovalRow(i),
            ActKind::Agent(i) => Hit::AgentRow(i),
            ActKind::Media => Hit::MediaRow,
        }
    }
}

fn act_rows(state: &ShellState) -> Vec<ActRow> {
    let mut rows = Vec::new();
    for (i, n) in crate::activity::pending_approvals(&state.notifications)
        .into_iter()
        .take(2)
        .enumerate()
    {
        rows.push(ActRow {
            kind: ActKind::Approval(i),
            text: n.summary.clone(),
            right: "Waiting".into(),
            button: None,
            live: true,
        });
    }
    for (i, a) in state.agents.iter().take(4).enumerate() {
        let doing = if a.paused {
            "paused".to_string()
        } else if a.busy && !a.tool.is_empty() {
            a.tool.clone()
        } else {
            format!("idle · {} calls", a.calls)
        };
        rows.push(ActRow {
            kind: ActKind::Agent(i),
            text: format!("{} · {doing}", a.agent),
            right: crate::activity::elapsed(a.started),
            button: Some(if a.paused { "Resume" } else { "Pause" }),
            live: a.busy && !a.paused,
        });
    }
    if let Some(m) = &state.media {
        rows.push(ActRow {
            kind: ActKind::Media,
            text: if m.artist.is_empty() {
                m.title.clone()
            } else {
                format!("{} — {}", m.title, m.artist)
            },
            right: String::new(),
            button: Some(if m.playing { "Pause" } else { "Play" }),
            live: m.playing,
        });
    }
    rows
}

fn act_height(rows: &[ActRow]) -> f64 {
    if rows.is_empty() {
        0.0
    } else {
        SEC_H + rows.len() as f64 * ROW_H
    }
}

/// Pill rect inside the panel surface (centred horizontally).
pub fn pill_rect(state: &ShellState) -> (f64, f64, f64, f64) {
    let (w, h) = state.panel_size;
    let pw = pill_width(state);
    ((w as f64 - pw) / 2.0, 4.0, pw, h as f64 - 8.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    ApprovalRow(usize),
    AgentRow(usize),
    MediaRow,
    ClipRow(usize),
    FileRow(usize),
    Backdrop,
}

fn theme(dark: bool) -> (Color, Color, Color, Color, CtColor, CtColor) {
    if dark {
        (
            Color::from_rgba8(0x1A, 0x1B, 0x1E, 0xF2),
            draw::accent_soft(true),
            Color::from_rgba8(0x3C, 0x3D, 0x42, 0xFF),
            Color::from_rgba8(0x2A, 0x2B, 0x30, 0xFF),
            CtColor::rgba(0xEC, 0xEC, 0xEE, 0xFF),
            CtColor::rgba(0x8C, 0x8C, 0x92, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xFA, 0xFA, 0xFB, 0xF6),
            draw::accent_soft(false),
            Color::from_rgba8(0xC4, 0xC4, 0xC8, 0xFF),
            Color::from_rgba8(0xEF, 0xEF, 0xF2, 0xFF),
            CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
            CtColor::rgba(0x6A, 0x6A, 0x6E, 0xFF),
        )
    }
}

fn clip_rows(state: &ShellState) -> usize {
    state.clip_history.len().min(MAX_CLIP_ROWS)
}

fn file_rows(state: &ShellState) -> usize {
    state.staged_files.len().min(MAX_FILE_ROWS)
}

/// Card height for the current content.
pub fn card_height(state: &ShellState) -> u32 {
    let mut h = PAD + HEAD_H + act_height(&act_rows(state));
    if clip_rows(state) > 0 || !state.staged_files.is_empty() || true {
        // CLIPBOARD section is always shown (empty state text when empty).
        h += SEC_H + clip_rows(state).max(1) as f64 * ROW_H;
    }
    if !state.staged_files.is_empty() {
        h += SEC_H + file_rows(state) as f64 * ROW_H;
    }
    (h + PAD) as u32
}

fn clip_row_y(state: &ShellState, i: usize) -> f64 {
    PAD + HEAD_H + act_height(&act_rows(state)) + SEC_H + i as f64 * ROW_H
}

fn file_row_y(state: &ShellState, i: usize) -> f64 {
    clip_row_y(state, clip_rows(state).max(1)) + SEC_H + i as f64 * ROW_H
}

pub fn hit_test(state: &ShellState, x: f64, y: f64) -> Hit {
    let (cw, ch) = (CARD_W, card_height(state) as f64);
    if x < 0.0 || x >= cw || y < 0.0 || y >= ch {
        return Hit::Backdrop;
    }
    for (i, r) in act_rows(state).iter().enumerate() {
        let ry = PAD + HEAD_H + SEC_H + i as f64 * ROW_H;
        if y >= ry && y < ry + ROW_H {
            return r.hit();
        }
    }
    for i in 0..clip_rows(state) {
        let ry = clip_row_y(state, i);
        if y >= ry && y < ry + ROW_H {
            return Hit::ClipRow(i);
        }
    }
    for i in 0..file_rows(state) {
        let ry = file_row_y(state, i);
        if y >= ry && y < ry + ROW_H {
            return Hit::FileRow(i);
        }
    }
    Hit::Backdrop
}

pub fn draw(state: &mut ShellState) {
    let (w, h) = (CARD_W as u32, card_height(state));
    if w == 0 {
        return;
    }
    let Some(layer) = state.island_surface.clone() else {
        return;
    };
    let dark = state.dark;
    let (card, sel, sep, row_bg, fg, fg_dim) = theme(dark);
    let clips: Vec<String> = state
        .clip_history
        .iter()
        .take(MAX_CLIP_ROWS)
        .cloned()
        .collect();
    let files: Vec<String> = state
        .staged_files
        .iter()
        .take(MAX_FILE_ROWS)
        .cloned()
        .collect();
    let hover = state.island_hover;
    // Staged-file rows sit below the clipboard section — precompute the
    // base y before the pool's mutable borrow below.
    let files_base_y = clip_row_y(state, clip_rows(state).max(1)) + SEC_H;
    let act = act_rows(state);
    let act_h = act_height(&act);

    let (pw, ph) = draw::phys(w, h);
    let stride = pw as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Abgr8888)
    else {
        tracing::warn!("island: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, pw, ph) else {
        return;
    };
    pixmap.fill(Color::TRANSPARENT);

    draw::shadow(&mut pixmap, 0.0, 0.0, w as f32, h as f32, CARD_R);
    glass::fill_glass(
        &mut pixmap,
        &crate::ShellState::wallpaper_name(),
        0.0,
        0.0,
        w as f32,
        h as f32,
        CARD_R,
        ((state.panel_size.0 as f32 - w as f32) / 2.0).max(0.0),
        crate::PANEL_HEIGHT as f32 + 4.0,
        state.dark,
        card,
    );
    draw::stroke_round_rect(&mut pixmap, 0.0, 0.0, w as f32, h as f32, CARD_R, 1.0, sep);

    draw::text_bold(
        &mut pixmap,
        PAD as f32,
        (PAD + 3.0) as f32,
        (CARD_W - PAD * 2.0) as f32,
        18.0,
        13.0,
        "Dynamic Island",
        fg,
    );

    // LIVE ACTIVITY section (approvals, agents, now playing)
    let mut y = PAD + HEAD_H;
    if !act.is_empty() {
        draw::text(
            &mut pixmap,
            PAD as f32,
            (y + 4.0) as f32,
            (CARD_W - PAD * 2.0) as f32,
            14.0,
            10.5,
            "LIVE ACTIVITY",
            fg_dim,
        );
        y += SEC_H;
        let accent = draw::accent(dark);
        let dim = Color::from_rgba8(fg_dim.r(), fg_dim.g(), fg_dim.b(), 0xB0);
        for (i, r) in act.iter().enumerate() {
            let ry = y + i as f64 * ROW_H;
            if hover == Some(r.hit()) {
                draw::fill_round_rect(
                    &mut pixmap,
                    6.0,
                    (ry + 2.0) as f32,
                    (CARD_W - 12.0) as f32,
                    (ROW_H - 4.0) as f32,
                    8.0,
                    sel,
                );
            }
            draw::fill_round_rect(
                &mut pixmap,
                (PAD + 4.0) as f32,
                (ry + ROW_H / 2.0 - 4.0) as f32,
                8.0,
                8.0,
                4.0,
                if r.live { accent } else { dim },
            );
            let btn_w = 64.0;
            let right_w = if r.right.is_empty() { 0.0 } else { 58.0 };
            let text_w = CARD_W
                - PAD * 2.0
                - 22.0
                - right_w
                - if r.button.is_some() { btn_w + 6.0 } else { 0.0 };
            let max_chars = (text_w / 6.6) as usize;
            let mut label: String = r.text.replace('\n', " ").chars().take(max_chars).collect();
            if r.text.chars().count() > max_chars && max_chars > 1 {
                label.pop();
                label.push('…');
            }
            draw::text(
                &mut pixmap,
                (PAD + 22.0) as f32,
                (ry + 9.0) as f32,
                text_w as f32,
                16.0,
                12.0,
                &label,
                fg,
            );
            let mut rx = CARD_W - PAD;
            if let Some(b) = r.button {
                rx -= btn_w;
                draw::fill_round_rect(
                    &mut pixmap,
                    rx as f32,
                    (ry + 6.0) as f32,
                    btn_w as f32,
                    (ROW_H - 12.0) as f32,
                    ((ROW_H - 12.0) / 2.0) as f32,
                    row_bg,
                );
                let tw = b.chars().count() as f64 * 6.4;
                draw::text(
                    &mut pixmap,
                    (rx + (btn_w - tw) / 2.0) as f32,
                    (ry + 10.0) as f32,
                    btn_w as f32,
                    14.0,
                    11.0,
                    b,
                    fg,
                );
                rx -= 6.0;
            }
            if !r.right.is_empty() {
                draw::text(
                    &mut pixmap,
                    (rx - right_w + 6.0) as f32,
                    (ry + 10.0) as f32,
                    right_w as f32,
                    14.0,
                    11.0,
                    &r.right,
                    fg_dim,
                );
            }
        }
        y += act.len() as f64 * ROW_H;
    }

    // CLIPBOARD section
    draw::text(
        &mut pixmap,
        PAD as f32,
        (y + 4.0) as f32,
        (CARD_W - PAD * 2.0) as f32,
        14.0,
        10.5,
        "CLIPBOARD",
        fg_dim,
    );
    y += SEC_H;
    if clips.is_empty() {
        draw::text(
            &mut pixmap,
            PAD as f32,
            (y + 8.0) as f32,
            (CARD_W - PAD * 2.0) as f32,
            16.0,
            12.0,
            "Copy something — it lands here.",
            fg_dim,
        );
        y += ROW_H;
    } else {
        for (i, text) in clips.iter().enumerate() {
            let ry = PAD + HEAD_H + act_h + SEC_H + i as f64 * ROW_H;
            if hover == Some(Hit::ClipRow(i)) {
                draw::fill_round_rect(
                    &mut pixmap,
                    6.0,
                    (ry + 2.0) as f32,
                    (CARD_W - 12.0) as f32,
                    (ROW_H - 4.0) as f32,
                    8.0,
                    sel,
                );
            }
            draw::fill_round_rect(
                &mut pixmap,
                (PAD + 2.0) as f32,
                (ry + 7.0) as f32,
                20.0,
                20.0,
                6.0,
                row_bg,
            );
            draw::text(
                &mut pixmap,
                (PAD + 30.0) as f32,
                (ry + 9.0) as f32,
                (CARD_W - PAD - 38.0) as f32,
                16.0,
                12.0,
                &text.replace('\n', " "),
                fg,
            );
        }
        y += clips.len() as f64 * ROW_H;
    }

    // FILES section (only when files are staged)
    if !files.is_empty() {
        draw::text(
            &mut pixmap,
            PAD as f32,
            (y + 4.0) as f32,
            (CARD_W - PAD * 2.0) as f32,
            14.0,
            10.5,
            "STAGED FILES",
            fg_dim,
        );
        for (i, path) in files.iter().enumerate() {
            let ry = files_base_y + i as f64 * ROW_H;
            if hover == Some(Hit::FileRow(i)) {
                draw::fill_round_rect(
                    &mut pixmap,
                    6.0,
                    (ry + 2.0) as f32,
                    (CARD_W - 12.0) as f32,
                    (ROW_H - 4.0) as f32,
                    8.0,
                    sel,
                );
            }
            let name = path.rsplit('/').next().unwrap_or(path);
            draw::text(
                &mut pixmap,
                (PAD + 6.0) as f32,
                (ry + 9.0) as f32,
                (CARD_W - PAD - 12.0) as f32,
                16.0,
                12.0,
                name,
                fg,
            );
        }
    }

    state.set_viewport(&layer.wl_surface().clone(), w, h);

    buffer.attach_to(layer.wl_surface()).ok();
    layer.wl_surface().damage_buffer(0, 0, pw as i32, ph as i32);
    layer.wl_surface().commit();
}

/// Pointer motion → hovered row changes.
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

/// Esc closes the card.
pub fn key_press(state: &mut ShellState, event: KeyEvent) {
    if event.keysym == Keysym::Escape {
        state.close_island();
    }
}
