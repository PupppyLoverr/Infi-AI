//! Structural components: the app window frame (52px unified toolbar,
//! 220px sidebar, status bar), toolbar buttons, sidebar rows, table and
//! grid views, cards/grouped settings rows, tabs, dropdowns, popovers,
//! context-menu items and sheets.

use crate::{c32, controls::body_font, focus_ring, icons, Icon, Kit, State};
use cosmos_theme::{radius, space, text, ROW_H};
use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Frame, Layout, Rect, Response, Sense, Shadow,
    Stroke, StrokeKind, Ui, UiBuilder,
};
use std::sync::Arc;

pub const TOOLBAR_H: f32 = 52.0;
pub const TOOLBAR_BUTTON: f32 = 28.0;
pub const SIDEBAR_W: f32 = 220.0;
pub const STATUS_H: f32 = 24.0;
pub const TILE_W: f32 = 96.0;
pub const THUMB: f32 = 64.0;

type Section<'a> = Box<dyn FnOnce(&mut Ui) + 'a>;

/// The window frame every app draws inside: translucent window material,
/// optional unified toolbar, sidebar and status bar around a body.
#[derive(Default)]
pub struct AppWindow<'a> {
    toolbar: Option<Section<'a>>,
    sidebar: Option<Section<'a>>,
    status: Option<Section<'a>>,
}

impl<'a> AppWindow<'a> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn toolbar(mut self, f: impl FnOnce(&mut Ui) + 'a) -> Self {
        self.toolbar = Some(Box::new(f));
        self
    }
    pub fn sidebar(mut self, f: impl FnOnce(&mut Ui) + 'a) -> Self {
        self.sidebar = Some(Box::new(f));
        self
    }
    pub fn status(mut self, f: impl FnOnce(&mut Ui) + 'a) -> Self {
        self.status = Some(Box::new(f));
        self
    }

    pub fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
        let kit = Kit::get(ui.ctx());
        let full = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(full, 0.0, c32(kit.p.window));
        let mut top = full.top();
        let mut bottom = full.bottom();
        if let Some(tb) = self.toolbar {
            let r = Rect::from_min_max(full.min, pos2(full.right(), top + TOOLBAR_H));
            p.hline(
                r.x_range(),
                r.bottom() - 0.5,
                Stroke::new(1.0, kit.hairline()),
            );
            region(
                ui,
                r.shrink2(vec2(space::S12, 0.0)),
                Layout::left_to_right(Align::Center),
                tb,
            );
            top = r.bottom();
        }
        if let Some(st) = self.status {
            let r = Rect::from_min_max(pos2(full.left(), bottom - STATUS_H), full.max);
            p.hline(r.x_range(), r.top() + 0.5, Stroke::new(1.0, kit.hairline()));
            region(
                ui,
                r.shrink2(vec2(space::S12, 0.0)),
                Layout::left_to_right(Align::Center),
                st,
            );
            bottom = r.top();
        }
        let mut left = full.left();
        if let Some(sb) = self.sidebar {
            let r = Rect::from_min_max(pos2(left, top), pos2(left + SIDEBAR_W, bottom));
            p.rect_filled(r, 0.0, c32(kit.p.sidebar));
            p.vline(
                r.right() - 0.5,
                r.y_range(),
                Stroke::new(1.0, kit.hairline()),
            );
            region(
                ui,
                r.shrink(space::S8),
                Layout::top_down(Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("kit-sidebar")
                        .auto_shrink([false, false])
                        .show(ui, sb);
                },
            );
            left = r.right();
        }
        let body_rect = Rect::from_min_max(pos2(left, top), pos2(full.right(), bottom));
        region(ui, body_rect, Layout::top_down(Align::Min), body);
        ui.allocate_rect(full, Sense::hover());
    }
}

fn region(ui: &mut Ui, rect: Rect, layout: Layout, f: impl FnOnce(&mut Ui)) {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(layout), |ui| {
        ui.set_clip_rect(rect.intersect(ui.clip_rect()));
        f(ui)
    });
}

