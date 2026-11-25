//! solyra-term: the Solyra terminal. zgx draws it. Each workspace tab holds a
//! binary tree of panes, and the window keeps at most 16 terminals alive.
//!
//! Keys (reserved, so a focused terminal does not take them first):
//! - Ctrl+Shift+D — split the focused pane to the right
//! - Ctrl+Shift+E — split the focused pane downward
//! - Ctrl+Shift+W — close the focused pane
//! - Ctrl+Shift+T — new workspace tab
//! - Ctrl+Tab / Ctrl+Shift+Tab — next / previous workspace
//!
//! The same actions are on the toolbar. Closing the last pane of a workspace
//! closes that tab; closing the last tab opens a fresh one. `--self-test`
//! renders for two seconds and exits 0.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::Duration,
};

use zgx::{
    actions::{Action, KeyBinding, Keymap, Keystroke},
    compose::prelude::*,
    input::{EventPhase, InputEvent, Key, Modifiers, PointerButton},
    terminal::{Terminal, TerminalOptions, terminal},
};
use zgx_components::{Tab, TabsOptions, tabs};
use zgx_desktop::{Application, WindowOptions};

mod panes;
mod tabs_model;

#[cfg(test)]
mod clear_tests;

use panes::{PaneTree, Session, SplitDir};

const CTRL: Modifiers = Modifiers {
    control: true,
    shift: false,
    alt: false,
    meta: false,
};
const CTRL_SHIFT: Modifiers = Modifiers {
    control: true,
    shift: true,
    alt: false,
    meta: false,
};

const CAP_HINT: &str = "16 panes is the limit";

#[derive(Clone, Copy)]
struct SplitRight;
#[derive(Clone, Copy)]
struct SplitDown;
#[derive(Clone, Copy)]
struct ClosePane;
#[derive(Clone, Copy)]
struct NewWorkspace;
#[derive(Clone, Copy)]
struct NextWorkspace;
#[derive(Clone, Copy)]
struct PrevWorkspace;

fn bind_char(
    map: Keymap,
    letter: char,
    modifiers: Modifiers,
    action: impl Copy + 'static,
) -> Keymap {
    let mut map = map
        .bind(KeyBinding::code(format!("Key{letter}"), modifiers, Action::new(action)).reserved());
    for ch in [letter.to_ascii_lowercase(), letter.to_ascii_uppercase()] {
        map = map.bind(
            KeyBinding::new(
                [Keystroke::new(Key::Character(ch.to_string()), modifiers)],
                Action::new(action),
            )
            .expect("one keystroke")
            .reserved(),
        );
    }
    map
}

fn bind_tab(map: Keymap, modifiers: Modifiers, action: impl Copy + 'static) -> Keymap {
    map.bind(KeyBinding::code("Tab", modifiers, Action::new(action)).reserved())
        .bind(
            KeyBinding::new([Keystroke::new(Key::Tab, modifiers)], Action::new(action))
                .expect("one keystroke")
                .reserved(),
        )
}

fn keymap() -> Keymap {
    let map = Keymap::new();
    let map = bind_char(map, 'D', CTRL_SHIFT, SplitRight);
    let map = bind_char(map, 'E', CTRL_SHIFT, SplitDown);
    let map = bind_char(map, 'W', CTRL_SHIFT, ClosePane);
    let map = bind_char(map, 'T', CTRL_SHIFT, NewWorkspace);
    let map = bind_tab(map, CTRL, NextWorkspace);
    bind_tab(map, CTRL_SHIFT, PrevWorkspace)
}

fn grow_weights(ratio: f32) -> (f32, f32) {
    let ratio = if ratio.is_finite() {
        ratio.clamp(0.15, 0.85)
    } else {
        0.5
    };
    (ratio, 1.0 - ratio)
}

#[derive(Clone)]
struct TermApp {
    rt: zgx::reactive::Runtime,
    session: Rc<RefCell<Session>>,
    /// Bumped when the tree or the focused pane changes, so the layout follows.
    revision: zgx::reactive::Signal<u32>,
    terms: Rc<RefCell<HashMap<String, Terminal>>>,
    /// Drag positions for `zgx` split dividers, keyed by the tree's split id.
    splits: Rc<RefCell<HashMap<String, zgx::reactive::Signal<f32>>>>,
    selected: zgx::reactive::Signal<String>,
    items: zgx::reactive::Signal<Vec<Tab>>,
    status: zgx::reactive::Signal<String>,
    next_id: Rc<Cell<u32>>,
    accent: zgx::scene::Color,
    hairline: zgx::scene::Color,
}

