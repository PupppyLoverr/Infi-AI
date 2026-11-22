//! solyra-term: multi-workspace terminal with split panes (max 16).
//!
//! Toolbar actions (and the intended keys):
//! - Split → / Ctrl+Shift+D — split right
//! - Split ↓ / Ctrl+Shift+E — split down
//! - Close pane / Ctrl+Shift+W — close focused pane
//! - + Workspace / Ctrl+Shift+T — new workspace (if under the pane cap)
//!
//! Closing the last pane of the last workspace opens a fresh one.

use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Duration};

use zgx::{
    compose::prelude::*,
    reactive::Signal,
    scene::Color,
    terminal::{terminal, Terminal, TerminalOptions},
};
use zgx_components::{tabs, Tab, TabsOptions};
use zgx_desktop::{Application, WindowOptions};

mod panes;
mod tabs_model;

#[cfg(test)]
mod clear_tests;

use panes::{PaneTree, Session, SplitDir, MAX_PANES};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let self_test = std::env::args().any(|a| a == "--self-test");
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
            let terms: Rc<RefCell<HashMap<String, Terminal>>> = Rc::default();
            let counter = Rc::new(RefCell::new(0u32));
            let session: Rc<RefCell<Session>> = Rc::new(RefCell::new(Session::default()));
            let items: Signal<Vec<Tab>> = rt.signal(Vec::new());
            let selected = rt.signal(String::new());
            let status = rt.signal(format!("Panes 1/{MAX_PANES}"));
            // Bumped after structural edits so the active tab body rebuilds.
            let tick = rt.signal(0u32);

            let mint = {
                let (terms, counter, rt) = (terms.clone(), counter.clone(), rt.clone());
                Rc::new(move || -> String {
                    let id = {
                        let mut n = counter.borrow_mut();
                        *n += 1;
                        format!("pane-{}", *n)
                    };
                    let term = Terminal::new(&rt, TerminalOptions::default());
                    terms.borrow_mut().insert(id.clone(), term);
                    id
                })
            };

            let sync_tabs = {
                let (session, items, selected) =
                    (session.clone(), items.clone(), selected.clone());
                Rc::new(move || {
                    let snap = session.borrow();
                    let tabs: Vec<Tab> = snap
                        .workspaces
                        .iter()
                        .enumerate()
                        .map(|(i, ws)| {
                            Tab::new(
                                format!("ws-{i}"),
                                format!("{} · {}", ws.name, ws.tree.count()),
                            )
                            .closable()
                        })
                        .collect();
                    let sel = format!("ws-{}", snap.active.min(tabs.len().saturating_sub(1)));
                    drop(snap);
                    items.set(tabs);
                    selected.set(sel);
                })
            };

            let bump = {
                let (tick, status, session) = (tick.clone(), status.clone(), session.clone());
                Rc::new(move || {
                    tick.set(tick.get().wrapping_add(1));
                    status.set(format!(
                        "Panes {}/{MAX_PANES}",
                        session.borrow().pane_count()
                    ));
                })
            };

            // First workspace.
            {
                let id = mint();
                session.borrow_mut().add_workspace("Workspace 1".into(), id);
                sync_tabs();
            }

            let ensure_one = {
                let (session, mint, sync_tabs, bump) = (
                    session.clone(),
                    mint.clone(),
                    sync_tabs.clone(),
                    bump.clone(),
                );
                Rc::new(move || {
                    if session.borrow().workspaces.is_empty() {
                        let id = mint();
                        session
                            .borrow_mut()
                            .add_workspace("Workspace 1".into(), id);
                        sync_tabs();
                        bump();
                    }
                })
            };

            let do_split = {
                let (session, mint, terms, sync_tabs, bump) = (
                    session.clone(),
                    mint.clone(),
                    terms.clone(),
                    sync_tabs.clone(),
                    bump.clone(),
                );
                Rc::new(move |dir: SplitDir| {
                    if !session.borrow().can_add_pane() {
                        bump();
                        return;
                    }
                    let id = mint();
                    let ok = session.borrow_mut().split_focused(id.clone(), dir);
                    if !ok {
                        if let Some(t) = terms.borrow_mut().remove(&id) {
                            t.close();
                        }
                    }
                    sync_tabs();
                    bump();
                })
            };

            let do_new_ws = {
                let (session, mint, sync_tabs, bump) = (
                    session.clone(),
                    mint.clone(),
                    sync_tabs.clone(),
                    bump.clone(),
                );
                Rc::new(move || {
                    if !session.borrow().can_add_pane() {
                        bump();
                        return;
                    }
                    let n = session.borrow().workspaces.len() + 1;
                    let id = mint();
                    session
                        .borrow_mut()
                        .add_workspace(format!("Workspace {n}"), id);
                    sync_tabs();
                    bump();
                })
            };

            let do_close = {
                let (session, terms, sync_tabs, bump, ensure_one) = (
                    session.clone(),
                    terms.clone(),
                    sync_tabs.clone(),
                    bump.clone(),
                    ensure_one.clone(),
                );
                Rc::new(move || {
                    if let Some(id) = session.borrow_mut().close_focused() {
                        if let Some(t) = terms.borrow_mut().remove(&id) {
                            t.close();
                        }
                    }
                    ensure_one();
                    sync_tabs();
                    bump();
                })
            };

            let on_tab_close = {
                let (session, terms, sync_tabs, bump, ensure_one, selected) = (
                    session.clone(),
                    terms.clone(),
                    sync_tabs.clone(),
                    bump.clone(),
                    ensure_one.clone(),
                    selected.clone(),
                );
                Rc::new(move |id: &str| {
                    let idx = id
                        .strip_prefix("ws-")
                        .and_then(|s| s.parse::<usize>().ok());
                    let Some(idx) = idx else {
                        return true;
                    };
                    let removed_ids = {
                        let mut s = session.borrow_mut();
                        if idx >= s.workspaces.len() {
                            return true;
                        }
                        let ids = s.workspaces[idx].tree.ids();
                        s.workspaces.remove(idx);
                        if s.active >= s.workspaces.len() && !s.workspaces.is_empty() {
                            s.active = s.workspaces.len() - 1;
                        }
                        ids
                    };
                    for pid in removed_ids {
                        if let Some(t) = terms.borrow_mut().remove(&pid) {
                            t.close();
                        }
                    }
                    ensure_one();
                    sync_tabs();
                    bump();
                    let _ = selected.get();
                    true
                })
            };

            // Selecting a tab updates session.active.
            {
                let (session, selected, bump) =
                    (session.clone(), selected.clone(), bump.clone());
                let effect = rt.effect(move || {
                    let sel = selected.get();
                    if let Some(idx) = sel
                        .strip_prefix("ws-")
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        let mut s = session.borrow_mut();
                        if idx < s.workspaces.len() && s.active != idx {
                            s.active = idx;
                            drop(s);
                            bump();
                        }
                    }
                });
                cx.on_closed(move || drop(effect));
            }

            let toolbar = row()
                .w_full()
                .gap(8.)
                .px(10.)
                .py(8.)
                .items_center()
                .bg(style.sidebar_bg())
                .child(
                    button()
                        .px(12.)
                        .h(30.)
                        .rounded(8.)
                        .child(text("Split →"))
                        .on_click({
                            let do_split = do_split.clone();
                            move || do_split(SplitDir::Vertical)
                        }),
                )
                .child(
                    button()
                        .px(12.)
                        .h(30.)
                        .rounded(8.)
                        .child(text("Split ↓"))
                        .on_click({
                            let do_split = do_split.clone();
                            move || do_split(SplitDir::Horizontal)
                        }),
                )
                .child(
                    button()
                        .px(12.)
                        .h(30.)
                        .rounded(8.)
                        .child(text("Close pane"))
                        .on_click({
                            let do_close = do_close.clone();
                            move || do_close()
                        }),
                )
                .child(
                    button()
                        .px(12.)
                        .h(30.)
                        .rounded(8.)
                        .child(text("+ Workspace"))
                        .on_click({
                            let do_new_ws = do_new_ws.clone();
                            move || do_new_ws()
                        }),
                )
                .child(
                    text_signal({
                        let status = status.clone();
                        move || status.get()
                    })
                    .text_size(12.)
                    .opacity(0.7),
                );

            let panel_terms = terms.clone();
            let panel_session = session.clone();
            let panel_tick = tick.clone();
            let panel_theme = theme;
            let strip = tabs(
                "Terminal workspaces",
                items.clone(),
                selected.clone(),
                TabsOptions {
                    reorderable: false,
                    on_close: Some(on_tab_close),
                },
                move |_id, _cx| {
                    // Depend on tick so splits rebuild the body.
                    let _ = panel_tick.get();
                    let snap = panel_session.borrow();
                    let Some(ws) = snap.active() else {
                        return text("No workspace").into();
                    };
                    let tree = ws.tree.clone();
                    let focused = ws.focused.clone();
                    drop(snap);
                    render_tree(
                        &tree,
                        &focused,
                        panel_terms.clone(),
                        panel_session.clone(),
                        panel_tick.clone(),
                        panel_theme,
                    )
                },
            );

            cx.render(solyra_ui::scope(
                &style,
                column()
                    .w_full()
                    .h_full()
                    .bg(style.content_bg())
                    .text_color(solyra_ui::color(theme.text))
                    .child(toolbar)
                    .child(
                        row()
                            .grow()
                            .w_full()
                            .items_start()
                            .bg(style.sidebar_bg())
                            .child(strip.grow().h_full()),
                    ),
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

fn render_tree(
    tree: &PaneTree,
    focused: &str,
    terms: Rc<RefCell<HashMap<String, Terminal>>>,
    session: Rc<RefCell<Session>>,
    tick: Signal<u32>,
    theme: solyra_tokens::Theme,
) -> View {
    match tree {
        PaneTree::Leaf { id } => {
            let id = id.clone();
            let is_focus = id == focused;
            let accent = solyra_ui::color(theme.accent);
            let idle = Color(255, 255, 255, 28);
            column()
                .grow()
                .w_full()
                .rounded(6.)
                .border_color(if is_focus { accent } else { idle })
                .border(1.)
                .child({
                    let terms = terms.clone();
                    let id_click = id.clone();
                    let session = session.clone();
                    let tick = tick.clone();
                    button()
                        .w_full()
                        .h(22.)
                        .child(text(if is_focus {
                            format!("● {id}")
                        } else {
                            format!("○ {id}")
                        }).text_size(11.))
                        .on_click(move || {
                            if let Some(ws) = session.borrow_mut().active_mut() {
                                ws.focused = id_click.clone();
                            }
                            tick.set(tick.get().wrapping_add(1));
                        })
                })
                .child({
                    match terms.borrow().get(&id) {
                        Some(term) => {
                            if is_focus {
                                term.focus();
                            }
                            terminal(term).grow().rounded(4.)
                        }
                        None => text("closed").into(),
                    }
                })
        }
        PaneTree::Split { dir, a, b, .. } => {
            let left = render_tree(a, focused, terms.clone(), session.clone(), tick.clone(), theme);
            let right = render_tree(b, focused, terms, session, tick, theme);
            match dir {
                SplitDir::Vertical => row()
                    .grow()
                    .w_full()
                    .gap(4.)
                    .child(left.grow())
                    .child(right.grow()),
                SplitDir::Horizontal => column()
                    .grow()
                    .w_full()
                    .gap(4.)
                    .child(left.grow())
                    .child(right.grow()),
            }
        }
    }
}
