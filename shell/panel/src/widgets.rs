//! Desktop widgets: the pearOS-style calendar and glance cards sitting on the
//! wallpaper at the upper left. They are informational only, so the surface
//! takes no input and lives below windows.

use chrono::{Datelike, Local, NaiveDate};

use crate::canvas::{text_width, Canvas, Fonts, Rgba};
use crate::tokens::Theme;

pub const W: u32 = 440;
pub const H: u32 = 212;
pub const RADIUS: f32 = 18.0;

const CAL_W: i32 = 204;
const CAL_H: i32 = 202;
const GLANCE_X: i32 = 224;
const GLANCE_W: i32 = 204;
const GLANCE_H: i32 = 160;

fn normalize_weather(raw: Option<&str>) -> Option<String> {
    let line = raw.map(str::trim).filter(|s| !s.is_empty())?;
    if line.contains('°') {
        return Some(line.to_string());
    }
    let numeric = line
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | '+' | '-'))
        && line.chars().any(|c| c.is_ascii_digit());
    Some(if numeric {
        format!("{line}°")
    } else {
        line.to_string()
    })
}

fn weather_reading() -> Option<String> {
    let path = solyra_conf::config_path("weather.txt")?;
    let raw = std::fs::read_to_string(path).ok()?;
    normalize_weather(raw.lines().next())
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_y, next_m) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(next_y, next_m, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(30)
}

/// The two widget bodies, for the compositor's blur region.
pub fn blur_shapes() -> Vec<(i32, i32, i32, i32)> {
    vec![(0, 0, CAL_W, CAL_H), (GLANCE_X, 0, GLANCE_W, GLANCE_H)]
}

pub fn draw(cv: &mut Canvas<'_>, th: &Theme, fonts: &Fonts) {
    let now = Local::now();
    let dark = th.surface.0 < 128;
    let card = if dark {
        Rgba::rgba(18, 19, 22, 178)
    } else {
        Rgba::rgba(248, 248, 250, 188)
    };
    let edge = if dark {
        Rgba::rgba(255, 255, 255, 28)
    } else {
        Rgba::rgba(0, 0, 0, 20)
    };
    let text = if dark {
        Rgba::rgb(245, 245, 247)
    } else {
        th.text
    };
    let dim = if dark {
        Rgba::rgba(235, 235, 240, 150)
    } else {
        th.text_dim
    };

    // Month calendar, modelled on pearOS's desktop widget: compact month
    // header, Monday-first week, and a red ringless disc for today.
    cv.round_rect(0, 0, CAL_W, CAL_H, RADIUS, card);
    cv.stroke_round_rect(0, 0, CAL_W, CAL_H, RADIUS, 1.0, edge);
    let month = now.format("%B").to_string().to_uppercase();
    cv.text(
        fonts.semibold.as_ref(),
        &month,
        20,
        24,
        12.5,
        Rgba::rgb(0xff, 0x45, 0x3a),
    );
    let labels = ["M", "T", "W", "T", "F", "S", "S"];
    let col_w = 26;
    let grid_x = 18;
    for (i, label) in labels.iter().enumerate() {
        let x = grid_x
            + i as i32 * col_w
            + (col_w - text_width(fonts.regular.as_ref(), label, 9.5)) / 2;
        cv.text(fonts.regular.as_ref(), label, x, 49, 9.5, dim);
    }
    let first = NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
        .map(|d| d.weekday().num_days_from_monday() as i32)
        .unwrap_or(0);
    let days = days_in_month(now.year(), now.month()) as i32;
    let row_h = 24;
    for day in 1..=days {
        let idx = first + day - 1;
        let row = idx / 7;
        let col = idx % 7;
        let cx = grid_x + col * col_w + col_w / 2;
        let cy = 68 + row * row_h + 8;
        let label = day.to_string();
        let tw = text_width(fonts.regular.as_ref(), &label, 11.0);
        if day == now.day() as i32 {
            cv.circle(cx as f32, cy as f32 - 3.0, 9.0, Rgba::rgb(0xff, 0x45, 0x3a));
            cv.text(
                fonts.semibold.as_ref(),
                &label,
                cx - tw / 2,
                cy + 1,
                11.0,
                Rgba::rgb(255, 255, 255),
            );
        } else {
            cv.text(
                fonts.regular.as_ref(),
                &label,
                cx - tw / 2,
                cy + 1,
                11.0,
                text,
            );
        }
    }

    // Time/weather glance. pearOS uses a blue weather card; without a real
    // weather reading it remains a useful clock/date card rather than inventing
    // a forecast.
    cv.round_rect_vgrad(
        GLANCE_X,
        0,
        GLANCE_W,
        GLANCE_H,
        RADIUS,
        Rgba::rgba(0x20, 0x72, 0xd8, 205),
        Rgba::rgba(0x14, 0x45, 0x92, 214),
    );
    cv.stroke_round_rect(
        GLANCE_X,
        0,
        GLANCE_W,
        GLANCE_H,
        RADIUS,
        1.0,
        Rgba::rgba(255, 255, 255, 42),
    );
    let gx = GLANCE_X + 20;
    let white = Rgba::rgb(255, 255, 255);
    let soft = Rgba::rgba(255, 255, 255, 175);
    if let Some(weather) = weather_reading() {
        cv.text(fonts.semibold.as_ref(), "Weather", gx, 28, 13.0, soft);
        cv.text(fonts.regular.as_ref(), &weather, gx, 82, 44.0, white);
        cv.text(
            fonts.regular.as_ref(),
            &now.format("%A %-d %B").to_string(),
            gx,
            112,
            13.0,
            soft,
        );
    } else {
        cv.text(fonts.semibold.as_ref(), "Today", gx, 28, 13.0, soft);
        cv.text(
            fonts.regular.as_ref(),
            &now.format("%-H:%M").to_string(),
            gx,
            84,
            44.0,
            white,
        );
        cv.text(
            fonts.regular.as_ref(),
            &now.format("%A %-d %B").to_string(),
            gx,
            114,
            13.0,
            soft,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_lengths_cover_leap_years() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2025, 2), 28);
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 12), 31);
    }

    #[test]
    fn weather_lines_are_normalised() {
        assert_eq!(normalize_weather(Some("25")), Some("25°".into()));
        assert_eq!(normalize_weather(Some(" 18 °C ")), Some("18 °C".into()));
        assert_eq!(normalize_weather(None), None);
    }
}
