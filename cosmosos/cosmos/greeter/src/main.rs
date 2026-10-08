//! cosmos-greeter — the greetd greeter for CosmosOS.
//!
//! Runs fullscreen under cage on tty1 (greetd's `command`). Draws the same
//! centered-card UI as cosmos-lock and talks greetd's IPC over $GREETD_SOCK:
//! CreateSession → answer the PAM auth messages → StartSession with
//! `cosmos-session` → exit; greetd then launches the real session on the
//! same seat. IPC runs on a worker thread so the card stays live while
//! PAM does its auth-delay dance.

use std::os::unix::net::UnixStream;
use std::sync::mpsc::{channel, Receiver};

use anyhow::{Context as _, Result};
use egui::{Align2, Color32, FontId, Key, RichText, Vec2};
use greetd_ipc::{codec::SyncCodec, AuthMessageType, Request, Response};

/// Result of one sign-in attempt on the IPC worker thread.
enum AuthOutcome {
    /// Authenticated and session command accepted — exit and let greetd
    /// launch the session.
    Started,
    /// Authentication failed (wrong password, unknown user, …).
    Rejected(String),
    /// IPC/PAM plumbing broke.
    Failed(String),
}

struct Greeter {
    user: String,
    password: String,
    working: bool,
    error: Option<String>,
    rx: Receiver<AuthOutcome>,
}

fn session_cmd() -> Vec<String> {
    std::env::var("COSMOS_SESSION_CMD")
        .map(|s| vec!["/bin/sh".into(), "-c".into(), s])
        .unwrap_or_else(|_| vec!["cosmos-session".into()])
}

/// One full greetd IPC conversation for a sign-in attempt. Runs off the
/// UI thread; returns the outcome to post back.
fn authenticate(user: String, password: String) -> AuthOutcome {
    let inner = || -> Result<AuthOutcome> {
        let sock = std::env::var("GREETD_SOCK").context("GREETD_SOCK not set — not under greetd")?;
        let mut stream = UnixStream::connect(sock).context("greetd socket")?;
        Request::CreateSession {
            username: user.clone(),
        }
        .write_to(&mut stream)
        .context("CreateSession")?;

        loop {
            match Response::read_from(&mut stream).context("read response")? {
                Response::AuthMessage {
                    auth_message_type,
                    auth_message,
                } => {
                    let response = match auth_message_type {
                        // "Password:" and friends — answer with the password.
                        AuthMessageType::Secret => Some(password.clone()),
                        // Visible prompts get the username back (rare in PAM's
                        // common-auth but correct per the protocol).
                        AuthMessageType::Visible => Some(user.clone()),
                        AuthMessageType::Info => None,
                        AuthMessageType::Error => None,
                    };
                    if let AuthMessageType::Error = auth_message_type {
                        return Ok(AuthOutcome::Rejected(auth_message));
                    }
                    Request::PostAuthMessageResponse { response }
                        .write_to(&mut stream)
                        .context("PostAuthMessageResponse")?;
                }
                Response::Error {
                    error_type,
                    description,
                } => {
                    let _ = Request::CancelSession.write_to(&mut stream);
                    return Ok(AuthOutcome::Rejected(format!(
                        "{error_type:?}: {description}"
                    )));
                }
                Response::Success => {
                    // Authenticated — launch the session.
                    Request::StartSession {
                        cmd: session_cmd(),
                        env: vec![],
                    }
                    .write_to(&mut stream)
                    .context("StartSession")?;
                    match Response::read_from(&mut stream).context("start response")? {
                        Response::Success => return Ok(AuthOutcome::Started),
                        Response::Error { description, .. } => {
                            let _ = Request::CancelSession.write_to(&mut stream);
                            return Ok(AuthOutcome::Rejected(description));
                        }
                        _ => return Ok(AuthOutcome::Failed("unexpected greetd reply".into())),
                    }
                }
            }
        }
    };
    inner().unwrap_or_else(|e| AuthOutcome::Failed(format!("{e:#}")))
}