impl TermApp {
    fn alloc(&self) -> u32 {
        let n = self.next_id.get() + 1;
        self.next_id.set(n);
        n
    }

    fn bump(&self) {
        self.revision.update(|n| *n = n.wrapping_add(1));
    }

    fn spawn(&self, cwd_from: Option<&str>) -> Terminal {
        let cwd = cwd_from
            .and_then(|id| self.terms.borrow().get(id).map(|term| term.pwd()))
            .filter(|path| !path.is_empty())
            .map(std::path::PathBuf::from);
        Terminal::new(
            &self.rt,
            TerminalOptions {
                working_directory: cwd,
                ..Default::default()
            },
        )
    }

    fn insert_pane(&self, id: &str, cwd_from: Option<&str>) {
        let term = self.spawn(cwd_from);
        self.terms.borrow_mut().insert(id.to_string(), term);
    }

    fn drop_panes(&self, ids: impl IntoIterator<Item = String>) {
        let mut terms = self.terms.borrow_mut();
        for id in ids {
            if let Some(term) = terms.remove(&id) {
                term.close();
            }
        }
    }

    fn focus_only(&self, id: &str) {
        let terms = self.terms.borrow();
        for (pane, term) in terms.iter() {
            if pane == id {
                term.focus();
            } else {
                term.blur();
            }
        }
    }

    fn refuse(&self) {
        self.status.set(CAP_HINT.to_string());
    }

    fn show_active(&self) {
        let (name, focused) = {
            let session = self.session.borrow();
            (
                session.active().map(|w| w.name.clone()),
                session.active().map(|w| w.focused.clone()),
            )
        };
        if let Some(name) = name {
            self.selected.set(name);
        }
        if let Some(id) = focused {
            self.focus_only(&id);
        }
        self.bump();
    }

    /// The window is never left without a terminal.
    fn reopen_if_empty(&self) {
        if !self.session.borrow().workspaces.is_empty() {
            return;
        }
        let n = self.alloc();
        let pane = format!("pane-{n}");
        let tab = format!("tab-{n}");
        self.insert_pane(&pane, None);
        self.session.borrow_mut().add_workspace(tab, pane);
    }

    fn split(&self, dir: SplitDir) {
        if !self.session.borrow().can_add_pane() {
            self.refuse();
            return;
        }
        let cwd_from = self.session.borrow().active().map(|w| w.focused.clone());
        let n = self.alloc();
        let pane = format!("pane-{n}");
        let key = format!("split-{n}");
        // Near half the window; the divider drags from there.
        let seed = match dir {
            SplitDir::Vertical => 440.,
            SplitDir::Horizontal => 240.,
        };
        self.splits
            .borrow_mut()
            .insert(key.clone(), self.rt.signal(seed));
        self.insert_pane(&pane, cwd_from.as_deref());
        let opened = self
            .session
            .borrow_mut()
            .split_focused(pane.clone(), dir, key.clone());
        if opened {
            self.status.set(String::new());
            self.show_active();
        } else {
            self.splits.borrow_mut().remove(&key);
            self.drop_panes([pane]);
            self.refuse();
        }
    }

    fn close_focused(&self) {
        let Some(closed) = self.session.borrow_mut().close_focused() else {
            return;
        };
        self.drop_panes([closed]);
        self.reopen_if_empty();
        self.status.set(String::new());
        self.show_active();
    }

    fn new_workspace(&self) {
        if !self.session.borrow().can_add_pane() {
            self.refuse();
            return;
        }
        let cwd_from = self.session.borrow().active().map(|w| w.focused.clone());
        let n = self.alloc();
        let pane = format!("pane-{n}");
        let tab = format!("tab-{n}");
        self.insert_pane(&pane, cwd_from.as_deref());
        if self.session.borrow_mut().add_workspace(tab, pane.clone()) {
            self.status.set(String::new());
            self.show_active();
        } else {
            self.drop_panes([pane]);
            self.refuse();
        }
    }

