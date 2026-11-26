//! Desktop widgets at the top left of the desktop, as on pearOS / macOS:
//! a month calendar and a weather card, side by side, under the windows.
//!
//! Weather is real or absent. With a city set in Settings (`weather_city` in
//! `shell.conf`) a background thread asks Open-Meteo (free, no key) every 30
//! minutes and caches the answer in `$XDG_CACHE_HOME/solyra/weather.json`.
//! With no city the card says how to set one; nothing is invented.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{Datelike, NaiveDate};

use crate::canvas::{text_width, Canvas, Fonts, Rgba};

pub const CARD: i32 = 196;
pub const GAP: i32 = 14;
pub const W: i32 = CARD * 2 + GAP;
pub const H: i32 = CARD;
pub const RADIUS: f32 = 22.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Weather {
    pub city: String,
    pub temp: f32,
    pub code: i32,
    pub hi: f32,
    pub lo: f32,
}

/// WMO weather code → a short condition.
pub fn condition(code: i32) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mostly Clear",
        2 => "Partly Cloudy",
        3 => "Cloudy",
        45 | 48 => "Fog",
        51..=57 => "Drizzle",
        61..=67 | 80..=82 => "Rain",
        71..=77 | 85 | 86 => "Snow",
        95..=99 => "Thunderstorms",
        _ => "—",
    }
}

/// `(lat, lon, name)` from Open-Meteo's geocoding answer.
pub fn parse_geocode(json: &str) -> Option<(f64, f64, String)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let r = v.get("results")?.get(0)?;
    Some((
        r.get("latitude")?.as_f64()?,
        r.get("longitude")?.as_f64()?,
        r.get("name")?.as_str()?.to_string(),
    ))
}

/// The current reading from Open-Meteo's forecast answer.
pub fn parse_forecast(json: &str, city: &str) -> Option<Weather> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let cur = v.get("current")?;
    let daily = v.get("daily")?;
    let first = |k: &str| daily.get(k)?.get(0)?.as_f64();
    Some(Weather {
        city: city.to_string(),
        temp: cur.get("temperature_2m")?.as_f64()? as f32,
        code: cur.get("weather_code")?.as_i64()? as i32,
        hi: first("temperature_2m_max")? as f32,
        lo: first("temperature_2m_min")? as f32,
    })
}

fn get(url: &str) -> Option<String> {
    let out = std::process::Command::new("curl")
        .args(["-fsS", "-m", "15", url])
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn cache_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))?;
    Some(base.join("solyra/weather.json"))
}

fn fetch(city: &str) -> Option<Weather> {
    let geo = get(&format!(
        "https://geocoding-api.open-meteo.com/v1/search?count=1&name={}",
        url_encode(city)
    ))?;
    let (lat, lon, name) = parse_geocode(&geo)?;
    let fc = get(&format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &current=temperature_2m,weather_code\
         &daily=temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=1"
    ))?;
    let w = parse_forecast(&fc, &name)?;
    if let Some(p) = cache_path() {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(
            p,
            serde_json::json!({"city": w.city, "temp": w.temp, "code": w.code, "hi": w.hi, "lo": w.lo}).to_string(),
        );
    }
    Some(w)
}

/// The last cached reading (so the card has something right after login).
pub fn cached() -> Option<Weather> {
    let text = std::fs::read_to_string(cache_path()?).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(Weather {
        city: v.get("city")?.as_str()?.to_string(),
        temp: v.get("temp")?.as_f64()? as f32,
        code: v.get("code")?.as_i64()? as i32,
        hi: v.get("hi")?.as_f64()? as f32,
        lo: v.get("lo")?.as_f64()? as f32,
    })
}

/// Keep `slot` fresh for whatever city `city()` returns (re-read each round,
/// so a change in Settings takes effect within a minute).
pub fn spawn_fetcher(
    slot: Arc<Mutex<Option<Weather>>>,
    city: impl Fn() -> Option<String> + Send + 'static,
    wake: impl Fn() + Send + 'static,
) {
    std::thread::spawn(move || {
        let mut last: Option<(String, std::time::Instant)> = None;
        loop {
            let want = city();
            let due = match (&want, &last) {
                (None, _) => false,
                (Some(c), Some((lc, t))) => c != lc || t.elapsed() >= Duration::from_secs(30 * 60),
                (Some(_), None) => true,
            };
            if due {
                let c = want.clone().unwrap_or_default();
                if let Some(w) = fetch(&c) {
                    if let Ok(mut s) = slot.lock() {
                        *s = Some(w);
                    }
                    wake();
                }
                last = Some((c, std::time::Instant::now()));
            }
            if want.is_none() && last.is_some() {
                last = None;
                if let Ok(mut s) = slot.lock() {
                    *s = None;
                }
                wake();
            }
            std::thread::sleep(Duration::from_secs(60));
        }
    });
}

