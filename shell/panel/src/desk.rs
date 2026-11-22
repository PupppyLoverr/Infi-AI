//! Agent-workspace desk: Mica/Liquid Glass widget board showing live clock,
//! workspace project, agent runtime activity, and execution environment.
//!
//! Shown when `desktop_mode=agent`. The panel keeps a real layer-shell surface
//! on the right of the desktop (`solyra-desk`). The same card is painted into
//! the agents overlay when that surface is open and empty.

use crate::canvas::{lerp_rgba, text_width, Canvas, Fonts, Rgba};
use crate::tokens::Theme;

pub const W: i32 = 360;
pub const H: i32 = 392;
pub const RADIUS: f32 = 22.0;

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

fn env_subtitle(env: u8) -> &'static str {
    match env {
        0 => "Remote sandbox",
        1 => "This machine",
        _ => "Isolated branch",
    }
}

fn draw_env_icon(cv: &mut Canvas<'_>, env: u8, cx: f32, cy: f32, size: f32, col: Rgba) {
    match env {
        0 => cv.icon_cloud(cx, cy, size, col),
        1 => cv.icon_laptop(cx, cy, size, col),
        _ => cv.icon_branch(cx, cy, size, col),
    }
}

/// Nested Mica glass tile used for Workspace and Bento telemetry cards.
fn sub_card(cv: &mut Canvas<'_>, dark: bool, x: i32, y: i32, w: i32, h: i32, r: f32) {
    let (top, bot) = if dark {
        (Rgba::rgba(255, 255, 255, 19), Rgba::rgba(255, 255, 255, 10))
    } else {
        (Rgba::rgba(255, 255, 255, 178), Rgba::rgba(248, 250, 252, 138))
    };
    cv.round_rect_vgrad(x, y, w, h, r, top, bot);
    cv.round_rect_glass_rim(
        x,
        y,
        w,
        h,
        r,
        if dark {
            Rgba::rgba(255, 255, 255, 42)
        } else {
            Rgba::rgba(255, 255, 255, 224)
        },
        if dark {
            Rgba::rgba(255, 255, 255, 18)
        } else {
            Rgba::rgba(0, 0, 0, 14)
        },
        if dark {
            Rgba::rgba(255, 255, 255, 12)
        } else {
            Rgba::rgba(0, 0, 0, 22)
        },
    );
}

