//! Solyra first-boot setup. Asks for a name, an account and a look, writes a
//! request file, and the root helper (`solyra-setup-apply`, started by
//! systemd) creates the account. Nothing here is privileged.
//!
//! Status: experimental. The wizard and the helper are tested; starting the
//! wizard on a fresh install needs a login flow that does not exist yet (see
//! docs/setup.md), so it is not started by default.
//!
//! `--self-test` opens the window for two seconds and exits 0 (CI).

use std::{os::unix::fs::OpenOptionsExt, rc::Rc, time::Duration};

use solyra_setup_core::{
    Request, validate_full_name, validate_hostname, validate_password, validate_username,
};
use zgx::{compose::prelude::*, reactive::Signal};
use zgx_components::segmented;
use zgx_desktop::{Application, WindowOptions};

/// `Signal::set` reports whether the value changed; the wizard only needs it set.
trait Put<T> {
    fn put(&self, value: T);
}

impl<T: PartialEq + 'static> Put<T> for Signal<T> {
    fn put(&self, value: T) {
        self.set(value);
    }
}

fn field_error(label: &str, result: Result<(), String>) -> Option<String> {
    result.err().map(|e| format!("{label}: {e}"))
}

/// Why the account page cannot continue yet, if so.
fn account_problem(
    username: &str,
    full: &str,
    password: &str,
    again: &str,
    host: &str,
) -> Option<String> {
    field_error("User name", validate_username(username))
        .or_else(|| field_error("Name", validate_full_name(full)))
        .or_else(|| field_error("Password", validate_password(password)))
        .or_else(|| (password != again).then(|| "The two passwords do not match.".to_string()))
        .or_else(|| field_error("Computer name", validate_hostname(host)))
}

