//! Desktop widgets: a compact clock/date card and, while Open-Meteo
//! answers, a weather card — glass on the wallpaper, top-left, beneath
//! windows (layer `Bottom`). Input passes through to the desktop.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::{compositor::Region, shell::WaylandSurface};
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{dock, draw, glass, weather, ShellState};

pub const W: u32 = 280;
pub const CARD_H: u32 = 104;
const GAP: u32 = 12;
/// Inset from the usable area's top-left (below the menubar, right of a
/// left dock).
pub const MARGIN: i32 = 16;
const RADIUS: f32 = 18.0;

pub fn height(has_weather: bool) -> u32 {
    if has_weather {
        CARD_H * 2 + GAP
    } else {
        CARD_H
    }
}

/// Local (hour, minute, weekday 0=Sun, mday, month0).
fn local_now() -> Option<(i32, i32, i32, i32, i32)> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as libc::time_t;
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&secs, &mut tm).is_null() {
            return None;
        }
        Some((tm.tm_hour, tm.tm_min, tm.tm_wday, tm.tm_mday, tm.tm_mon))
    }
}

/// ("22:14", "Thursday, 8 October").
pub fn clock_lines() -> (String, String) {
    const DAYS: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    local_now()
        .map(|(h, m, wd, d, mo)| {
            (
                format!("{h:02}:{m:02}"),
                format!(
                    "{}, {d} {}",
                    DAYS[wd.rem_euclid(7) as usize],
                    MONTHS[mo.rem_euclid(12) as usize]
                ),
            )
        })
        .unwrap_or_default()
}

fn deg(t: f64) -> String {
    format!("{}°", t.round() as i64)
}

pub fn draw(state: &mut ShellState) {
    let (w, h) = state.widgets_size;
    if w == 0 || h == 0 {
        return;
    }
    let Some(layer) = state.widgets_surface.clone() else {
        return;
    };
    let dark = state.dark;
    let (fallback, hairline, fg, fg2) = if dark {
        (
            Color::from_rgba8(0x1A, 0x1B, 0x1E, 0xF2),
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x24),
            CtColor::rgba(0xF2, 0xF2, 0xF4, 0xFF),
            CtColor::rgba(0xC8, 0xC8, 0xCE, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xFA, 0xFA, 0xFB, 0xF6),
            Color::from_rgba8(0x00, 0x00, 0x00, 0x1A),
            CtColor::rgba(0x16, 0x16, 0x19, 0xFF),
            CtColor::rgba(0x48, 0x48, 0x4E, 0xFF),
        )
    };

    let (pw, ph) = draw::phys(w, h);
    let Ok((buffer, canvas)) = state.pool.create_buffer(
        pw as i32,
        ph as i32,
        pw as i32 * 4,
        wl_shm::Format::Abgr8888,
    ) else {
        tracing::warn!("widgets: pool create_buffer failed");
        return;
    };
    canvas.fill(0);
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, pw, ph) else {
        return;
    };

    // Screen origin of the surface, for wallpaper sampling.
    let left_zone = if state.dock_position == dock::DockPos::Left {
        (dock::STRIP + dock::MARGIN) as f32
    } else {
        0.0
    };
    let sx = left_zone + MARGIN as f32;
    let sy = crate::PANEL_HEIGHT as f32 + MARGIN as f32;
    let wallpaper = ShellState::wallpaper_name();
    let card = |pixmap: &mut PixmapMut<'_>, y: f32| {
        glass::fill_glass(
            pixmap,
            &wallpaper,
            0.0,
            y,
            w as f32,
            CARD_H as f32,
            RADIUS,
            sx,
            sy + y,
            dark,
            fallback,
        );
        draw::stroke_round_rect(
            pixmap,
            0.5,
            y + 0.5,
            w as f32 - 1.0,
            CARD_H as f32 - 1.0,
            RADIUS,
            1.0,
            hairline,
        );
    };
    let tw = w as f32 - 40.0;

    let (time, date) = clock_lines();
    card(&mut pixmap, 0.0);
    draw::text_bold(&mut pixmap, 20.0, 14.0, tw, 50.0, 42.0, &time, fg);
    draw::text(&mut pixmap, 20.0, 70.0, tw, 18.0, 13.0, &date, fg2);

    if let Some(wx) = &state.weather {
        let y = (CARD_H + GAP) as f32;
        card(&mut pixmap, y);
        draw::text_bold(&mut pixmap, 20.0, y + 14.0, tw, 18.0, 13.0, &wx.place, fg);
        draw::text_bold(
            &mut pixmap,
            20.0,
            y + 32.0,
            tw,
            44.0,
            34.0,
            &deg(wx.temp),
            fg,
        );
        let detail = format!(
            "{}   H:{}  L:{}",
            weather::condition(wx.code),
            deg(wx.high),
            deg(wx.low)
        );
        draw::text(&mut pixmap, 20.0, y + 76.0, tw, 16.0, 12.0, &detail, fg2);
    }

    let wl_surface = layer.wl_surface().clone();
    state.set_viewport(&wl_surface, w, h);
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, pw as i32, ph as i32);
    // Empty input region: clicks fall through to the desktop.
    if let Ok(region) = Region::new(&state.compositor_state) {
        wl_surface.set_input_region(Some(region.wl_region()));
    }
    wl_surface.commit();
}

#[cfg(test)]
mod tests {
    #[test]
    fn heights_and_temps() {
        assert_eq!(super::height(false), super::CARD_H);
        assert_eq!(super::height(true), super::CARD_H * 2 + 12);
        assert_eq!(super::deg(-0.4), "0°");
        assert_eq!(super::deg(14.6), "15°");
        let (t, d) = super::clock_lines();
        assert_eq!(t.len(), 5);
        assert!(d.contains(", "));
    }
}