/// Paint the Mica/Liquid Glass desk board at `x`,`y`. Returns card size and
/// clickable environment segment targets.
pub fn paint(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    g: &Glance<'_>,
    tint: Option<Rgba>,
    hover: Option<(f64, f64)>,
    x: i32,
    y: i32,
) -> (i32, i32, Vec<Target>) {
    let w = W;
    let h = H;
    let dark = th.surface.0 < 128;

    // Outer Mica / Liquid Glass board: subtle wallpaper-tinted luminosity shell.
    let base_top = if dark {
        let b = Rgba::rgb(30, 32, 36);
        let t = tint.map_or(b, |c| lerp_rgba(b, c, 0.16));
        Rgba::rgba(t.0, t.1, t.2, 212)
    } else {
        let b = Rgba::rgb(250, 251, 253);
        let t = tint.map_or(b, |c| lerp_rgba(b, c, 0.12));
        Rgba::rgba(t.0, t.1, t.2, 226)
    };
    let base_bot = if dark {
        let b = Rgba::rgb(18, 20, 23);
        let t = tint.map_or(b, |c| lerp_rgba(b, c, 0.12));
        Rgba::rgba(t.0, t.1, t.2, 224)
    } else {
        let b = Rgba::rgb(238, 240, 244);
        let t = tint.map_or(b, |c| lerp_rgba(b, c, 0.08));
        Rgba::rgba(t.0, t.1, t.2, 236)
    };
    cv.round_rect_vgrad(x, y, w, h, RADIUS, base_top, base_bot);
    // Soft upper frosted luminosity sheen.
    cv.round_rect_vgrad(
        x + 1,
        y + 1,
        w - 2,
        120,
        RADIUS - 1.0,
        if dark {
            Rgba::rgba(255, 255, 255, 18)
        } else {
            Rgba::rgba(255, 255, 255, 110)
        },
        Rgba::rgba(255, 255, 255, 0),
    );
    cv.round_rect_glass_rim(
        x,
        y,
        w,
        h,
        RADIUS,
        if dark {
            Rgba::rgba(255, 255, 255, 112)
        } else {
            Rgba::rgba(255, 255, 255, 236)
        },
        if dark {
            Rgba::rgba(255, 255, 255, 34)
        } else {
            Rgba::rgba(0, 0, 0, 16)
        },
        if dark {
            Rgba::rgba(255, 255, 255, 20)
        } else {
            Rgba::rgba(0, 0, 0, 24)
        },
    );

    let pad = 16;
    let inner_x = x + pad;
    let inner_w = w - pad * 2;

    // 1. Header: tracked date + bold tabular clock + live runtime status badge.
    cv.text_tracked(
        fonts.semibold.as_ref(),
        &g.date.to_uppercase(),
        inner_x + 4,
        y + 30,
        10.5,
        0.7,
        th.text_dim,
    );
    cv.text_tracked(
        fonts.bold.as_ref(),
        g.clock,
        inner_x + 2,
        y + 72,
        38.0,
        -1.0,
        th.text,
    );

    let (pill_label, pill_col, pill_live) = match g.agents_running {
        None => ("Offline".to_string(), th.text_dim, false),
        Some(0) => ("Idle".to_string(), th.text_med, false),
        Some(1) => ("1 active".to_string(), th.agent_active, true),
        Some(n) => (format!("{n} active"), th.agent_active, true),
    };
    let pill_tw = text_width(fonts.semibold.as_ref(), &pill_label, 11.2);
    let pill_w = pill_tw + 30;
    let pill_h = 26;
    let pill_x = inner_x + inner_w - pill_w;
    let pill_y = y + 22;
    let pill_bg = if pill_live {
        Rgba::rgba(pill_col.0, pill_col.1, pill_col.2, if dark { 32 } else { 26 })
    } else if dark {
        Rgba::rgba(255, 255, 255, 16)
    } else {
        Rgba::rgba(0, 0, 0, 12)
    };
    cv.round_rect(pill_x, pill_y, pill_w, pill_h, 13.0, pill_bg);
    cv.stroke_round_rect(
        pill_x,
        pill_y,
        pill_w,
        pill_h,
        13.0,
        1.0,
        if pill_live {
            Rgba::rgba(pill_col.0, pill_col.1, pill_col.2, 86)
        } else if dark {
            Rgba::rgba(255, 255, 255, 28)
        } else {
            Rgba::rgba(0, 0, 0, 18)
        },
    );
    let dot_cx = (pill_x + 12) as f32;
    let dot_cy = (pill_y + pill_h / 2) as f32;
    if pill_live {
        cv.circle(dot_cx, dot_cy, 5.0, Rgba::rgba(pill_col.0, pill_col.1, pill_col.2, 64));
        cv.circle(dot_cx, dot_cy, 3.0, pill_col);
    } else {
        cv.ring(dot_cx, dot_cy, 3.2, 1.3, pill_col);
    }
    cv.text(
        fonts.semibold.as_ref(),
        &pill_label,
        pill_x + 21,
        pill_y + 17,
        11.2,
        if pill_live { th.text } else { th.text_med },
    );

    // 2. Workspace / Project nested Mica card.
    let placeholder: &[&str] = &PROJECT_PLACEHOLDER;
    let lines = if g.project_lines.is_empty() {
        placeholder
    } else {
        g.project_lines
    };
    let proj_y = y + 90;
    let proj_h = 94;
    sub_card(cv, dark, inner_x, proj_y, inner_w, proj_h, 15.0);
    cv.text_tracked(
        fonts.semibold.as_ref(),
        "WORKSPACE",
        inner_x + 14,
        proj_y + 20,
        10.0,
        0.75,
        th.text_dim,
    );
    // Clean sculpted project folder badge + bold title + monospace/regular metadata.
    let badge_s = 34;
    let badge_x = inner_x + 14;
    let badge_y = proj_y + 34;
    cv.draw_app_squircle(badge_x, badge_y, badge_s, "files", None, fonts);

    let text_x = badge_x + badge_s + 12;
    let text_w = inner_w - (text_x - inner_x) - 14;
    if let Some(first) = lines.first() {
        cv.text_truncated(
            fonts.bold.as_ref(),
            first,
            text_x,
            proj_y + 46,
            15.5,
            text_w,
            th.text,
        );
    }
    if let Some(second) = lines.get(1) {
        let is_path = second.starts_with('/') || second.starts_with('~');
        let f = if is_path {
            fonts.mono.as_ref()
        } else {
            fonts.regular.as_ref()
        };
        cv.text_truncated(f, second, text_x, proj_y + 65, 11.8, text_w, th.text_med);
    }
    if let Some(third) = lines.get(2) {
        cv.text_truncated(
            fonts.regular.as_ref(),
            third,
            text_x,
            proj_y + 81,
            11.4,
            text_w,
            th.text_dim,
        );
    }

    // 3. Two-column Bento telemetry tiles: Agents + Target.
    let grid_y = proj_y + proj_h + 10;
    let grid_h = 96;
    let col_gap = 10;
    let col_w = (inner_w - col_gap) / 2;

    // 3a. Left Bento Tile — Agents count + circular activity ring.
    let ax = inner_x;
    sub_card(cv, dark, ax, grid_y, col_w, grid_h, 15.0);
    cv.text_tracked(
        fonts.semibold.as_ref(),
        "AGENTS",
        ax + 14,
        grid_y + 20,
        10.0,
        0.75,
        th.text_dim,
    );
    let (count_str, count_sub, ring_col, active_ring) = match g.agents_running {
        None => ("—".to_string(), "No runtime", th.text_dim, false),
        Some(0) => ("0".to_string(), "Idle", th.text_dim, false),
        Some(1) => ("1".to_string(), "Running", th.agent_active, true),
        Some(n) => (n.to_string(), "Running", th.agent_active, true),
    };
    cv.text_tracked(
        fonts.bold.as_ref(),
        &count_str,
        ax + 14,
        grid_y + 58,
        28.0,
        -0.6,
        th.text,
    );
    cv.text(
        fonts.medium.as_ref(),
        count_sub,
        ax + 14,
        grid_y + 79,
        11.8,
        if active_ring { th.text_med } else { th.text_dim },
    );
    // Sleek open-arc telemetry activity gauge on the right side of the Agents tile.
    let gcx = (ax + col_w - 32) as f32;
    let gcy = (grid_y + 56) as f32;
    cv.ring(
        gcx,
        gcy,
        15.5,
        3.2,
        if dark {
            Rgba::rgba(255, 255, 255, 24)
        } else {
            Rgba::rgba(0, 0, 0, 18)
        },
    );
    if active_ring {
        let count = g.agents_running.unwrap_or(1).min(6) as f32;
        let sweep = (115.0 + count * 42.0).min(315.0);
        cv.arc(gcx, gcy, 15.5, 3.2, -90.0, -90.0 + sweep, ring_col);
        // Subtle luminous inner accent pip at 12 o'clock origin + clean pulse bar.
        let tip_rad = (-90.0 + sweep).to_radians();
        cv.circle(
            gcx + tip_rad.cos() * 15.5,
            gcy + tip_rad.sin() * 15.5,
            1.6,
            ring_col,
        );
        // Mini 3-bar live activity waveform inside the ring.
        for (i, bh) in [6, 10, 5].into_iter().enumerate() {
            let bx = gcx as i32 - 5 + i as i32 * 4;
            cv.round_rect(bx, gcy as i32 - bh / 2, 2, bh, 1.0, ring_col);
        }
    } else {
        cv.circle(gcx, gcy, 3.0, Rgba::rgba(ring_col.0, ring_col.1, ring_col.2, 120));
    }

    // 3b. Right Bento Tile — Target environment details.
    let tx = inner_x + col_w + col_gap;
    sub_card(cv, dark, tx, grid_y, col_w, grid_h, 15.0);
    cv.text_tracked(
        fonts.semibold.as_ref(),
        "TARGET",
        tx + 14,
        grid_y + 20,
        10.0,
        0.75,
        th.text_dim,
    );
    let cur_env = g.environment.min(2);
    let env_name = ENVIRONMENTS[cur_env as usize];
    // Tinted icon badge.
    cv.round_rect(
        tx + 14,
        grid_y + 32,
        28,
        28,
        8.0,
        Rgba::rgba(th.accent.0, th.accent.1, th.accent.2, if dark { 42 } else { 28 }),
    );
    draw_env_icon(cv, cur_env, tx as f32 + 28.0, grid_y as f32 + 46.0, 14.0, th.accent);
    cv.text_truncated(
        fonts.bold.as_ref(),
        env_name,
        tx + 50,
        grid_y + 52,
        15.0,
        col_w - 58,
        th.text,
    );
    cv.text_truncated(
        fonts.medium.as_ref(),
        env_subtitle(cur_env),
        tx + 14,
        grid_y + 79,
        11.6,
        col_w - 24,
        th.text_dim,
    );

    // 4. Full-width recessed segmented control for Environment selection.
    let seg_label_y = grid_y + grid_h + 24;
    cv.text_tracked(
        fonts.semibold.as_ref(),
        "ENVIRONMENT",
        inner_x + 4,
        seg_label_y,
        10.0,
        0.75,
        th.text_dim,
    );

    let track_y = seg_label_y + 10;
    let track_h = 44;
    cv.round_rect(
        inner_x,
        track_y,
        inner_w,
        track_h,
        14.0,
        if dark {
            Rgba::rgba(0, 0, 0, 74)
        } else {
            Rgba::rgba(0, 0, 0, 16)
        },
    );
    cv.stroke_round_rect(
        inner_x,
        track_y,
        inner_w,
        track_h,
        14.0,
        1.0,
        if dark {
            Rgba::rgba(255, 255, 255, 24)
        } else {
            Rgba::rgba(0, 0, 0, 14)
        },
    );

    let mut targets = Vec::with_capacity(ENVIRONMENTS.len());
    let seg_pad = 4;
    let seg_gap = 4;
    let seg_h = track_h - seg_pad * 2;
    let seg_w = (inner_w - seg_pad * 2 - seg_gap * 2) / 3;
    let seg_y = track_y + seg_pad;

    for (i, label) in ENVIRONMENTS.iter().enumerate() {
        let sx = inner_x + seg_pad + i as i32 * (seg_w + seg_gap);
        let sw = if i == 2 {
            (inner_x + inner_w - seg_pad) - sx
        } else {
            seg_w
        };
        let env = i as u8;
        let active = env == cur_env;
        let over = hover.is_some_and(|(hx, hy)| {
            hx >= sx as f64
                && hx < (sx + sw) as f64
                && hy >= seg_y as f64
                && hy < (seg_y + seg_h) as f64
        });
        if active {
            let a_top = lerp_rgba(th.accent, Rgba::rgb(255, 255, 255), 0.16);
            let a_bot = lerp_rgba(th.accent, Rgba::rgb(0, 0, 0), 0.10);
            cv.round_rect_vgrad(sx, seg_y, sw, seg_h, 10.5, a_top, a_bot);
            cv.round_rect_glass_rim(
                sx,
                seg_y,
                sw,
                seg_h,
                10.5,
                Rgba::rgba(255, 255, 255, 96),
                Rgba::rgba(255, 255, 255, 32),
                Rgba::rgba(0, 0, 0, 28),
            );
        } else if over {
            cv.round_rect(sx, seg_y, sw, seg_h, 10.5, th.tile_hover);
        }

        let fg = if active { th.on_accent } else { th.text_med };
        let font = if active {
            fonts.semibold.as_ref()
        } else {
            fonts.medium.as_ref()
        };
        let tw = text_width(font, label, 12.2);
        let ic_w = 13;
        let group_w = ic_w + 5 + tw;
        let gx = sx + (sw - group_w) / 2;
        let cy = seg_y as f32 + seg_h as f32 * 0.5;
        draw_env_icon(cv, env, gx as f32 + 6.5, cy, 11.5, fg);
        cv.text(font, label, gx + ic_w + 5, seg_y + 22, 12.2, fg);
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
