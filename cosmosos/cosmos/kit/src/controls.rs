//! Leaf controls: buttons, toggle, segmented control, text/search
//! fields, slider. Each draws hover / pressed / focus / disabled states
//! from [`Kit`] and returns the egui `Response`.

use crate::{focus_ring, Kit, State};
use cosmos_theme::{radius, space, text, ROW_H};
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Response, Sense, Stroke, StrokeKind, Ui};

fn sense(ui: &Ui, s: Sense) -> Sense {
    if ui.is_enabled() {
        s
    } else {
        Sense::hover()
    }
}

fn dim(ui: &Ui, c: Color32) -> Color32 {
    if ui.is_enabled() {
        c
    } else {
        c.gamma_multiply(0.4)
    }
}

pub fn body_font() -> FontId {
    FontId::proportional(text::BODY.size)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    /// Accent-filled default action.
    Primary,
    /// Neutral filled.
    Secondary,
    /// Text only until hovered.
    Plain,
    /// Danger-filled (delete, revoke).
    Destructive,
}

/// A 28px push button sized to its label.
pub fn button(ui: &mut Ui, kind: ButtonKind, label: &str) -> Response {
    let kit = Kit::get(ui.ctx());
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), body_font(), Color32::WHITE);
    let size = vec2(galley.size().x + 2.0 * space::S12, ROW_H);
    let (rect, resp) = ui.allocate_exact_size(size, sense(ui, Sense::click()));
    let st = State::of(&resp);
    let danger = crate::rgb(cosmos_theme::semantic::DANGER);
    let (fill, fg) = match kind {
        ButtonKind::Primary => (
            match st {
                State::Press => kit.accent_pressed(),
                State::Hover => crate::rgb(kit.accent.hover),
                State::Rest => kit.accent(),
            },
            crate::rgb(kit.accent.on_accent),
        ),
        ButtonKind::Destructive => (
            match st {
                State::Rest => danger,
                _ => danger.gamma_multiply(0.85),
            },
            Color32::WHITE,
        ),
        ButtonKind::Secondary => (kit.fill(st), kit.text()),
        ButtonKind::Plain => (
            if st == State::Rest {
                Color32::TRANSPARENT
            } else {
                kit.fill(st)
            },
            kit.accent(),
        ),
    };
    // Disabled buttons drop their kind colour for a neutral bordered
    // chip, so they stay legible on dark glass instead of fading out.
    let enabled = ui.is_enabled();
    let (fill, fg) = if enabled {
        (fill, fg)
    } else {
        (kit.fill(State::Rest), kit.text2().gamma_multiply(0.7))
    };
    let p = ui.painter();
    p.rect_filled(rect, radius::CONTROL, fill);
    if kind == ButtonKind::Secondary || !enabled {
        p.rect_stroke(
            rect,
            radius::CONTROL,
            Stroke::new(1.0, kit.hairline()),
            StrokeKind::Inside,
        );
    }
    p.text(rect.center(), Align2::CENTER_CENTER, label, body_font(), fg);
    if resp.has_focus() {
        focus_ring(ui, &kit, rect, radius::CONTROL);
    }
    resp
}

/// macOS-style switch: 38×22 track, 18px knob, accent when on.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let kit = Kit::get(ui.ctx());
    let (rect, mut resp) = ui.allocate_exact_size(vec2(38.0, 22.0), sense(ui, Sense::click()));
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    // Off track: a solid neutral, stronger than control fills, so the
    // switch reads as a switch (not a lone knob) on dark glass.
    let off = crate::with_alpha(kit.p.text, if kit.dark { 72 } else { 40 });
    let track = lerp_color(off, kit.accent(), t);
    let p = ui.painter();
    p.rect_filled(rect, rect.height() / 2.0, dim(ui, track));
    // The off track is a faint fill; a hairline keeps its shape on glass.
    p.rect_stroke(
        rect,
        rect.height() / 2.0,
        Stroke::new(1.0, kit.hairline().gamma_multiply(1.0 - t)),
        StrokeKind::Inside,
    );
    let x = egui::lerp(rect.left() + 11.0..=rect.right() - 11.0, t);
    let c = pos2(x, rect.center().y);
    p.circle_filled(c + vec2(0.0, 0.5), 9.5, Color32::from_black_alpha(40));
    p.circle_filled(c, 9.0, dim(ui, Color32::WHITE));
    if resp.has_focus() {
        focus_ring(ui, &kit, rect, rect.height() / 2.0);
    }
    resp
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

