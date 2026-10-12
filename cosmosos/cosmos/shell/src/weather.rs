//! Desktop weather card data from Open-Meteo (free, keyless).
//!
//! A worker thread geocodes the configured place (or the time zone's
//! city), fetches current conditions plus today's high/low, and sends
//! `Some(Weather)`. A failed fetch sends the last good reading for the
//! same place from ~/.cache/cosmos/weather.json (the card captions its
//! age), or `None` when there is none, so the card hides rather than
//! inventing numbers.

use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct Weather {
    pub place: String,
    pub temp: f64,
    pub high: f64,
    pub low: f64,
    pub code: u32,
    /// Unix seconds when this reading was fetched.
    pub fetched: u64,
}

const REFRESH: Duration = Duration::from_secs(15 * 60);
const RETRY: Duration = Duration::from_secs(2 * 60);

/// Spawn the fetch loop. Send a new place name ("" = time zone city) on
/// the returned sender to refetch immediately.
pub fn start(tx: calloop::channel::Sender<Option<Weather>>, place: String) -> mpsc::Sender<String> {
    let (city_tx, city_rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut place = place;
        loop {
            let name = if place.is_empty() {
                tz_city().unwrap_or_default()
            } else {
                place.clone()
            };
            let fresh = if name.is_empty() { None } else { fetch(&name) };
            if let Some(w) = &fresh {
                save_cache(&name, w);
            }
            let got = fresh.clone().or_else(|| load_cache(&name));
            let wait = if fresh.is_some() { REFRESH } else { RETRY };
            if tx.send(got).is_err() {
                return;
            }
            match city_rx.recv_timeout(wait) {
                Ok(p) => place = p,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    });
    city_tx
}

fn fetch(name: &str) -> Option<Weather> {
    let geo = get_json(&format!(
        "https://geocoding-api.open-meteo.com/v1/search?count=1&format=json&name={}",
        url_encode(name)
    ))?;
    let hit = geo.get("results")?.get(0)?;
    let lat = hit.get("latitude")?.as_f64()?;
    let lon = hit.get("longitude")?.as_f64()?;
    let place = hit.get("name")?.as_str()?.to_string();
    let fc = get_json(&format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &current=temperature_2m,weather_code\
         &daily=temperature_2m_max,temperature_2m_min&forecast_days=1&timezone=auto"
    ))?;
    parse_forecast(place, &fc)
}

fn parse_forecast(place: String, fc: &serde_json::Value) -> Option<Weather> {
    let cur = fc.get("current")?;
    let daily = fc.get("daily")?;
    Some(Weather {
        fetched: now_secs(),
        place,
        temp: cur.get("temperature_2m")?.as_f64()?,
        code: cur.get("weather_code")?.as_u64()? as u32,
        high: daily.get("temperature_2m_max")?.get(0)?.as_f64()?,
        low: daily.get("temperature_2m_min")?.get(0)?.as_f64()?,
    })
}

/// HTTPS GET via the image's wget (keeps a TLS stack out of the shell).
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))?;
    Some(base.join("cosmos/weather.json"))
}

fn save_cache(query: &str, w: &Weather) {
    let Some(path) = cache_path() else {
        return;
    };
    let v = serde_json::json!({
        "query": query, "place": w.place, "temp": w.temp, "high": w.high,
        "low": w.low, "code": w.code, "fetched": w.fetched,
    });
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, v.to_string());
}

fn load_cache(query: &str) -> Option<Weather> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(cache_path()?).ok()?).ok()?;
    if v.get("query")?.as_str()? != query {
        return None;
    }
    Some(Weather {
        place: v.get("place")?.as_str()?.to_string(),
        temp: v.get("temp")?.as_f64()?,
        high: v.get("high")?.as_f64()?,
        low: v.get("low")?.as_f64()?,
        code: v.get("code")?.as_u64()? as u32,
        fetched: v.get("fetched")?.as_u64()?,
    })
}