    fn close_workspace(&self, name: &str) {
        let removed = {
            let mut session = self.session.borrow_mut();
            let Some(idx) = session.workspaces.iter().position(|w| w.name == name) else {
                return;
            };
            let ids: Vec<String> = session.workspaces[idx].tree.ids().into_iter().collect();
            session.workspaces.remove(idx);
            if session.workspaces.is_empty() {
                session.active = 0;
            } else if idx < session.active {
                session.active -= 1;
            } else if session.active >= session.workspaces.len() {
                session.active = session.workspaces.len() - 1;
            }
            ids
        };
        self.drop_panes(removed);
        self.reopen_if_empty();
        self.status.set(String::new());
        self.show_active();
    }

    fn cycle(&self, delta: isize) {
        {
            let mut session = self.session.borrow_mut();
            if session.workspaces.is_empty() {
                return;
            }
            if delta < 0 {
                session.prev_workspace();
            } else {
                session.next_workspace();
            }
        }
        self.show_active();
    }

    fn sync_tabs(&self) {
        let _revision = self.revision.get();
        let labels: Vec<(String, String)> = {
            let session = self.session.borrow();
            let terms = self.terms.borrow();
            session
                .workspaces
                .iter()
                .map(|workspace| {
                    let name = terms
                        .get(&workspace.focused)
                        .map(|term| {
                            let title = term.title();
                            if title.is_empty() {
                                term.process()
                            } else {
                                title
                            }
                        })
                        .unwrap_or_default();
                    (
                        workspace.name.clone(),
                        tabs_model::label(&workspace.name, &name),
                    )
                })
                .collect()
        };

        let mut selected = self.selected.get();
        if !labels.iter().any(|(name, _)| name == &selected)
            && let Some((name, _)) = labels.first()
        {
            selected = name.clone();
            self.selected.set(selected.clone());
        }

        let current = self.items.get();
        let mut next = Vec::new();
        for tab in &current {
            if let Some((_, label)) = labels.iter().find(|(name, _)| name == &tab.id) {
                next.push(Tab {
                    label: label.clone(),
                    ..tab.clone()
                });
            }
        }
        for (name, label) in &labels {
            if !next.iter().any(|tab| &tab.id == name) {
                next.push(Tab::new(name.clone(), label.clone()).closable());
            }
        }
        let changed = current.len() != next.len()
            || current
                .iter()
                .zip(&next)
                .any(|(before, after)| before.id != after.id || before.label != after.label);
        if changed {
            self.items.set(next.clone());
        }
        // The strip is the order Ctrl+Tab walks, including after a drag.
        let visual: Vec<String> = next.into_iter().map(|tab| tab.id).collect();
        let mut session = self.session.borrow_mut();
        session.workspaces.sort_by_key(|workspace| {
            visual
                .iter()
                .position(|name| name == &workspace.name)
                .unwrap_or(usize::MAX)
        });
        if let Some(idx) = session
            .workspaces
            .iter()
            .position(|workspace| workspace.name == selected)
        {
            session.active = idx;
        }
    }

    fn render_tree(&self, tree: &PaneTree, workspace: &str) -> View {
        match tree {
            PaneTree::Leaf { id } => self.pane(id, workspace),
            PaneTree::Split {
                dir,
                key,
                ratio,
                a,
                b,
            } => {
                let a = self.render_tree(a, workspace);
                let b = self.render_tree(b, workspace);
                let stacked = matches!(dir, SplitDir::Horizontal);
                let size = self.splits.borrow().get(key).cloned();
                match size {
                    Some(size) => split(
                        size,
                        SplitOptions {
                            vertical: stacked,
                            min: 80.,
                            min_rest: 80.,
                            label: Some(
                                if stacked {
                                    "Split height"
                                } else {
                                    "Split width"
                                }
                                .into(),
                            ),
                            ..Default::default()
                        },
                        a,
                        b,
                    )
                    .grow()
                    .w_full()
                    .h_full(),
                    None => {
                        let (wa, wb) = grow_weights(*ratio);
                        let pair = if stacked { column() } else { row() };
                        pair.gap(4.)
                            .w_full()
                            .h_full()
                            .child(weighted(a, wa))
                            .child(weighted(b, wb))
                    }
                }
            }
        }
    }