/// Monday-first month grid: weeks of 7 cells, `None` outside the month.
pub fn month_grid(year: i32, month: u32) -> Vec<[Option<u32>; 7]> {
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("valid month");
    let lead = first.weekday().num_days_from_monday() as usize;
    let days = {
        let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
        NaiveDate::from_ymd_opt(ny, nm, 1)
            .expect("valid")
            .pred_opt()
            .expect("valid")
            .day()
    };
    let mut weeks = Vec::new();
    let mut week = [None; 7];
    let mut col = lead;
    for d in 1..=days {
        week[col] = Some(d);
        col += 1;
        if col == 7 {
            weeks.push(week);
            week = [None; 7];
            col = 0;
        }
    }
    if col > 0 {
        weeks.push(week);
    }
    weeks
}

/// Draw both cards. `dark` picks the card colours; the weather card is blue
/// in both themes, as on pearOS.
pub fn paint(cv: &mut Canvas<'_>, fonts: &Fonts, dark: bool, today: NaiveDate, weather: Option<&Weather>, city_set: bool) {
    calendar(cv, fonts, dark, today, 0, 0);
    weather_card(cv, fonts, weather, city_set, CARD + GAP, 0);
}

fn calendar(cv: &mut Canvas<'_>, fonts: &Fonts, dark: bool, today: NaiveDate, x: i32, y: i32) {
    let (bg, ink, dim) = if dark {
        (Rgba::rgba(20, 21, 26, 208), Rgba::rgb(245, 245, 250), Rgba::rgba(235, 235, 245, 148))
    } else {
        (Rgba::rgba(255, 255, 255, 188), Rgba::rgb(28, 28, 30), Rgba::rgba(60, 60, 67, 140))
    };
    cv.round_rect(x, y, CARD, CARD, RADIUS, bg);
    cv.round_rect_vgrad(
        x,
        y,
        CARD,
        CARD,
        RADIUS,
        Rgba::rgba(255, 255, 255, if dark { 20 } else { 42 }),
        Rgba::rgba(255, 255, 255, 4),
    );
    cv.round_rect_glass_rim(
        x,
        y,
        CARD,
        CARD,
        RADIUS,
        Rgba::rgba(255, 255, 255, if dark { 86 } else { 196 }),
        Rgba::rgba(255, 255, 255, if dark { 42 } else { 110 }),
        Rgba::rgba(255, 255, 255, if dark { 22 } else { 70 }),
    );
    let red = Rgba::rgb(0xff, 0x45, 0x3a);
    let month = today.format("%B").to_string().to_uppercase();
    cv.text(fonts.bold.as_ref(), &month, x + 16, y + 26, 12.5, red);
    let cols = ["M", "T", "W", "T", "F", "S", "S"];
    let cw = (CARD - 20) as f32 / 7.0;
    for (i, c) in cols.iter().enumerate() {
        let tw = text_width(fonts.semibold.as_ref(), c, 10.0);
        let cx = x + 10 + (cw * (i as f32 + 0.5)) as i32;
        cv.text(fonts.semibold.as_ref(), c, cx - tw / 2, y + 50, 10.0, dim);
    }
    let weeks = month_grid(today.year(), today.month());
    let row_h = ((CARD - 66) as f32 / weeks.len().max(5) as f32).min(23.0);
    for (r, week) in weeks.iter().enumerate() {
        for (i, d) in week.iter().enumerate() {
            let Some(d) = d else { continue };
            let label = d.to_string();
            let cx = x + 10 + (cw * (i as f32 + 0.5)) as i32;
            let cy = y + 62 + (row_h * (r as f32 + 0.5)) as i32;
            let is_today = *d == today.day();
            if is_today {
                cv.circle(cx as f32, cy as f32 - 0.5, 10.0, red);
            }
            let tw = text_width(fonts.medium.as_ref(), &label, 11.5);
            let col = if is_today { Rgba::rgb(255, 255, 255) } else { ink };
            cv.text(fonts.medium.as_ref(), &label, cx - tw / 2, cy + 4, 11.5, col);
        }
    }
}