/// One-line text that ends in "…" when wider than `max_w`.
pub fn ellipsized(ui: &Ui, s: &str, font: FontId, color: Color32, max_w: f32) -> Arc<egui::Galley> {
    let mut job =
        egui::text::LayoutJob::single_section(s.to_owned(), egui::TextFormat::simple(font, color));
    job.wrap = egui::text::TextWrapping {
        max_width: max_w.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

/// Toolbar title (the window's document or location name).
pub fn toolbar_title(ui: &mut Ui, title: &str) {
    let kit = Kit::get(ui.ctx());
    ui.add_space(space::S8);
    ui.label(
        egui::RichText::new(title)
            .size(text::TITLE3.size)
            .strong()
            .color(kit.text()),
    );
}

/// Push the rest of a toolbar row to the right edge.
pub fn toolbar_spacer(ui: &mut Ui, right_width: f32) {
    let w = (ui.available_width() - right_width).max(0.0);
    ui.add_space(w);
}

/// 28px icon button; `active` shows the toggled state.
pub fn toolbar_button(ui: &mut Ui, icon: Icon, tooltip: &str, active: bool) -> Response {
    let kit = Kit::get(ui.ctx());
    let sense = if ui.is_enabled() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(TOOLBAR_BUTTON, TOOLBAR_BUTTON), sense);
    let st = State::of(&resp);
    if active || st != State::Rest {
        let fill = if active && st == State::Rest {
            kit.fill(State::Hover)
        } else {
            kit.fill(st)
        };
        ui.painter().rect_filled(rect, radius::ROW, fill);
    }
    let color = if !ui.is_enabled() {
        kit.text3()
    } else if active {
        kit.accent()
    } else {
        kit.text2()
    };
    icons::paint(
        ui,
        icon,
        Rect::from_center_size(rect.center(), vec2(18.0, 18.0)),
        color,
    );
    if resp.has_focus() {
        focus_ring(ui, &kit, rect, radius::ROW);
    }
    resp.on_hover_text(tooltip)
}

/// Uppercase caption heading a sidebar group.
pub fn sidebar_section(ui: &mut Ui, title: &str) {
    let kit = Kit::get(ui.ctx());
    ui.add_space(space::S8);
    ui.horizontal(|ui| {
        ui.add_space(space::S8);
        ui.label(
            egui::RichText::new(title)
                .size(text::CAPTION.size)
                .strong()
                .color(kit.text3()),
        );
    });
    ui.add_space(2.0);
}

/// 28px sidebar row: accent icon, label, selection fill.
pub fn sidebar_item(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> Response {
    let kit = Kit::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let st = State::of(&resp);
    if selected {
        ui.painter()
            .rect_filled(rect, radius::ROW, kit.fill(State::Press));
    } else if st != State::Rest {
        ui.painter()
            .rect_filled(rect, radius::ROW, kit.fill(State::Rest));
    }
    let ir = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(16.0, 16.0));
    icons::paint(ui, icon, ir, kit.accent());
    let g = ellipsized(ui, label, body_font(), kit.text(), rect.width() - 42.0);
    ui.painter().galley(
        pos2(rect.left() + 34.0, rect.center().y - g.size().y / 2.0),
        g,
        kit.text(),
    );
    resp
}

/// Status bar text (tertiary caption).
pub fn status_text(ui: &mut Ui, s: &str) {
    let kit = Kit::get(ui.ctx());
    ui.label(
        egui::RichText::new(s)
            .size(text::CAPTION.size)
            .color(kit.text2()),
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColAlign {
    Left,
    Right,
}

/// A table column: title, width (0 = take the remaining width), align.
#[derive(Clone, Copy, Debug)]
pub struct Column<'a> {
    pub title: &'a str,
    pub width: f32,
    pub align: ColAlign,
}

/// Column-aligned 28px rows with a hairline header (Finder list view).
pub struct Table<'a> {
    pub cols: &'a [Column<'a>],
}

