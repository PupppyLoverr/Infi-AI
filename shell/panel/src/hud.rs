//! HUDs in the notch (Droppy's "Beautiful HUDs"): volume, brightness,
//! charging, Focus and lock / unlock show for a moment inside the island
//! instead of a separate overlay, then it settles back.
//!
//! Levels are read from the system at the moment the HUD is asked for
//! (`wpctl`, `brightnessctl`, `/sys/class/power_supply`), never guessed.

use std::time::Duration;

/// How long a HUD stays before the island settles back.
pub const SHOW_FOR: Duration = Duration::from_millis(1600);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudKind {
    Volume,
    Brightness,
    Charging,
    Focus,
    Unlocked,
}

impl HudKind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "volume" => Self::Volume,
            "brightness" => Self::Brightness,
            "charging" | "battery" => Self::Charging,
            "focus" | "dnd" => Self::Focus,
            "unlocked" | "unlock" => Self::Unlocked,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hud {
    pub kind: HudKind,
    /// 0..=100 where the kind has a level.
    pub level: Option<u8>,
    /// Muted, charging, Focus on.
    pub on: bool,
}

/// `wpctl get-volume @DEFAULT_AUDIO_SINK@` prints `Volume: 0.40 [MUTED]`.
pub fn parse_wpctl(out: &str) -> Option<(u8, bool)> {
    let v = out.split_whitespace().nth(1)?.parse::<f32>().ok()?;
    Some((
        (v * 100.0).round().clamp(0.0, 150.0) as u8,
        out.contains("MUTED"),
    ))
}

/// `brightnessctl -m` prints `device,class,raw,50%,max`.
pub fn parse_brightnessctl(out: &str) -> Option<u8> {
    let pct = out.lines().next()?.split(',').nth(3)?.trim_end_matches('%');
    pct.parse::<u8>().ok().map(|p| p.min(100))
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(cmd).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The charging state of the first battery: `Some(true)` charging,
/// `Some(false)` on battery, `None` with no battery.
pub fn charging() -> Option<bool> {
    let dir = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for e in dir.flatten() {
        let p = e.path();
        if std::fs::read_to_string(p.join("type")).ok()?.trim() != "Battery" {
            continue;
        }
        let st = std::fs::read_to_string(p.join("status")).ok()?;
        return Some(matches!(st.trim(), "Charging" | "Full"));
    }
    None
}

/// Read what a HUD of `kind` should show now. `focus_on` is the panel's own
/// Focus state; `battery` the last battery percentage it read.
pub fn read(kind: HudKind, focus_on: bool, battery: Option<u8>) -> Hud {
    match kind {
        HudKind::Volume => {
            let (level, muted) = run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])
                .as_deref()
                .and_then(parse_wpctl)
                .map(|(l, m)| (Some(l.min(100)), m))
                .unwrap_or((None, false));
            Hud {
                kind,
                level,
                on: muted,
            }
        }
        HudKind::Brightness => Hud {
            kind,
            level: run("brightnessctl", &["-m"])
                .as_deref()
                .and_then(parse_brightnessctl),
            on: false,
        },
        HudKind::Charging => Hud {
            kind,
            level: battery,
            on: charging().unwrap_or(false),
        },
        HudKind::Focus => Hud {
            kind,
            level: None,
            on: focus_on,
        },
        HudKind::Unlocked => Hud {
            kind,
            level: None,
            on: true,
        },
    }
}

/// The words shown on the right of a HUD.
pub fn label(h: &Hud) -> String {
    match h.kind {
        HudKind::Volume if h.on => "Muted".into(),
        HudKind::Volume | HudKind::Brightness => {
            h.level.map(|l| format!("{l}%")).unwrap_or_default()
        }
        HudKind::Charging => {
            let what = if h.on { "Charging" } else { "On battery" };
            match h.level {
                Some(l) => format!("{what} · {l}%"),
                None => what.into(),
            }
        }
        HudKind::Focus => if h.on { "Focus on" } else { "Focus off" }.into(),
        HudKind::Unlocked => "Unlocked".into(),
    }
}

/// Kinds that draw a level bar between the icon and the label.
pub fn has_bar(h: &Hud) -> bool {
    matches!(h.kind, HudKind::Volume | HudKind::Brightness) && h.level.is_some() && !h.on
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_tools_parse() {
        assert_eq!(parse_wpctl("Volume: 0.40\n"), Some((40, false)));
        assert_eq!(parse_wpctl("Volume: 0.73 [MUTED]\n"), Some((73, true)));
        assert_eq!(parse_wpctl("garbage"), None);
        assert_eq!(
            parse_brightnessctl("intel_backlight,backlight,512,50%,1024\n"),
            Some(50)
        );
        assert_eq!(parse_brightnessctl(""), None);
    }

    #[test]
    fn labels_say_what_happened() {
        let h = |kind, level, on| Hud { kind, level, on };
        assert_eq!(label(&h(HudKind::Volume, Some(40), false)), "40%");
        assert_eq!(label(&h(HudKind::Volume, Some(40), true)), "Muted");
        assert_eq!(
            label(&h(HudKind::Charging, Some(64), true)),
            "Charging · 64%"
        );
        assert_eq!(label(&h(HudKind::Focus, None, true)), "Focus on");
        assert!(has_bar(&h(HudKind::Brightness, Some(5), false)));
        assert!(
            !has_bar(&h(HudKind::Volume, Some(5), true)),
            "muted shows no bar"
        );
        assert_eq!(HudKind::parse("dnd"), Some(HudKind::Focus));
    }
}
