//! Solyra Browse: a small browser on zgx's WebKitGTK web view. It is an
//! addition, not a replacement: Firefox stays installed and is the default.
//!
//! `--self-test` opens a window for two seconds and exits 0 (CI).

use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Duration};

use zgx::{
    compose::prelude::*,
    input::{EventPhase, InputEvent, Key},
    reactive::Signal,
    web_view::{WebView, WebViewEvent, WebViewOptions, web_view},
};
use zgx_components::{Tab, TabsOptions, tabs};
use zgx_desktop::{Application, WindowOptions};

mod address;
mod store;

use address::{Target, resolve};
use store::Store;

#[derive(Clone)]
struct Browser {
    rt: zgx::reactive::Runtime,
    items: Signal<Vec<Tab>>,
    selected: Signal<String>,
    views: Rc<RefCell<HashMap<String, WebView>>>,
    counter: Rc<RefCell<u32>>,
    store: Rc<Store>,
    /// Shown under the toolbar: refused addresses, failed loads.
    notice: Signal<String>,
    /// Bumped on any navigation so toolbar state (star, title) refreshes.
    tick: Signal<u32>,
}

impl Browser {
    fn current(&self) -> Option<WebView> {
        self.views.borrow().get(&self.selected.get()).cloned()
    }

    fn open_tab(&self, input: Option<&str>) {
        let id = {
            let mut n = self.counter.borrow_mut();
            *n += 1;
            format!("tab-{}", *n)
        };
        let web = WebView::new(&self.rt, WebViewOptions::default());
        let (store, tick, notice, items, tab_id) = (
            self.store.clone(),
            self.tick.clone(),
            self.notice.clone(),
            self.items.clone(),
            id.clone(),
        );
        let handle = web.clone();
        let sub = web.on_event(move |ev| match ev {
            WebViewEvent::NavigationFinished { url } => {
                let _ = store.visit(url, &handle.title());
                bump(&tick);
            }
            WebViewEvent::TitleChanged { title } => {
                let label: String = if title.is_empty() {
                    "New tab".into()
                } else {
                    title.chars().take(24).collect()
                };
                items.update(|v| {
                    if let Some(t) = v.iter_mut().find(|t| t.id == tab_id) {
                        t.label = label;
                    }
                });
            }
            WebViewEvent::NavigationFailed { url, error } => {
                notice.set(format!("Could not load {url}: {error}"));
            }
            WebViewEvent::NavigationStarted { .. } => {
                notice.set(String::new()).then_some(()).unwrap_or(())
            }
            _ => {}
        });
        // Keep the subscription as long as the view lives.
        std::mem::forget(sub);

        // Downloads go to ~/Downloads, never overwriting.
        web.set_download_handler(|request, responder| {
            let Some(home) = std::env::var_os("HOME") else {
                responder.cancel();
                return;
            };
            let dir = std::path::PathBuf::from(home).join("Downloads");
            let _ = std::fs::create_dir_all(&dir);
            let name = clean_file_name(&request.name);
            responder.save_to(unique_path(&dir, &name), |_| {});
        });

        self.views.borrow_mut().insert(id.clone(), web.clone());
        self.items
            .update(|v| v.push(Tab::new(id.clone(), "New tab").closable()));
        self.selected.set(id);
        self.go(&web, input.unwrap_or(""));
    }

    fn go(&self, web: &WebView, input: &str) {
        if input.trim().is_empty() {
            web.load_html(
                store::home_html(&self.store.bookmarks(), &self.store.history()),
                Some("about:blank"),
            );
            return;
        }
        match resolve(input) {
            Target::Url(u) if u == "about:blank" => {
                web.load_html(
                    store::home_html(&self.store.bookmarks(), &self.store.history()),
                    Some("about:blank"),
                );
            }
            Target::Url(u) => web.load_url(u),
            Target::Refused(why) => self.notice.set(why).then_some(()).unwrap_or(()),
        }
    }
}

fn bump(tick: &Signal<u32>) {
    tick.update(|n| *n = n.wrapping_add(1));
}

/// A download's file name with path separators and control characters removed.
fn clean_file_name(name: &str) -> String {
    let n: String = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let n = n.trim().trim_start_matches('.');
    if n.is_empty() {
        "download".into()
    } else {
        n.chars().take(120).collect()
    }
}

