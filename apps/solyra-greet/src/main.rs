//! Solyra's greetd greeter. Runs as the `greeter` user under cage on tty1,
//! collects a user name and password, and asks greetd over its IPC socket
//! to start `/usr/bin/solyra-session --session` as that user.
//!
//! greetd protocol: 4-byte little-endian length + JSON on $GREETD_SOCK.
//! create_session -> auth_message(s) -> post_auth_message_response ... ->
//! success -> start_session -> success, then we exit so the session starts.
//!
//! `--self-test` opens the window for two seconds and exits 0 (CI).

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    rc::Rc,
    sync::mpsc,
    time::Duration,
};

use zgx::{compose::prelude::*, reactive::Signal};
use zgx_desktop::{Application, WindowOptions};

/// The session the greeter asks greetd to start after login.
const SESSION: [&str; 2] = ["/usr/bin/solyra-session", "--session"];

/// `Signal::set` reports whether the value changed; we only need it set.
trait Put<T> {
    fn put(&self, value: T);
}

impl<T: PartialEq + 'static> Put<T> for Signal<T> {
    fn put(&self, value: T) {
        self.set(value);
    }
}

fn send(stream: &mut UnixStream, msg: &serde_json::Value) -> std::io::Result<()> {
    let body = msg.to_string();
    stream.write_all(&(body.len() as u32).to_le_bytes())?;
    stream.write_all(body.as_bytes())
}

fn recv(stream: &mut UnixStream) -> std::io::Result<serde_json::Value> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len)?;
    let mut body = vec![0u8; u32::from_le_bytes(len) as usize];
    stream.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Try to log in; the string is what the UI shows afterwards.
