//! Window chrome for Solyra apps: a pearOS / macOS-style titlebar with
//! traffic lights, and the Snap menu on the green light.
//!
//! niri draws no titlebars (`prefer-no-csd`), so each app draws this one.
//! Metrics follow pearOS's window spec: a 30 px bar (8 px padding), 14 px
//! lights 9 px apart filled #ff5c60 / #fac800 / #35c759 with a 0.5 px dark
//! rim, the title in 13 px bold starting 92 px from the edge. The bar is a
//! window-drag region, so the window moves from anywhere on it.
//!
//! The lights do what they say:
//! * red closes the window;
//! * yellow minimises. niri has no minimised state, so the window moves to
//!   an empty workspace out of the way (the dock brings it back);
//! * green zooms (maximise or restore). Resting the pointer on it opens the
//!   **Snap menu**: widths, centre, float, fullscreen and moving between
//!   columns, each a real niri action.

use std::rc::Rc;

use zgx::compose::prelude::*;
use zgx::compose::{Placement, PlacementEdge, PlacementSide};
use zgx::input::{EventPhase, InputEvent};
use zgx::reactive::{Runtime, Signal};
use zgx::scene::Color;

use crate::{color, Style};

pub const BAR_H: f32 = 30.0;
pub const LIGHT: f32 = 14.0;
pub const LIGHT_GAP: f32 = 9.0;
pub const RED: Color = Color(0xff, 0x5c, 0x60, 255);
pub const YELLOW: Color = Color(0xfa, 0xc8, 0x00, 255);
pub const GREEN: Color = Color(0x35, 0xc7, 0x59, 255);

/// What the window's owner does for each light.
#[derive(Clone)]
pub struct WindowCtl {
    pub close: Rc<dyn Fn()>,
    pub zoom: Rc<dyn Fn()>,
}

