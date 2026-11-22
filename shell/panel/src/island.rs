//! The notch and Dynamic Island: one black shape at the top centre that
//! morphs between an idle size, a compact live activity and an expanded card.
//!
//! Two looks, chosen in Settings (`notch_style`):
//! * **Notch** – hangs from the top edge with concave "ears", like a laptop
//!   display notch; expands downward.
//! * **Island** – a floating pill inside the bar; expands into a rounded card.
//!
//! It lives on its own layer surface, larger than the shape; only the shape
//! takes input, and the rest is transparent, so the shape can grow and shrink
//! smoothly without a surface resize per frame.

use crate::agent::{AskKind, Scope, Verdict};
use crate::canvas::{text_width, wrap_text, Canvas, Fonts, Rgba};
use crate::media::{Control, Media};
use crate::state::{LaunchTarget, ShellCmd};
use crate::surfaces::{HitAction, Region};
use crate::tokens::{Theme, T_BODY, T_CAPTION, T_LABEL, T_META};
use solyra_conf::look::NotchStyle;

/// The island's surface; the shape never exceeds it.
pub const SURF_W: u32 = 480;
pub const SURF_H: u32 = 214;
/// Concave fillet radius joining a notch to the top edge.
const EAR: f32 = 9.0;

/// What the island is showing.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Idle,
    Message(String),
    Activity { who: String, text: String },
    Attention(Ask),
    Media(Media),
}

/// An approval or question waiting on the human.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub task: usize,
    pub who: String,
    pub kind: AskKind,
    pub text: String,
    pub resource: Option<String>,
    pub reversible: Option<bool>,
    pub task_scope: bool,
    pub choices: Vec<String>,
    /// A decision was already sent; waiting for the agent.
    pub sent: bool,
}

/// Facts the idle card shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Glance {
    pub battery: Option<u8>,
    pub network: String,
    pub agents_running: usize,
    pub windows: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub w: f32,
    pub h: f32,
    pub y: f32,
    pub r: f32,
}

impl Shape {
    pub fn x(&self) -> f32 {
        (SURF_W as f32 - self.w) / 2.0
    }
}

/// Where the shape wants to be.
pub fn target(style: NotchStyle, content: &Content, expanded: bool) -> Shape {
    let busy = !matches!(content, Content::Idle);
    match style {
        NotchStyle::Island => {
            if expanded {
                Shape {
                    w: 440.0,
                    h: 186.0,
                    y: 5.0,
                    r: 34.0,
                }
            } else if busy {
                Shape {
                    w: 360.0,
                    h: 26.0,
                    y: 4.0,
                    r: 13.0,
                }
            } else {
                Shape {
                    w: 126.0,
                    h: 26.0,
                    y: 4.0,
                    r: 13.0,
                }
            }
        }
        // Off has no surface; the notch numbers are a harmless default.
        NotchStyle::Notch | NotchStyle::Off => {
            if expanded {
                Shape {
                    w: 452.0,
                    h: 200.0,
                    y: 0.0,
                    r: 28.0,
                }
            } else if busy {
                Shape {
                    w: 380.0,
                    h: 34.0,
                    y: 0.0,
                    r: 14.0,
                }
            } else {
                Shape {
                    w: 188.0,
                    h: 32.0,
                    y: 0.0,
                    r: 13.0,
                }
            }
        }
    }
}

/// Smoothly approaches its target, frame by frame.
#[derive(Clone, Copy, Debug)]
pub struct Anim {
    pub cur: Shape,
    pub to: Shape,
}

impl Anim {
    pub fn new(at: Shape) -> Self {
        Self { cur: at, to: at }
    }

