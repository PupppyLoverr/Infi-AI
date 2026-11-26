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
    web_view::{WebView, WebViewOptions, web_view},
};
use zgx_components::{Tab, TabsOptions, tabs};
use zgx_desktop::{Application, Titlebar, WindowOptions};

mod chat;
mod chrome_ui;
mod icons;
mod shell_integration;
mod panes;
mod sidepanel;
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
struct ToggleSidePanel;
#[derive(Clone, Copy)]
struct ToggleChat;
#[derive(Clone, Copy)]
struct ToggleBrowser;
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
    let map = bind_char(map, 'P', CTRL_SHIFT, ToggleSidePanel);
    let map = bind_char(map, 'M', CTRL_SHIFT, ToggleChat);
        let map = bind_char(map, 'B', CTRL_SHIFT, ToggleBrowser);
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
    /// ADE side panel open; agents_rev bumps to rescan the runtime dir.
    panel_open: zgx::reactive::Signal<bool>,
    agents_rev: zgx::reactive::Signal<u32>,
    /// Panes in harness-chat mode: pane id -> live claude stream-json session.
    chats: Rc<RefCell<HashMap<String, Rc<chat::Chat>>>>,
    browsers: Rc<RefCell<HashMap<String, BrowserPane>>>,
    tasks: zgx_desktop::TaskSpawner,
    /// Panes showing a harness's own TUI: pane id -> harness.
    tui: Rc<RefCell<HashMap<String, String>>>,
    /// A chat parked while its pane shows the TUI: options, session, history.
    chat_opts: Rc<RefCell<HashMap<String, (chat::Opts, String, Vec<chat::Bubble>)>>>,
    menu_open: zgx::reactive::Signal<bool>,
    /// One-second clock for "Writing… Ns" and the sidebar.
    tick: zgx::reactive::Signal<u32>,
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
        let (command, env) = shell_integration::launch();
        Terminal::new(
            &self.rt,
            TerminalOptions {
                command,
                env,
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

    /// Switch the focused pane between raw terminal and harness chat.
    /// Entering chat spawns claude in the terminal's working directory;
    /// leaving it keeps the shell alive underneath.
    fn toggle_chat(&self) {
        let Some(id) = self.session.borrow().active().map(|w| w.focused.clone()) else {
            return;
        };
        if self.chats.borrow_mut().remove(&id).is_some() {
            self.status.set("terminal".into());
        } else {
            let cwd = self
                .terms
                .borrow()
                .get(&id)
                .map(|t| t.pwd())
                .filter(|p| !p.is_empty())
                .map(std::path::PathBuf::from);
            let _ = cwd;
            self.start_chat(&id, chat::Opts { mode: "auto".into(), effort: "medium".into(), ..Default::default() }, Vec::new());
        }
        self.bump();
    }

    /// Switch the focused pane between terminal and the built-in browser.
    /// The shell stays alive underneath, like chat mode does.
    fn toggle_browser(&self) {
        let Some(id) = self.session.borrow().active().map(|w| w.focused.clone()) else {
            return;
        };
        if self.browsers.borrow_mut().remove(&id).is_some() {
            self.status.set("terminal".into());
        } else {
            let web = WebView::new(&self.rt, WebViewOptions::default());
            web.load_url("https://github.com");
            self.browsers.borrow_mut().insert(
                id,
                BrowserPane {
                    web,
                    input: self.rt.signal(String::new()),
                },
            );
            self.status.set("browser — Ctrl+Shift+B switches back".into());
        }
        self.bump();
    }

    /// A browser pane: address row over the web view.
    fn browser_view(&self, b: BrowserPane) -> View {
        let theme = solyra_ui::current().theme;
        let input = b.input.clone();
        let web = b.web.clone();
        let go = {
            let (web, input) = (b.web.clone(), b.input.clone());
            move || {
                let mut url = input.get();
                if !url.is_empty() {
                    if !url.contains("://") {
                        url = format!("https://{url}");
                    }
                    web.load_url(url);
                }
            }
        };
        let addr = text_input("Address or URL", input.clone())
            .grow()
            .rounded(14.)
            .on_event(move |cx| {
                if cx.phase == EventPhase::Capture {
                    return;
                }
                if let InputEvent::KeyDown {
                    key: Key::Enter, ..
                } = &cx.event
                {
                    let mut url = input.get();
                    if !url.is_empty() {
                        if !url.contains("://") {
                            url = format!("https://{url}");
                        }
                        web.load_url(url);
                    }
                    cx.prevent_default();
                }
            });
        column()
            .grow()
            .w_full()
            .h_full()
            .child(
                row()
                    .w_full()
                    .shrink_0()
                    .gap(6.)
                    .px(6.)
                    .py(4.)
                    .child(addr)
                    .child(tool_button("Go", go)),
            )
            .child(web_view(&b.web).grow().w_full())
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

    /// Video-1 chat surface: bubbles over the glass, input pill at the
    /// bottom. Rebuilt when the session's rev bumps (new frame or send).
    fn pane(&self, id: &str, workspace: &str) -> View {
        let in_browser = self.browsers.borrow().get(id).cloned();
        let in_chat = self.chats.borrow().get(id).cloned();
        let body = if let Some(b) = in_browser {
            self.browser_view(b)
        } else if let Some(c) = in_chat {
            self.chat_view(c, id)
        } else {
            match self.terms.borrow().get(id).cloned() {
                Some(term) => div().grow().w_full().h_full().pl(12.).pt(10.).pr(6.).pb(6.).child(chrome_ui::term_view(&term)),
                None => text("closed"),
            }
        };
        let chat_pane = self.chats.borrow().contains_key(id);
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
            .rounded(10.)
            .border(1.)
            .overflow_hidden()
            .bg(zgx::scene::Color(0, 0, 0, if chat_pane { 52 } else { 40 }))
            .reactive_style(move || {
                let _ = revision.get();
                let s = session.borrow();
                let many = s.workspaces.iter().find(|w| w.name == watch_ws).is_some_and(|w| w.tree.ids().len() > 1);
                let on = s.workspaces.iter().any(|w| w.name == watch_ws && w.focused == watch_id);
                let _ = (accent, hairline);
                Styles::new().border_color(zgx::scene::Color(255, 255, 255, if on && many { 44 } else { 20 }))
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

    /// ADE side panel: live agent tasks with approve/deny for pending
    /// approvals (the claude/codex IDE-style surface), or the empty state.
    fn side_panel_view(&self) -> View {
        let (open_sig, rev_sig) = (self.panel_open.clone(), self.agents_rev.clone());
        let theme = solyra_ui::current().theme;
        switch(
            move || {
                let open = open_sig.get();
                let _ = rev_sig.get();
                if open { Some(sidepanel::scan()) } else { None }
            },
            move |rows, _cx| {
                let Some(rows) = rows else {
                    return column().w(0.).h_full();
                };
                let mut col = column()
                    .w(280.)
                    .h_full()
                    .shrink_0()
                    .gap(6.)
                    .p(8.)
                    .bg(solyra_ui::current().sidebar_bg())
                    .child(
                        text("Agents")
                            .text_size(14.)
                            .text_color(solyra_ui::color(theme.text)),
                    );
                if rows.is_empty() {
                    col = col.child(
                        text("No agent runtime is connected")
                            .text_size(12.)
                            .text_color(solyra_ui::color(theme.text_dim)),
                    );
                }
                for task in rows {
                    let mut card = column()
                        .w_full()
                        .gap(2.)
                        .p(8.)
                        .rounded(8.)
                        .bg(zgx::scene::Color(255, 255, 255, 14))
                        .child(
                            text(format!("{} — {}", task.name, task.status))
                                .text_size(12.)
                                .text_color(solyra_ui::color(theme.text)),
                        );
                    if !task.last_text.is_empty() {
                        card = card.child(
                            text(task.last_text.clone())
                                .text_size(11.)
                                .text_color(solyra_ui::color(theme.text_dim)),
                        );
                    }
                    if let Some((action, resource)) = &task.pending {
                        card = card.child(
                            text(format!("wants to: {action} {resource}"))
                                .text_size(11.)
                                .text_color(solyra_ui::color(theme.warning)),
                        );
                        let (id_a, id_d) = (task.id.clone(), task.id.clone());
                        card = card.child(
                            row()
                                .gap(6.)
                                .child(tool_button("Allow", move || {
                                    sidepanel::decide(&id_a, "approve");
                                }))
                                .child(tool_button("Deny", move || {
                                    sidepanel::decide(&id_d, "deny");
                                })),
                        );
                    }
                    col = col.child(card);
                }
                col
            },
        )
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

/// A browser living in one pane: the web view plus its address field.
struct BrowserPane {
    web: WebView,
    input: zgx::reactive::Signal<String>,
}

impl Clone for BrowserPane {
    fn clone(&self) -> Self {
        Self {
            web: self.web.clone(),
            input: self.input.clone(),
        }
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
            titlebar: Titlebar::Hidden,
            ..Default::default()
        })
        .run(move |cx| {
            let rt = cx.ui.runtime.clone();
            let mut session = Session::default();
            session.add_workspace("tab-1".into(), "pane-1".into());
            let terms: Rc<RefCell<HashMap<String, Terminal>>> = Rc::default();
            terms.borrow_mut().insert("pane-1".into(), {
                let (command, env) = shell_integration::launch();
                Terminal::new(&rt, TerminalOptions { command, env, ..Default::default() })
            });
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
                accent: zgx::scene::Color(255, 255, 255, 60),
                hairline: style.hairline(),
                panel_open: rt.signal(std::env::var("SOLYRA_TERM_SIDEBAR").is_ok()),
                agents_rev: rt.signal(0),
                chats: Rc::default(),
                browsers: Rc::default(),
                tasks: cx.tasks.clone(),
                tui: Rc::default(),
                chat_opts: Rc::default(),
                menu_open: rt.signal(false),
                tick: rt.signal(0),
            };
            {
                let tick = app.tick.clone();
                cx.tasks.spawn(async move {
                    loop {
                        zgx::timer::sleep(Duration::from_secs(1)).await;
                        tick.update(|n| *n = n.wrapping_add(1));
                    }
                });
            }
            // Recording reproduction for screenshots: SOLYRA_TERM_LAYOUT=split
            // opens a pane on the right with one below it, plus the sidebar.
            if std::env::var("SOLYRA_TERM_LAYOUT").as_deref() == Ok("agents") {
                // Recording A in one window: Claude Code (chat) left, two
                // shells stacked right, the Agents sidebar open.
                app.start_chat("pane-1", chat::Opts { mode: "auto".into(), effort: "medium".into(), ..Default::default() }, Vec::new());
                app.split(SplitDir::Vertical);
                app.split(SplitDir::Horizontal);
                app.panel_open.set(true);
                app.focus_only("pane-1");
                if let Some(w) = app.session.borrow_mut().active_mut() {
                    w.focused = "pane-1".into();
                }
                app.bump();
            }
            if std::env::var("SOLYRA_TERM_LAYOUT").as_deref() == Ok("split") {
                app.split(SplitDir::Vertical);
                app.split(SplitDir::Horizontal);
                app.panel_open.set(true);
            }
            app.focus_only("pane-1");
            // Verification hooks: 1 enters chat mode, 2 also sends a test
            // message once the harness is up (screenshot tests).
            if std::env::var("SOLYRA_TERM_BROWSER").is_ok() {
                app.toggle_browser();
            }
            if let Ok(mode) = std::env::var("SOLYRA_TERM_CHAT") {
                app.toggle_chat();
                if mode == "2" {
                    let app2 = app.clone();
                    cx.tasks.spawn(async move {
                        zgx::timer::sleep(Duration::from_millis(800)).await;
                        if let Some(c) = app2.chats.borrow().values().next().cloned() {
                            c.input.set("hello from the harness test".into());
                            c.send();
                        }
                    });
                }
            }

            // The agent panel rescans the runtime directory every two seconds.
            {
                let rev = app.agents_rev.clone();
                cx.tasks.spawn(async move {
                    loop {
                        zgx::timer::sleep(Duration::from_secs(2)).await;
                        rev.update(|n| *n += 1);
                    }
                });
            }
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
            let panel_app2 = app.clone();
            let root = column()
                .w_full()
                .h_full()
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
                .on_action({
                    let app = app.clone();
                    move |_: &ToggleChat, cx| {
                        cx.prevent_default();
                        app.toggle_chat();
                    }
                })
                .on_action({
                    let app = app.clone();
                    move |_: &ToggleBrowser, cx| {
                        cx.prevent_default();
                        app.toggle_browser();
                    }
                })
                .on_action({
                    let app = app.clone();
                    move |_: &ToggleSidePanel, cx| {
                        cx.prevent_default();
                        let open = !app.panel_open.get();
                        app.panel_open.set(open);
                        app.bump();
                    }
                })
                .child(
                    row()
                        .grow()
                        .w_full()
                        .items_start()
                        .child(app.agents_sidebar())
                        .child({
                            let (app, sel) = (app.clone(), app.selected.clone());
                                            switch(move || sel.get(), move |id, _cx| app.panel(id))
                                .grow()
                                .h_full()
                                .px(8.)
                                .pb(8.)
                        }),
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
                    let lights = solyra_ui::chrome::traffic_lights(&cx.ui.runtime.clone(), &style, ctl);
                    let bar = app.titlebar(lights);
                    // One frosted surface: the compositor blurs what is behind
                    // the window; this single tint sits under titlebar and
                    // panes alike (recording A), so nothing reads as a band.
                    column()
                        .w_full()
                        .h_full()
                        .rounded(14.)
                        .border(1.)
                        .reactive_style(|| Styles::new().border_color(zgx::scene::Color(255, 255, 255, 34)))
                        .bg(if style.glass { zgx::scene::Color(10, 28, 27, 150) } else { style.content_bg() })
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