fn login(socket: &str, username: &str, password: &str) -> Result<(), String> {
    let mut stream =
        UnixStream::connect(socket).map_err(|e| format!("Cannot reach greetd: {e}"))?;
    let mut step = || -> Result<(), String> {
        send(
            &mut stream,
            &serde_json::json!({"type": "create_session", "username": username}),
        )
        .map_err(|e| e.to_string())?;
        // Answer auth messages until the session is authenticated or fails.
        for _ in 0..8 {
            let reply = recv(&mut stream).map_err(|e| e.to_string())?;
            match reply.get("type").and_then(|t| t.as_str()) {
                Some("auth_message") => {
                    let secret = reply
                        .get("auth_message_type")
                        .and_then(|t| t.as_str())
                        == Some("secret");
                    send(
                        &mut stream,
                        &serde_json::json!({
                            "type": "post_auth_message_response",
                            "response": if secret { password } else { "" },
                        }),
                    )
                    .map_err(|e| e.to_string())?;
                }
                Some("success") => return Ok(()),
                Some("error") => {
                    return Err(reply
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("Authentication failed")
                        .to_string());
                }
                _ => return Err("Unexpected reply from greetd".to_string()),
            }
        }
        Err("Too many authentication messages".to_string())
    };
    step()?;
    send(
        &mut stream,
        &serde_json::json!({
            "type": "start_session",
            "cmd": SESSION,
            "env": ["XDG_SESSION_TYPE=wayland"],
        }),
    )
    .map_err(|e| e.to_string())?;
    match recv(&mut stream).map_err(|e| e.to_string())? {
        r if r.get("type").and_then(|t| t.as_str()) == Some("success") => Ok(()),
        r => Err(r
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("greetd could not start the session")
            .to_string()),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let self_test = std::env::args().any(|a| a == "--self-test");
    let style = solyra_ui::current();
    let theme = style.theme;
    Application::new()
        .window(WindowOptions {
            title: "Solyra".into(),
            width: 480.,
            height: 420.,
            transparent: style.glass,
            ..Default::default()
        })
        .run(move |cx| {
            let rt = cx.ui.runtime.clone();
            let username = rt.signal(String::new());
            let password = rt.signal(String::new());
            let message = rt.signal(String::new());
            let busy = rt.signal(false);

            // Login results arrive from a worker thread so IPC never blocks UI.
            let (tx, rx) = mpsc::channel::<Result<(), String>>();
            {
                let (message, busy) = (message.clone(), busy.clone());
                cx.tasks.spawn(async move {
                    loop {
                        match rx.try_recv() {
                            Ok(Ok(())) => std::process::exit(0),
                            Ok(Err(e)) => {
                                message.put(e);
                                busy.put(false);
                            }
                            Err(mpsc::TryRecvError::Empty) => {}
                            Err(mpsc::TryRecvError::Disconnected) => busy.put(false),
                        }
                        zgx::timer::sleep(Duration::from_millis(50)).await;
                    }
                });
            }

            let log_in = {
                let (username, password, message, busy, tx) =
                    (username.clone(), password.clone(), message.clone(), busy.clone(), tx.clone());
                Rc::new(move || {
                    if busy.get() || username.get().is_empty() {
                        return;
                    }
                    let Some(socket) = std::env::var("GREETD_SOCK").ok() else {
                        message.put("GREETD_SOCK is not set; is this running under greetd?".into());
                        return;
                    };
                    busy.put(true);
                    message.put("Signing in…".into());
                    let (username, password, tx) =
                        (username.get(), password.get(), tx.clone());
                    std::thread::spawn(move || {
                        // A dropped receiver surfaces as Disconnected there.
                        let _ = tx.send(login(&socket, &username, &password));
                    });
                })
            };

            cx.render(solyra_ui::scope(
                &style,
                column()
                    .w_full()
                    .h_full()
                    .p(32.)
                    .gap(12.)
                    .bg(style.content_bg())
                    .text_color(solyra_ui::color(theme.text))
                    .child(text("Solyra").text_size(28.).font_bold())
                    .child(text("Sign in"))
                    .child(
                        text_input("User name", username.clone())
                            .h(36.)
                            .px(10.)
                            .rounded(8.),
                    )
                    .child(
                        text_input("Password", password.clone())
                            .secure(true)
                            .h(36.)
                            .px(10.)
                            .rounded(8.),
                    )
                    .child(
                        button()
                            .px(16.)
                            .h(36.)
                            .rounded(8.)
                            .items_center()
                            .child(text("Sign in"))
                            .on_click(move || log_in()),
                    )
                    .child(text_signal({
                        let m = message.clone();
                        move || m.get()
                    })),
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
    fn greetd_frames_are_length_prefixed_json() {
        let dir = std::env::temp_dir().join(format!("solyra-greet-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();

        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            // create_session -> a secret prompt, response -> success.
            let q = recv(&mut s).unwrap();
            assert_eq!(q["type"], "create_session");
            assert_eq!(q["username"], "ada");
            send(
                &mut s,
                &serde_json::json!({"type": "auth_message", "auth_message_type": "secret",
                    "auth_message": "Password:"}),
            )
            .unwrap();
            let q = recv(&mut s).unwrap();
            assert_eq!(q["response"], "hunter2");
            send(&mut s, &serde_json::json!({"type": "success"})).unwrap();
            let q = recv(&mut s).unwrap();
            assert_eq!(q["type"], "start_session");
            assert_eq!(q["cmd"][0], "/usr/bin/solyra-session");
            send(&mut s, &serde_json::json!({"type": "success"})).unwrap();
        });

        login(path.to_str().unwrap(), "ada", "hunter2").unwrap();
        server.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn auth_errors_surface_the_description() {
        let dir = std::env::temp_dir().join(format!("solyra-greet-err-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();

        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let _ = recv(&mut s).unwrap();
            send(
                &mut s,
                &serde_json::json!({"type": "auth_message", "auth_message_type": "secret",
                    "auth_message": "Password:"}),
            )
            .unwrap();
            let _ = recv(&mut s).unwrap();
            send(
                &mut s,
                &serde_json::json!({"type": "error", "error_type": "auth_error",
                    "description": "Bad password"}),
            )
            .unwrap();
        });

        assert_eq!(
            login(path.to_str().unwrap(), "ada", "wrong").unwrap_err(),
            "Bad password"
        );
        server.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
