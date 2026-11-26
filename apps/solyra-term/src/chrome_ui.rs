//! The window chrome after the reference recordings (docs/ref/terminal-*.png):
//!
//! - Recording A: traffic lights, a sidebar toggle, tab pills that show one
//!   glyph per pane plus the focused pane's title, `+`, and split buttons on
//!   the right; rounded pane cards; an "Agents" sidebar.
//! - Recording B: a pane talking to a harness becomes the chat view; with a
//!   single chat pane the titlebar shows the conversation title, the folder,
//!   the session cost and the `>_` button that swaps to the harness's own TUI
//!   on the same session id (and back).

use std::rc::Rc;

use zgx::compose::prelude::*;
use zgx::compose::{Placement, PlacementEdge, PlacementSide};
use zgx::input::{EventPhase, InputEvent, Key};
use zgx::scene::Color;
use zgx::terminal::{Terminal, TerminalOptions, terminal};

use crate::chat::{self, Chat};
use crate::panes::SplitDir;
use crate::{icons, TermApp};

pub const CLAUDE_ORANGE: Color = Color(217, 119, 87, 255);
const INK: Color = Color(236, 236, 236, 255);
const INK_DIM: Color = Color(138, 138, 138, 255);
const TAB_INK: Color = Color(169, 180, 176, 255);

/// What a pane is showing.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Shell,
    Chat,
    /// A harness's own TUI ("claude", "codex").
    Tui(String),
}

impl TermApp {
    pub(crate) fn kind(&self, id: &str) -> Kind {
        if self.chats.borrow().contains_key(id) {
            Kind::Chat
        } else if let Some(h) = self.tui.borrow().get(id) {
            Kind::Tui(h.clone())
        } else {
            Kind::Shell
        }
    }

    fn short_dir(path: &str) -> String {
        let home = std::env::var("HOME").unwrap_or_default();
        if path.is_empty() || path == home {
            "~".into()
        } else if !home.is_empty() && path.starts_with(&home) {
            format!("~{}", &path[home.len()..])
        } else {
            path.to_string()
        }
    }

    fn pane_dir(&self, id: &str) -> String {
        Self::short_dir(&self.terms.borrow().get(id).map(|t| t.pwd()).unwrap_or_default())
    }

    /// The title a tab pill shows for its focused pane.
    pub(crate) fn pane_title(&self, id: &str) -> String {
        match self.kind(id) {
            Kind::Chat => self
                .chats
                .borrow()
                .get(id)
                .and_then(|c| c.history().into_iter().find(|b| b.role == "user").map(|b| b.text))
                .map(|t| t.chars().take(40).collect())
                .unwrap_or_else(|| "New session".into()),
            Kind::Tui(h) => harness_name(&h).into(),
            Kind::Shell => {
                let title = self.terms.borrow().get(id).map(|t| t.title()).unwrap_or_default();
                match title.rsplit_once(':') {
                    Some((_, dir)) if !dir.trim().is_empty() => dir.trim().to_string(),
                    _ if !title.trim().is_empty() && !title.contains('@') => title.trim().to_string(),
                    _ => self.pane_dir(id),
                }
            }
        }
    }

    // ------------------------------------------------------------ actions

    /// A new tab holding a harness: Claude Code opens in the chat view,
    /// other harnesses in their own TUI.
    pub(crate) fn new_agent_tab(&self, harness: &str) {
        self.menu_open.set(false);
        self.new_workspace();
        let Some(id) = self.session.borrow().active().map(|w| w.focused.clone()) else { return };
        if harness == "claude" {
            self.start_chat(&id, chat::Opts { mode: "auto".into(), effort: "medium".into(), ..Default::default() }, Vec::new());
        } else {
            self.open_tui(&id, harness, harness.to_string());
        }
        self.bump();
    }

    pub(crate) fn start_chat(&self, id: &str, opts: chat::Opts, seed: Vec<chat::Bubble>) -> bool {
        let cwd = self
            .terms
            .borrow()
            .get(id)
            .map(|t| t.pwd())
            .filter(|p| !p.is_empty())
            .map(std::path::PathBuf::from);
        match Chat::spawn(&self.rt, &self.tasks, cwd, opts, seed) {
            Some(c) => {
                self.chats.borrow_mut().insert(id.to_string(), Rc::new(c));
                true
            }
            None => {
                self.status.set("Claude Code is not installed (claude not on PATH)".into());
                false
            }
        }
    }