impl Table<'_> {
    /// Column spans; `None` for columns dropped because the window is too
    /// narrow. Trailing fixed columns go first so the flexible (name)
    /// column keeps at least `MIN_FLEX` — like Finder narrowing a list.
    fn spans(&self, rect: Rect) -> Vec<Option<(f32, f32)>> {
        const MIN_FLEX: f32 = 160.0;
        let avail = rect.width() - 2.0 * space::S12;
        let mut shown = self.cols.len();
        let fixed = |n: usize| -> f32 { self.cols[..n].iter().map(|c| c.width).sum() };
        while shown > 1 && avail - fixed(shown) < MIN_FLEX && self.cols[shown - 1].width > 0.0 {
            shown -= 1;
        }
        let flex = (avail - fixed(shown)).max(40.0);
        let mut x = rect.left() + space::S12;
        self.cols
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (i < shown).then(|| {
                    let w = if c.width == 0.0 { flex } else { c.width };
                    let span = (x, w);
                    x += w;
                    span
                })
            })
            .collect()
    }

    pub fn header(&self, ui: &mut Ui) {
        let kit = Kit::get(ui.ctx());
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
        let font = FontId::proportional(text::CAPTION.size);
        for (c, span) in self.cols.iter().zip(self.spans(rect)) {
            let Some((x, w)) = span else { continue };
            let g = ellipsized(ui, c.title, font.clone(), kit.text2(), w - space::S8);
            let gx = match c.align {
                ColAlign::Left => x,
                ColAlign::Right => x + w - space::S8 - g.size().x,
            };
            ui.painter()
                .galley(pos2(gx, rect.center().y - g.size().y / 2.0), g, kit.text2());
        }
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0, kit.hairline()),
        );
    }

    /// One row; `leading` paints a 16px icon before the first cell.
    pub fn row(
        &self,
        ui: &mut Ui,
        selected: bool,
        leading: Option<&dyn Fn(&Ui, Rect)>,
        cells: &[&str],
    ) -> Response {
        let kit = Kit::get(ui.ctx());
        let (rect, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
        let st = State::of(&resp);
        let sel_text = Color32::WHITE;
        if selected {
            ui.painter()
                .rect_filled(rect.shrink2(vec2(4.0, 0.0)), radius::ROW, kit.accent());
        } else if st != State::Rest {
            ui.painter().rect_filled(
                rect.shrink2(vec2(4.0, 0.0)),
                radius::ROW,
                kit.fill(State::Rest),
            );
        }
        for (i, (span, cell)) in self.spans(rect).into_iter().zip(cells).enumerate() {
            let Some((x, w)) = span else { continue };
            let mut x0 = x;
            if i == 0 {
                if let Some(paint) = leading {
                    paint(
                        ui,
                        Rect::from_center_size(pos2(x + 8.0, rect.center().y), vec2(16.0, 16.0)),
                    );
                    x0 += 24.0;
                }
            }
            let color = if selected {
                sel_text
            } else if i == 0 {
                kit.text()
            } else {
                kit.text2()
            };
            let g = ellipsized(ui, cell, body_font(), color, x + w - x0 - space::S8);
            let gx = match self.cols[i].align {
                ColAlign::Left => x0,
                ColAlign::Right => x + w - space::S8 - g.size().x,
            };
            ui.painter()
                .galley(pos2(gx, rect.center().y - g.size().y / 2.0), g, color);
        }
        resp
    }
}

