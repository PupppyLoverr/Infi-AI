//! Quick Settings flyout (Windows-style): clicking the tray opens a card with
//! the real network state, a draggable PipeWire volume slider + mute, battery
//! when present, and Settings / Log out buttons.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::shell::WaylandSurface;
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, icons, ShellState};

pub const QUICK_W: u32 = 300;
const PAD: f64 = 16.0;
const HEADER_H: f64 = 34.0;
const NET_H: f64 = 40.0;
const VOL_H: f64 = 56.0;
const BAT_H: f64 = 40.0;
const THEME_H: f64 = 40.0;
const BTN_H: f64 = 52.0;
const TRACK_X: f64 = PAD + 8.0;
const TRACK_W: f64 = QUICK_W as f64 - PAD * 2.0 - 16.0 - 30.0; // minus mute button

#[derive(Debug, Clone, Copy, PartialEq)]
enum Hit {
    Slider,
    Mute,
    /// The whole network row — Win11's connectivity tile toggles.
    Network,
    /// Accent preset swatch (index into cosmos_ipc::ACCENT_PRESETS).
    Swatch(usize),
    /// Dark/light mode pill on the Theme row.
    Mode,
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
            draw::accent(true),                        // active fill
            CtColor::rgba(0xEC, 0xEC, 0xEE, 0xFF),
            CtColor::rgba(0x8C, 0x8C, 0x92, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xFA, 0xFA, 0xFB, 0xF6),
            Color::from_rgba8(0xC8, 0xC8, 0xCC, 0xFF),
            Color::from_rgba8(0xE6, 0xE6, 0xE9, 0xFF),
            draw::accent(false),
            CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
            CtColor::rgba(0x6A, 0x6A, 0x6E, 0xFF),
        )
    }
}