/// Run a niri action (`niri msg action ...`) for the focused window, without
/// waiting. The window is focused, since its own button was just pressed.
pub fn niri_action(args: &[&str]) {
    let _ = std::process::Command::new("niri")
        .arg("msg")
        .arg("action")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// The Snap menu's choices: label, a sketch of the result (fractions of the
/// screen width covered, from the left), and the niri action.
pub const SNAPS: &[(&str, (f32, f32), &[&str])] = &[
    ("Full width", (0.0, 1.0), &["maximize-column"]),
    ("Half", (0.0, 0.5), &["set-column-width", "50%"]),
    ("One third", (0.0, 0.333), &["set-column-width", "33.333%"]),
    ("Two thirds", (0.0, 0.667), &["set-column-width", "66.667%"]),
    ("Centre", (0.25, 0.75), &["center-column"]),
    ("Float", (0.3, 0.7), &["toggle-window-floating"]),
    ("Fullscreen", (0.0, 1.0), &["fullscreen-window"]),
    ("Move left", (0.0, 0.5), &["move-column-left"]),
    ("Move right", (0.5, 1.0), &["move-column-right"]),
];

/// "Minimise": niri keeps no minimised windows, so move this one to the empty
/// workspace niri always keeps below the others, without following it.
pub const MINIMIZE: &[&str] = &["move-window-to-workspace", "--focus", "false", "255"];

fn light(fill: Color, glyph: &'static str, show: Signal<bool>) -> View {
    let s2 = show.clone();
    div()
        .w(LIGHT)
        .h(LIGHT)
        .rounded(LIGHT / 2.)
        .bg(fill)
        .border(0.5)
        .border_color(Color(0, 0, 0, 115))
        .items_center()
        .justify_center()
        .child(
            text(glyph.to_string())
                .text_size(10.)
                .font_bold()
                .reactive_style(move || {
                    Styles::new().text_color(if s2.get() {
                        Color(0, 0, 0, 150)
                    } else {
                        Color(0, 0, 0, 0)
                    })
                }),
        )
}

/// A small drawing of where the window ends up.
fn sketch(style: &Style, span: (f32, f32)) -> View {
    let (w, h) = (44.0f32, 28.0f32);
    let ink = if style.dark() { Color(255, 255, 255, 60) } else { Color(0, 0, 0, 40) };
    let fill = color(style.theme.accent);
    div()
        .w(w)
        .h(h)
        .rounded(5.)
        .border(1.)
        .border_color(ink)
        .child(
            div()
                .ml(2. + span.0 * (w - 4.))
                .mt(2.)
                .w(((span.1 - span.0) * (w - 4.)).max(4.))
                .h(h - 4.)
                .rounded(3.)
                .bg(fill),
        )
}

fn snap_menu(style: &Style, open: Signal<bool>) -> View {
    let grid = SNAPS.chunks(3).fold(column().gap(8.), |col, chunk| {
        col.child(chunk.iter().fold(row().gap(8.), |r, (label, span, action)| {
            let open = open.clone();
            let action: Vec<&'static str> = action.to_vec();
            r.child(
                button()
                    .w(82.)
                    .p(6.)
                    .gap(4.)
                    .rounded(8.)
                    .border(0.)
                    .flex_col()
                    .items_center()
                    .bg(Color(0, 0, 0, 0))
                    .hover(|s| s.bg(Color(127, 127, 127, 40)))
                    .accessible_label(label.to_string())
                    .child(sketch(style, *span))
                    .child(text(label.to_string()).text_size(11.))
                    .on_click(move || {
                        open.set(false);
                        niri_action(&action);
                    }),
            )
        }))
    });
    column()
        .gap(8.)
        .child(text("Snap").text_size(11.).font_bold().opacity(0.6))
        .child(grid)
}

/// The titlebar. `tools` sits after the title (Finder-style toolbars put
/// their controls there); `snap_open` is the Snap menu's state, owned by the
/// window so it survives re-renders.
pub fn titlebar(rt: &Runtime, style: &Style, title: &str, ctl: WindowCtl, tools: Option<View>) -> View {
    let show = rt.signal(false);
    let snap_open = rt.signal(false);
    let (s_in, s_out) = (show.clone(), show.clone());
    let lights = row()
        .gap(LIGHT_GAP)
        .items_center()
        .pr(8.)
        .on_event(move |cx| {
            if cx.phase != EventPhase::Target && cx.phase != EventPhase::Bubble {
                return;
            }
            match cx.event {
                InputEvent::PointerEnter => {
                    s_in.set(true);
                }
                InputEvent::PointerLeave => {
                    s_out.set(false);
                }
                _ => {}
            }
        })
        .child({
            let close = ctl.close.clone();
            button()
                .border(0.)
                .p(0.)
                .bg(Color(0, 0, 0, 0))
                .accessible_label("Close")
                .child(light(RED, "×", show.clone()))
                .on_click(move || close())
        })
        .child(
            button()
                .border(0.)
                .p(0.)
                .bg(Color(0, 0, 0, 0))
                .accessible_label("Minimise")
                .child(light(YELLOW, "−", show.clone()))
                .on_click(|| niri_action(MINIMIZE)),
        )
        .child({
            let zoom = ctl.zoom.clone();
            let (open_hover, open_anchor) = (snap_open.clone(), snap_open.clone());
            let anchor = button()
                .border(0.)
                .p(0.)
                .bg(Color(0, 0, 0, 0))
                .accessible_label("Zoom (rest the pointer here for Snap)")
                .child(light(GREEN, "+", show.clone()))
                .on_event(move |cx| {
                    if matches!(cx.event, InputEvent::PointerEnter) {
                        open_hover.set(true);
                    }
                })
                .on_click(move || {
                    open_anchor.set(false);
                    zoom();
                });
            popover("Snap", snap_open.clone(), anchor)
                .placement(Placement {
                    side: PlacementSide::Bottom,
                    align: PlacementEdge::Start,
                    offset: 6.,
                })
                .p(12.)
                .rounded(14.)
                .child(snap_menu(style, snap_open.clone()))
        });

    let ink = color(style.theme.text);
    let bar_bg = if style.dark() {
        Color(31, 31, 31, if style.glass { 140 } else { 255 })
    } else {
        Color(246, 246, 248, if style.glass { 190 } else { 255 })
    };
    let rim = if style.dark() { Color(255, 255, 255, 26) } else { Color(0, 0, 0, 22) };
    let mut bar = row()
        .h(BAR_H)
        .w_full()
        .px(8.)
        .gap(16.)
        .items_center()
        .bg(bar_bg)
        .border_edges(zgx::scene::Insets { top: 0., right: 0., bottom: 0.67, left: 0. })
        .border_color(rim)
        .window_drag()
        .child(lights)
        .child(
            text(title.to_string())
                .text_size(13.)
                .font_bold()
                .text_color(Color(ink.0, ink.1, ink.2, 217)),
        );
    if let Some(t) = tools {
        bar = bar.child(div().grow()).child(t);
    }
    bar
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pearos_metrics() {
        // 8 padding + 3 lights + 2 gaps + 8 + 16 gap = 92 px to the title.
        let to_title = 8. + 3. * LIGHT + 2. * LIGHT_GAP + 8. + 16.;
        assert_eq!(to_title, 92.);
        assert_eq!(BAR_H, 30.);
    }

    #[test]
    fn every_snap_is_a_niri_action_with_a_sane_sketch() {
        for (label, (a, b), action) in SNAPS {
            assert!(!action.is_empty(), "{label}");
            assert!(*a >= 0. && b > a && *b <= 1., "{label}");
            assert!(action[0].chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{label}");
        }
        assert_eq!(MINIMIZE[0], "move-window-to-workspace");
    }
}
