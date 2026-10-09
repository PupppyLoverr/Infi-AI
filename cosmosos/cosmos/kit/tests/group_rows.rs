//! Headless egui frames: a switch in a detail row must be laid out at the
//! row's right edge, inside the visible clip, and flip when clicked.

use cosmos_kit::controls::toggle;
use cosmos_kit::layout::group;
use egui::{Event, PointerButton, Pos2, RawInput, Rect};

#[derive(Default)]
struct S {
    on: bool,
    sw: Option<Rect>,
    clip: Option<Rect>,
    panel: Option<Rect>,
}

fn frame(ctx: &egui::Context, s: &mut S, events: Vec<Event>) -> egui::FullOutput {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(880.0, 540.0))),
        events,
        ..Default::default()
    };
    let out = ctx.run_ui(input, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.set_width(ui.available_width().min(620.0));
            s.panel = Some(ui.max_rect());
            group(ui, None, |g| {
                g.row_detail("Lite mode", Some("Flat surfaces"), |ui| {
                    let r = toggle(ui, &mut s.on);
                    s.sw = Some(r.rect);
                    s.clip = Some(ui.clip_rect());
                });
            });
        });
    });
    out
}

fn step(ctx: &egui::Context, s: &mut S, events: Vec<Event>) {
    frame(ctx, s, events).textures_delta.clear();
}

#[test]
fn detail_row_switch_is_visible_and_clickable() {
    let ctx = egui::Context::default();
    let mut s = S::default();
    step(&ctx, &mut s, vec![]);
    step(&ctx, &mut s, vec![]);
    let sw = s.sw.expect("switch laid out");
    let clip = s.clip.expect("clip");
    let panel = s.panel.expect("panel");
    eprintln!("switch {sw:?} clip {clip:?} panel {panel:?}");
    assert!(
        clip.contains_rect(sw),
        "switch {sw:?} outside clip {clip:?}"
    );
    assert!(
        sw.right() > panel.right() - 40.0,
        "switch {sw:?} not at the row end of {panel:?}"
    );
    let at = sw.center();
    step(&ctx, &mut s, vec![Event::PointerMoved(at)]);
    let press = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    step(&ctx, &mut s, vec![press(true)]);
    step(&ctx, &mut s, vec![press(false)]);
    assert!(s.on, "click on {sw:?} did not flip the switch");
}

#[test]
fn detail_row_switch_paints_at_125_percent() {
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(1.25);
    let mut s = S::default();
    let mut painter = cosmos_uitk::raster::Painter::default();
    let mut last = None;
    for _ in 0..3 {
        let mut out = frame(&ctx, &mut s, vec![]);
        for (id, ds) in &out.textures_delta.set {
            for d in ds {
                painter.set_texture(*id, d);
            }
        }
        out.textures_delta.clear();
        last = Some(out);
    }
    let out = last.unwrap();
    let ppp = out.pixels_per_point;
    let prims = ctx.tessellate(out.shapes, ppp);
    let (w, h) = ((880.0 * ppp) as u32, (540.0 * ppp) as u32);
    let mut buf = vec![0u8; (w * h * 4) as usize];
    painter.paint(&mut buf, w, h, &prims, ppp, [0x40, 0x40, 0x40, 0xFF]);
    let sw = s.sw.unwrap();
    let px = |x: f32, y: f32| {
        let i = (((y * ppp) as u32 * w + (x * ppp) as u32) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2]]
    };
    // Off: the knob sits left, so the right end shows the bare track.
    let track = px(sw.right() - 6.0, sw.center().y);
    let bg = px(sw.left() - 30.0, sw.center().y);
    let diff = (0..3).map(|i| track[i].abs_diff(bg[i])).max().unwrap();
    eprintln!("ppp {ppp} switch {sw:?} track {track:?} row bg {bg:?}");
    assert!(diff > 20, "off track invisible: {track:?} on {bg:?}");
}