    /// Replace pane `id`'s terminal with one running `command` (a harness TUI).
    fn open_tui(&self, id: &str, harness: &str, command: String) {
        let cwd = self.terms.borrow().get(id).map(|t| t.pwd()).filter(|p| !p.is_empty()).map(std::path::PathBuf::from);
        if let Some(old) = self.terms.borrow_mut().remove(id) {
            old.close();
        }
        let term = Terminal::new(&self.rt, TerminalOptions { command: Some(command), working_directory: cwd, ..Default::default() });
        self.terms.borrow_mut().insert(id.to_string(), term);
        self.tui.borrow_mut().insert(id.to_string(), harness.to_string());
        self.focus_only(id);
    }

    /// The `>_` button: chat -> the harness's TUI on the same session id, and
    /// TUI -> back to chat, resuming that session.
    pub(crate) fn toggle_tui(&self, id: &str) {
        match self.kind(id) {
            Kind::Chat => {
                let Some(c) = self.chats.borrow_mut().remove(id) else { return };
                let mut cmd = String::from("claude");
                if !c.opts.model.is_empty() {
                    cmd.push_str(&format!(" --model {}", c.opts.model));
                }
                cmd.push_str(&format!(" --resume {}", c.session.get()));
                self.chat_opts.borrow_mut().insert(id.to_string(), (c.opts.clone(), c.session.get(), c.history()));
                drop(c);
                self.open_tui(id, "claude", cmd);
            }
            Kind::Tui(h) if h == "claude" => {
                self.tui.borrow_mut().remove(id);
                let saved = self.chat_opts.borrow_mut().remove(id);
                let (mut opts, sid, seed) = saved.unwrap_or_default();
                if !sid.is_empty() {
                    opts.resume = Some(sid);
                }
                // The pane's shell comes back underneath the chat.
                if let Some(old) = self.terms.borrow_mut().remove(id) {
                    old.close();
                }
                let shell = self.spawn(None);
                self.terms.borrow_mut().insert(id.to_string(), shell);
                self.start_chat(id, opts, seed);
            }
            _ => {}
        }
        self.bump();
    }

    /// A composer chip changed: respawn the harness with the new option on
    /// the same session, keeping the conversation on screen.
    fn respawn_chat(&self, id: &str, edit: impl FnOnce(&mut chat::Opts)) {
        let Some(old) = self.chats.borrow_mut().remove(id) else { return };
        let mut opts = old.opts.clone();
        edit(&mut opts);
        let sid = old.session.get();
        // Only resume a session the harness has written (after a turn).
        opts.resume = if old.history().iter().any(|b| b.role == "user") { Some(sid) } else { None };
        let seed = old.history();
        drop(old);
        self.start_chat(id, opts, seed);
        self.bump();
    }

    // ------------------------------------------------------------ views