fn unique_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (2..10_000)
        .map(|i| dir.join(format!("{stem}-{i}{ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

fn toolbar(b: &Browser, theme: &solyra_tokens::Theme) -> View {
    let btn = |label: &str, on: Box<dyn Fn()>| {
        button()
            .w(34.)
            .h(32.)
            .rounded(8.)
            .items_center()
            .justify_center()
            .child(text(label.to_string()))
            .on_click(on)
    };
    let back = {
        let b = b.clone();
        btn(
            "‹",
            Box::new(move || {
                if let Some(w) = b.current() {
                    w.go_back()
                }
            }),
        )
    };
    let forward = {
        let b = b.clone();
        btn(
            "›",
            Box::new(move || {
                if let Some(w) = b.current() {
                    w.go_forward()
                }
            }),
        )
    };
    let reload = {
        let b = b.clone();
        btn(
            "⟳",
            Box::new(move || {
                if let Some(w) = b.current() {
                    if w.loading() { w.stop() } else { w.reload() }
                }
            }),
        )
    };
    let new_tab = {
        let b = b.clone();
        btn("+", Box::new(move || b.open_tab(None)))
    };
    let star = {
        let b = b.clone();
        let tick = b.tick.clone();
        button()
            .w(34.)
            .h(32.)
            .rounded(8.)
            .items_center()
            .justify_center()
            .child(text_signal({
                let b = b.clone();
                move || {
                    let _ = b.tick.get();
                    let on = b.current().is_some_and(|w| b.store.is_bookmarked(&w.url()));
                    if on { "★" } else { "☆" }.to_string()
                }
            }))
            .on_click(move || {
                if let Some(w) = b.current() {
                    let _ = b.store.toggle_bookmark(&w.url(), &w.title());
                    bump(&tick);
                }
            })
    };
    let typed = b.rt.signal(String::new());
    // The address bar follows the page until the user types in it.
    {
        let (b2, typed) = (b.clone(), typed.clone());
        let effect = b.rt.effect(move || {
            let _ = b2.tick.get();
            let _ = b2.selected.get();
            if let Some(w) = b2.current() {
                let u = w.url();
                typed.set(if u == "about:blank" { String::new() } else { u });
            }
        });
        std::mem::forget(effect);
    }
    let address = {
        let (b, typed) = (b.clone(), typed.clone());
        text_input("Address or search", typed.clone())
            .grow()
            .h(34.)
            .px(12.)
            .rounded(8.)
            .bg(solyra_ui::color(theme.field))
            .on_event(move |cx| {
                // Enter in the address field goes there. (Capture phase is
                // skipped so the field has already seen the key.)
                if cx.phase == EventPhase::Capture {
                    return;
                }
                if let InputEvent::KeyDown {
                    key: Key::Enter, ..
                } = &cx.event
                {
                    if let Some(w) = b.current() {
                        b.go(&w, &typed.get());
                        w.focus();
                    }
                    cx.prevent_default();
                }
            })
    };
    row()
        .gap(6.)
        .p(8.)
        .items_center()
        .child(back)
        .child(forward)
        .child(reload)
        .child(address)
        .child(star)
        .child(new_tab)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let self_test = std::env::args().any(|a| a == "--self-test");
    let start_url = std::env::args().skip(1).find(|a| !a.starts_with("--"));
    let theme = solyra_ui::current_theme();
    Application::new()
        .window(WindowOptions {
            title: "Browse".into(),
            width: 1100.,
            height: 720.,
            min_size: Some((560., 360.)),
            ..Default::default()
        })
        .run(move |cx| {
            let rt = cx.ui.runtime.clone();
            let dir =
                store::data_dir().unwrap_or_else(|| std::env::temp_dir().join("solyra-browse"));
            let b = Browser {
                rt: rt.clone(),
                items: rt.signal(Vec::new()),
                selected: rt.signal(String::new()),
                views: Rc::default(),
                counter: Rc::default(),
                store: Rc::new(Store::new(dir)),
                notice: rt.signal(String::new()),
                tick: rt.signal(0),
            };
            b.open_tab(start_url.as_deref());

            let panel_b = b.clone();
            let strip = tabs(
                "Browser tabs",
                b.items.clone(),
                b.selected.clone(),
                TabsOptions {
                    reorderable: true,
                    on_close: Some(Rc::new({
                        let b = b.clone();
                        move |id: &str| {
                            if let Some(w) = b.views.borrow_mut().remove(id) {
                                w.close();
                            }
                            if b.items.get().len() <= 1 {
                                b.open_tab(None);
                            }
                            true
                        }
                    })),
                },
                move |id, _cx| match panel_b.views.borrow().get(&id) {
                    Some(w) => web_view(w).grow(),
                    None => text("closed"),
                },
            );

            cx.render(
                column()
                    .w_full()
                    .h_full()
                    .bg(solyra_ui::window_bg(&theme))
                    .text_color(solyra_ui::color(theme.text))
                    .child(strip)
                    .child(toolbar(&b, &theme))
                    .child(
                        row()
                            .px(12.)
                            .pb(6.)
                            .gap(8.)
                            .items_center()
                            .child(
                                text_signal({
                                    let n = b.notice.clone();
                                    move || n.get()
                                })
                                .text_color(solyra_ui::color(theme.danger))
                                .grow(),
                            )
                            .child(
                                button()
                                    .px(10.)
                                    .h(26.)
                                    .rounded(6.)
                                    .items_center()
                                    .child(text("Clear history").text_size(12.))
                                    .on_click({
                                        let b = b.clone();
                                        move || {
                                            let msg = match b.store.clear_history() {
                                                Ok(()) => "History cleared.".to_string(),
                                                Err(e) => format!("Could not clear history: {e}"),
                                            };
                                            b.notice.set(msg);
                                        }
                                    }),
                            ),
                    ),
            );

            if self_test {
                let window = cx.window.clone();
                cx.tasks.spawn(async move {
                    zgx::timer::sleep(Duration::from_secs(2)).await;
                    window.close();
                });
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_names_are_cleaned() {
        assert_eq!(clean_file_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_file_name("C:\\x\\evil.exe"), "evil.exe");
        assert_eq!(clean_file_name(".bashrc"), "bashrc");
        assert_eq!(clean_file_name(""), "download");
        assert_eq!(clean_file_name("a\nb.txt"), "ab.txt");
    }

    #[test]
    fn downloads_never_overwrite() {
        let d = std::env::temp_dir().join(format!("solyra-dl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.pdf"), "x").unwrap();
        assert_eq!(unique_path(&d, "a.pdf"), d.join("a-2.pdf"));
        assert_eq!(unique_path(&d, "b.pdf"), d.join("b.pdf"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
