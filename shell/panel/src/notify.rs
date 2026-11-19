//! A small `org.freedesktop.Notifications` server, so `notify-send` and every
//! desktop app that sends a notification reaches the notch.
//!
//! Scope, stated plainly: notifications are shown as a one-line message in the
//! island (the shell's one interruption surface) and respect Focus mode. There
//! is no history list, no action buttons and no images yet; the capability list
//! says so, so apps do not wait for them.

use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc, Mutex,
};

use dbus::{arg::PropMap, blocking::Connection};
use dbus_crossroads::Crossroads;

use crate::state::ShellCmd;

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const MAX_CHARS: usize = 90;

/// One line for the island: `App · Summary`, then the first line of the body.
pub fn island_line(app: &str, summary: &str, body: &str) -> String {
    let flat = |s: &str| -> String {
        s.chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect::<String>()
            .trim()
            .to_string()
    };
    let (app, summary) = (flat(app), flat(summary));
    let first_body = flat(body.lines().next().unwrap_or(""));
    let mut line = match (app.is_empty(), summary.is_empty()) {
        (false, false) => format!("{app} · {summary}"),
        (true, false) => summary,
        (false, true) => app,
        (true, true) => "Notification".to_string(),
    };
    if !first_body.is_empty() {
        line.push_str(" — ");
        line.push_str(&first_body);
    }
    if line.chars().count() > MAX_CHARS {
        line = line.chars().take(MAX_CHARS - 1).collect::<String>() + "…";
    }
    line
}

/// Start the server on the session bus in a background thread. A failure (no
/// session bus, or another notification daemon already owns the name) is
/// reported and the panel carries on without it.
pub fn spawn_server(queue: Arc<Mutex<Vec<ShellCmd>>>, wake: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        if let Err(err) = serve(queue, wake) {
            eprintln!("solyra-panel: notifications are not served: {err}");
        }
    });
}

fn serve(
    queue: Arc<Mutex<Vec<ShellCmd>>>,
    wake: impl Fn() + Send + 'static,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::new_session()?;
    use dbus::blocking::stdintf::org_freedesktop_dbus::RequestNameReply;
    let reply = conn.request_name(NAME, false, false, true)?;
    if reply != RequestNameReply::PrimaryOwner {
        return Err("another notification daemon is running".into());
    }
    let next_id = Arc::new(AtomicU32::new(1));
    let wake = Arc::new(Mutex::new(wake));
    let mut cr = Crossroads::new();
    let iface = cr.register(NAME, move |b| {
        b.method("GetCapabilities", (), ("capabilities",), |_, _, ()| {
            Ok((vec!["body".to_string()],))
        });
        b.method(
            "GetServerInformation",
            (),
            ("name", "vendor", "version", "spec_version"),
            |_, _, ()| {
                Ok((
                    "solyra-panel".to_string(),
                    "Solyra".to_string(),
                    env!("CARGO_PKG_VERSION").to_string(),
                    "1.2".to_string(),
                ))
            },
        );
        b.method("CloseNotification", ("id",), (), |_, _, (_id,): (u32,)| {
            Ok(())
        });
        let (queue, next_id, wake) = (queue.clone(), next_id.clone(), wake.clone());
        b.method(
            "Notify",
            (
                "app_name",
                "replaces_id",
                "app_icon",
                "summary",
                "body",
                "actions",
                "hints",
                "expire_timeout",
            ),
            ("id",),
            move |_,
                  _,
                  (app, replaces, _icon, summary, body, _actions, _hints, _expire): (
                String,
                u32,
                String,
                String,
                String,
                Vec<String>,
                PropMap,
                i32,
            )| {
                let id = if replaces != 0 {
                    replaces
                } else {
                    next_id.fetch_add(1, Ordering::Relaxed)
                };
                if let Ok(mut q) = queue.lock() {
                    // Bounded: a runaway sender cannot grow memory without limit.
                    if q.len() < 64 {
                        q.push(ShellCmd::Notify(island_line(&app, &summary, &body)));
                    }
                }
                if let Ok(w) = wake.lock() {
                    w();
                }
                Ok((id,))
            },
        );
    });
    cr.insert(PATH, &[iface], ());
    cr.serve(&conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_one_line_and_bounded() {
        assert_eq!(
            island_line("Firefox", "Download finished", "report.pdf\nsecond line"),
            "Firefox · Download finished — report.pdf"
        );
        assert_eq!(island_line("", "Hello", ""), "Hello");
        assert_eq!(island_line("App", "", ""), "App");
        assert_eq!(island_line("", "", ""), "Notification");
        let long = island_line("A", &"x".repeat(300), "");
        assert_eq!(long.chars().count(), MAX_CHARS);
        assert!(long.ends_with('…'));
    }

    /// A real round trip on a private session bus: what `notify-send` does.
    #[test]
    fn a_notification_sent_over_dbus_reaches_the_queue() {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        let Ok(mut daemon) = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
        else {
            eprintln!("skipped: no dbus-daemon");
            return;
        };
        let mut addr = String::new();
        BufReader::new(daemon.stdout.take().unwrap())
            .read_line(&mut addr)
            .unwrap();
        std::env::set_var("DBUS_SESSION_BUS_ADDRESS", addr.trim());

        let queue: Arc<Mutex<Vec<ShellCmd>>> = Arc::default();
        let woke = Arc::new(AtomicU32::new(0));
        let w = woke.clone();
        spawn_server(queue.clone(), move || {
            w.fetch_add(1, Ordering::Relaxed);
        });
        let conn = Connection::new_session().unwrap();
        let proxy = conn.with_proxy(NAME, PATH, std::time::Duration::from_secs(3));
        // The server thread needs a moment to own the name.
        let mut id = None;
        for _ in 0..50 {
            let r: Result<(u32,), _> = proxy.method_call(
                NAME,
                "Notify",
                (
                    "Firefox",
                    0u32,
                    "",
                    "Download finished",
                    "report.pdf",
                    Vec::<String>::new(),
                    PropMap::new(),
                    -1i32,
                ),
            );
            if let Ok((n,)) = r {
                id = Some(n);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let id = id.expect("the server answered Notify");
        assert!(id >= 1);
        let caps: (Vec<String>,) = proxy.method_call(NAME, "GetCapabilities", ()).unwrap();
        assert_eq!(caps.0, vec!["body"], "no actions or images are claimed");
        let q = queue.lock().unwrap();
        assert!(
            matches!(&q[0], ShellCmd::Notify(t) if t == "Firefox · Download finished — report.pdf"),
            "{q:?}"
        );
        assert!(woke.load(Ordering::Relaxed) >= 1);
        let _ = daemon.kill();
    }

    #[test]
    fn control_characters_never_reach_the_island() {
        let l = island_line("A\x1b[31m", "s\nt", "b\x07");
        assert!(!l.chars().any(char::is_control), "{l:?}");
    }
}