fn weather_card(cv: &mut Canvas<'_>, fonts: &Fonts, w: Option<&Weather>, city_set: bool, x: i32, y: i32) {
    let top = Rgba::rgba(0x1e, 0x5d, 0xb8, 212);
    let bottom = Rgba::rgba(0x2c, 0x7b, 0xe0, 218);
    cv.round_rect_vgrad(x, y, CARD, CARD, RADIUS, top, bottom);
    cv.round_rect_vgrad(
        x,
        y,
        CARD,
        CARD,
        RADIUS,
        Rgba::rgba(255, 255, 255, 34),
        Rgba::rgba(255, 255, 255, 0),
    );
    cv.round_rect_glass_rim(
        x,
        y,
        CARD,
        CARD,
        RADIUS,
        Rgba::rgba(255, 255, 255, 138),
        Rgba::rgba(255, 255, 255, 68),
        Rgba::rgba(255, 255, 255, 34),
    );
    cv.stroke_round_rect(x, y, CARD, CARD, RADIUS, 1.0, Rgba::rgba(255, 255, 255, 46));
    let white = Rgba::rgb(255, 255, 255);
    let soft = Rgba::rgba(255, 255, 255, 200);
    match w {
        Some(w) => {
            cv.text_truncated(fonts.semibold.as_ref(), &w.city, x + 16, y + 28, 15.0, CARD - 32, white);
            let t = format!("{}°", w.temp.round() as i32);
            cv.text(fonts.regular.as_ref(), &t, x + 14, y + 86, 50.0, white);
            let (gx, gy) = ((x + 26) as f32, (y + 130) as f32);
            match w.code {
                0 | 1 => cv.icon_sun(gx, gy, white),
                _ => cv.icon_cloud(gx, gy, 20.0, white),
            }
            cv.text(fonts.semibold.as_ref(), condition(w.code), x + 16, y + 160, 12.5, white);
            let hl = format!("H:{}°  L:{}°", w.hi.round() as i32, w.lo.round() as i32);
            cv.text(fonts.medium.as_ref(), &hl, x + 16, y + 180, 12.5, soft);
        }
        None => {
            cv.text(fonts.semibold.as_ref(), "Weather", x + 16, y + 28, 15.0, white);
            cv.icon_cloud((x + CARD / 2) as f32, (y + 86) as f32, 30.0, soft);
            let lines: &[&str] = if city_set {
                &["Waiting for the", "forecast…"]
            } else {
                &["Choose your city in", "Settings › Desktop"]
            };
            for (i, l) in lines.iter().enumerate() {
                let tw = text_width(fonts.medium.as_ref(), l, 12.0);
                cv.text(fonts.medium.as_ref(), l, x + (CARD - tw) / 2, y + 140 + i as i32 * 17, 12.0, soft);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn november_2026_starts_on_a_sunday() {
        let g = month_grid(2026, 11);
        assert_eq!(g[0], [None, None, None, None, None, None, Some(1)]);
        assert_eq!(g.last().unwrap()[0], Some(30));
        assert_eq!(g.len(), 6);
        assert_eq!(month_grid(2026, 2).iter().flatten().flatten().count(), 28);
        assert_eq!(month_grid(2028, 2).iter().flatten().flatten().count(), 29);
    }

    #[test]
    fn open_meteo_answers_parse() {
        let geo = r#"{"results":[{"name":"Phoenix","latitude":33.448,"longitude":-112.074}]}"#;
        assert_eq!(parse_geocode(geo), Some((33.448, -112.074, "Phoenix".into())));
        assert_eq!(parse_geocode(r#"{"generationtime_ms":0.1}"#), None, "unknown city");
        let fc = r#"{"current":{"time":"2026-11-25T12:00","temperature_2m":16.4,"weather_code":2},
                     "daily":{"temperature_2m_max":[21.3],"temperature_2m_min":[8.9]}}"#;
        let w = parse_forecast(fc, "Phoenix").unwrap();
        assert_eq!((w.temp, w.code, w.hi, w.lo), (16.4, 2, 21.3, 8.9));
        assert_eq!(condition(w.code), "Partly Cloudy");
        assert_eq!(url_encode("São Paulo"), "S%C3%A3o%20Paulo");
    }
}