/// "Updated 2 h ago" once a reading is older than one refresh cycle.
pub fn age_caption(fetched: u64, now: u64) -> Option<String> {
    let age = now.saturating_sub(fetched);
    if age < 20 * 60 {
        return None;
    }
    Some(if age < 3600 {
        format!("Updated {} min ago", age / 60)
    } else if age < 86_400 {
        format!("Updated {} h ago", age / 3600)
    } else {
        format!("Updated {} d ago", age / 86_400)
    })
}

#[cfg(test)]
mod cache_tests {
    #[test]
    fn age_caption_shows_only_stale_readings() {
        assert_eq!(super::age_caption(0, 600), None);
        assert_eq!(
            super::age_caption(0, 1800).as_deref(),
            Some("Updated 30 min ago")
        );
        assert_eq!(
            super::age_caption(0, 7200).as_deref(),
            Some("Updated 2 h ago")
        );
        assert_eq!(
            super::age_caption(0, 3 * 86_400).as_deref(),
            Some("Updated 3 d ago")
        );
    }
}

fn get_json(url: &str) -> Option<serde_json::Value> {
    let out = Command::new("wget")
        .args(["-qO-", "--timeout=10", "--tries=1", url])
        .output()
        .ok()?;
    if !out.status.success() {
        tracing::debug!(url, "weather: fetch failed");
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// "Europe/London" → "London"; none for UTC-style zones.
fn tz_city() -> Option<String> {
    let zone = std::env::var("TZ")
        .ok()
        .map(|t| t.trim_start_matches(':').to_string())
        .or_else(|| {
            std::fs::read_link("/etc/localtime")
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .or_else(|| std::fs::read_to_string("/etc/timezone").ok())?;
    city_from_zone(&zone)
}

fn city_from_zone(zone: &str) -> Option<String> {
    let zone = zone.trim();
    let zone = zone
        .rsplit_once("zoneinfo/")
        .map(|(_, z)| z)
        .unwrap_or(zone);
    if !zone.contains('/') || zone.starts_with("Etc/") {
        return None;
    }
    let city = zone.rsplit('/').next()?.replace('_', " ");
    (!city.is_empty()).then_some(city)
}

/// WMO weather interpretation code → short condition.
pub fn condition(code: u32) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51..=57 => "Drizzle",
        61..=67 => "Rain",
        71..=77 => "Snow",
        80..=82 => "Showers",
        85 | 86 => "Snow showers",
        95..=99 => "Thunderstorm",
        _ => "—",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_to_city() {
        assert_eq!(
            city_from_zone("/usr/share/zoneinfo/Europe/London").as_deref(),
            Some("London")
        );
        assert_eq!(
            city_from_zone("America/New_York").as_deref(),
            Some("New York")
        );
        assert_eq!(
            city_from_zone("America/Argentina/Buenos_Aires").as_deref(),
            Some("Buenos Aires")
        );
        assert_eq!(city_from_zone("/usr/share/zoneinfo/Etc/UTC"), None);
        assert_eq!(city_from_zone("UTC"), None);
    }

    #[test]
    fn encodes_query() {
        assert_eq!(url_encode("São Paulo"), "S%C3%A3o%20Paulo");
    }

    #[test]
    fn parses_open_meteo_forecast() {
        let fc: serde_json::Value = serde_json::from_str(
            r#"{"current":{"time":"2026-10-08T10:15","temperature_2m":14.3,"weather_code":3},
                "daily":{"time":["2026-10-08"],"temperature_2m_max":[16.1],"temperature_2m_min":[9.4]}}"#,
        )
        .unwrap();
        let w = parse_forecast("London".into(), &fc).unwrap();
        assert_eq!((w.temp, w.high, w.low, w.code), (14.3, 16.1, 9.4, 3));
        assert_eq!(condition(w.code), "Overcast");
        assert!(parse_forecast("x".into(), &serde_json::json!({"current":{}})).is_none());
    }
}
