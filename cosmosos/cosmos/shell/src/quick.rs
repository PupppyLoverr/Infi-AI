//! Quick Settings flyout (Windows-style): clicking the tray opens a card with
//! the real network state, a draggable PipeWire volume slider + mute, battery
//! when present, and Settings / Log out buttons.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::shell::WaylandSurface;
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, ShellState};

pub const QUICK_W: u32 = 300;
const PAD: f64 = 16.0;
const HEADER_H: f64 = 34.0;
const NET_H: f64 = 40.0;
const VOL_H: f64 = 56.0;
const BAT_H: f64 = 40.0;
const BTN_H: f64 = 52.0;
const TRACK_X: f64 = PAD + 8.0;
const TRACK_W: f64 = QUICK_W as f64 - PAD * 2.0 - 16.0 - 30.0; // minus mute button

#[derive(Debug, Clone, Copy, PartialEq)]
enum Hit {
    Slider,
    Mute,
    Settings,
    Logout,
    Card,
}

fn theme(dark: bool) -> (Color, Color, Color, Color, CtColor, CtColor) {
    if dark {
        (
            Color::from_rgba8(0x1A, 0x1B, 0x1E, 0xF2), // card
            Color::from_rgba8(0x3C, 0x3D, 0x42, 0xFF), // border/track
            Color::from_rgba8(0x30, 0x31, 0x35, 0xFF), // button bg
            Color::from_rgba8(0xE8, 0xE8, 0xEA, 0xFF), // fill/accent-white
            CtColor::rgba(0xEC, 0xEC, 0xEE, 0xFF),
            CtColor::rgba(0x8C, 0x8C, 0x92, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xFA, 0xFA, 0xFB, 0xF6),
            Color::from_rgba8(0xC8, 0xC8, 0xCC, 0xFF),
            Color::from_rgba8(0xE6, 0xE6, 0xE9, 0xFF),
            Color::from_rgba8(0x30, 0x30, 0x33, 0xFF),
            CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
            CtColor::rgba(0x6A, 0x6A, 0x6E, 0xFF),
        )
    }
}

/// Card height for the current sysinfo (battery row only when present).
pub fn desired_height(info: &crate::sysinfo::SysInfo) -> u32 {
    let mut h = PAD * 2.0 + HEADER_H + NET_H + VOL_H + BTN_H;
    if info.battery.as_ref().map(|b| b.present).unwrap_or(false) {
        h += BAT_H;
    }
    h as u32
}

fn vol_row_y() -> f64 {
    PAD + HEADER_H + NET_H
}

fn bat_row_y() -> f64 {
    vol_row_y() + VOL_H
}

fn hit_test(x: f64, y: f64, state: &ShellState) -> Hit {
    let vy = vol_row_y();
    if y >= vy + 8.0 && y <= vy + VOL_H {
        if x >= TRACK_X && x <= TRACK_X + TRACK_W + 12.0 {
            return Hit::Slider;
        }
        if x > TRACK_X + TRACK_W + 12.0 {
            return Hit::Mute;
        }
    }
    let by = state.quick_size.1 as f64 - BTN_H;
    if y >= by {
        if x < QUICK_W as f64 / 2.0 {
            return Hit::Settings;
        }
        return Hit::Logout;
    }
    Hit::Card
}

/// Apply a volume level for real via wpctl, then optimistically update state.
fn set_volume(state: &mut ShellState, x: f64) {
    let v = ((x - TRACK_X) / TRACK_W).clamp(0.0, 1.0);
    let _ = std::process::Command::new("wpctl")
        .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{v:.2}")])
        .status();
    state.sysinfo.volume = Some(crate::sysinfo::Volume {
        level: v as f32,
        muted: state
            .sysinfo
            .volume
            .as_ref()
            .map(|v| v.muted)
            .unwrap_or(false),
    });
}

pub fn press(state: &mut ShellState, x: f64, y: f64) -> bool {
    match hit_test(x, y, state) {
        Hit::Slider => {
            state.vol_drag = true;
            set_volume(state, x);
            true
        }
        Hit::Mute => {
            let _ = std::process::Command::new("wpctl")
                .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
                .status();
            if let Some(v) = state.sysinfo.volume.as_mut() {
                v.muted = !v.muted;
            }
            true
        }
        Hit::Settings => {
            let _ = std::process::Command::new("cosmos-settings").spawn();
            state.set_quick_open(false);
            false
        }
        Hit::Logout => {
            if let Some(app) = state.apps.iter().find(|a| a.id == "sys.logout").cloned() {
                let _ = crate::desktop::launch(&app);
            }
            state.set_quick_open(false);
            false
        }
        Hit::Card => false,
    }
}

/// Pointer motion while a click is held on the slider.
pub fn drag(state: &mut ShellState, x: f64, _y: f64) -> bool {
    if !state.vol_drag {
        return false;
    }
    set_volume(state, x);
    true
}

pub fn release(state: &mut ShellState) {
    state.vol_drag = false;
}