    /// Advance by `dt` seconds. Returns true while still moving.
    pub fn step(&mut self, dt: f32, reduced_motion: bool) -> bool {
        if reduced_motion {
            self.cur = self.to;
            return false;
        }
        // Critically-damped feel: fast at first, settling without overshoot.
        let k = 1.0 - (-dt * 16.0).exp();
        let lerp = |a: f32, b: f32| a + (b - a) * k;
        self.cur = Shape {
            w: lerp(self.cur.w, self.to.w),
            h: lerp(self.cur.h, self.to.h),
            y: lerp(self.cur.y, self.to.y),
            r: lerp(self.cur.r, self.to.r),
        };
        let close = |a: f32, b: f32| (a - b).abs() < 0.6;
        if close(self.cur.w, self.to.w)
            && close(self.cur.h, self.to.h)
            && close(self.cur.y, self.to.y)
        {
            self.cur = self.to;
            false
        } else {
            true
        }
    }

    pub fn moving(&self) -> bool {
        self.cur != self.to
    }
}

/// The shape's rectangle in surface pixels, for the input region.
pub fn input_rect(s: &Shape) -> (i32, i32, i32, i32) {
    (
        s.x().floor() as i32,
        s.y.floor() as i32,
        s.w.ceil() as i32,
        s.h.ceil() as i32,
    )
}

fn hovered(hover: Option<(f64, f64)>, x: i32, y: i32, w: i32, h: i32) -> bool {
    hover.is_some_and(|(hx, hy)| {
        hx >= x as f64 && hx < (x + w) as f64 && hy >= y as f64 && hy < (y + h) as f64
    })
}

/// Everything the expanded card may need beyond the content itself.
pub struct Extra<'a> {
    pub glance: &'a Glance,
    pub clock: &'a str,
    pub date: &'a str,
    pub reply: &'a str,
    pub typing: bool,
    /// What the shape is made of. Solid is the classic pure black.
    pub material: crate::material::Material,
}