/// Grid tile: 96px wide, 64px thumbnail painted by `thumb`, 2-line label.
pub fn grid_tile(
    ui: &mut Ui,
    selected: bool,
    label: &str,
    thumb: impl FnOnce(&Ui, Rect),
) -> Response {
    let kit = Kit::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(TILE_W, THUMB + 48.0), Sense::click());
    let st = State::of(&resp);
    let tr = Rect::from_center_size(
        pos2(rect.center().x, rect.top() + 6.0 + THUMB / 2.0),
        vec2(THUMB, THUMB),
    );
    if selected || st != State::Rest {
        let fill = if selected {
            kit.fill(State::Press)
        } else {
            kit.fill(State::Rest)
        };
        ui.painter()
            .rect_filled(tr.expand(4.0), radius::CONTROL, fill);
    }
    thumb(ui, tr);
    let mut job = egui::text::LayoutJob::single_section(
        label.to_owned(),
        egui::TextFormat::simple(FontId::proportional(text::CAPTION.size + 1.0), kit.text()),
    );
    job.wrap = egui::text::TextWrapping {
        max_width: TILE_W - 8.0,
        max_rows: 2,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    job.halign = Align::Center;
    let g = ui.painter().layout_job(job);
    let lr = Rect::from_center_size(
        pos2(rect.center().x, tr.bottom() + 10.0 + g.size().y / 2.0),
        g.size() + vec2(8.0, 2.0),
    );
    let tc = if selected { Color32::WHITE } else { kit.text() };
    if selected {
        ui.painter().rect_filled(lr, radius::ROW, kit.accent());
    }
    ui.painter()
        .galley(pos2(rect.center().x, lr.top() + 1.0), g, tc);
    resp
}

/// Card surface (raised fill, hairline, 12px radius, 12px padding).
pub fn card<R>(ui: &mut Ui, f: impl FnOnce(&mut Ui) -> R) -> R {
    let kit = Kit::get(ui.ctx());
    card_frame(&kit)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            f(ui)
        })
        .inner
}

pub fn card_frame(kit: &Kit) -> Frame {
    let fill = if kit.dark {
        kit.fill(State::Rest)
    } else {
        c32(kit.p.raised)
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, kit.hairline()))
        .corner_radius(radius::CARD)
        .inner_margin(space::S12)
}

/// System-Settings style grouped card: label left, control right, 40px
/// rows separated by inset hairlines.
pub struct Group<'u> {
    ui: &'u mut Ui,
    first: bool,
}

pub fn group(ui: &mut Ui, title: Option<&str>, f: impl FnOnce(&mut Group)) {
    let kit = Kit::get(ui.ctx());
    if let Some(t) = title {
        ui.add_space(space::S8);
        ui.label(
            egui::RichText::new(t)
                .size(text::BODY.size)
                .strong()
                .color(kit.text()),
        );
        ui.add_space(2.0);
    }
    let mut frame = card_frame(&kit);
    frame.inner_margin = egui::Margin::symmetric(space::S12 as i8, 0);
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        f(&mut Group { ui, first: true });
    });
}

impl Group<'_> {
    /// A labelled row; `control` is laid out right-aligned.
    pub fn row(&mut self, label: &str, control: impl FnOnce(&mut Ui)) {
        self.row_detail(label, None, control)
    }

    pub fn row_detail(&mut self, label: &str, detail: Option<&str>, control: impl FnOnce(&mut Ui)) {
        let kit = Kit::get(self.ui.ctx());
        let h = if detail.is_some() { 52.0 } else { 40.0 };
        let (rect, _) = self
            .ui
            .allocate_exact_size(vec2(self.ui.available_width(), h), Sense::hover());
        if !self.first {
            self.ui.painter().hline(
                rect.x_range(),
                rect.top() + 0.5,
                Stroke::new(1.0, kit.hairline()),
            );
        }
        self.first = false;
        let p = self.ui.painter();
        match detail {
            None => {
                p.text(
                    pos2(rect.left(), rect.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    body_font(),
                    kit.text(),
                );
            }
            Some(d) => {
                p.text(
                    pos2(rect.left(), rect.center().y - 1.0),
                    Align2::LEFT_BOTTOM,
                    label,
                    body_font(),
                    kit.text(),
                );
                p.text(
                    pos2(rect.left(), rect.center().y + 2.0),
                    Align2::LEFT_TOP,
                    d,
                    FontId::proportional(text::CAPTION.size),
                    kit.text2(),
                );
            }
        }
        region(self.ui, rect, Layout::right_to_left(Align::Center), control);
    }

    /// Full-width custom content row.
    pub fn custom(&mut self, f: impl FnOnce(&mut Ui)) {
        let kit = Kit::get(self.ui.ctx());
        if !self.first {
            let r = self.ui.available_rect_before_wrap();
            self.ui
                .painter()
                .hline(r.x_range(), r.top() + 0.5, Stroke::new(1.0, kit.hairline()));
        }
        self.first = false;
        self.ui.add_space(space::S12);
        f(self.ui);
        self.ui.add_space(space::S12);
    }
}