    fn pane(&self, id: &str, workspace: &str) -> View {
        let body = match self.terms.borrow().get(id).cloned() {
            Some(term) => terminal(&term).grow().rounded(4.),
            None => text("closed"),
        };
        let (session, revision, accent, hairline) = (
            self.session.clone(),
            self.revision.clone(),
            self.accent,
            self.hairline,
        );
        let (workspace, id) = (workspace.to_string(), id.to_string());
        let click = self.clone();
        let watch_id = id.clone();
        let watch_ws = workspace.clone();
        column()
            .grow()
            .w_full()
            .h_full()
            .rounded(6.)
            .border(2.)
            .overflow_hidden()
            .reactive_style(move || {
                let _ = revision.get();
                let on = session
                    .borrow()
                    .workspaces
                    .iter()
                    .any(|w| w.name == watch_ws && w.focused == watch_id);
                Styles::new().border_color(if on { accent } else { hairline })
            })
            .on_event(move |cx| {
                if cx.phase == EventPhase::Bubble {
                    return;
                }
                if matches!(
                    cx.event,
                    InputEvent::PointerDown {
                        button: PointerButton::Primary,
                        ..
                    }
                ) {
                    let mut moved = false;
                    if let Some(workspace) = click
                        .session
                        .borrow_mut()
                        .workspaces
                        .iter_mut()
                        .find(|w| w.name == workspace)
                    {
                        if workspace.tree.contains(&id) && workspace.focused != id {
                            workspace.focused = id.clone();
                            moved = true;
                        }
                    }
                    click.focus_only(&id);
                    if moved {
                        click.bump();
                    }
                }
            })
            .child(body)
    }

    fn panel(&self, id: String) -> View {
        let app = self.clone();
        let watch = id.clone();
        switch(
            move || {
                let _ = app.revision.get();
                let session = app.session.borrow();
                session
                    .workspaces
                    .iter()
                    .find(|w| w.name == watch)
                    .map(|w| w.tree.clone())
            },
            {
                let app = self.clone();
                move |tree, _cx| match tree {
                    Some(tree) => app.render_tree(&tree, &id).grow().w_full().h_full(),
                    None => text("closed"),
                }
            },
        )
        .grow()
        .w_full()
        .h_full()
    }
}

fn weighted(view: View, weight: f32) -> View {
    view.flex_grow(weight)
        .flex_shrink(1.)
        .flex_basis(0.)
        .min_w(48.)
        .min_h(32.)
}