    pub(crate) fn titlebar(&self, lights: View) -> View {
        let app = self.clone();
        let sw = switch(
            {
                let app = app.clone();
                move || {
                    let _ = app.revision.get();
                    let s = app.session.borrow();
                    let tabs: Vec<(String, Vec<String>, String)> =
                        s.workspaces.iter().map(|w| (w.name.clone(), w.tree.ordered(), w.focused.clone())).collect();
                    let active = s.active().map(|w| w.name.clone()).unwrap_or_default();
                    (tabs, active)
                }
            },
            move |(tabs, active), _cx| {
                let focused = tabs.iter().find(|t| t.0 == active).map(|t| t.2.clone()).unwrap_or_default();
                // One app: the same tab pills whether a pane is a shell or a
                // harness chat (the chat lives inside the glass window).
                let single_chat = false;
                let mut bar = row().w_full().h_full().gap(4.).items_center();
                if single_chat {
                    bar = bar
                        .child(text(app.pane_title(&focused)).text_size(15.).font_weight(600).text_color(INK).shrink_0())
                        .child(text(app.pane_dir(&focused).rsplit('/').next().unwrap_or("~").to_string()).text_size(13.).text_color(INK_DIM).shrink_0().pl(4.))
                        .child(div().grow());
                } else {
                    for (name, ids, fid) in &tabs {
                        bar = bar.child(app.tab_pill(name, ids, fid, *name == active));
                    }
                    bar = bar.child(app.new_tab_button()).child(div().grow());
                }
                // Session cost and the chat/TUI switch for an agent pane.
                let kind = app.kind(&focused);
                if let Some(c) = app.chats.borrow().get(&focused).cloned() {
                    bar = bar.child(text_signal(move || format!("${:.2}", c.cost.get())).text_size(13.).text_color(INK_DIM));
                }
                if matches!(kind, Kind::Chat) || kind == Kind::Tui("claude".into()) {
                    let (a, id) = (app.clone(), focused.clone());
                    bar = bar.child(icon_button(if kind == Kind::Chat { ">_" } else { "◐" }, move || a.toggle_tui(&id)));
                }
                if !single_chat {
                    let (r, d) = (app.clone(), app.clone());
                    bar = bar
                        .child(icon_button("split-columns", move || r.split(SplitDir::Vertical)).tooltip("Split right (Ctrl+Shift+D)"))
                        .child(icon_button("split-rows", move || d.split(SplitDir::Horizontal)).tooltip("Split down (Ctrl+Shift+E)"));
                }
                bar
            },
        );
        let side = self.clone();
        row()
            .w_full()
            .h(42.)
            .shrink_0()
            .px(14.)
            .gap(10.)
            .items_center()
            .window_drag()
            .child(lights)
            .child(icon_button("sidebar", move || {
                side.panel_open.set(!side.panel_open.get());
                side.bump();
            }))
            .child(sw.grow().h_full())
    }

    fn tab_pill(&self, name: &str, ids: &[String], focused: &str, on: bool) -> View {
        let mut pill = row()
            .h(28.)
            .px(8.)
            .gap(5.)
            .items_center()
            .rounded(8.)
            .bg(if on { Color(255, 255, 255, 26) } else { Color(0, 0, 0, 0) })
            .text_color(if on { Color(234, 234, 234, 255) } else { TAB_INK });
        for id in ids {
            pill = pill.child(glyph(&self.kind(id), 14.));
        }
        pill = pill.child(text(self.pane_title(focused)).text_size(13.).max_w(200.).pl(3.));
        let (app, name) = (self.clone(), name.to_string());
        button().border(0.).p(0.).bg(Color(0, 0, 0, 0)).child(pill).on_click(move || {
            let idx = app.session.borrow().workspaces.iter().position(|w| w.name == name);
            if let Some(i) = idx {
                app.session.borrow_mut().active = i;
                app.show_active();
            }
        })
    }