pub enum TabEvent {
    Select(usize),
    Close(usize),
    New,
}

/// Document tab strip: 32px tabs with close buttons and a trailing "+".
pub fn tab_strip(
    ui: &mut Ui,
    selected: usize,
    titles: &[String],
    closable: bool,
) -> Option<TabEvent> {
    let kit = Kit::get(ui.ctx());
    let mut ev = None;
    let n = titles.len().max(1) as f32;
    let tab_w = ((ui.available_width() - 36.0) / n).clamp(96.0, 220.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (i, t) in titles.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(tab_w, 30.0), Sense::click());
            let st = State::of(&resp);
            let sel = i == selected;
            if sel {
                ui.painter()
                    .rect_filled(rect, radius::ROW, kit.fill(State::Hover));
            } else if st != State::Rest {
                ui.painter()
                    .rect_filled(rect, radius::ROW, kit.fill(State::Rest));
            }
            let close_r = Rect::from_center_size(
                pos2(rect.right() - 14.0, rect.center().y),
                vec2(16.0, 16.0),
            );
            let tw = rect.width() - if closable { 40.0 } else { 16.0 };
            let col = if sel { kit.text() } else { kit.text2() };
            let g = ellipsized(ui, t, body_font(), col, tw);
            ui.painter().galley(
                pos2(
                    rect.center().x - g.size().x / 2.0,
                    rect.center().y - g.size().y / 2.0,
                ),
                g,
                col,
            );
            if closable && (sel || resp.hovered()) {
                let x = ui.interact(close_r, resp.id.with("close"), Sense::click());
                if x.hovered() {
                    ui.painter()
                        .rect_filled(close_r, 4.0, kit.fill(State::Hover));
                }
                icons::paint(ui, Icon::Close, close_r.shrink(2.0), kit.text2());
                if x.clicked() {
                    ev = Some(TabEvent::Close(i));
                    continue;
                }
            }
            if resp.clicked() {
                ev = Some(TabEvent::Select(i));
            }
        }
        if toolbar_button(ui, Icon::Plus, "New tab", false).clicked() {
            ev = Some(TabEvent::New);
        }
    });
    ev
}

/// Popover/menu surface: raised, hairline, 14px radius, E2 shadow.
pub fn menu_frame(kit: &Kit) -> Frame {
    let mut fill = c32(kit.p.raised);
    if !kit.dark {
        fill = Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), 245);
    }
    let e = cosmos_theme::elevation::E2;
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, kit.hairline()))
        .corner_radius(radius::PANEL)
        .inner_margin(6.0)
        .shadow(Shadow {
            offset: [0, e.dy as i8],
            blur: e.blur as u8,
            spread: 0,
            color: Color32::from_black_alpha((e.alpha * 255.0) as u8),
        })
}

/// A popover anchored under `anchor`; closes on outside click or Esc.
pub fn popover<R>(
    ctx: &egui::Context,
    id: egui::Id,
    anchor: Rect,
    open: &mut bool,
    f: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    if !*open {
        return None;
    }
    let kit = Kit::get(ctx);
    let shown = egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(anchor.left_bottom() + vec2(0.0, 4.0))
        .show(ctx, |ui| menu_frame(&kit).show(ui, f).inner);
    let outside = ctx.input(|i| i.pointer.any_pressed())
        && ctx
            .pointer_interact_pos()
            .is_some_and(|p| !shown.response.rect.contains(p) && !anchor.contains(p));
    if outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        *open = false;
    }
    Some(shown.inner)
}

