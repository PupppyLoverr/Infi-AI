//! Desktop widgets: a compact clock/date card and, while Open-Meteo
//! answers, a weather card — glass on the wallpaper, top-left, beneath
//! windows (layer `Bottom`). Input passes through to the desktop.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::{compositor::Region, shell::WaylandSurface};
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, weather, ShellState};

pub const W: u32 = 320;
pub const CARD_H: u32 = 132;
const WX_H: u32 = 104;
const GAP: u32 = 12;
/// Inset from the usable area's top-left (below the menubar, right of a
/// left dock).
pub const MARGIN: i32 = 16;
const RADIUS: f32 = 18.0;

pub fn height(has_weather: bool) -> u32 {
    if has_weather {
        CARD_H + GAP + WX_H
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
/// 12-hour time when the locale prefers it (en_US and similar).
pub fn twelve_hour() -> bool {
    let loc = ["LC_ALL", "LC_TIME", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default();
    ["en_US", "en_CA", "en_AU", "en_PH", "en_IN"]
        .iter()
        .any(|p| loc.starts_with(p))
}

fn fmt_time(h: i32, m: i32, twelve: bool) -> String {
    if twelve {
        let h12 = if h % 12 == 0 { 12 } else { h % 12 };
        format!("{h12}:{m:02} {}", if h < 12 { "am" } else { "pm" })
    } else {
        format!("{h:02}:{m:02}")
    }
}

/// Menubar clock, "Sat 10 Oct  4:15 pm", from the same local time as the
/// desktop clock card so the two never disagree.
pub fn menubar_clock() -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    local_now()
        .map(|(h, m, wd, d, mo)| {
            format!(
                "{} {d} {}  {}",
                DAYS[wd.rem_euclid(7) as usize],
                MONTHS[mo.rem_euclid(12) as usize],
                fmt_time(h, m, twelve_hour())
            )
        })
        .unwrap_or_default()
}

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
                fmt_time(h, m, twelve_hour()),
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
    // Translucent: the wallpaper shows through; a soft 8% edge, no frame.
    let (fill, hairline, fg, fg2) = if dark {
        (
            Color::from_rgba8(0x00, 0x00, 0x00, 0x38),
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x14),
            CtColor::rgba(0xF2, 0xF2, 0xF4, 0xFF),
            CtColor::rgba(0xC8, 0xC8, 0xCE, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x66),
            Color::from_rgba8(0x00, 0x00, 0x00, 0x14),
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

    let card = |pixmap: &mut PixmapMut<'_>, y: f32, ch: u32| {
        draw::fill_round_rect(pixmap, 0.0, y, w as f32, ch as f32, RADIUS, fill);
        draw::stroke_round_rect(
            pixmap,
            0.5,
            y + 0.5,
            w as f32 - 1.0,
            ch as f32 - 1.0,
            RADIUS,
            1.0,
            hairline,
        );
    };
    let tw = w as f32 - 40.0;

    let (time, date) = clock_lines();
    card(&mut pixmap, 0.0, CARD_H);
    // 64pt light numerals; a 12 h suffix ("pm") sits small beside them.
    let (digits, suffix) = time.split_once(' ').unwrap_or((&time, ""));
    draw::text_light(&mut pixmap, 18.0, 8.0, tw, 80.0, 64.0, digits, fg);
    if !suffix.is_empty() {
        let dx = 18.0 + draw::text_width(64.0, digits) + 6.0;
        draw::text(&mut pixmap, dx, 50.0, tw - dx, 26.0, 20.0, suffix, fg2);
    }
    draw::text(&mut pixmap, 21.0, 96.0, tw, 20.0, 14.0, &date, fg2);

    if let Some(wx) = &state.weather {
        let y = (CARD_H + GAP) as f32;
        card(&mut pixmap, y, WX_H);
        let age = weather::age_caption(wx.fetched, weather::now_secs());
        let age_w = age
            .as_deref()
            .map_or(0.0, |a| draw::text_width(11.0, a) + 8.0);
        draw::text_bold(
            &mut pixmap,
            20.0,
            y + 14.0,
            tw - age_w,
            18.0,
            13.0,
            &wx.place,
            fg,
        );
        if let Some(a) = &age {
            let ax = w as f32 - 20.0 - draw::text_width(11.0, a);
            draw::text(&mut pixmap, ax, y + 16.0, age_w, 16.0, 11.0, a, fg2);
        }
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
    fn time_follows_the_hour_format() {
        assert_eq!(super::fmt_time(0, 5, true), "12:05 am");
        assert_eq!(super::fmt_time(12, 0, true), "12:00 pm");
        assert_eq!(super::fmt_time(16, 15, true), "4:15 pm");
        assert_eq!(super::fmt_time(16, 15, false), "16:15");
        assert_eq!(super::fmt_time(7, 3, false), "07:03");
    }

    #[test]
    fn heights_and_temps() {
        assert_eq!(super::height(false), super::CARD_H);
        assert_eq!(super::height(true), super::CARD_H + 12 + super::WX_H);
        assert_eq!(super::deg(-0.4), "0°");
        assert_eq!(super::deg(14.6), "15°");
        let (t, d) = super::clock_lines();
        assert_eq!(t.len(), 5);
        assert!(d.contains(", "));
    }
}