    fn new_tab_button(&self) -> View {
        let open = self.menu_open.clone();
        let anchor = {
            let open = open.clone();
            icon_button("plus", move || { open.set(!open.get()); })
        };
        let (t, c, x) = (self.clone(), self.clone(), self.clone());
        let have = |b: &str| std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(b).is_file()));
        let (has_claude, has_codex) = (have("claude"), have("codex"));
        popover("New tab", open.clone(), anchor)
            .placement(Placement { side: PlacementSide::Bottom, align: PlacementEdge::Start, offset: 6. })
            .p(5.)
            .rounded(12.)
            .child(
                column()
                    .w(220.)
                    .gap(2.)
                    .child(menu_item(glyph(&Kind::Shell, 14.), "New Terminal", "⌃⇧T", true, move || {
                        t.menu_open.set(false);
                        t.new_workspace();
                    }))
                    .child(menu_item(glyph(&Kind::Chat, 14.), "Claude Code", if has_claude { "chat" } else { "not installed" }, has_claude, move || c.new_agent_tab("claude")))
                    .child(menu_item(glyph(&Kind::Tui("codex".into()), 14.), "Codex", if has_codex { "terminal" } else { "not installed" }, has_codex, move || x.new_agent_tab("codex"))),
            )
    }

    /// The Agents sidebar: every harness pane, its state and folder.
    pub(crate) fn agents_sidebar(&self) -> View {
        let app = self.clone();
        switch(
            {
                let app = app.clone();
                move || {
                    let open = app.panel_open.get();
                    let _ = app.revision.get();
                    let _ = app.tick.get();
                    open
                }
            },
            move |open, _cx| {
                if !open {
                    return column().w(0.).h_full();
                }
                let s = app.session.borrow();
                let mut rows = Vec::new();
                for w in &s.workspaces {
                    for id in w.tree.ordered() {
                        let kind = app.kind(&id);
                        if kind == Kind::Shell {
                            continue;
                        }
                        let state = match app.chats.borrow().get(&id).map(|c| c.status.get()) {
                            Some(p) if p == "Writing" => "Writing",
                            Some(p) if p == "Thinking" => "Thinking",
                            _ => "Idle",
                        };
                        rows.push((w.name.clone(), id.clone(), kind, state, app.pane_dir(&id)));
                    }
                }
                drop(s);
                let mut col = column().w(236.).h_full().shrink_0().px(10.).py(6.).gap(4.).child(
                    row()
                        .px(6.)
                        .pt(4.)
                        .pb(8.)
                        .gap(6.)
                        .child(text("Agents").text_size(13.).font_weight(600).text_color(Color(201, 211, 207, 255)))
                        .child(text(rows.len().to_string()).text_size(13.).text_color(Color(127, 138, 134, 255))),
                );
                if rows.is_empty() {
                    col = col.child(text("Start one from the + menu.").text_size(12.).text_color(Color(127, 138, 134, 255)).px(6.));
                }
                for (ws, id, kind, state, dir) in rows {
                    let name = match &kind {
                        Kind::Chat => "Claude Code".to_string(),
                        Kind::Tui(h) => harness_name(h).to_string(),
                        Kind::Shell => String::new(),
                    };
                    let busy = state != "Idle";
                    let a = app.clone();
                    let card = row()
                        .w_full()
                        .gap(10.)
                        .px(8.)
                        .py(8.)
                        .rounded(10.)
                        .child(div().pt(2.).child(glyph(&kind, 18.)))
                        .child(
                            column()
                                .grow()
                                .gap(3.)
                                .child(
                                    row()
                                        .w_full()
                                        .child(text(name).text_size(14.).text_color(Color(238, 242, 240, 255)).grow())
                                        .child(text("now").text_size(11.).text_color(Color(127, 138, 134, 255))),
                                )
                                .child(
                                    row()
                                        .gap(6.)
                                        .items_center()
                                        .child(
                                            div()
                                                .w(7.)
                                                .h(7.)
                                                .rounded(4.)
                                                .border(if busy { 0. } else { 1.3 })
                                                .bg(if busy { Color(126, 231, 135, 255) } else { Color(0, 0, 0, 0) })
                                                .reactive_style(|| Styles::new().border_color(Color(141, 152, 148, 255))),
                                        )
                                        .child(text(format!("{state} · {dir}")).text_size(12.).text_color(Color(141, 152, 148, 255))),
                                ),
                        );
                    col = col.child(button().border(0.).p(0.).bg(Color(0, 0, 0, 0)).w_full().child(card).on_click(move || {
                        let idx = a.session.borrow().workspaces.iter().position(|w| w.name == ws);
                        if let Some(i) = idx {
                            let mut s = a.session.borrow_mut();
                            s.active = i;
                            if let Some(w) = s.active_mut() {
                                w.focused = id.clone();
                            }
                        }
                        a.show_active();
                    }));
                }
                // Runtime tasks (approvals) from solyra-agent, when there are any.
                if !crate::sidepanel::scan().is_empty() {
                    col = col.child(app.side_panel_view().w_full());
                }
                col
            },
        )
    }

    /// The chat view (recording B).
    pub(crate) fn chat_view(&self, c: Rc<Chat>, id: &str) -> View {
        let (bubbles_sig, rev) = (c.bubbles.clone(), c.rev.clone());
        let dir = self.pane_dir(id).rsplit('/').next().unwrap_or("~").to_string();
        let dir2 = dir.clone();
        let ask = "What should Claude do?";
        let offset = self.rt.signal(1.0e9f32);
        let scroll_offset = offset.clone();
        let log = switch(
            move || {
                let _ = rev.get();
                bubbles_sig.get()
            },
            move |bubbles, _cx| {
                scroll_offset.set(1.0e9);
                if bubbles.is_empty() {
                    return column()
                        .w_full()
                        .min_h(420.)
                        .items_center()
                        .justify_center()
                        .gap(10.)
                        .child(text(ask).text_size(22.).font_weight(600).text_color(INK))
                        .child(text(format!("in {dir}")).text_size(16.).text_color(INK_DIM));
                }
                let mut col = column().w_full().max_w(720.).gap(14.).py(26.);
                for b in bubbles {
                    col = col.child(match b.role {
                        "user" => row().w_full().justify_end().child(
                            text(b.text).text_size(16.).text_color(INK).px(16.).py(10.).rounded(16.).bg(Color(255, 255, 255, 26)).max_w(520.),
                        ),
                        "error" => text(b.text).text_size(13.).text_color(Color(255, 143, 143, 255)),
                        _ => text(b.text).text_size(15.).line_height(1.55).text_color(Color(232, 232, 232, 255)),
                    });
                }
                row().w_full().justify_center().px(24.).child(col)
            },
        );
        let status = {
            let (st, since, tick) = (c.status.clone(), c.since.clone(), self.tick.clone());
            text_signal(move || {
                let _ = tick.get();
                match st.get().as_str() {
                    "Writing" => format!("Writing… {}s", since.get().elapsed().as_secs()),
                    "Thinking" => "Thinking…".into(),
                    "" => String::new(),
                    other => other.to_string(),
                }
            })
            .text_size(14.)
            .text_color(Color(143, 143, 143, 255))
            .h(22.)
            .px(8.)
        };
        let (send, stop, busy) = (c.clone(), c.clone(), c.status.clone());
        let input = c.input.clone();
        let opts = c.opts.clone();
        let (a1, a2, a3) = (self.clone(), self.clone(), self.clone());
        let (i1, i2, i3) = (id.to_string(), id.to_string(), id.to_string());
        let model_label = {
            let (m, rep) = (opts.model.clone(), c.model.clone());
            text_signal(move || if m.is_empty() { match chat::pretty_model(&rep.get()) { s if s.is_empty() => "Default model".into(), s => s } } else { chat::label(&chat::MODELS, &m) })
        };
        let composer = column()
            .w_full()
            .rounded(18.)
            .bg(Color(0, 0, 0, 70))
            .border(1.)
            .reactive_style(|| Styles::new().border_color(Color(255, 255, 255, 30)))
            .px(16.)
            .pt(14.)
            .pb(10.)
            .gap(6.)
            .child({
                let empty = input.clone();
                let field = text_input("", input)
                    .w_full()
                    .text_size(16.)
                    .h(28.)
                    .text_color(INK)
                    .bg(Color(0, 0, 0, 0))
                    .border(0.)
                    .on_event({
                        let (send, stop) = (send.clone(), stop.clone());
                        move |cx| {
                            if cx.phase == EventPhase::Capture {
                                return;
                            }
                            if let InputEvent::KeyDown { key, .. } = &cx.event {
                                match key {
                                    Key::Enter => {
                                        send.send();
                                        cx.prevent_default();
                                    }
                                    Key::Escape => stop.stop(),
                                    _ => {}
                                }
                            }
                        }
                    })
                ;
                div()
                    .w_full()
                    .h(28.)
                    .child(field)
                    .child(
                        text_signal(move || if empty.get().is_empty() { "Ask Claude to do something".into() } else { String::new() })
                            .text_size(16.)
                            .text_color(Color(150, 160, 157, 255))
                            .absolute()
                            .top(4.)
                            .left(2.),
                    )
            })
            .child(
                row()
                    .w_full()
                    .gap(4.)
                    .items_center()
                    .child(chip(text(chat::label(&chat::MODES, &opts.mode)), move || a1.respawn_chat(&i1, |o| o.mode = chat::cycle(&chat::MODES, &o.mode))))
                    .child(chip(model_label, move || a2.respawn_chat(&i2, |o| o.model = chat::cycle(&chat::MODELS, &o.model))))
                    .child(chip(text(chat::label(&chat::EFFORTS, &opts.effort)), move || a3.respawn_chat(&i3, |o| o.effort = chat::cycle(&chat::EFFORTS, &o.effort))))
                    .child(div().grow())
                    .child(
                        button()
                            .w(30.)
                            .h(30.)
                            .rounded(15.)
                            .p(0.)
                            .border(0.)
                            .bg(Color(242, 242, 242, 255))
                            .items_center()
                            .justify_center()
                            .child(switch(move || busy.get().is_empty(), |idle, _cx| icons::icon(if idle { "arrow-up" } else { "stop" }, 17., Color(17, 17, 17, 255))))
                            .on_click(move || {
                                if !send.send() {
                                    stop.stop();
                                }
                            }),
                    ),
            );
        let empty = c.bubbles.clone();
        let body = switch(
            move || empty.get().is_empty(),
            move |is_empty, _cx| {
                if is_empty {
                    column()
                        .grow()
                        .w_full()
                        .items_center()
                        .justify_center()
                        .gap(10.)
                        .child(text(ask).text_size(22.).font_weight(600).text_color(INK))
                        .child(text(format!("in {dir2}")).text_size(16.).text_color(INK_DIM))
                } else {
                    div().grow().w_full()
                }
            },
        );
        column()
            .w_full()
            .h_full()
            .child(body.reactive_style({ let b = c.bubbles.clone(); move || if b.get().is_empty() { Styles::new().grow() } else { Styles::new().h(0.) } }))
            .child(scroll(offset).w_full().reactive_style({ let b = c.bubbles.clone(); move || if b.get().is_empty() { Styles::new().h(0.) } else { Styles::new().grow() } }).child(log))
            .child(row().w_full().justify_center().shrink_0().px(16.).pb(16.).child(column().w_full().max_w(736.).child(status).child(composer)))
    }
}

