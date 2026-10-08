//! Notification popups — stacked cards anchored top-right, with action
//! buttons (Approve/Deny style) that signal back to the notifier.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::shell::WaylandSurface;
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, glass, icons, notify::Notification, ShellState, NOTIFY_WIDTH};

const CARD_H: u32 = 76;
const ACT_H: u32 = 34;
const CARD_GAP: u32 = 8;

fn card_h(n: &Notification) -> u32 {
    if n.actions.is_empty() {
        CARD_H
    } else {
        CARD_H + ACT_H
    }
}

pub fn desired_height(notifications: &[Notification]) -> u32 {
    let h: u32 = notifications.iter().map(card_h).sum::<u32>()
        + notifications.len().saturating_sub(1) as u32 * CARD_GAP;
    h.max(1)
}

/// Top y of each card in draw order (newest first).
fn card_tops(notifications: &[Notification]) -> Vec<f32> {
    let mut tops = Vec::with_capacity(notifications.len());
    let mut y = 0.0f32;
    for n in notifications.iter().rev() {
        tops.push(y);
        y += (card_h(n) + CARD_GAP) as f32;
    }
    tops
}

fn theme(dark: bool) -> (Color, Color, CtColor, CtColor) {
    if dark {
        (
            Color::from_rgba8(0x1C, 0x1C, 0x1F, 0xF4),
            Color::from_rgba8(0x38, 0x38, 0x3C, 0xFF),
            CtColor::rgba(0xEC, 0xEC, 0xEE, 0xFF),
            CtColor::rgba(0x9A, 0x9A, 0xA0, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xFB, 0xFB, 0xFB, 0xF8),
            Color::from_rgba8(0xD8, 0xD8, 0xD8, 0xFF),
            CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
            CtColor::rgba(0x55, 0x55, 0x5A, 0xFF),
        )
    }
}

/// Horizontal extents of the i-th action button inside a card.
pub fn button_geom(n_actions: usize, i: usize) -> (f32, f32) {
    let pad = 8.0f32;
    let gap = 8.0f32;
    let inner = NOTIFY_WIDTH as f32 - pad * 2.0;
    let bw = (inner - gap * (n_actions as f32 - 1.0)) / n_actions as f32;
    (pad + i as f32 * (bw + gap), bw)
}

