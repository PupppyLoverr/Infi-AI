//! Control Centre (macOS module tiles, spec v5 §2.4): a connectivity tile
//! (Network, Bluetooth when BlueZ has an adapter), Focus / Dark Mode /
//! Lite Mode tiles, Display (when a backlight exists) and Sound sliders,
//! a Now Playing tile while an MPRIS player exists, and Settings / Lock /
//! Log out icon buttons. Every control drives the real system.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::shell::WaylandSurface;
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, glass, icons, ShellState};

pub const QUICK_W: u32 = 340;
const PAD: f32 = 12.0;
const GAP: f32 = 10.0;
const ROW: f32 = 48.0;
const SMALL_H: f32 = 84.0;
const SLIDER_H: f32 = 64.0;
const MEDIA_H: f32 = 64.0;
const FOOT_H: f32 = 48.0;
const BTN: f32 = 28.0;
const TILE_R: f32 = 14.0;

type Rect = (f32, f32, f32, f32);

fn inside(r: Rect, x: f32, y: f32) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

/// Which optional tiles exist — they only appear for real hardware/state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Flags {
    bluetooth: bool,
    backlight: bool,
    media: bool,
}

fn flags(state: &ShellState) -> Flags {
    Flags {
        bluetooth: state.sysinfo.bluetooth.is_some(),
        backlight: state.sysinfo.backlight.is_some(),
        media: state.media.is_some(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    conn: Rect,
    net: Rect,
    bt: Option<Rect>,
    focus: Rect,
    dark: Rect,
    lite: Rect,
    display: Option<Rect>,
    sound: Rect,
    media: Option<Rect>,
    footer: Rect,
}

fn layout(f: Flags) -> Layout {
    let w = QUICK_W as f32 - PAD * 2.0;
    let mut y = PAD;
    let rows = if f.bluetooth { 2.0 } else { 1.0 };
    let conn = (PAD, y, w, rows * ROW + 8.0);
    let net = (PAD, y + 4.0, w, ROW);
    let bt = f.bluetooth.then_some((PAD, y + 4.0 + ROW, w, ROW));
    y += conn.3 + GAP;
    let tw = (w - GAP * 2.0) / 3.0;
    let focus = (PAD, y, tw, SMALL_H);
    let dark = (PAD + tw + GAP, y, tw, SMALL_H);
    let lite = (PAD + (tw + GAP) * 2.0, y, tw, SMALL_H);
    y += SMALL_H + GAP;
    let display = if f.backlight {
        y += SLIDER_H + GAP;
        Some((PAD, y - SLIDER_H - GAP, w, SLIDER_H))
    } else {
        None
    };
    let sound = (PAD, y, w, SLIDER_H);
    y += SLIDER_H + GAP;
    let media = if f.media {
        y += MEDIA_H + GAP;
        Some((PAD, y - MEDIA_H - GAP, w, MEDIA_H))
    } else {
        None
    };
    let footer = (PAD, y - GAP + 2.0, w, FOOT_H);
    Layout {
        conn,
        net,
        bt,
        focus,
        dark,
        lite,
        display,
        sound,
        media,
        footer,
    }
}

/// Slider track inside a slider tile: (x0, x1, centre y).
fn track(r: Rect) -> (f32, f32, f32) {
    (r.0 + 52.0, r.0 + r.2 - 18.0, r.1 + 42.0)
}

fn slider_value(r: Rect, x: f32) -> f32 {
    let (x0, x1, _) = track(r);
    ((x - x0) / (x1 - x0)).clamp(0.0, 1.0)
}

fn footer_button(f: Rect, i: usize) -> Rect {
    (
        f.0 + 4.0 + i as f32 * (BTN + 12.0),
        f.1 + (FOOT_H - BTN) / 2.0,
        BTN,
        BTN,
    )
}

fn media_button(m: Rect) -> Rect {
    (
        m.0 + m.2 - 12.0 - BTN,
        m.1 + (MEDIA_H - BTN) / 2.0,
        BTN,
        BTN,
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Hit {
    Network,
    Bluetooth,
    Focus,
    Dark,
    Lite,
    Brightness,
    Mute,
    Volume,
    PlayPause,
    Settings,
    Lock,
    Logout,
    Card,
}

fn hit_test(l: &Layout, x: f32, y: f32) -> Hit {
    if inside(l.net, x, y) {
        return Hit::Network;
    }
    if l.bt.is_some_and(|r| inside(r, x, y)) {
        return Hit::Bluetooth;
    }
    for (r, hit) in [
        (l.focus, Hit::Focus),
        (l.dark, Hit::Dark),
        (l.lite, Hit::Lite),
    ] {
        if inside(r, x, y) {
            return hit;
        }
    }
    if let Some(r) = l.display.filter(|r| inside(*r, x, y)) {
        return if x >= track(r).0 - 8.0 {
            Hit::Brightness
        } else {
            Hit::Card
        };
    }
    if inside(l.sound, x, y) {
        let x0 = track(l.sound).0;
        return if x < x0 - 8.0 { Hit::Mute } else { Hit::Volume };
    }
    if let Some(r) = l.media.filter(|r| inside(*r, x, y)) {
        return if inside(media_button(r), x, y) {
            Hit::PlayPause
        } else {
            Hit::Card
        };
    }
    for (i, hit) in [Hit::Settings, Hit::Lock, Hit::Logout]
        .into_iter()
        .enumerate()
    {
        if inside(footer_button(l.footer, i), x, y) {
            return hit;
        }
    }
    Hit::Card
}

/// Card height for the tiles that currently exist.
pub fn desired_height(state: &ShellState) -> u32 {
    let l = layout(flags(state));
    (l.footer.1 + l.footer.3) as u32
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
fn set_volume(state: &mut ShellState, x: f32) {
    let v = slider_value(layout(flags(state)).sound, x);
    let _ = std::process::Command::new("wpctl")
        .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{v:.2}")])
        .status();
    let muted = state.sysinfo.volume.as_ref().is_some_and(|v| v.muted);
    state.sysinfo.volume = Some(crate::sysinfo::Volume { level: v, muted });
}

fn set_brightness(state: &mut ShellState, x: f32) {
    let Some(r) = layout(flags(state)).display else {
        return;
    };
    let v = slider_value(r, x);
    if let Some(b) = state.sysinfo.backlight.as_mut() {
        if crate::sysinfo::set_brightness(&b.name, v) {
            b.level = v;
        }
    }
}

pub fn press(state: &mut ShellState, x: f64, y: f64) -> bool {
    let (x, y) = (x as f32, y as f32);
    match hit_test(&layout(flags(state)), x, y) {
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
        Hit::Bluetooth => {
            if let Some(b) = state.sysinfo.bluetooth.as_mut() {
                if crate::sysinfo::set_bluetooth(&b.adapter, !b.powered) {
                    b.powered = !b.powered;
                    if !b.powered {
                        b.device = None;
                    }
                }
            }
            true
        }
        Hit::Focus => {
            let on = !state.dnd;
            state.set_dnd(on);
            true
        }
        Hit::Dark => {
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
        Hit::Lite => {
            let on = !state.lite;
            state.set_lite(on);
            state.ipc.send(&cosmos_ipc::Request::SetConfig {
                key: "lite_mode".to_string(),
                value: serde_json::json!(on),
            });
            true
        }
        Hit::Brightness => {
            state.bright_drag = true;
            set_brightness(state, x);
            true
        }
        Hit::Volume => {
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
        Hit::PlayPause => {
            if let Some(m) = state.media.as_mut() {
                crate::activity::play_pause(m.bus.clone());
                m.playing = !m.playing;
            }
            true
        }
        Hit::Settings => {
            let _ = std::process::Command::new("cosmos-settings").spawn();
            state.set_quick_open(false);
            false
        }
        Hit::Lock => {
            if let Err(err) = crate::logind::request_lock() {
                tracing::warn!("quick: lock failed: {err}");
            }
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

/// Pointer motion while a click is held on a slider.
pub fn drag(state: &mut ShellState, x: f64, _y: f64) -> bool {
    if state.vol_drag {
        set_volume(state, x as f32);
    } else if state.bright_drag {
        set_brightness(state, x as f32);
    } else {
        return false;
    }
    true
}

pub fn release(state: &mut ShellState) {
    state.vol_drag = false;
    state.bright_drag = false;
}

struct Pal {
    tile: Color,
    off: Color,
    track: Color,
    fill: Color,
    knob: Color,
    hair: Color,
    glyph: Color,
    fg: CtColor,
    dim: CtColor,
}

fn palette(dark: bool) -> Pal {
    let rgba = Color::from_rgba8;
    if dark {
        Pal {
            tile: rgba(0xFF, 0xFF, 0xFF, 0x12),
            off: rgba(0xFF, 0xFF, 0xFF, 0x1F),
            track: rgba(0xFF, 0xFF, 0xFF, 0x2E),
            fill: draw::accent(true),
            knob: rgba(0xF2, 0xF2, 0xF4, 0xFF),
            hair: rgba(0xFF, 0xFF, 0xFF, 0x1F),
            glyph: rgba(0xEC, 0xEC, 0xEE, 0xFF),
            fg: CtColor::rgba(0xEC, 0xEC, 0xEE, 0xFF),
            dim: CtColor::rgba(0xB8, 0xB8, 0xBE, 0xFF),
        }
    } else {
        Pal {
            tile: rgba(0xFF, 0xFF, 0xFF, 0x80),
            off: rgba(0x00, 0x00, 0x00, 0x14),
            track: rgba(0x00, 0x00, 0x00, 0x24),
            fill: draw::accent(false),
            knob: rgba(0xFF, 0xFF, 0xFF, 0xFF),
            hair: rgba(0x00, 0x00, 0x00, 0x1A),
            glyph: rgba(0x18, 0x18, 0x1B, 0xFF),
            fg: CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
            dim: CtColor::rgba(0x52, 0x52, 0x58, 0xFF),
        }
    }
}

fn tile(pm: &mut PixmapMut<'_>, r: Rect, p: &Pal) {
    draw::fill_round_rect(pm, r.0, r.1, r.2, r.3, TILE_R, p.tile);
}

/// 28px circle icon button — filled accent with a white glyph when on.
fn round_button(pm: &mut PixmapMut<'_>, x: f32, y: f32, on: bool, key: &str, p: &Pal) {
    draw::fill_round_rect(
        pm,
        x,
        y,
        BTN,
        BTN,
        BTN / 2.0,
        if on { p.fill } else { p.off },
    );
    let c = if on {
        Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF)
    } else {
        p.glyph
    };
    icons::icon(pm, key, x + 6.0, y + 6.0, 16.0, c);
}

/// Module row: round button, bold title, status line.
fn toggle_row(
    pm: &mut PixmapMut<'_>,
    r: Rect,
    on: bool,
    key: &str,
    title: &str,
    status: &str,
    p: &Pal,
) {
    round_button(pm, r.0 + 10.0, r.1 + (r.3 - BTN) / 2.0, on, key, p);
    let tx = r.0 + 48.0;
    draw::text_bold(pm, tx, r.1 + 7.0, r.2 - 60.0, 17.0, 13.0, title, p.fg);
    draw::text(pm, tx, r.1 + 25.0, r.2 - 60.0, 16.0, 12.0, status, p.dim);
}

fn small_tile(pm: &mut PixmapMut<'_>, r: Rect, on: bool, key: &str, title: &str, p: &Pal) {
    tile(pm, r, p);
    round_button(pm, r.0 + (r.2 - BTN) / 2.0, r.1 + 12.0, on, key, p);
    draw::text_centered(
        pm,
        r.0 + 4.0,
        r.1 + 46.0,
        r.2 - 8.0,
        16.0,
        12.0,
        title,
        p.fg,
    );
    draw::text_centered(
        pm,
        r.0 + 4.0,
        r.1 + 62.0,
        r.2 - 8.0,
        15.0,
        11.0,
        if on { "On" } else { "Off" },
        p.dim,
    );
}

#[allow(clippy::too_many_arguments)]
fn slider_tile(
    pm: &mut PixmapMut<'_>,
    r: Rect,
    title: &str,
    value: &str,
    level: f32,
    key: &str,
    pressed: bool,
    p: &Pal,
) {
    tile(pm, r, p);
    draw::text_bold(pm, r.0 + 14.0, r.1 + 8.0, 160.0, 17.0, 13.0, title, p.fg);
    let tw = draw::text_width(12.0, value).ceil();
    draw::text(
        pm,
        r.0 + r.2 - 14.0 - tw,
        r.1 + 9.0,
        tw + 2.0,
        16.0,
        12.0,
        value,
        p.dim,
    );
    round_button(pm, r.0 + 10.0, r.1 + 28.0, pressed, key, p);
    let (x0, x1, cy) = track(r);
    let level = level.clamp(0.0, 1.0);
    draw::fill_round_rect(pm, x0, cy - 2.0, x1 - x0, 4.0, 2.0, p.track);
    if level > 0.0 && !pressed {
        draw::fill_round_rect(
            pm,
            x0,
            cy - 2.0,
            ((x1 - x0) * level).max(4.0),
            4.0,
            2.0,
            p.fill,
        );
    }
    let kx = x0 + (x1 - x0) * level - 7.0;
    draw::fill_round_rect(pm, kx, cy - 7.0, 14.0, 14.0, 7.0, p.knob);
    draw::stroke_round_rect(pm, kx + 0.5, cy - 6.5, 13.0, 13.0, 6.5, 1.0, p.hair);
}

pub fn draw(state: &mut ShellState) {
    let (w, h) = state.quick_size;
    if w == 0 || h == 0 {
        return;
    }
    let Some(layer) = state.quick_surface.clone() else {
        return;
    };
    let l = layout(flags(state));
    let p = palette(state.dark);
    let card = if state.dark {
        Color::from_rgba8(0x1A, 0x1B, 0x1E, 0xF2)
    } else {
        Color::from_rgba8(0xFA, 0xFA, 0xFB, 0xF6)
    };

    let (pw, ph) = draw::phys(w, h);
    let stride = pw as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Abgr8888)
    else {
        tracing::warn!("quick: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, pw, ph) else {
        return;
    };
    // Clear before painting — the rounded corners and any unpainted rows
    // would otherwise show stale shm pool memory as garbage glyphs.
    pixmap.fill(Color::TRANSPARENT);

    // The drop shadow comes from the compositor (layer decal) — the
    // card itself only needs the rounded fill + hairline.
    glass::fill_glass(
        &mut pixmap,
        &crate::ShellState::wallpaper_name(),
        0.0,
        0.0,
        w as f32,
        h as f32,
        cosmos_theme::radius::PANEL,
        (state.panel_size.0 as f32 - w as f32 - 8.0).max(0.0),
        crate::PANEL_HEIGHT as f32 + 4.0,
        state.dark,
        card,
    );
    draw::stroke_round_rect(
        &mut pixmap,
        0.5,
        0.5,
        w as f32 - 1.0,
        h as f32 - 1.0,
        cosmos_theme::radius::PANEL - 0.5,
        1.0,
        p.hair,
    );

    let info = &state.sysinfo;

    // Connectivity tile.
    tile(&mut pixmap, l.conn, &p);
    let online = info.network.online;
    let net_status = if online {
        info.network.label.as_str()
    } else {
        "Off"
    };
    toggle_row(
        &mut pixmap,
        l.net,
        online,
        if online { "net-on" } else { "net-off" },
        "Network",
        net_status,
        &p,
    );
    if let (Some(r), Some(bt)) = (l.bt, info.bluetooth.as_ref()) {
        let status = match (&bt.device, bt.powered) {
            (Some(d), true) => d.as_str(),
            (None, true) => "On",
            _ => "Off",
        };
        toggle_row(
            &mut pixmap,
            r,
            bt.powered,
            "bluetooth",
            "Bluetooth",
            status,
            &p,
        );
    }

    small_tile(&mut pixmap, l.focus, state.dnd, "focus", "Focus", &p);
    small_tile(
        &mut pixmap,
        l.dark,
        state.dark,
        "appearance",
        "Dark Mode",
        &p,
    );
    small_tile(&mut pixmap, l.lite, state.lite, "lite", "Lite Mode", &p);

    if let (Some(r), Some(b)) = (l.display, info.backlight.as_ref()) {
        let pct = format!("{}%", (b.level * 100.0).round() as i32);
        slider_tile(&mut pixmap, r, "Display", &pct, b.level, "sun", false, &p);
    }

    let level = info.volume.as_ref().map(|v| v.level).unwrap_or(0.0);
    let muted = info.volume.as_ref().is_some_and(|v| v.muted);
    let vol = match &info.volume {
        None => "No output".to_string(),
        Some(_) if muted => "Muted".to_string(),
        Some(_) => format!("{}%", (level.clamp(0.0, 1.0) * 100.0).round() as i32),
    };
    slider_tile(
        &mut pixmap,
        l.sound,
        "Sound",
        &vol,
        level,
        if muted { "vol-mute" } else { "vol-on" },
        muted,
        &p,
    );

    if let (Some(r), Some(m)) = (l.media, state.media.as_ref()) {
        tile(&mut pixmap, r, &p);
        let tw = r.2 - 28.0 - BTN - 12.0;
        let title = if m.title.is_empty() {
            "Now Playing"
        } else {
            m.title.as_str()
        };
        draw::text_bold(
            &mut pixmap,
            r.0 + 14.0,
            r.1 + 13.0,
            tw,
            17.0,
            13.0,
            title,
            p.fg,
        );
        draw::text(
            &mut pixmap,
            r.0 + 14.0,
            r.1 + 33.0,
            tw,
            16.0,
            12.0,
            &m.artist,
            p.dim,
        );
        let b = media_button(r);
        round_button(
            &mut pixmap,
            b.0,
            b.1,
            false,
            if m.playing { "pause" } else { "play" },
            &p,
        );
    }

    // Footer: Settings / Lock / Log out icon buttons; battery on the right.
    for (i, key) in ["cosmos-settings", "sys-lock", "sys-logout"]
        .into_iter()
        .enumerate()
    {
        let b = footer_button(l.footer, i);
        round_button(&mut pixmap, b.0, b.1, false, key, &p);
    }
    if let Some(b) = info.battery.as_ref().filter(|b| b.present) {
        let txt = format!(
            "{}%{}",
            b.percent,
            if b.charging { " charging" } else { "" }
        );
        let tw = draw::text_width(12.0, &txt).ceil();
        let f = l.footer;
        let tx = f.0 + f.2 - 6.0 - tw;
        draw::text(
            &mut pixmap,
            tx,
            f.1 + 16.0,
            tw + 2.0,
            16.0,
            12.0,
            &txt,
            p.dim,
        );
        icons::battery(&mut pixmap, tx - 22.0, f.1 + 16.0, 16.0, p.glyph, b.percent);
    }

    let wl_surface = layer.wl_surface().clone();
    state.set_viewport(&wl_surface, w, h);
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, pw as i32, ph as i32);
    wl_surface.commit();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centre(r: Rect) -> (f32, f32) {
        (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0)
    }

    #[test]
    fn tiles_stack_without_overlap_and_optional_ones_appear() {
        let bare = layout(Flags::default());
        assert!(bare.bt.is_none() && bare.display.is_none() && bare.media.is_none());
        assert_eq!(bare.focus.1, bare.conn.1 + bare.conn.3 + GAP);
        assert_eq!(bare.sound.1, bare.focus.1 + SMALL_H + GAP);
        assert!(bare.footer.1 >= bare.sound.1 + SLIDER_H);
        let full = layout(Flags {
            bluetooth: true,
            backlight: true,
            media: true,
        });
        let d = full.display.unwrap();
        let m = full.media.unwrap();
        assert_eq!(d.1, full.focus.1 + SMALL_H + GAP);
        assert_eq!(full.sound.1, d.1 + SLIDER_H + GAP);
        assert_eq!(m.1, full.sound.1 + SLIDER_H + GAP);
        assert!(full.footer.1 >= m.1 + MEDIA_H);
        let grow = (full.footer.1 - bare.footer.1).round();
        assert_eq!(grow, ROW + SLIDER_H + GAP + MEDIA_H + GAP);
        // Three small tiles fill the row edge to edge.
        assert_eq!(full.lite.0 + full.lite.2, QUICK_W as f32 - PAD);
    }

    #[test]
    fn hit_test_finds_every_control() {
        let l = layout(Flags {
            bluetooth: true,
            backlight: true,
            media: true,
        });
        let at = |r: Rect| {
            let (x, y) = centre(r);
            hit_test(&l, x, y)
        };
        assert_eq!(at(l.net), Hit::Network);
        assert_eq!(at(l.bt.unwrap()), Hit::Bluetooth);
        assert_eq!(at(l.focus), Hit::Focus);
        assert_eq!(at(l.dark), Hit::Dark);
        assert_eq!(at(l.lite), Hit::Lite);
        assert_eq!(at(l.display.unwrap()), Hit::Brightness);
        assert_eq!(at(l.sound), Hit::Volume);
        assert_eq!(hit_test(&l, l.sound.0 + 24.0, l.sound.1 + 42.0), Hit::Mute);
        assert_eq!(at(media_button(l.media.unwrap())), Hit::PlayPause);
        assert_eq!(at(footer_button(l.footer, 0)), Hit::Settings);
        assert_eq!(at(footer_button(l.footer, 1)), Hit::Lock);
        assert_eq!(at(footer_button(l.footer, 2)), Hit::Logout);
        assert_eq!(hit_test(&l, 1.0, 1.0), Hit::Card);
    }

    #[test]
    fn slider_maps_track_ends() {
        let r = layout(Flags::default()).sound;
        let (x0, x1, _) = track(r);
        assert_eq!(slider_value(r, x0 - 20.0), 0.0);
        assert_eq!(slider_value(r, x1 + 20.0), 1.0);
        assert!((slider_value(r, (x0 + x1) / 2.0) - 0.5).abs() < 1e-4);
    }
}