pub fn harness_name(h: &str) -> &'static str {
    match h {
        "claude" => "Claude Code",
        "codex" => "Codex",
        _ => "Agent",
    }
}

/// One glyph per pane, from zeron's icon set: a terminal for a shell, the
/// Claude mark for Claude Code, the OpenAI mark for Codex.
pub fn glyph(kind: &Kind, size: f32) -> View {
    match kind {
        Kind::Shell => icons::icon("terminal", size, Color(200, 208, 205, 255)),
        Kind::Chat => icons::icon("claude", size, CLAUDE_ORANGE),
        Kind::Tui(h) if h == "claude" => icons::icon("claude", size, CLAUDE_ORANGE),
        Kind::Tui(h) if h == "opencode" => icons::icon("opencode", size, Color(230, 230, 230, 255)),
        Kind::Tui(_) => icons::icon("openai", size, Color(230, 230, 230, 255)),
    }
}

fn icon_button(label: &str, on: impl FnMut() + 'static) -> View {
    // Icons by name; ">_" and "◐" stay text (the recordings draw them so).
    let ink = Color(182, 193, 189, 255);
    let content = match label {
        "sidebar" | "plus" | "split-columns" | "split-rows" => icons::icon(label, 16., ink),
        _ => text(label).text_size(13.).font_family("JetBrains Mono").text_color(ink),
    };
    button()
        .w(28.)
        .h(26.)
        .p(0.)
        .rounded(7.)
        .border(0.)
        .bg(Color(0, 0, 0, 0))
        .items_center()
        .justify_center()
        .child(content)
        .on_click(on)
}

fn chip(label: View, on: impl FnMut() + 'static) -> View {
    button().px(8.).py(4.).rounded(8.).border(0.).bg(Color(0, 0, 0, 0)).child(label.text_size(14.).text_color(Color(168, 168, 168, 255))).on_click(on)
}

fn menu_item(icon: View, label: &str, hint: &str, enabled: bool, on: impl FnMut() + 'static) -> View {
    let row = row()
        .w_full()
        .gap(9.)
        .px(9.)
        .py(7.)
        .items_center()
        .child(icon)
        .child(text(label).text_size(13.).grow())
        .child(text(hint).text_size(11.).text_color(INK_DIM));
    let b = button().w_full().p(0.).rounded(8.).border(0.).bg(Color(0, 0, 0, 0)).child(row);
    if enabled { b.on_click(on) } else { b.disabled_when(|| true) }
}

/// The pane body for a shell or a harness TUI.
pub fn term_view(term: &Terminal) -> View {
    terminal(term).grow()
}