/// Segmented control: a neutral track with the selected segment raised.
pub fn segmented(ui: &mut Ui, selected: &mut usize, labels: &[&str]) -> Response {
    segmented_with(ui, selected, labels.len(), 0.0, |ui, i, rect, color| {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            labels[i],
            body_font(),
            color,
        );
    })
}

/// Segmented control with caller-drawn segments (icons); `seg_w` 0 sizes
/// segments to the widest label.
pub fn segmented_with(
    ui: &mut Ui,
    selected: &mut usize,
    n: usize,
    seg_w: f32,
    draw: impl Fn(&Ui, usize, Rect, Color32),
) -> Response {
    let kit = Kit::get(ui.ctx());
    let w = if seg_w > 0.0 { seg_w } else { 72.0 };
    let (rect, mut resp) =
        ui.allocate_exact_size(vec2(w * n as f32 + 4.0, ROW_H), sense(ui, Sense::click()));
    let p = ui.painter();
    p.rect_filled(rect, radius::CONTROL, kit.fill(State::Rest));
    for i in 0..n {
        let seg = Rect::from_min_size(
            rect.min + vec2(2.0 + w * i as f32, 2.0),
            vec2(w, ROW_H - 4.0),
        );
        let hovered = resp.hover_pos().is_some_and(|h| seg.contains(h));
        if i == *selected {
            // Dark: a lifted chip plus hairline, so the choice reads at a
            // glance instead of blending into the track.
            let raised = if kit.dark {
                crate::with_alpha(kit.p.text, 72)
            } else {
                crate::c32(kit.p.raised)
            };
            p.rect_filled(seg, radius::ROW, raised);
            {
                p.rect_stroke(
                    seg,
                    radius::ROW,
                    Stroke::new(1.0, kit.hairline()),
                    StrokeKind::Inside,
                );
            }
        } else if hovered {
            p.rect_filled(seg, radius::ROW, kit.fill(State::Rest));
        }
        if resp.clicked()
            && resp.interact_pointer_pos().is_some_and(|h| seg.contains(h))
            && *selected != i
        {
            *selected = i;
            resp.mark_changed();
        }
        let color = if i == *selected {
            kit.text()
        } else {
            kit.text2()
        };
        draw(ui, i, seg, dim(ui, color));
    }
    resp
}

fn field_frame(ui: &mut Ui, width: f32) -> (Rect, Kit) {
    let kit = Kit::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
    let fill = if kit.dark {
        kit.fill(State::Rest)
    } else {
        crate::c32(kit.p.raised)
    };
    ui.painter().rect_filled(rect, radius::CONTROL, fill);
    ui.painter().rect_stroke(
        rect,
        radius::CONTROL,
        Stroke::new(1.0, kit.hairline()),
        StrokeKind::Inside,
    );
    (rect, kit)
}

fn inner_edit(ui: &mut Ui, rect: Rect, value: &mut String, hint: &str, password: bool) -> Response {
    let edit = egui::TextEdit::singleline(value)
        .frame(egui::Frame::NONE)
        .password(password)
        .hint_text(hint)
        .font(body_font())
        .vertical_align(egui::Align::Center)
        .margin(egui::Margin::ZERO);
    ui.put(rect, edit)
}