#[allow(clippy::too_many_arguments)]
/// The shape's area as rectangles, for the compositor's blur region.
pub fn blur_rects(style: NotchStyle, s: &Shape) -> Vec<(i32, i32, i32, i32)> {
    let (x, y, w, h) = (
        s.x().round() as i32,
        s.y.round() as i32,
        s.w.round() as i32,
        s.h.round() as i32,
    );
    match style {
        NotchStyle::Island => crate::material::blur_rects(x, y, w, h, s.r),
        _ => {
            // Square top (it hangs from the screen edge), rounded bottom:
            // the lower half of a rounded rect twice as tall.
            let r = s.r.min(w as f32 / 2.0).min(h as f32).max(0.0);
            let full = crate::material::blur_rects(x, y - h, w, 2 * h, r);
            full.into_iter()
                .filter_map(|(rx, ry, rw, rh)| {
                    let top = ry.max(y);
                    let bot = (ry + rh).min(y + h);
                    (bot > top).then_some((rx, top, rw, bot - top))
                })
                .collect()
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    cv: &mut Canvas<'_>,
    fonts: &Fonts,
    style: NotchStyle,
    anim: &Anim,
    content: &Content,
    expanded: bool,
    extra: &Extra<'_>,
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    // The shape is always black; what is on it uses the dark palette.
    let th = Theme::dark();
    let s = anim.cur;
    let (x, y, w, h) = (s.x(), s.y, s.w, s.h);
    let (xi, yi, wi, hi) = (
        x.round() as i32,
        y.round() as i32,
        w.round() as i32,
        h.round() as i32,
    );

    let slab = crate::material::slab(extra.material, crate::material::notch_base(extra.material), None);
    let body = slab.fill;
    match style {
        NotchStyle::Island => {
            cv.round_rect(xi, yi, wi, hi, s.r, body);
            if extra.material != crate::material::Material::Solid {
                cv.stroke_round_rect(xi, yi, wi, hi, s.r, 1.0, slab.edge);
            }
        }
        _ => {
            cv.notch_body(xi, 0, wi, hi, s.r, body);
            cv.concave_corner(x - EAR, 0.0, EAR, true, body);
            cv.concave_corner(x + w, 0.0, EAR, false, body);
        }
    }
    if let Some(hl) = slab.highlight {
        if style == NotchStyle::Island {
            let inset = (s.r * 0.6).round() as i32;
            cv.fill_rect(xi + inset, yi + 1, (wi - 2 * inset).max(0), 1, hl);
        }
    }

    let mut regions = Vec::new();
    // The whole shape toggles expansion; buttons drawn later take priority
    // because hit-testing walks regions last-first (see `pick`).
    regions.push(Region {
        x: xi,
        y: yi,
        w: wi,
        h: hi,
        action: HitAction::Shell(ShellCmd::ToggleIsland),
    });

    let settled = !anim.moving();
    let to = anim.to;
    // Draw content only once the shape is (nearly) its final size, so text
    // never spills outside it mid-animation.
    let fits = (s.w - to.w).abs() < 10.0 && (s.h - to.h).abs() < 10.0;
    if !(settled || fits) {
        return regions;
    }

    // The compact row: the top strip of the shape (the bar's height).
    let row_h = if style == NotchStyle::Island { 26 } else { 32 };
    let row_y = yi;
    compact_row(cv, &th, fonts, xi, row_y, wi, row_h, content, style);

    if expanded {
        let body = (xi + 22, row_y + row_h + 10, wi - 44, hi - row_h - 22);
        expanded_body(cv, &th, fonts, body, content, extra, hover, &mut regions);
    }
    regions
}

#[allow(clippy::too_many_arguments)]
fn compact_row(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    content: &Content,
    style: NotchStyle,
) {
    let mid = y as f32 + h as f32 / 2.0;
    let left = x + if style == NotchStyle::Island { 14 } else { 22 };
    let right = x + w - if style == NotchStyle::Island { 14 } else { 22 };
    let baseline = y + h / 2 + 4;
    let text_max = (right - left - 48).max(0);
    match content {
        Content::Idle => {}
        Content::Message(t) => {
            cv.icon_bell(left as f32 + 6.0, mid - 2.0, th.text);
            cv.text_truncated(
                fonts.semibold.as_ref(),
                t,
                left + 20,
                baseline,
                12.0,
                text_max,
                th.text,
            );
        }
        Content::Activity { who, text } => {
            cv.circle(left as f32 + 5.0, mid, 4.0, th.agent_active);
            let line = format!("{who} · {text}");
            cv.text_truncated(
                fonts.semibold.as_ref(),
                &line,
                left + 16,
                baseline,
                12.0,
                text_max,
                th.text,
            );
            // Right: three short bars: "working".
            for (i, bh) in [6, 10, 7].into_iter().enumerate() {
                cv.round_rect(
                    right - 14 + i as i32 * 5,
                    (mid as i32) - bh / 2,
                    3,
                    bh,
                    1.5,
                    th.agent_active,
                );
            }
        }
        Content::Attention(a) => {
            cv.ring(left as f32 + 5.0, mid, 4.5, 1.8, th.agent_approval);
            cv.circle(left as f32 + 5.0, mid, 1.6, th.agent_approval);
            let what = match a.kind {
                AskKind::Permission => "Approval needed",
                AskKind::Question => "Question",
            };
            let line = format!("{what} · {}", a.who);
            cv.text_truncated(
                fonts.semibold.as_ref(),
                &line,
                left + 16,
                baseline,
                12.0,
                text_max,
                th.text,
            );
            let chip = if a.sent { "Sent" } else { "Review" };
            let cw = text_width(fonts.semibold.as_ref(), chip, 10.5) + 14;
            cv.round_rect(right - cw, y + (h - 18) / 2, cw, 18, 9.0, th.agent_approval);
            cv.text(
                fonts.semibold.as_ref(),
                chip,
                right - cw + 7,
                y + (h - 18) / 2 + 13,
                10.5,
                th.on_accent,
            );
        }
        Content::Media(m) => {
            cv.icon_note(left as f32 + 4.0, mid, th.text);
            let line = if m.artist.is_empty() {
                m.title.clone()
            } else {
                format!("{} — {}", m.title, m.artist)
            };
            cv.text_truncated(
                fonts.semibold.as_ref(),
                &line,
                left + 18,
                baseline,
                12.0,
                text_max,
                th.text,
            );
            if m.playing {
                for (i, bh) in [8, 12, 6, 10].into_iter().enumerate() {
                    cv.round_rect(
                        right - 18 + i as i32 * 5,
                        (mid as i32) - bh / 2,
                        3,
                        bh,
                        1.5,
                        th.success,
                    );
                }
            } else {
                cv.icon_pause(right as f32 - 8.0, mid, 10.0, th.text_dim);
            }
        }
    }
}

fn button(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    rect: (i32, i32, i32, i32),
    label: &str,
    primary: bool,
    hover: Option<(f64, f64)>,
) {
    let (x, y, w, h) = rect;
    let fill = if primary {
        th.accent
    } else if hovered(hover, x, y, w, h) {
        Rgba::rgba(255, 255, 255, 40)
    } else {
        Rgba::rgba(255, 255, 255, 24)
    };
    cv.round_rect(x, y, w, h, h as f32 / 2.0, fill);
    let tw = text_width(fonts.semibold.as_ref(), label, T_LABEL);
    let fg = if primary { th.on_accent } else { th.text };
    cv.text(
        fonts.semibold.as_ref(),
        label,
        x + (w - tw).max(0) / 2,
        y + h / 2 + 4,
        T_LABEL,
        fg,
    );
}

/// Evenly split `n` buttons across `w` at the bottom of the body.
fn button_row(x: i32, y: i32, w: i32, n: i32) -> Vec<(i32, i32, i32, i32)> {
    let gap = 8;
    let bw = (w - gap * (n - 1)) / n.max(1);
    (0..n).map(|i| (x + i * (bw + gap), y, bw, 32)).collect()
}

#[allow(clippy::too_many_arguments)]
fn expanded_body(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    body: (i32, i32, i32, i32),
    content: &Content,
    extra: &Extra<'_>,
    hover: Option<(f64, f64)>,
    regions: &mut Vec<Region>,
) {
    let (bx, by, bw, bh) = body;
    let buttons_y = by + bh - 32;
    match content {
        Content::Idle => {
            cv.text(
                fonts.semibold.as_ref(),
                extra.clock,
                bx,
                by + 30,
                30.0,
                th.text,
            );
            cv.text(
                fonts.regular.as_ref(),
                extra.date,
                bx,
                by + 52,
                T_LABEL,
                th.text_dim,
            );
            let g = extra.glance;
            let mut facts = vec![extra.glance.network.clone()];
            if let Some(b) = g.battery {
                facts.push(format!("Battery {b}%"));
            }
            facts.push(format!("{} windows", g.windows));
            if g.agents_running > 0 {
                facts.push(format!("{} agent(s) working", g.agents_running));
            }
            cv.text_truncated(
                fonts.regular.as_ref(),
                &facts.join("  ·  "),
                bx,
                by + 76,
                T_META,
                bw,
                th.text_med,
            );
            let row = button_row(bx, buttons_y, bw, 3);
            let acts = [
                ("Agent tasks", HitAction::Shell(ShellCmd::ToggleAgents)),
                ("Control Centre", HitAction::Shell(ShellCmd::ToggleControl)),
                (
                    "Overview",
                    HitAction::Launch(LaunchTarget::Ipc("{\"Action\":{\"ToggleOverview\":{}}}")),
                ),
            ];
            for (r, (label, act)) in row.into_iter().zip(acts) {
                button(cv, th, fonts, r, label, false, hover);
                regions.push(Region {
                    x: r.0,
                    y: r.1,
                    w: r.2,
                    h: r.3,
                    action: act,
                });
            }
        }
        Content::Message(t) => {
            let mut y = by + 16;
            for l in wrap_text(fonts.regular.as_ref(), t, T_BODY, bw, 4) {
                cv.text(fonts.regular.as_ref(), &l, bx, y, T_BODY, th.text);
                y += 19;
            }
            let r = (bx + bw - 110, buttons_y, 110, 32);
            button(cv, th, fonts, r, "Dismiss", false, hover);
            regions.push(Region {
                x: r.0,
                y: r.1,
                w: r.2,
                h: r.3,
                action: HitAction::Shell(ShellCmd::Notify(String::new())),
            });
        }
        Content::Activity { who, text } => {
            cv.text(
                fonts.semibold.as_ref(),
                &who.to_uppercase(),
                bx,
                by + 12,
                T_CAPTION,
                th.agent_active,
            );
            let mut y = by + 34;
            for l in wrap_text(fonts.regular.as_ref(), text, T_BODY, bw, 3) {
                cv.text(fonts.regular.as_ref(), &l, bx, y, T_BODY, th.text);
                y += 19;
            }
            let r = (bx + bw - 140, buttons_y, 140, 32);
            button(cv, th, fonts, r, "Open agent tasks", false, hover);
            regions.push(Region {
                x: r.0,
                y: r.1,
                w: r.2,
                h: r.3,
                action: HitAction::Shell(ShellCmd::ToggleAgents),
            });
        }
        Content::Attention(a) => attention_body(cv, th, fonts, body, a, extra, hover, regions),
        Content::Media(m) => {
            cv.round_rect(bx, by + 2, 56, 56, 12.0, Rgba::rgba(255, 255, 255, 24));
            cv.icon_note(bx as f32 + 26.0, by as f32 + 30.0, th.text);
            let tx = bx + 70;
            cv.text_truncated(
                fonts.semibold.as_ref(),
                &m.title,
                tx,
                by + 22,
                T_BODY + 1.0,
                bw - 70,
                th.text,
            );
            cv.text_truncated(
                fonts.regular.as_ref(),
                &m.artist,
                tx,
                by + 42,
                T_LABEL,
                bw - 70,
                th.text_dim,
            );
            if let Some((elapsed, total, pct)) = m.elapsed_label() {
                let bar_y = by + 58;
                let bar_w = bw;
                cv.round_rect(bx, bar_y, bar_w, 4, 2.0, Rgba::rgba(255, 255, 255, 28));
                let fill = ((bar_w as f32) * pct).round() as i32;
                if fill > 0 {
                    cv.round_rect(bx, bar_y, fill.max(4), 4, 2.0, th.accent);
                }
                cv.text(
                    fonts.regular.as_ref(),
                    &elapsed,
                    bx,
                    bar_y + 18,
                    T_CAPTION,
                    th.text_dim,
                );
                let tw = text_width(fonts.regular.as_ref(), &total, T_CAPTION);
                cv.text(
                    fonts.regular.as_ref(),
                    &total,
                    bx + bar_w - tw,
                    bar_y + 18,
                    T_CAPTION,
                    th.text_dim,
                );
            }
            let cy = buttons_y + 16;
            let centre = bx + bw / 2;
            let ctrls = [
                (centre - 64, Control::Previous),
                (centre, Control::PlayPause),
                (centre + 64, Control::Next),
            ];
            for (cx, c) in ctrls {
                let r = (cx - 22, buttons_y, 44, 32);
                if hovered(hover, r.0, r.1, r.2, r.3) {
                    cv.round_rect(r.0, r.1, r.2, r.3, 16.0, Rgba::rgba(255, 255, 255, 30));
                }
                match c {
                    Control::Previous => cv.icon_skip(cx as f32, cy as f32, 16.0, false, th.text),
                    Control::Next => cv.icon_skip(cx as f32, cy as f32, 16.0, true, th.text),
                    Control::PlayPause if m.playing => {
                        cv.icon_pause(cx as f32, cy as f32, 16.0, th.text)
                    }
                    Control::PlayPause => cv.icon_play(cx as f32, cy as f32, 16.0, th.text),
                }
                regions.push(Region {
                    x: r.0,
                    y: r.1,
                    w: r.2,
                    h: r.3,
                    action: HitAction::Media(c),
                });
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn attention_body(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    body: (i32, i32, i32, i32),
    a: &Ask,
    extra: &Extra<'_>,
    hover: Option<(f64, f64)>,
    regions: &mut Vec<Region>,
) {
    let (bx, by, bw, bh) = body;
    let buttons_y = by + bh - 32;
    let head = match a.kind {
        AskKind::Permission => format!("{} WANTS TO", a.who.to_uppercase()),
        AskKind::Question => format!("{} ASKS", a.who.to_uppercase()),
    };
    cv.text(
        fonts.semibold.as_ref(),
        &head,
        bx,
        by + 12,
        T_CAPTION,
        th.agent_approval,
    );
    let mut y = by + 32;
    for l in wrap_text(fonts.semibold.as_ref(), &a.text, T_BODY, bw, 2) {
        cv.text(fonts.semibold.as_ref(), &l, bx, y, T_BODY, th.text);
        y += 18;
    }
    if a.kind == AskKind::Permission {
        let res = a.resource.as_deref().unwrap_or("Not reported by the agent");
        cv.text_truncated(
            fonts.regular.as_ref(),
            &format!("Affects: {res}"),
            bx,
            y + 2,
            T_META,
            bw,
            th.text_med,
        );
        let rev = match a.reversible {
            Some(true) => "Can be undone: yes, as reported",
            Some(false) => "Can be undone: no",
            None => "Can be undone: not reported",
        };
        cv.text_truncated(
            fonts.regular.as_ref(),
            rev,
            bx,
            y + 18,
            T_META,
            bw,
            th.text_dim,
        );
    }
    if a.sent {
        cv.text(
            fonts.regular.as_ref(),
            "Sent. Waiting for the agent.",
            bx,
            buttons_y + 20,
            T_LABEL,
            th.text_med,
        );
        return;
    }
    match a.kind {
        AskKind::Permission => {
            let n = if a.task_scope { 3 } else { 2 };
            let row = button_row(bx, buttons_y, bw, n);
            let mut acts = vec![
                (
                    "Deny (D)",
                    HitAction::Decide {
                        task: a.task,
                        verdict: Verdict::Deny,
                        scope: Scope::Once,
                    },
                    false,
                ),
                (
                    "Allow once (A)",
                    HitAction::Decide {
                        task: a.task,
                        verdict: Verdict::Approve,
                        scope: Scope::Once,
                    },
                    false,
                ),
            ];
            if a.task_scope {
                acts.push((
                    "This task (T)",
                    HitAction::Decide {
                        task: a.task,
                        verdict: Verdict::Approve,
                        scope: Scope::Task,
                    },
                    false,
                ));
            }
            for (r, (label, act, primary)) in row.into_iter().zip(acts) {
                button(cv, th, fonts, r, label, primary, hover);
                regions.push(Region {
                    x: r.0,
                    y: r.1,
                    w: r.2,
                    h: r.3,
                    action: act,
                });
            }
        }
        AskKind::Question if !a.choices.is_empty() => {
            let n = a.choices.len().min(4) as i32;
            let row = button_row(bx, buttons_y, bw, n);
            for (i, (r, c)) in row.into_iter().zip(&a.choices).enumerate() {
                button(cv, th, fonts, r, &format!("{} {c}", i + 1), false, hover);
                regions.push(Region {
                    x: r.0,
                    y: r.1,
                    w: r.2,
                    h: r.3,
                    action: HitAction::Choose {
                        task: a.task,
                        choice: i,
                    },
                });
            }
        }
        AskKind::Question => {
            let (fx, fw) = (bx, bw);
            cv.round_rect(fx, buttons_y, fw, 32, 16.0, Rgba::rgba(255, 255, 255, 22));
            if extra.typing {
                cv.stroke_round_rect(fx, buttons_y, fw, 32, 16.0, 1.0, th.agent_approval);
            }
            if extra.reply.is_empty() {
                cv.text(
                    fonts.regular.as_ref(),
                    "Type your answer, Enter to send",
                    fx + 14,
                    buttons_y + 21,
                    T_LABEL,
                    th.text_dim,
                );
            } else {
                let mut shown = extra.reply.to_string();
                while text_width(fonts.regular.as_ref(), &shown, T_LABEL) > fw - 30
                    && shown.chars().count() > 1
                {
                    shown.remove(0);
                }
                let tw = cv.text(
                    fonts.regular.as_ref(),
                    &shown,
                    fx + 14,
                    buttons_y + 21,
                    T_LABEL,
                    th.text,
                );
                if extra.typing {
                    cv.fill_rect(fx + 15 + tw, buttons_y + 8, 2, 16, th.agent_approval);
                }
            }
            regions.push(Region {
                x: fx,
                y: buttons_y,
                w: fw,
                h: 32,
                action: HitAction::StartReply,
            });
        }
    }
}

/// Region under (`x`, `y`), the most specific (last drawn) first.
pub fn pick(regions: &[Region], x: f64, y: f64) -> Option<HitAction> {
    regions
        .iter()
        .rev()
        .find(|r| r.contains(x, y))
        .map(|r| r.action.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_grow_with_content_and_expansion_and_fit_the_surface() {
        for style in [NotchStyle::Notch, NotchStyle::Island] {
            let idle = target(style, &Content::Idle, false);
            let busy = target(style, &Content::Message("x".into()), false);
            let big = target(style, &Content::Idle, true);
            assert!(busy.w > idle.w);
            assert!(big.h > busy.h);
            for s in [idle, busy, big] {
                assert!(
                    s.w <= SURF_W as f32 - 2.0 * EAR && s.y + s.h <= SURF_H as f32,
                    "{style:?} {s:?}"
                );
            }
        }
        assert_eq!(
            target(NotchStyle::Notch, &Content::Idle, false).y,
            0.0,
            "a notch hangs from the edge"
        );
        assert!(
            target(NotchStyle::Island, &Content::Idle, false).y > 0.0,
            "an island floats"
        );
    }

    #[test]
    fn animation_settles_and_reduced_motion_snaps() {
        let a = target(NotchStyle::Notch, &Content::Idle, false);
        let b = target(NotchStyle::Notch, &Content::Idle, true);
        let mut an = Anim::new(a);
        an.to = b;
        let mut frames = 0;
        while an.step(1.0 / 60.0, false) {
            frames += 1;
            assert!(frames < 120, "never settles");
        }
        assert_eq!(an.cur, b);
        assert!(frames > 5, "it actually animates ({frames} frames)");
        let mut snap = Anim::new(a);
        snap.to = b;
        assert!(!snap.step(1.0 / 60.0, true));
        assert_eq!(snap.cur, b);
    }

    #[test]
    fn later_regions_win_hit_tests() {
        let regions = vec![
            Region {
                x: 0,
                y: 0,
                w: 100,
                h: 100,
                action: HitAction::Shell(ShellCmd::ToggleIsland),
            },
            Region {
                x: 10,
                y: 10,
                w: 10,
                h: 10,
                action: HitAction::Shell(ShellCmd::ToggleAgents),
            },
        ];
        assert!(matches!(
            pick(&regions, 15.0, 15.0),
            Some(HitAction::Shell(ShellCmd::ToggleAgents))
        ));
        assert!(matches!(
            pick(&regions, 50.0, 50.0),
            Some(HitAction::Shell(ShellCmd::ToggleIsland))
        ));
        assert!(pick(&regions, 500.0, 5.0).is_none());
    }
}
