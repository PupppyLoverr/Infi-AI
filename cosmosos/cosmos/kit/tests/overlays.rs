//! Headless egui frames: clicking a button that opens a popover or a sheet
//! must leave it open on the following frames (gallery regression).

use cosmos_kit::controls::{button, ButtonKind};
use cosmos_kit::layout::{popover, sheet};
use egui::{Event, PointerButton, Pos2, RawInput, Rect};

#[derive(Default)]
struct S {
    pop: bool,
    sheet: bool,
    pop_drawn: bool,
    sheet_drawn: bool,
    pop_btn: Option<Rect>,
    sheet_btn: Option<Rect>,
}

fn frame(ctx: &egui::Context, s: &mut S, events: Vec<Event>) {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
        events,
        ..Default::default()
    };
    s.pop_drawn = false;
    s.sheet_drawn = false;
    let mut out = ctx.run_ui(input, |ui| {
        ui.horizontal(|ui| {
            let r = button(ui, ButtonKind::Secondary, "Popover");
            s.pop_btn = Some(r.rect);
            if r.clicked() {
                s.pop = !s.pop;
            }
            let drawn = popover(ui.ctx(), egui::Id::new("t-pop"), r.rect, &mut s.pop, |ui| {
                ui.label("body");
            });
            s.pop_drawn = drawn.is_some();
            let r = button(ui, ButtonKind::Secondary, "Sheet");
            s.sheet_btn = Some(r.rect);
            if r.clicked() {
                s.sheet = true;
            }
        });
        if s.sheet {
            let mut drawn = false;
            s.sheet = sheet(ui.ctx(), egui::Id::new("t-sheet"), "Rename", |ui| {
                drawn = true;
                ui.label("body");
            });
            s.sheet_drawn = drawn;
        }
    });
    out.textures_delta.clear();
}

fn click(ctx: &egui::Context, s: &mut S, at: Pos2) {
    frame(ctx, s, vec![Event::PointerMoved(at)]);
    let press = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(ctx, s, vec![press(true)]);
    frame(ctx, s, vec![press(false)]);
}

#[test]
fn popover_stays_open_after_click() {
    let ctx = egui::Context::default();
    let mut s = S::default();
    frame(&ctx, &mut s, vec![]);
    frame(&ctx, &mut s, vec![]);
    let at = s.pop_btn.unwrap().center();
    click(&ctx, &mut s, at);
    assert!(s.pop, "popover state closed on the click frame");
    for _ in 0..3 {
        frame(&ctx, &mut s, vec![]);
    }
    assert!(s.pop && s.pop_drawn, "popover not drawn after click");
}

#[test]
fn sheet_stays_open_after_click() {
    let ctx = egui::Context::default();
    let mut s = S::default();
    frame(&ctx, &mut s, vec![]);
    frame(&ctx, &mut s, vec![]);
    let at = s.sheet_btn.unwrap().center();
    click(&ctx, &mut s, at);
    for _ in 0..3 {
        frame(&ctx, &mut s, vec![]);
    }
    assert!(
        s.sheet && s.sheet_drawn,
        "sheet closed itself right after opening"
    );
}

/// Press and release delivered in one frame (fast synthetic clicks).
fn tap(ctx: &egui::Context, s: &mut S, at: Pos2) {
    frame(ctx, s, vec![Event::PointerMoved(at)]);
    let press = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(ctx, s, vec![press(true), press(false)]);
}

#[test]
fn fast_tap_opens_popover_and_sheet() {
    let ctx = egui::Context::default();
    let mut s = S::default();
    frame(&ctx, &mut s, vec![]);
    frame(&ctx, &mut s, vec![]);
    let at = s.pop_btn.unwrap().center();
    tap(&ctx, &mut s, at);
    frame(&ctx, &mut s, vec![]);
    assert!(
        s.pop && s.pop_drawn,
        "popover not open after a one-frame tap"
    );
    let at = s.sheet_btn.unwrap().center();
    tap(&ctx, &mut s, at);
    frame(&ctx, &mut s, vec![]);
    assert!(
        s.sheet && s.sheet_drawn,
        "sheet not open after a one-frame tap"
    );
}
