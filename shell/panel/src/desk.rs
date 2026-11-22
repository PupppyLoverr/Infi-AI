//! Agent-workspace desk: clock, project, running agents, environment chips.
//!
//! Shown when `desktop_mode=agent`. The panel keeps a real layer-shell surface
//! on the right of the desktop (`solyra-desk`). The same card is painted into
//! the agents overlay when that surface is open and empty.

use crate::canvas::{text_width, Canvas, Fonts, Rgba};
use crate::material::{self, Material};
use crate::tokens::Theme;

pub const W: i32 = 360;
pub const H: i32 = 420;
pub const RADIUS: f32 = 18.0;

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
    PROJECT_PLACEHOLDER.iter().map(|s| (*s).to_string()).collect()
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
        PROJECT_PLACEHOLDER.iter().map(|s| (*s).to_string()).collect()
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

fn agents_label(running: Option<usize>) -> String {
    match running {
        None => "No runtime".to_string(),
        Some(0) => "None running".to_string(),
        Some(1) => "1 running".to_string(),
        Some(n) => format!("{n} running"),
    }
}

/// Paint the desk card at `x`,`y`. Returns the card size and chip hit targets.
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
    let slab = material::slab(Material::Glass, th.surface, tint);
    material::paint(cv, &slab, x, y, w, h, RADIUS);

    let placeholder: &[&str] = &PROJECT_PLACEHOLDER;
    let lines = if g.project_lines.is_empty() {
        placeholder
    } else {
        g.project_lines
    };

    cv.text(fonts.regular.as_ref(), g.date, x + 22, y + 36, 13.0, th.text_dim);
    cv.text(fonts.semibold.as_ref(), g.clock, x + 22, y + 78, 36.0, th.text);

    cv.text(fonts.semibold.as_ref(), "Project", x + 22, y + 122, 13.0, th.text);
    let agents_x = x + 214;
    cv.text(fonts.semibold.as_ref(), "Agents", agents_x, y + 122, 13.0, th.text);
    let running = agents_label(g.agents_running);
    cv.text_truncated(
        fonts.semibold.as_ref(),
        &running,
        agents_x,
        y + 146,
        15.0,
        w - (agents_x - x) - 20,
        th.text,
    );

    let mut ly = y + 176;
    for line in lines.iter().take(3) {
        cv.text_truncated(
            fonts.regular.as_ref(),
            line,
            x + 22,
            ly,
            13.5,
            w - 44,
            th.text_med,
        );
        ly += 22;
    }

    cv.text(fonts.semibold.as_ref(), "Environment", x + 22, y + 292, 13.0, th.text);

    let mut targets = Vec::with_capacity(ENVIRONMENTS.len());
    let mut cx = x + 22;
    let chip_y = y + 310;
    let chip_h = 34;
    for (i, label) in ENVIRONMENTS.iter().enumerate() {
        let tw = text_width(fonts.semibold.as_ref(), label, 12.5) + 22;
        let env = i as u8;
        let active = env == g.environment;
        let over = hover.is_some_and(|(hx, hy)| {
            hx >= cx as f64 && hx < (cx + tw) as f64 && hy >= chip_y as f64 && hy < (chip_y + chip_h) as f64
        });
        let fill = if active { th.accent } else if over { th.tile_hover } else { th.tile };
        let fg = if active { th.on_accent } else { th.text };
        cv.round_rect(cx, chip_y, tw, chip_h, 17.0, fill);
        if !active {
            cv.stroke_round_rect(cx, chip_y, tw, chip_h, 17.0, 1.0, th.hairline);
        }
        cv.text(fonts.semibold.as_ref(), label, cx + 11, chip_y + 22, 12.5, fg);
        targets.push(Target { x: cx, y: chip_y, w: tw, h: chip_h, environment: env });
        cx += tw + 8;
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
        assert_eq!(hits.iter().map(|t| t.environment).collect::<Vec<_>>(), vec![0, 1, 2]);
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
