//! Agent-workspace desk: one calm panel with the clock, the workspace project,
//! agent activity and the execution environment switch.
//!
//! Shown when `desktop_mode=agent`. The panel keeps a real layer-shell surface
//! on the right of the desktop (`solyra-desk`). The same card is painted into
//! the agents overlay when that surface is open and empty.

use crate::canvas::{text_width, Canvas, Fonts, Rgba};
use crate::tokens::Theme;

pub const W: i32 = 360;
pub const H: i32 = 304;
pub const RADIUS: f32 = 24.0;

/// Shown when neither `SOLYRA_PROJECT` nor the project file is set.
pub const PROJECT_PLACEHOLDER: [&str; 3] = [
    "No project open",
    "Set SOLYRA_PROJECT, or write a path",
    "in ~/.config/solyra/project.txt",
];

pub const ENVIRONMENTS: [&str; 3] = ["Cloud", "Local", "Worktree"];

pub struct Glance<'a> {
    pub clock: &'a str,
    pub date: &'a str,
    pub project_lines: &'a [&'a str],
    /// 0 = Cloud, 1 = Local, 2 = Worktree.
    pub environment: u8,
    /// `None` when no agent runtime is connected.
    pub agents_running: Option<usize>,
}

/// A clickable environment chip, in the same coordinates as the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub environment: u8,
}

/// Lines for the Project section.
///
/// `SOLYRA_PROJECT` wins when it is set. Otherwise the first non-empty lines
/// of `~/.config/solyra/project.txt` are used. Both absent: the calm placeholder.
pub fn project_lines() -> Vec<String> {
    let env = std::env::var("SOLYRA_PROJECT").ok();
    let file = solyra_conf::config_path("project.txt").and_then(|p| std::fs::read_to_string(p).ok());
    resolve_project(env.as_deref(), file.as_deref())
}

/// Pure project resolution so tests do not touch the process environment.
pub fn resolve_project(env_value: Option<&str>, file_text: Option<&str>) -> Vec<String> {
    if let Some(raw) = env_value.map(str::trim).filter(|s| !s.is_empty()) {
        return describe_project(raw);
    }
    if let Some(text) = file_text {
        let lines = file_lines(text);
        if !lines.is_empty() {
            return lines;
        }
    }
    PROJECT_PLACEHOLDER
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

fn describe_project(raw: &str) -> Vec<String> {
    let mut lines: Vec<String> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(3)
        .map(str::to_string)
        .collect();
    if lines.len() == 1 {
        let only = &lines[0];
        if let Some(name) = std::path::Path::new(only)
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|n| *n != only.as_str() && !n.is_empty())
        {
            lines.insert(0, name.to_string());
        }
    }
    if lines.is_empty() {
        PROJECT_PLACEHOLDER
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    } else {
        lines
    }
}

fn file_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .take(3)
        .map(str::to_string)
        .collect()
}

fn draw_env_icon(cv: &mut Canvas<'_>, env: u8, cx: f32, cy: f32, size: f32, col: Rgba) {
    match env {
        0 => cv.icon_cloud(cx, cy, size, col),
        1 => cv.icon_laptop(cx, cy, size, col),
        _ => cv.icon_branch(cx, cy, size, col),
    }
}