fn tool_button(label: &str, on: impl FnMut() + 'static) -> View {
    button()
        .h(28.)
        .px(10.)
        .rounded(8.)
        .items_center()
        .child(text(label.to_string()).text_size(12.))
        .on_click(on)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let self_test = std::env::args().any(|arg| arg == "--self-test");
    let style = solyra_ui::current();
    let theme = style.theme;
    Application::new()
        .window(WindowOptions {
            title: "Terminal".into(),
            width: 1100.,
            height: 700.,
            min_size: Some((480., 300.)),
            transparent: style.glass,
            ..Default::default()
        })
        .run(move |cx| {
            let rt = cx.ui.runtime.clone();
            let mut session = Session::default();
            session.add_workspace("tab-1".into(), "pane-1".into());
            let terms: Rc<RefCell<HashMap<String, Terminal>>> = Rc::default();
            terms.borrow_mut().insert(
                "pane-1".into(),
                Terminal::new(&rt, TerminalOptions::default()),
            );
            let app = TermApp {
                rt: rt.clone(),
                session: Rc::new(RefCell::new(session)),
                revision: rt.signal(0),
                terms,
                splits: Rc::default(),
                selected: rt.signal(String::new()),
                items: rt.signal(Vec::new()),
                status: rt.signal(String::new()),
                next_id: Rc::new(Cell::new(1)),
                accent: solyra_ui::color(theme.accent),
                hairline: style.hairline(),
            };
            app.focus_only("pane-1");

            let watching = app.clone();
            let effect = rt.effect(move || watching.sync_tabs());
            cx.on_closed(move || drop(effect));

            let on_close = {
                let app = app.clone();
                Rc::new(move |id: &str| {
                    app.close_workspace(id);
                    true
                })
            };

            let panel_app = app.clone();
            let strip = tabs(
                "Terminal tabs",
                app.items.clone(),
                app.selected.clone(),
                TabsOptions {
                    reorderable: true,
                    on_close: Some(on_close),
                },
                move |id, _cx| panel_app.panel(id),
            );

            let add = {
                let app = app.clone();
                tool_button("+", move || app.new_workspace())
            };

            let toolbar = {
                let (right, down, close, new_tab) =
                    (app.clone(), app.clone(), app.clone(), app.clone());
                let capped = {
                    let (session, revision) = (app.session.clone(), app.revision.clone());
                    move || {
                        let _ = revision.get();
                        !session.borrow().can_add_pane()
                    }
                };
                let capped_down = {
                    let (session, revision) = (app.session.clone(), app.revision.clone());
                    move || {
                        let _ = revision.get();
                        !session.borrow().can_add_pane()
                    }
                };
                let capped_tab = {
                    let (session, revision) = (app.session.clone(), app.revision.clone());
                    move || {
                        let _ = revision.get();
                        !session.borrow().can_add_pane()
                    }
                };
                row()
                    .w_full()
                    .shrink_0()
                    .gap(6.)
                    .p(6.)
                    .items_center()
                    .bg(style.sidebar_bg())
                    .child(
                        tool_button("Split right", move || right.split(SplitDir::Vertical))
                            .disabled_when(capped),
                    )
                    .child(
                        tool_button("Split down", move || down.split(SplitDir::Horizontal))
                            .disabled_when(capped_down),
                    )
                    .child(tool_button("Close pane", move || close.close_focused()))
                    .child(
                        tool_button("New tab", move || new_tab.new_workspace())
                            .disabled_when(capped_tab),
                    )
                    .child(div().grow())
                    .child(
                        text_signal({
                            let (session, revision, status) = (
                                app.session.clone(),
                                app.revision.clone(),
                                app.status.clone(),
                            );
                            move || {
                                let _ = revision.get();
                                let count = session.borrow().pane_count();
                                let hint = status.get();
                                if hint.is_empty() {
                                    format!("{count}/16")
                                } else {
                                    format!("{hint}  ·  {count}/16")
                                }
                            }
                        })
                        .text_size(12.)
                        .text_color(solyra_ui::color(theme.text_dim)),
                    )
            };

            let root = column()
                .w_full()
                .h_full()
                .bg(style.content_bg())
                .text_color(solyra_ui::color(theme.text))
                .keymap(keymap())
                .on_action({
                    let app = app.clone();
                    move |_: &SplitRight, cx| {
                        cx.prevent_default();
                        app.split(SplitDir::Vertical);
                    }
                })
                .on_action({
                    let app = app.clone();
                    move |_: &SplitDown, cx| {
                        cx.prevent_default();
                        app.split(SplitDir::Horizontal);
                    }
                })
                .on_action({
                    let app = app.clone();
                    move |_: &ClosePane, cx| {
                        cx.prevent_default();
                        app.close_focused();
                    }
                })
                .on_action({
                    let app = app.clone();
                    move |_: &NewWorkspace, cx| {
                        cx.prevent_default();
                        app.new_workspace();
                    }
                })
                .on_action({
                    let app = app.clone();
                    move |_: &NextWorkspace, cx| {
                        cx.prevent_default();
                        app.cycle(1);
                    }
                })
                .on_action({
                    let app = app.clone();
                    move |_: &PrevWorkspace, cx| {
                        cx.prevent_default();
                        app.cycle(-1);
                    }
                })
                .child(toolbar)
                .child(
                    row()
                        .grow()
                        .w_full()
                        .items_start()
                        .bg(style.sidebar_bg())
                        .child(strip.grow().h_full())
                        .child(add),
                );

            cx.render(solyra_ui::scope(
                &style,
                {
                    let w_close = cx.window.clone();
                    let w_zoom = cx.window.clone();
                    let maxed = std::rc::Rc::new(std::cell::Cell::new(false));
                    let ctl = solyra_ui::chrome::WindowCtl {
                        close: std::rc::Rc::new(move || w_close.close()),
                        zoom: std::rc::Rc::new(move || {
                            let m = !maxed.get();
                            maxed.set(m);
                            w_zoom.set_maximized(m);
                        }),
                    };
                    let bar = solyra_ui::chrome::titlebar(&cx.ui.runtime.clone(), &style, "Terminal", ctl, None);
                    column()
                        .w_full()
                        .h_full()
                        .child(bar)
                        .child(root.grow())
                },
            ));

            if self_test {
                let window = cx.window.clone();
                cx.tasks.spawn(async move {
                    zgx::timer::sleep(Duration::from_secs(2)).await;
                    window.close();
                });
            }
        })
}