fn draw_greeter(ui: &mut egui::Ui, greeter: &mut Greeter) {
    while let Ok(outcome) = greeter.rx.try_recv() {
        greeter.working = false;
        match outcome {
            AuthOutcome::Started => std::process::exit(0),
            AuthOutcome::Rejected(msg) => {
                greeter.password.clear();
                greeter.error = Some(if msg.is_empty() { "Sign in failed".into() } else { msg });
            }
            AuthOutcome::Failed(msg) => {
                greeter.error = Some(msg);
            }
        }
    }

    let rect = ui.max_rect();
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, Color32::from_black_alpha(120));

    // Wordmark top-center.
    ui.painter().text(
        egui::Pos2::new(rect.center().x, rect.height() * 0.20),
        Align2::CENTER_CENTER,
        "Cosmos",
        FontId::proportional(56.0),
        Color32::WHITE,
    );
    ui.painter().text(
        egui::Pos2::new(rect.center().x, rect.height() * 0.20 + 44.0),
        Align2::CENTER_CENTER,
        "Sign in to start your session",
        FontId::proportional(15.0),
        Color32::from_white_alpha(160),
    );

    let ctx = ui.ctx().clone();
    egui::Area::new(egui::Id::new("greeter-card"))
        .anchor(Align2::CENTER_CENTER, Vec2::new(0.0, rect.height() * 0.05))
        .show(&ctx, |ui| {
            egui::Frame::new()
                .fill(Color32::from_black_alpha(170))
                .stroke(egui::Stroke::new(1.0, Color32::from_white_alpha(30)))
                .corner_radius(16.0)
                .inner_margin(28.0)
                .show(ui, |ui| {
                    ui.set_width(300.0);
                    ui.vertical_centered(|ui| {
                        let (_r, painter) =
                            ui.allocate_painter(Vec2::splat(64.0), egui::Sense::hover());
                        let c = painter.clip_rect().center();
                        let accent = ui.visuals().hyperlink_color;
                        painter.circle_filled(c, 30.0, accent.gamma_multiply(0.3));
                        painter.circle_stroke(c, 30.0, egui::Stroke::new(1.5, accent));
                        painter.text(
                            c,
                            Align2::CENTER_CENTER,
                            ">",
                            FontId::proportional(24.0),
                            Color32::WHITE,
                        );
                        ui.add_space(12.0);

                        let user_id = egui::Id::new("greeter-user");
                        let pwd_id = egui::Id::new("greeter-pwd");
                        ui.add_sized(
                            Vec2::new(300.0, 40.0),
                            egui::TextEdit::singleline(&mut greeter.user)
                                .id(user_id)
                                .desired_width(300.0)
                                .font(FontId::proportional(15.0))
                                .hint_text("Username"),
                        );
                        ui.add_space(8.0);
                        let pwd = ui.add_sized(
                            Vec2::new(300.0, 40.0),
                            egui::TextEdit::singleline(&mut greeter.password)
                                .id(pwd_id)
                                .password(true)
                                .desired_width(300.0)
                                .font(FontId::proportional(15.0))
                                .hint_text("Password"),
                        );
                        if !ctx.memory(|m| m.has_focus(user_id) || m.has_focus(pwd_id)) {
                            ctx.memory_mut(|m| m.request_focus(pwd_id));
                        }
                        ui.add_space(10.0);

                        if let Some(err) = &greeter.error {
                            ui.label(
                                RichText::new(err.clone())
                                    .size(12.5)
                                    .color(Color32::from_rgb(255, 120, 110)),
                            );
                            ui.add_space(6.0);
                        }

                        let label = if greeter.working { "Signing in…" } else { "Sign in" };
                        let btn = ui.add_enabled(
                            !greeter.working,
                            egui::Button::new(RichText::new(label).size(15.0))
                                .min_size(Vec2::new(300.0, 38.0)),
                        );

                        // Submit on Enter whether egui's singleline
                        // TextEdit releases focus on the key (lost_focus)
                        // or keeps it (has_focus) — versions differ.
                        let enter = ui.input(|i| i.key_pressed(Key::Enter))
                            && (pwd.lost_focus() || ctx.memory(|m| m.has_focus(pwd_id)));
                        if (btn.clicked() || enter)
                            && !greeter.working
                            && !greeter.password.is_empty()
                        {
                            greeter.working = true;
                            greeter.error = None;
                            let (tx, rx) = channel();
                            greeter.rx = rx;
                            let user = greeter.user.clone();
                            let pass = greeter.password.clone();
                            std::thread::spawn(move || {
                                let _ = tx.send(authenticate(user, pass));
                            });
                        }
                        if greeter.working {
                            ui.ctx().request_repaint();
                        }
                    });
                });
        });
}

fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let (_tx, rx) = channel();
    let mut greeter = Greeter {
        user: std::env::var("COSMOS_GREETER_USER").unwrap_or_else(|_| "cosmos".into()),
        password: String::new(),
        working: false,
        error: None,
        rx,
    };
    cosmos_uitk::run("Cosmos", "cosmos-greeter", (480, 360), move |ui| {
        draw_greeter(ui, &mut greeter);
    })
}