/// A menu row for popovers and context menus: icon, label, shortcut.
pub fn menu_item(ui: &mut Ui, icon: Option<Icon>, label: &str, shortcut: Option<&str>) -> Response {
    let kit = Kit::get(ui.ctx());
    let w = ui.available_width().max(200.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
    let hot = resp.hovered();
    if hot {
        ui.painter().rect_filled(rect, radius::ROW, kit.accent());
    }
    let fg = if hot { Color32::WHITE } else { kit.text() };
    let mut x = rect.left() + space::S8;
    if let Some(i) = icon {
        icons::paint(
            ui,
            i,
            Rect::from_center_size(pos2(x + 8.0, rect.center().y), vec2(16.0, 16.0)),
            fg,
        );
        x += 24.0;
    }
    ui.painter().text(
        pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        body_font(),
        fg,
    );
    if let Some(s) = shortcut {
        let sc = if hot { Color32::WHITE } else { kit.text3() };
        ui.painter().text(
            pos2(rect.right() - space::S8, rect.center().y),
            Align2::RIGHT_CENTER,
            s,
            body_font(),
            sc,
        );
    }
    resp
}

pub fn menu_separator(ui: &mut Ui) {
    let kit = Kit::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter().hline(
        rect.shrink2(vec2(space::S8, 0.0)).x_range(),
        rect.center().y,
        Stroke::new(1.0, kit.hairline()),
    );
}

/// Dropdown (pop-up button): current value + chevron, menu of options.
pub fn dropdown(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    selected: &mut usize,
    options: &[&str],
    width: f32,
) -> Response {
    let kit = Kit::get(ui.ctx());
    let id = ui.id().with(id);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
    let st = State::of(&resp);
    let fill = if kit.dark {
        kit.fill(st)
    } else {
        c32(kit.p.raised)
    };
    ui.painter().rect_filled(rect, radius::CONTROL, fill);
    ui.painter().rect_stroke(
        rect,
        radius::CONTROL,
        Stroke::new(1.0, kit.hairline()),
        StrokeKind::Inside,
    );
    let cur = options.get(*selected).copied().unwrap_or("");
    let g = ellipsized(ui, cur, body_font(), kit.text(), width - 40.0);
    ui.painter().galley(
        pos2(
            rect.left() + space::S8 + 2.0,
            rect.center().y - g.size().y / 2.0,
        ),
        g,
        kit.text(),
    );
    let cr = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(14.0, 14.0));
    icons::paint(ui, Icon::ChevronDown, cr, kit.text2());
    if resp.has_focus() {
        focus_ring(ui, &kit, rect, radius::CONTROL);
    }
    let mut open = ui.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false));
    if resp.clicked() {
        open = !open;
    }
    let mut picked = None;
    popover(ui.ctx(), id.with("menu"), rect, &mut open, |ui| {
        ui.set_min_width(width - 12.0);
        for (i, o) in options.iter().enumerate() {
            let icon = (i == *selected).then_some(Icon::Check);
            if menu_item(ui, icon, o, None).clicked() {
                picked = Some(i);
            }
        }
    });
    if let Some(i) = picked {
        open = false;
        if i != *selected {
            *selected = i;
            resp.mark_changed();
        }
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, open));
    resp
}

/// Modal sheet: dimmed backdrop, centred card with a title and body.
/// Returns false once dismissed (Esc / backdrop click).
pub fn sheet(ctx: &egui::Context, id: egui::Id, title: &str, f: impl FnOnce(&mut Ui)) -> bool {
    let kit = Kit::get(ctx);
    let frame = menu_frame(&kit)
        .inner_margin(space::S20)
        .corner_radius(radius::PANEL);
    let resp = egui::Modal::new(id)
        .frame(frame)
        .backdrop_color(Color32::from_black_alpha(90))
        .show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(
                egui::RichText::new(title)
                    .size(text::TITLE3.size)
                    .strong()
                    .color(kit.text()),
            );
            ui.add_space(space::S12);
            f(ui);
        });
    !resp.should_close()
}
