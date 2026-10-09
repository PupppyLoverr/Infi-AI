//! cosmos-greeter — the greetd greeter for CosmosOS.
//!
//! Runs fullscreen under cage on tty1 (greetd's `command`). Draws the same
//! centered-card UI as cosmos-lock and talks greetd's IPC over $GREETD_SOCK:
//! CreateSession → answer the PAM auth messages → StartSession with
//! `cosmos-session` → exit; greetd then launches the real session on the
//! same seat. IPC runs on a worker thread so the card stays live while
//! PAM does its auth-delay dance.

use std::os::unix::net::UnixStream;
use std::sync::mpsc::{Receiver, channel};

use anyhow::{Context as _, Result};
use egui::{Align2, Color32, FontId, Key, RichText, Vec2};
use greetd_ipc::{AuthMessageType, Request, Response, codec::SyncCodec};

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
    /// Active wallpaper, blurred once and painted fullscreen.
    wallpaper: Option<egui::TextureHandle>,
}

/// Load + blur the session's active wallpaper for the login backdrop.
fn load_wallpaper(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()));
    let cfg = std::fs::read_to_string(home.join(".config/cosmos/config.json")).unwrap_or_default();
    let name = serde_json_lenient(&cfg, "wallpaper").unwrap_or_else(|| "violet".to_string());
    let dark = serde_json_lenient(&cfg, "appearance").as_deref() != Some("light");
    let name = cosmos_ipc::wallpaper_for(&name, dark);
    let dir = std::env::var("COSMOS_WALLPAPER_DIR")
        .unwrap_or_else(|_| "/usr/share/cosmos/wallpapers".to_string());
    for res in ["1920x1080", "3840x2160"] {
        let p = std::path::Path::new(&dir).join(format!("{name}-{res}.png"));
        if let Ok(img) = image::open(&p) {
            let rgba = image::imageops::blur(&img.to_rgba8(), 20.0);
            let (w, h) = rgba.dimensions();
            let ci =
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
            return Some(ctx.load_texture("wallpaper", ci, Default::default()));
        }
    }
    None
}

/// Pull `"key": "value"` out of the config without a JSON dep.
fn serde_json_lenient(text: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    let i = text.find(&pat)? + pat.len();
    let rest = text[i..].trim_start_matches([' ', ':', '\t']);
    rest.strip_prefix('"')?
        .split('"')
        .next()
        .map(str::to_string)
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
        let sock =
            std::env::var("GREETD_SOCK").context("GREETD_SOCK not set — not under greetd")?;
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
                        // pam_systemd reads these when greetd opens the
                        // session, so logind registers a graphical
                        // wayland user session (loginctl lock-session
                        // then reaches it).
                        env: vec![
                            "XDG_SESSION_TYPE=wayland".into(),
                            "XDG_SESSION_CLASS=user".into(),
                            "XDG_SESSION_DESKTOP=cosmos".into(),
                            "XDG_CURRENT_DESKTOP=cosmos".into(),
                        ],
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
                greeter.error = Some(if msg.is_empty() {
                    "Sign in failed".into()
                } else {
                    msg
                });
            }
            AuthOutcome::Failed(msg) => {
                greeter.error = Some(msg);
            }
        }
    }

    let rect = ui.max_rect();
    if greeter.wallpaper.is_none() {
        greeter.wallpaper = load_wallpaper(ui.ctx());
    }
    if let Some(tex) = &greeter.wallpaper {
        ui.painter().image(
            tex.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    // Legibility scrim over the blurred wallpaper.
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::ZERO,
        Color32::from_black_alpha(140),
    );

    // Large clock above the card.
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let (hh, mm) = (now.hour(), now.minute());
    let date = format!("{}, {} {}", now.weekday(), now.month(), now.day());
    ui.painter().text(
        egui::Pos2::new(rect.center().x, rect.height() * 0.18),
        Align2::CENTER_CENTER,
        format!("{hh:02}:{mm:02}"),
        FontId::proportional(64.0),
        Color32::WHITE,
    );
    ui.painter().text(
        egui::Pos2::new(rect.center().x, rect.height() * 0.18 + 46.0),
        Align2::CENTER_CENTER,
        date,
        FontId::proportional(15.0),
        Color32::from_white_alpha(170),
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
                        // Round avatar with the user's initial, not '>'.
                        let initial = greeter
                            .user
                            .chars()
                            .next()
                            .map(|c| c.to_ascii_uppercase())
                            .unwrap_or('?');
                        painter.text(
                            c,
                            Align2::CENTER_CENTER,
                            initial.to_string(),
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

                        let label = if greeter.working {
                            "Signing in…"
                        } else {
                            "Sign in"
                        };
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
        wallpaper: None,
        working: false,
        error: None,
        rx,
    };
    cosmos_uitk::run("Cosmos", "cosmos-greeter", (480, 360), move |ui| {
        draw_greeter(ui, &mut greeter);
    })
}
