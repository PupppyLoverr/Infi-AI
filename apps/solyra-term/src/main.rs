//! solyra-term: the Solyra terminal. zgx draws it; tabs come from
//! `zgx-components`. `--self-test` renders for two seconds and exits 0, so CI
//! can tell a crash from a working window.

use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Duration};

use zgx::{
    compose::prelude::*,
    terminal::{Terminal, TerminalOptions, terminal},
};
use zgx_components::{Tab, TabsOptions, tabs};
use zgx_desktop::{Application, WindowOptions};

mod tabs_model;

#[cfg(test)]
mod clear_tests;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let self_test = std::env::args().any(|a| a == "--self-test");
    let style = solyra_ui::current();
    let theme = style.theme;
    Application::new()
        .window(WindowOptions {
            title: "Terminal".into(),
            width: 960.,
            height: 600.,
            min_size: Some((480., 300.)),
            transparent: style.glass,
            ..Default::default()
        })
        .run(move |cx| {
            let rt = cx.ui.runtime.clone();
            let items: zgx::reactive::Signal<Vec<Tab>> = rt.signal(Vec::new());
            let selected = rt.signal(String::new());
            let terms: Rc<RefCell<HashMap<String, Terminal>>> = Rc::default();
            let counter = Rc::new(RefCell::new(0u32));

            let open_tab = {
                let (items, selected, terms, counter, rt) = (
                    items.clone(),
                    selected.clone(),
                    terms.clone(),
                    counter.clone(),
                    rt.clone(),
                );
                Rc::new(move || {
                    let id = {
                        let mut n = counter.borrow_mut();
                        *n += 1;
                        format!("tab-{}", *n)
                    };
                    // New tabs start where the selected one is, as shells do elsewhere.
                    let cwd = terms
                        .borrow()
                        .get(&selected.get())
                        .map(|t| t.pwd())
                        .filter(|p| !p.is_empty())
                        .map(std::path::PathBuf::from);
                    let term = Terminal::new(
                        &rt,
                        TerminalOptions {
                            working_directory: cwd,
                            ..Default::default()
                        },
                    );
                    terms.borrow_mut().insert(id.clone(), term);
                    let label = tabs_model::label(&id, "");
                    items.update(|v| v.push(Tab::new(id.clone(), label).closable()));
                    selected.set(id);
                })
            };
            open_tab();

            // Keep each tab's label in step with its shell's title / program.
            {
                let (items, terms) = (items.clone(), terms.clone());
                let effect = rt.effect(move || {
                    let current = items.get();
                    let relabelled: Vec<Tab> = current
                        .iter()
                        .map(|t| {
                            let name = terms
                                .borrow()
                                .get(&t.id)
                                .map(|term| {
                                    let title = term.title();
                                    if title.is_empty() {
                                        term.process()
                                    } else {
                                        title
                                    }
                                })
                                .unwrap_or_default();
                            Tab {
                                label: tabs_model::label(&t.id, &name),
                                ..t.clone()
                            }
                        })
                        .collect();
                    if relabelled
                        .iter()
                        .zip(&current)
                        .any(|(a, b)| a.label != b.label)
                    {
                        items.set(relabelled);
                    }
                });
                // The effect lives as long as the window.
                cx.on_closed(move || drop(effect));
            }

            let on_close = {
                let (terms, items, open_tab) = (terms.clone(), items.clone(), open_tab.clone());
                Rc::new(move |id: &str| {
                    if let Some(t) = terms.borrow_mut().remove(id) {
                        t.close();
                    }
                    // Closing the last tab opens a fresh one: the window is never empty.
                    if items.get().len() <= 1 {
                        open_tab();
                    }
                    true
                })
            };

            let panel_terms = terms.clone();
            let strip = tabs(
                "Terminal tabs",
                items.clone(),
                selected.clone(),
                TabsOptions {
                    reorderable: true,
                    on_close: Some(on_close),
                },
                move |id, _cx| match panel_terms.borrow().get(&id) {
                    Some(term) => {
                        term.focus();
                        terminal(term).grow().rounded(8.)
                    }
                    None => text("closed"),
                },
            );

            let add = button().child(text("+")).px(12.).on_click({
                let open_tab = open_tab.clone();
                move || open_tab()
            });

            cx.render(solyra_ui::scope(
                &style,
                column()
                    .w_full()
                    .h_full()
                    .bg(style.content_bg())
                    .text_color(solyra_ui::color(theme.text))
                    .child(
                        // The tabs component holds the strip and the active
                        // terminal, so this row must take the window's height.
                        row()
                            .grow()
                            .w_full()
                            .items_start()
                            .bg(style.sidebar_bg())
                            .child(strip.grow().h_full())
                            .child(add),
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