/// A 28px text field; `password` masks input.
pub fn text_field(
    ui: &mut Ui,
    value: &mut String,
    hint: &str,
    width: f32,
    password: bool,
) -> Response {
    let (rect, kit) = field_frame(ui, width);
    let inner = rect.shrink2(vec2(space::S8, 2.0));
    let resp = inner_edit(ui, inner, value, hint, password);
    if resp.has_focus() {
        focus_ring(ui, &kit, rect, radius::CONTROL);
    }
    resp
}

/// Search field: magnifier glyph, hint, and a clear button once typed.
pub fn search_field(ui: &mut Ui, value: &mut String, hint: &str, width: f32) -> Response {
    let (rect, kit) = field_frame(ui, width);
    let glyph = kit.text3();
    let c = pos2(rect.left() + 14.0, rect.center().y - 1.0);
    let p = ui.painter();
    p.circle_stroke(c, 4.5, Stroke::new(1.5, glyph));
    p.line_segment(
        [c + vec2(3.3, 3.3), c + vec2(6.5, 6.5)],
        Stroke::new(1.5, glyph),
    );
    let clear_w = if value.is_empty() { 0.0 } else { 22.0 };
    let inner = Rect::from_min_max(
        pos2(rect.left() + 26.0, rect.top() + 2.0),
        pos2(rect.right() - space::S8 - clear_w, rect.bottom() - 2.0),
    );
    let mut resp = inner_edit(ui, inner, value, hint, false);
    if !value.is_empty() {
        let cr =
            Rect::from_center_size(pos2(rect.right() - 15.0, rect.center().y), vec2(16.0, 16.0));
        let x = ui.interact(cr, resp.id.with("clear"), Sense::click());
        let p = ui.painter();
        p.circle_filled(
            cr.center(),
            7.0,
            if x.hovered() {
                kit.text2()
            } else {
                kit.text3()
            },
        );
        let bg = crate::c32(kit.p.window).to_opaque();
        for (a, b) in [
            (vec2(-2.5, -2.5), vec2(2.5, 2.5)),
            (vec2(-2.5, 2.5), vec2(2.5, -2.5)),
        ] {
            p.line_segment([cr.center() + a, cr.center() + b], Stroke::new(1.5, bg));
        }
        if x.clicked() {
            value.clear();
            resp.mark_changed();
        }
    }
    if resp.has_focus() {
        focus_ring(ui, &kit, rect, radius::CONTROL);
    }
    resp
}

/// Horizontal slider: 4px track filled with accent up to the value.
pub fn slider(
    ui: &mut Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    width: f32,
) -> Response {
    let kit = Kit::get(ui.ctx());
    let (rect, mut resp) =
        ui.allocate_exact_size(vec2(width, 22.0), sense(ui, Sense::click_and_drag()));
    let (lo, hi) = (*range.start(), *range.end());
    let track = Rect::from_min_max(
        pos2(rect.left() + 8.0, rect.center().y - 2.0),
        pos2(rect.right() - 8.0, rect.center().y + 2.0),
    );
    if let Some(pos) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() {
            let t = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
            let v = lo + t * (hi - lo);
            if v != *value {
                *value = v;
                resp.mark_changed();
            }
        }
    }
    let t = if hi > lo {
        ((*value - lo) / (hi - lo)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let x = track.left() + t * track.width();
    let p = ui.painter();
    p.rect_filled(track, 2.0, kit.fill(State::Press));
    p.rect_filled(
        Rect::from_min_max(track.min, pos2(x, track.bottom())),
        2.0,
        dim(ui, kit.accent()),
    );
    let c = pos2(x, rect.center().y);
    p.circle_filled(c + vec2(0.0, 0.5), 8.5, Color32::from_black_alpha(45));
    p.circle_filled(c, 8.0, dim(ui, Color32::WHITE));
    if resp.has_focus() {
        focus_ring(ui, &kit, Rect::from_center_size(c, vec2(16.0, 16.0)), 8.0);
    }
    resp
}