/// Paint the desk panel at `x`,`y`: one calm surface with the time, the
/// project and the environment switch. Returns the panel size and the
/// clickable environment segments.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    g: &Glance<'_>,
    _tint: Option<Rgba>,
    hover: Option<(f64, f64)>,
    x: i32,
    y: i32,
) -> (i32, i32, Vec<Target>) {
    let (w, h) = (W, H);
    let dark = th.surface.0 < 128;

    let body = if dark {
        Rgba::rgba(29, 30, 34, 238)
    } else {
        Rgba::rgba(248, 248, 250, 240)
    };
    cv.round_rect(x, y, w, h, RADIUS, body);
    cv.stroke_round_rect(
        x,
        y,
        w,
        h,
        RADIUS,
        1.0,
        if dark {
            Rgba::rgba(255, 255, 255, 30)
        } else {
            Rgba::rgba(0, 0, 0, 22)
        },
    );

    let pad = 24;
    let (ix, iw) = (x + pad, w - pad * 2);
    let hair = if dark {
        Rgba::rgba(255, 255, 255, 22)
    } else {
        Rgba::rgba(0, 0, 0, 16)
    };

    // Time, then what the agents are doing as one quiet status.
    cv.text(fonts.regular.as_ref(), g.date, ix, y + 40, 13.0, th.text_dim);
    cv.text_tracked(fonts.regular.as_ref(), g.clock, ix - 2, y + 96, 56.0, -1.6, th.text);
    let (status, live) = match g.agents_running {
        None => ("Offline".to_string(), false),
        Some(0) => ("Idle".to_string(), false),
        Some(n) => (format!("{n} active"), true),
    };
    let sw = text_width(fonts.medium.as_ref(), &status, 12.0);
    let sx = ix + iw - sw;
    cv.text(fonts.medium.as_ref(), &status, sx, y + 40, 12.0, th.text_med);
    let dot = (sx - 12) as f32;
    if live {
        cv.circle(dot, (y + 36) as f32, 3.2, th.agent_active);
    } else {
        cv.ring(dot, (y + 36) as f32, 3.0, 1.2, th.text_dim);
    }
    cv.fill_rect(ix, y + 120, iw, 1, hair);

    // Project.
    let placeholder: &[&str] = &PROJECT_PLACEHOLDER;
    let lines = if g.project_lines.is_empty() {
        placeholder
    } else {
        g.project_lines
    };
    let py = y + 138;
    cv.draw_app_squircle(ix, py + 2, 38, "files", None, fonts);
    let tx = ix + 38 + 14;
    let tw = iw - 38 - 14;
    if let Some(first) = lines.first() {
        cv.text_truncated(fonts.semibold.as_ref(), first, tx, py + 18, 15.0, tw, th.text);
    }
    for (i, line) in lines.iter().skip(1).take(2).enumerate() {
        let is_path = line.starts_with('/') || line.starts_with('~');
        let font = if is_path {
            fonts.mono.as_ref()
        } else {
            fonts.regular.as_ref()
        };
        let col = if i == 0 { th.text_med } else { th.text_dim };
        cv.text_truncated(font, line, tx, py + 37 + i as i32 * 16, 12.0, tw, col);
    }

    // Environment: a plain segmented control.
    let cur_env = g.environment.min(2);
    let label_y = y + 224;
    cv.text(fonts.regular.as_ref(), "Run agents in", ix, label_y, 12.0, th.text_dim);
    let track_y = label_y + 12;
    let track_h = 34;
    cv.round_rect(
        ix,
        track_y,
        iw,
        track_h,
        11.0,
        if dark {
            Rgba::rgba(255, 255, 255, 12)
        } else {
            Rgba::rgba(0, 0, 0, 9)
        },
    );
    let seg_pad = 3;
    let seg_h = track_h - seg_pad * 2;
    let seg_w = (iw - seg_pad * 2) / 3;
    let seg_y = track_y + seg_pad;
    let mut targets = Vec::with_capacity(ENVIRONMENTS.len());
    for (i, label) in ENVIRONMENTS.iter().enumerate() {
        let sx = ix + seg_pad + i as i32 * seg_w;
        let sw = if i == 2 { ix + iw - seg_pad - sx } else { seg_w };
        let env = i as u8;
        let active = env == cur_env;
        let over = hover.is_some_and(|(hx, hy)| {
            hx >= sx as f64
                && hx < (sx + sw) as f64
                && hy >= seg_y as f64
                && hy < (seg_y + seg_h) as f64
        });
        if active {
            cv.round_rect(
                sx,
                seg_y,
                sw,
                seg_h,
                9.0,
                if dark {
                    Rgba::rgba(255, 255, 255, 30)
                } else {
                    Rgba::rgb(255, 255, 255)
                },
            );
            if !dark {
                cv.stroke_round_rect(sx, seg_y, sw, seg_h, 9.0, 1.0, Rgba::rgba(0, 0, 0, 16));
            }
        } else if over {
            cv.round_rect(sx, seg_y, sw, seg_h, 9.0, th.tile_hover);
        }
        let fg = if active { th.text } else { th.text_med };
        let font = if active {
            fonts.semibold.as_ref()
        } else {
            fonts.regular.as_ref()
        };
        let lw = text_width(font, label, 12.5);
        let group_w = 13 + 6 + lw;
        let gx = sx + (sw - group_w) / 2;
        draw_env_icon(cv, env, gx as f32 + 6.5, seg_y as f32 + seg_h as f32 / 2.0, 11.5, fg);
        cv.text(font, label, gx + 19, seg_y + seg_h / 2 + 4, 12.5, fg);
        targets.push(Target {
            x: sx,
            y: seg_y,
            w: sw,
            h: seg_h,
            environment: env,
        });
    }

    (w, h, targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::{Accent, ThemeKind};

    #[test]
    fn paints_chips_inside_the_card() {
        let fonts = Fonts::load();
        let th = Theme::with(ThemeKind::Dark, Accent::Blue);
        let mut buf = vec![0u8; 480 * 520 * 4];
        let mut cv = Canvas::new(&mut buf, 480, 520, 1);
        let g = Glance {
            clock: "9:50",
            date: "Tuesday 17 November",
            project_lines: &["Infi-AI", "solyra-panel", "agent wrap"],
            environment: 1,
            agents_running: Some(2),
        };
        let (w, h, hits) = paint(&mut cv, &th, &fonts, &g, None, None, 20, 20);
        assert_eq!((w, h), (W, H));
        assert_eq!(hits.len(), 3);
        assert!(hits.iter().all(|t| t.h >= 24 && t.w >= 24));
        for t in &hits {
            assert!(t.x >= 20 && t.y >= 20);
            assert!(t.x + t.w <= 20 + W);
            assert!(t.y + t.h <= 20 + H);
        }
        assert!(hits.windows(2).all(|p| p[0].x + p[0].w <= p[1].x));
        assert_eq!(
            hits.iter().map(|t| t.environment).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn project_source_order_is_env_then_file_then_placeholder() {
        let from_env = resolve_project(Some("/home/me/Infi-AI"), Some("ignored\n"));
        assert_eq!(from_env[0], "Infi-AI");
        assert_eq!(from_env[1], "/home/me/Infi-AI");
        let from_file = resolve_project(Some("  "), Some("# note\nAlpha\n\nBeta\nGamma\nDelta\n"));
        assert_eq!(from_file, vec!["Alpha", "Beta", "Gamma"]);
        assert_eq!(resolve_project(Some("Infi-AI"), None), vec!["Infi-AI"]);
        let none = resolve_project(None, Some("   \n# only a comment\n"));
        assert_eq!(none[0], PROJECT_PLACEHOLDER[0]);
        assert!(!none.iter().any(|l| l.contains('●') || l.contains('■')));
    }
}