/// Card height for the current sysinfo (battery row only when present).
pub fn desired_height(info: &crate::sysinfo::SysInfo) -> u32 {
    let mut h = PAD * 2.0 + HEADER_H + NET_H + VOL_H + THEME_H + BTN_H;
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

/// Theme swatch row — after the battery row when it exists, else right
/// under the volume row.
fn theme_row_y(state: &ShellState) -> f64 {
    bat_row_y()
        + if state
            .sysinfo
            .battery
            .as_ref()
            .map(|b| b.present)
            .unwrap_or(false)
        {
            BAT_H
        } else {
            0.0
        }
}

/// Swatch circle center x for preset `i` — 22px stride so all six fit
/// between the label and the dark/light pill.
fn swatch_x(i: usize) -> f64 {
    PAD + 68.0 + i as f64 * 22.0
}

/// Dark/light pill rect on the Theme row (right edge).
const MODE_W: f64 = 66.0;
fn mode_x() -> f64 {
    QUICK_W as f64 - PAD - MODE_W
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
    let ny = PAD + HEADER_H;
    if y >= ny && y < ny + NET_H {
        return Hit::Network;
    }
    let ty = theme_row_y(state);
    if y >= ty && y < ty + THEME_H {
        for i in 0..cosmos_ipc::ACCENT_PRESETS.len() {
            let cx = swatch_x(i);
            if (x - cx).abs() <= 11.0 {
                return Hit::Swatch(i);
            }
        }
        if x >= mode_x() && x < mode_x() + MODE_W {
            return Hit::Mode;
        }
        return Hit::Card;
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

/// Flip NetworkManager's networking master switch over D-Bus (the same
/// interface `sysinfo` reads `State` from — no nmcli dependency).
/// Returns false when NM is unavailable so the optimistic flip is skipped.
fn nm_enable(on: bool) -> bool {
    let Ok(conn) = zbus::blocking::Connection::system() else {
        return false;
    };
    conn.call_method(
        Some("org.freedesktop.NetworkManager"),
        "/org/freedesktop/NetworkManager",
        Some("org.freedesktop.NetworkManager"),
        "Enable",
        &(on),
    )
    .is_ok()
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
        Hit::Network => {
            let on = !state.sysinfo.network.online;
            if nm_enable(on) {
                state.sysinfo.network.online = on;
                if !on {
                    state.sysinfo.network.label = "Offline".to_string();
                }
            }
            true
        }
        Hit::Mode => {
            let next = if state.dark { "light" } else { "dark" };
            state.dark = !state.dark;
            state.panel_dirty = true;
            state.launcher_dirty = true;
            state.notify_dirty = true;
            state.quick_dirty = true;
            state.dock_dirty = true;
            state.switcher_dirty = true;
            state.help_dirty = true;
            state.ipc.send(&cosmos_ipc::Request::SetConfig {
                key: "appearance".to_string(),
                value: serde_json::json!(next),
            });
            true
        }
        Hit::Swatch(i) => {
            if let Some((name, _, _, _)) = cosmos_ipc::ACCENT_PRESETS.get(i) {
                state.ipc.send(&cosmos_ipc::Request::SetConfig {
                    key: "accent".to_string(),
                    value: (*name).into(),
                });
                // Optimistic: the Config broadcast lands on the next
                // event loop pass, but repaint the card immediately.
                state.accent_name = (*name).to_string();
                state.quick_dirty = true;
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
    let glyph = Color::from_rgba8(fg.r(), fg.g(), fg.b(), fg.a());
    // Row positions borrow `state` — compute before the pool borrow below.
    let vy = vol_row_y() as f32;
    let bat_y = bat_row_y() as f32;
    let theme_y = theme_row_y(state) as f32;

    let stride = w as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Abgr8888)
    else {
        tracing::warn!("quick: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, w, h) else {
        return;
    };
    // Clear before painting — the rounded corners and any unpainted rows
    // would otherwise show stale shm pool memory as garbage glyphs.
    pixmap.fill(Color::TRANSPARENT);

    // The drop shadow comes from the compositor (layer decal) — the
    // card itself only needs the rounded fill + hairline.
    draw::fill_round_rect(&mut pixmap, 0.0, 0.0, w as f32, h as f32, 12.0, card);
    draw::stroke_round_rect(
        &mut pixmap,
        0.5,
        0.5,
        w as f32 - 1.0,
        h as f32 - 1.0,
        11.5,
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
    icons::icon(
        &mut pixmap,
        if info.network.online {
            "net-on"
        } else {
            "net-off"
        },
        PAD as f32,
        ny + 2.0,
        14.0,
        glyph,
    );
    draw::text(
        &mut pixmap,
        PAD as f32 + 20.0,
        ny + 4.0,
        200.0,
        15.0,
        13.0,
        "Network",
        fg,
    );
    draw::text(
        &mut pixmap,
        w as f32 - PAD as f32 - 140.0 - 42.0,
        ny + 4.0,
        140.0,
        15.0,
        12.0,
        net_label,
        fg_dim,
    );
    // Win11-style toggle: the whole row flips NM's networking switch.
    let sw_x = w as f32 - PAD as f32 - 34.0;
    let sw_y = ny + 11.0;
    draw::fill_round_rect(
        &mut pixmap,
        sw_x,
        sw_y,
        34.0,
        18.0,
        9.0,
        if info.network.online { fill } else { sep },
    );
    let knob_x = if info.network.online {
        sw_x + 25.0
    } else {
        sw_x + 9.0
    };
    let mut pb = tiny_skia::PathBuilder::new();
    pb.push_circle(knob_x, sw_y + 9.0, 6.0);
    if let Some(path) = pb.finish() {
        pixmap.fill_path(
            &path,
            &tiny_skia::Paint {
                // The knob stays near-white either way — it must read
                // against both the accent and the grey track.
                shader: tiny_skia::Shader::SolidColor(Color::from_rgba8(0xF2, 0xF2, 0xF4, 0xFF)),
                anti_alias: true,
                ..Default::default()
            },
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::default(),
            None,
        );
    }

    // Volume row: label, slider track + fill + knob, mute button.
    let level = info
        .volume
        .as_ref()
        .map(|v| v.level.clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let muted = info.volume.as_ref().map(|v| v.muted).unwrap_or(false);
    icons::icon(
        &mut pixmap,
        if muted { "vol-mute" } else { "vol-on" },
        PAD as f32,
        vy - 2.0,
        14.0,
        glyph,
    );
    draw::text(
        &mut pixmap,
        PAD as f32 + 20.0,
        vy,
        100.0,
        15.0,
        13.0,
        "Volume",
        fg,
    );
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
    // Knob stays near-white so it reads against the accent progress.
    let kx = TRACK_X as f32 + TRACK_W as f32 * level - 6.0;
    draw::fill_round_rect(
        &mut pixmap,
        kx,
        ty - 4.0,
        12.0,
        12.0,
        6.0,
        if state.dark {
            Color::from_rgba8(0xF2, 0xF2, 0xF4, 0xFF)
        } else {
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF)
        },
    );
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
    // Speaker glyph centred in the pill (matches the menubar tray icon).
    icons::icon(
        &mut pixmap,
        if muted { "vol-mute" } else { "vol-on" },
        mx + 9.0,
        ty - 4.0,
        12.0,
        if muted {
            // White glyph on the accent mute pill.
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xF0)
        } else {
            glyph
        },
    );

    // Battery row.
    if let Some(b) = &info.battery {
        if b.present {
            icons::battery(&mut pixmap, PAD as f32, bat_y + 3.0, 14.0, glyph, b.percent);
            draw::text(
                &mut pixmap,
                PAD as f32 + 22.0,
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

    // Accent preset row — Omarchy's theme dial, as a swatch strip.
    let ty = theme_y;
    draw::text(
        &mut pixmap,
        PAD as f32,
        ty + 13.0,
        50.0,
        15.0,
        13.0,
        "Theme",
        fg,
    );
    for (i, (name, _, d_rgb, l_rgb)) in cosmos_ipc::ACCENT_PRESETS.iter().enumerate() {
        let [r, g, b] = if state.dark { *d_rgb } else { *l_rgb };
        let cx = swatch_x(i) as f32;
        let cy = ty + 20.0;
        // Filled disc; the selected preset gets a hairline ring outside it.
        let mut pb = tiny_skia::PathBuilder::new();
        pb.push_circle(cx, cy, 8.0);
        if let Some(path) = pb.finish() {
            pixmap.fill_path(
                &path,
                &tiny_skia::Paint {
                    shader: tiny_skia::Shader::SolidColor(Color::from_rgba8(r, g, b, 0xFF)),
                    anti_alias: true,
                    ..Default::default()
                },
                tiny_skia::FillRule::Winding,
                tiny_skia::Transform::default(),
                None,
            );
        }
        if state.accent_name == *name {
            let mut pb = tiny_skia::PathBuilder::new();
            pb.push_circle(cx, cy, 11.0);
            if let Some(path) = pb.finish() {
                pixmap.stroke_path(
                    &path,
                    &tiny_skia::Paint {
                        shader: tiny_skia::Shader::SolidColor(Color::from_rgba8(
                            fg.r(),
                            fg.g(),
                            fg.b(),
                            0xFF,
                        )),
                        anti_alias: true,
                        ..Default::default()
                    },
                    &tiny_skia::Stroke {
                        width: 1.0,
                        ..Default::default()
                    },
                    tiny_skia::Transform::default(),
                    None,
                );
            }
        }
    }

    // Dark/light mode pill — right edge of the Theme row, macOS Control
    // Center's dark-mode tile shape.
    let mx = mode_x() as f32;
    let my = ty + 9.0;
    draw::fill_round_rect(&mut pixmap, mx, my, MODE_W as f32, 22.0, 11.0, btn_bg);
    let gx = mx + 13.0;
    let gcy = my + 11.0;
    let mut pb = tiny_skia::PathBuilder::new();
    if state.dark {
        // Moon crescent — a big disc minus an offset disc (EvenOdd).
        pb.push_circle(gx, gcy, 5.5);
        pb.push_circle(gx + 3.0, gcy - 2.0, 4.6);
        if let Some(path) = pb.finish() {
            pixmap.fill_path(
                &path,
                &tiny_skia::Paint {
                    shader: tiny_skia::Shader::SolidColor(Color::from_rgba8(
                        0xFF, 0xFF, 0xFF, 0xD8,
                    )),
                    anti_alias: true,
                    ..Default::default()
                },
                tiny_skia::FillRule::EvenOdd,
                tiny_skia::Transform::default(),
                None,
            );
        }
    } else {
        // Sun — filled disc + 8 stroked rays.
        let ink = tiny_skia::Paint {
            shader: tiny_skia::Shader::SolidColor(Color::from_rgba8(0x20, 0x20, 0x24, 0xE0)),
            anti_alias: true,
            ..Default::default()
        };
        let mut disc = tiny_skia::PathBuilder::new();
        disc.push_circle(gx, gcy, 3.6);
        if let Some(path) = disc.finish() {
            pixmap.fill_path(
                &path,
                &ink,
                tiny_skia::FillRule::Winding,
                tiny_skia::Transform::default(),
                None,
            );
        }
        let mut rays = tiny_skia::PathBuilder::new();
        for i in 0..8 {
            let a = i as f32 * std::f32::consts::FRAC_PI_4;
            rays.move_to(gx + a.cos() * 5.2, gcy + a.sin() * 5.2);
            rays.line_to(gx + a.cos() * 7.4, gcy + a.sin() * 7.4);
        }
        if let Some(path) = rays.finish() {
            pixmap.stroke_path(
                &path,
                &ink,
                &tiny_skia::Stroke {
                    width: 1.2,
                    ..Default::default()
                },
                tiny_skia::Transform::default(),
                None,
            );
        }
    }
    draw::text(
        &mut pixmap,
        mx + 22.0,
        my + 4.0,
        MODE_W as f32 - 26.0,
        15.0,
        11.0,
        if state.dark { "Dark" } else { "Light" },
        fg,
    );

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
    icons::icon(
        &mut pixmap,
        "cosmos-settings",
        PAD as f32 + 10.0,
        by + 19.0,
        14.0,
        glyph,
    );
    draw::text(
        &mut pixmap,
        PAD as f32 + 28.0,
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
    icons::icon(
        &mut pixmap,
        "sys-logout",
        half + 12.0,
        by + 19.0,
        14.0,
        glyph,
    );
    draw::text(
        &mut pixmap,
        half + 30.0,
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