/// Write the request with mode 0600; refuses to follow or replace anything odd.
fn write_request(r: &Request, path: &str) -> std::io::Result<()> {
    let body = serde_json::json!({
        "username": r.username, "full_name": r.full_name,
        "password": r.password, "hostname": r.hostname,
    })
    .to_string();
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    std::io::Write::write_all(&mut f, body.as_bytes())?;
    f.sync_all()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let self_test = std::env::args().any(|a| a == "--self-test");
    let style = solyra_ui::current();
    let theme = style.theme;
    Application::new()
        .window(WindowOptions { title: "Welcome".into(), width: 640., height: 560., transparent: style.glass, ..Default::default() })
        .run(move |cx| {
            let rt = cx.ui.runtime.clone();
            let step = rt.signal(0usize);
            let username = rt.signal(String::new());
            let full = rt.signal(String::new());
            let password = rt.signal(String::new());
            let again = rt.signal(String::new());
            let host = rt.signal("solyra".to_string());
            let theme_sel = rt.signal(0usize);
            // The look: top bar, notch, dock style and dock edge.
            let bar_sel = rt.signal(0usize);
            let notch_sel = rt.signal(1usize);
            let dock_sel = rt.signal(0usize);
            let pos_sel = rt.signal(0usize);
            let message = rt.signal(String::new());
            let done = rt.signal(false);

            let nav = |label: &str, f: Rc<dyn Fn()>| {
                button().px(16.).h(36.).rounded(8.).items_center().child(text(label.to_string())).on_click(move || f())
            };

            let welcome = || column()
                .gap(12.)
                .child(text("Welcome to Solyra").text_size(26.).font_bold())
                .child(text("A few questions, then your desktop. You can change all of this later in Settings."));

            let account = {
                let (full, username, password, again, host) = (full.clone(), username.clone(), password.clone(), again.clone(), host.clone());
                move || column()
                .gap(8.)
                .child(text("Your account").text_size(20.).font_bold())
                .child(text("Name"))
                .child(text_input("Full name", full.clone()).h(36.).px(10.).rounded(8.))
                .child(text("User name"))
                .child(text_input("User name", username.clone()).h(36.).px(10.).rounded(8.))
                .child(text("Password"))
                .child(text_input("Password", password.clone()).secure(true).h(36.).px(10.).rounded(8.))
                .child(text("Password again"))
                .child(text_input("Password again", again.clone()).secure(true).h(36.).px(10.).rounded(8.))
                .child(text("Computer name"))
                .child(text_input("Computer name", host.clone()).h(36.).px(10.).rounded(8.))
            };

            let look = {
                let (theme_sel, bar_sel, notch_sel, dock_sel, pos_sel) =
                    (theme_sel.clone(), bar_sel.clone(), notch_sel.clone(), dock_sel.clone(), pos_sel.clone());
                move || column()
                .gap(10.)
                .child(text("Your desktop").text_size(20.).font_bold())
                .child(text("Theme"))
                .child(segmented("Theme", theme_sel.clone(), ["Dark", "Light"]))
                .child(text("Top bar"))
                .child(segmented("Top bar", bar_sel.clone(), ["Menu bar", "Compact", "Hidden"]))
                .child(text("Notch"))
                .child(segmented("Notch", notch_sel.clone(), ["Notch", "Dynamic Island", "Off"]))
                .child(text("Dock"))
                .child(segmented("Dock", dock_sel.clone(), ["Floating", "Solyra", "Taskbar", "Minimal"]))
                .child(text("Dock position"))
                .child(segmented("Dock position", pos_sel.clone(), ["Bottom", "Left", "Right"]))
                .child(text("Everything here, and the background, can be changed later in Settings."))
            };

            let finish = {
                let (done, message) = (done.clone(), message.clone());
                move || column().gap(12.).child(text_signal({
                    let (done, message) = (done.clone(), message.clone());
                    move || {
                        if done.get() {
                            "Your account is being created. Sign in with your user name and password.".to_string()
                        } else if message.get().is_empty() {
                            "Ready to create your account.".to_string()
                        } else {
                            message.get()
                        }
                    }
                }))
            };

            let body = switch(
                {
                    let step = step.clone();
                    move || step.get()
                },
                move |s, _cx| match s {
                    0 => welcome(),
                    1 => account(),
                    2 => look(),
                    _ => finish(),
                },
            );

            let back = {
                let (step, message) = (step.clone(), message.clone());
                nav("Back", Rc::new(move || {
                    step.put(step.get().saturating_sub(1));
                    message.put(String::new());
                }))
            };
            let next = {
                let (step, message, username, full, password, again, host, theme_sel, done) = (
                    step.clone(), message.clone(), username.clone(), full.clone(), password.clone(),
                    again.clone(), host.clone(), theme_sel.clone(), done.clone(),
                );
                let (bar_sel, notch_sel, dock_sel, pos_sel) =
                    (bar_sel.clone(), notch_sel.clone(), dock_sel.clone(), pos_sel.clone());
                nav("Continue", Rc::new(move || {
                    let s = step.get();
                    if s == 1 {
                        if let Some(p) = account_problem(&username.get(), &full.get(), &password.get(), &again.get(), &host.get()) {
                            message.put(p);
                            return;
                        }
                    }
                    if s == 3 {
                        if done.get() {
                            return;
                        }
                        let req = Request { username: username.get(), full_name: full.get(), password: password.get(), hostname: host.get() };
                        let path = std::env::var("SOLYRA_SETUP_REQUEST").unwrap_or_else(|_| solyra_setup_core::REQUEST_PATH.into());
                        match write_request(&req, &path) {
                            Ok(()) => {
                                let pick = |opts: &[&str], i: usize| Some(opts[i.min(opts.len() - 1)].to_string());
                                let _ = solyra_conf::config_path("shell.conf").map(|p| {
                                    solyra_conf::update(
                                        &p,
                                        &[
                                            ("theme", Some(if theme_sel.get() == 1 { "light" } else { "dark" }.into())),
                                            ("bar_style", pick(&["menubar", "compact", "hidden"], bar_sel.get())),
                                            ("notch_style", pick(&["notch", "island", "off"], notch_sel.get())),
                                            ("dock_style", pick(&["macos", "solyra", "windows", "minimal"], dock_sel.get())),
                                            ("dock_position", pick(&["bottom", "left", "right"], pos_sel.get())),
                                        ],
                                    )
                                });
                                done.put(true);
                            }
                            Err(e) => message.put(format!("Could not save your answers: {e}")),
                        }
                        return;
                    }
                    step.put((s + 1).min(3));
                    message.put(String::new());
                }))
            };

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
                    let bar = solyra_ui::chrome::titlebar(&cx.ui.runtime.clone(), &style, "Welcome", ctl, None);
                    column()
                        .w_full()
                        .h_full()
                        .child(bar)
                        .child(column()
                    .w_full()
                    .h_full()
                    .p(32.)
                    .gap(16.)
                    .bg(style.content_bg())
                    .text_color(solyra_ui::color(theme.text))
                    .child(body.grow())
                    .child(text_signal({
                        let m = message.clone();
                        move || m.get()
                    }).text_color(solyra_ui::color(theme.danger)))
                    .child(row().gap(8.).child(back).child(next)).grow())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_page_blocks_until_everything_is_valid() {
        assert!(
            account_problem("", "Ada", "correct horse", "correct horse", "h")
                .unwrap()
                .starts_with("User name")
        );
        assert!(
            account_problem("ada", "Ada", "short", "short", "h")
                .unwrap()
                .starts_with("Password")
        );
        assert!(
            account_problem("ada", "Ada", "correct horse", "different one", "h")
                .unwrap()
                .contains("do not match")
        );
        assert!(
            account_problem("ada", "Ada", "correct horse", "correct horse", "Bad Host")
                .unwrap()
                .starts_with("Computer name")
        );
        assert!(
            account_problem("ada", "Ada", "correct horse", "correct horse", "solyra").is_none()
        );
    }

    #[test]
    fn the_request_file_is_private_and_never_overwritten() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("solyra-setup-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("req.json");
        let r = Request {
            username: "ada".into(),
            full_name: "Ada".into(),
            password: "correct horse".into(),
            hostname: "h".into(),
        };
        write_request(&r, path.to_str().unwrap()).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(
            write_request(&r, path.to_str().unwrap()).is_err(),
            "never replaces an existing file"
        );
        // The helper accepts what the wizard wrote.
        let parsed =
            solyra_setup_core::parse_request(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed, r);
        let _ = std::fs::remove_dir_all(&d);
    }
}