pub fn draw(state: &mut ShellState) {
    let (w, h) = state.notify_size;
    if w == 0 || h == 0 {
        return;
    }
    let Some(layer) = state.notify_surface.clone() else {
        return;
    };
    let dark = state.dark;
    let (card, border, fg, fg_dim) = theme(dark);
    let btn_fill = if dark {
        Color::from_rgba8(0x30, 0x30, 0x34, 0xFF)
    } else {
        Color::from_rgba8(0xE8, 0xE8, 0xEA, 0xFF)
    };

    let stride = w as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Abgr8888)
    else {
        tracing::warn!("notify: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, w, h) else {
        return;
    };
    // Clear before painting — inter-card gaps and rounded corners would
    // otherwise show stale shm pool memory as garbage glyphs.
    pixmap.fill(Color::TRANSPARENT);

    // Cards stack from the top; newest last → drawn at the top of the stack.
    let ordered: Vec<&Notification> = state.notifications.iter().rev().collect();
    let tops = card_tops(&state.notifications);
    for (n, &y) in ordered.iter().zip(tops.iter()) {
        let ch = card_h(n) as f32;
        // Rounded card — the drop shadow comes from the compositor's
        // layer decal, so the card itself only needs fill + hairline.
        glass::fill_glass(&mut pixmap, &crate::ShellState::wallpaper_name(), 0.0, y, NOTIFY_WIDTH as f32, ch, 12.0, (state.panel_size.0 as f32 - NOTIFY_WIDTH as f32 - 8.0).max(0.0), crate::PANEL_HEIGHT as f32 + 8.0 + y, state.dark, card);
        draw::stroke_round_rect(
            &mut pixmap,
            0.5,
            y + 0.5,
            NOTIFY_WIDTH as f32 - 1.0,
            ch - 1.0,
            11.5,
            1.0,
            border,
        );
        // macOS idiom: the app's tinted icon rides the left edge.
        let glyph = Color::from_rgba8(fg.r(), fg.g(), fg.b(), fg.a());
        let key = icons::key_for(&n.app_name.to_lowercase().replace(' ', "-"));
        icons::app_tile(&mut pixmap, &key, 10.0, y + 30.0, 18.0);
        draw::text(
            &mut pixmap,
            34.0,
            y + 8.0,
            240.0,
            16.0,
            11.0,
            &n.app_name,
            fg_dim,
        );
        draw::text(
            &mut pixmap,
            34.0,
            y + 26.0,
            270.0,
            20.0,
            13.0,
            &n.summary,
            fg,
        );
        draw::text(
            &mut pixmap,
            34.0,
            y + 48.0,
            NOTIFY_WIDTH as f32 - 42.0,
            18.0,
            11.0,
            &n.body,
            fg_dim,
        );
        // Close hint glyph.
        draw::text(
            &mut pixmap,
            NOTIFY_WIDTH as f32 - 22.0,
            y + 8.0,
            18.0,
            16.0,
            12.0,
            "×",
            fg_dim,
        );
        // Action buttons row (Approve / Deny / …).
        for (i, (_key, label)) in n.actions.iter().enumerate() {
            let (bx, bw) = button_geom(n.actions.len(), i);
            let by = y + CARD_H as f32 + 3.0;
            draw::fill_round_rect(&mut pixmap, bx, by, bw, 26.0, 8.0, btn_fill);
            draw::text(
                &mut pixmap,
                bx,
                by + 6.0,
                bw,
                16.0,
                11.0,
                label,
                fg,
            );
        }
    }

    let wl_surface = layer.wl_surface().clone();
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, w as i32, h as i32);
    wl_surface.commit();
}

/// What a click resolved to — the shell emits the matching D-Bus signals.
pub enum Click {
    Nothing,
    /// Card removed: NotificationClosed(2).
    Dismissed(u32),
    /// Action pressed: ActionInvoked(id, key) + NotificationClosed(2).
    Actioned(u32, String),
}

/// Click at (x, y) → an action button if hit, else dismiss the card.
pub fn click(state: &mut ShellState, x: f64, y: f64) -> Click {
    let tops = card_tops(&state.notifications);
    let ordered: Vec<&Notification> = state.notifications.iter().rev().collect();
    let mut target: Option<(usize, Option<String>)> = None;
    for (i, n) in ordered.iter().enumerate() {
        let top = tops[i] as f64;
        let ch = card_h(n) as f64;
        if y < top || y >= top + ch {
            continue;
        }
        // Button row?
        if !n.actions.is_empty() && y >= top + CARD_H as f64 + 3.0 && y < top + ch - 3.0 {
            for (bi, (key, _)) in n.actions.iter().enumerate() {
                let (bx, bw) = button_geom(n.actions.len(), bi);
                if x >= bx as f64 && x < (bx + bw) as f64 {
                    target = Some((i, Some(key.clone())));
                    break;
                }
            }
        }
        if target.is_none() {
            target = Some((i, None));
        }
        break;
    }
    let Some((i, key)) = target else {
        return Click::Nothing;
    };
    let vec_idx = state.notifications.len() - 1 - i;
    let n = state.notifications.remove(vec_idx);
    state.notify_dirty = true;
    state.sync_notify_height();
    match key {
        Some(k) => Click::Actioned(n.id, k),
        None => Click::Dismissed(n.id),
    }
}

impl ShellState {
    /// After list change, resize the notify layer surface.
    pub fn sync_notify_height(&mut self) {
        if let Some(layer) = self.notify_surface.as_ref() {
            let h = desired_height(&self.notifications);
            layer.set_size(NOTIFY_WIDTH, h);
            layer.wl_surface().commit();
            self.notify_size = (NOTIFY_WIDTH, h);
        }
    }
}
