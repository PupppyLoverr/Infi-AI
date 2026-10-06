//! Notification popups — stacked cards anchored top-right.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::shell::WaylandSurface;
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, notify::Notification, ShellState, NOTIFY_WIDTH};

const CARD_H: u32 = 76;
const CARD_GAP: u32 = 8;

pub fn desired_height(notifications: &[Notification]) -> u32 {
    (notifications.len() as u32 * (CARD_H + CARD_GAP)).max(1)
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

pub fn draw(state: &mut ShellState) {
    let (w, h) = state.notify_size;
    if w == 0 || h == 0 {
        return;
    }
    let Some(layer) = state.notify_surface.clone() else {
        return;
    };
    let (card, border, fg, fg_dim) = theme(state.dark);

    let stride = w as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
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
    for (i, n) in ordered.iter().enumerate() {
        let y = i as f32 * (CARD_H + CARD_GAP) as f32;
        // Rounded card (blend doc: shell surfaces r10).
        draw::fill_round_rect(
            &mut pixmap,
            0.0,
            y,
            NOTIFY_WIDTH as f32,
            CARD_H as f32,
            10.0,
            card,
        );
        draw::stroke_round_rect(
            &mut pixmap,
            0.5,
            y + 0.5,
            NOTIFY_WIDTH as f32 - 1.0,
            CARD_H as f32 - 1.0,
            9.5,
            1.0,
            border,
        );
        draw::text(
            &mut pixmap,
            12.0,
            y + 8.0,
            240.0,
            16.0,
            11.0,
            &n.app_name,
            fg_dim,
        );
        draw::text(
            &mut pixmap,
            12.0,
            y + 26.0,
            300.0,
            20.0,
            13.0,
            &n.summary,
            fg,
        );
        draw::text(
            &mut pixmap,
            12.0,
            y + 48.0,
            NOTIFY_WIDTH as f32 - 20.0,
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
    }

    let wl_surface = layer.wl_surface().clone();
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, w as i32, h as i32);
    wl_surface.commit();
}

/// Click at y → dismiss the card under the pointer.
pub fn click(state: &mut ShellState, y: f64) {
    let idx = (y / (CARD_H + CARD_GAP) as f64) as usize;
    let len = state.notifications.len();
    // Reverse: index 0 on screen is the newest notification (end of vec).
    if idx < len {
        let vec_idx = len - 1 - idx;
        state.notifications.remove(vec_idx);
        state.notify_dirty = true;
        state.sync_notify_height();
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