pub fn draw(state: &mut ShellState) {
    let (w, h) = state.quick_size;
    if w == 0 || h == 0 {
        return;
    }
    let Some(layer) = state.quick_surface.clone() else {
        return;
    };
    let (card, sep, btn_bg, fill, fg, fg_dim) = theme(state.dark);
    // Row positions borrow `state` — compute before the pool borrow below.
    let vy = vol_row_y() as f32;
    let bat_y = bat_row_y() as f32;

    let stride = w as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
    else {
        tracing::warn!("quick: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, w, h) else {
        return;
    };

    draw::fill_round_rect(&mut pixmap, 0.0, 0.0, w as f32, h as f32, 10.0, card);
    draw::stroke_round_rect(
        &mut pixmap,
        0.5,
        0.5,
        w as f32 - 1.0,
        h as f32 - 1.0,
        9.5,
        1.0,
        sep,
    );

    let info = &state.sysinfo;

    // Header: date + clock, Win11 quick-settings style.
    let date = if info.date.is_empty() {
        "CosmosOS".to_string()
    } else {
        info.date.clone()
    };
    draw::text(
        &mut pixmap,
        PAD as f32,
        PAD as f32 + 6.0,
        200.0,
        16.0,
        12.0,
        &date,
        fg_dim,
    );
    draw::text(
        &mut pixmap,
        PAD as f32,
        PAD as f32 + 18.0,
        200.0,
        20.0,
        15.0,
        &info.clock,
        fg,
    );

    // Network row.
    let ny = (PAD + HEADER_H) as f32;
    let net_label = if info.network.online {
        info.network.label.as_str()
    } else {
        "Offline"
    };
    draw::text(
        &mut pixmap,
        PAD as f32,
        ny + 4.0,
        200.0,
        15.0,
        13.0,
        "Network",
        fg,
    );
    draw::text(
        &mut pixmap,
        w as f32 - PAD as f32 - 140.0,
        ny + 4.0,
        140.0,
        15.0,
        12.0,
        net_label,
        fg_dim,
    );

    // Volume row: label, slider track + fill + knob, mute button.
    let level = info
        .volume
        .as_ref()
        .map(|v| v.level.clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let muted = info.volume.as_ref().map(|v| v.muted).unwrap_or(false);
    draw::text(&mut pixmap, PAD as f32, vy, 100.0, 15.0, 13.0, "Volume", fg);
    let vol_label = if muted {
        "Muted".to_string()
    } else {
        format!("{}%", (level * 100.0).round() as i32)
    };
    draw::text(
        &mut pixmap,
        w as f32 - PAD as f32 - 60.0,
        vy,
        60.0,
        15.0,
        12.0,
        &vol_label,
        fg_dim,
    );
    let ty = vy + 26.0;
    draw::fill_round_rect(
        &mut pixmap,
        TRACK_X as f32,
        ty,
        TRACK_W as f32,
        4.0,
        2.0,
        sep,
    );
    if !muted && level > 0.0 {
        draw::fill_round_rect(
            &mut pixmap,
            TRACK_X as f32,
            ty,
            (TRACK_W as f32 * level).max(4.0),
            4.0,
            2.0,
            fill,
        );
    }
    // Knob.
    let kx = TRACK_X as f32 + TRACK_W as f32 * level - 6.0;
    draw::fill_round_rect(&mut pixmap, kx, ty - 4.0, 12.0, 12.0, 6.0, fill);
    // Mute button.
    let mx = w as f32 - PAD as f32 - 30.0;
    draw::fill_round_rect(
        &mut pixmap,
        mx,
        ty - 6.0,
        30.0,
        16.0,
        8.0,
        if muted { fill } else { btn_bg },
    );
    draw::text(
        &mut pixmap,
        mx + 5.0,
        ty - 4.0,
        24.0,
        12.0,
        10.0,
        if muted { "M" } else { "O" },
        if muted {
            if state.dark {
                CtColor::rgba(0x18, 0x18, 0x1B, 0xFF)
            } else {
                CtColor::rgba(0xFF, 0xFF, 0xFF, 0xFF)
            }
        } else {
            fg
        },
    );

    // Battery row.
    if let Some(b) = &info.battery {
        if b.present {
            draw::text(
                &mut pixmap,
                PAD as f32,
                bat_y + 4.0,
                200.0,
                15.0,
                13.0,
                "Battery",
                fg,
            );
            let txt = format!(
                "{}%{}",
                b.percent,
                if b.charging { " charging" } else { "" }
            );
            draw::text(
                &mut pixmap,
                w as f32 - PAD as f32 - 140.0,
                bat_y + 4.0,
                140.0,
                15.0,
                12.0,
                &txt,
                fg_dim,
            );
        }
    }

    // Footer buttons.
    let by = h as f32 - BTN_H as f32;
    draw::fill_rect(&mut pixmap, 1.0, by, w as f32 - 2.0, 1.0, sep);
    let half = w as f32 / 2.0;
    draw::fill_round_rect(
        &mut pixmap,
        PAD as f32,
        by + 10.0,
        half - PAD as f32 - 4.0,
        32.0,
        6.0,
        btn_bg,
    );
    draw::text(
        &mut pixmap,
        PAD as f32 + 14.0,
        by + 18.0,
        100.0,
        16.0,
        12.0,
        "Settings",
        fg,
    );
    draw::fill_round_rect(
        &mut pixmap,
        half + 4.0,
        by + 10.0,
        half - PAD as f32 - 4.0,
        32.0,
        6.0,
        btn_bg,
    );
    draw::text(
        &mut pixmap,
        half + 14.0,
        by + 18.0,
        100.0,
        16.0,
        12.0,
        "Log out",
        fg,
    );

    let wl_surface = layer.wl_surface().clone();
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, w as i32, h as i32);
    wl_surface.commit();
}
