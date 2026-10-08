//! cosmos-portal — the CosmosOS xdg-desktop-portal backend.
//!
//! Implements `org.freedesktop.impl.portal.Screenshot` on the
//! `org.freedesktop.impl.portal.desktop.cosmos` session-bus name so the
//! portal frontend (`xdg-desktop-portal`) can serve
//! `org.freedesktop.portal.Screenshot` to sandboxed and unsandboxed
//! apps alike. Captures go through the compositor IPC `Screenshot`
//! request (real GL readback of the live desktop — works on every
//! backend, llvmpipe included).
//!
//! The impl-side contract: each method gets a `handle` object path
//! owned by the caller, we publish an
//! `org.freedesktop.impl.portal.Request` there, do the work, then emit
//! that object's `Response` signal carrying the result dict. `Close`
//! cancels.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tracing::{error, info, warn};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, Value};
use zbus::{connection, interface};

/// A request handle the frontend handed us. `Close` lets the caller
/// cancel an in-flight operation; we flip the flag the worker checks.
struct PortalRequest {
    closed: Arc<Mutex<bool>>,
}

#[interface(name = "org.freedesktop.impl.portal.Request")]
impl PortalRequest {
    async fn close(&self) {
        if let Ok(mut flag) = self.closed.lock() {
            *flag = true;
        }
    }

    #[zbus(signal)]
    async fn response(
        signal_emitter: &SignalEmitter<'_>,
        response: u32,
        results: HashMap<String, Value<'_>>,
    ) -> zbus::Result<()>;
}

/// Register a `PortalRequest` at `handle`, run `work`, then emit
/// `Response` and drop the object.
async fn run_request<F, Fut>(
    conn: &zbus::Connection,
    handle: ObjectPath<'_>,
    work: F,
) -> zbus::Result<()>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut:
        std::future::Future<Output = Result<(u32, HashMap<String, Value<'static>>), String>> + Send,
{
    let closed = Arc::new(Mutex::new(false));
    let request = PortalRequest {
        closed: closed.clone(),
    };
    let path = handle.as_str().to_owned();
    conn.object_server().at(handle, request).await?;

    let conn2 = conn.clone();
    tokio::spawn(async move {
        let (response_code, results) = match work().await {
            Ok((code, r)) => (code, r),
            Err(message) => {
                warn!("portal request failed: {message}");
                let mut r = HashMap::new();
                r.insert("error".to_string(), Value::new(message));
                (1, r)
            }
        };
        let is_closed = closed.lock().map(|f| *f).unwrap_or(false);
        let response_code = if is_closed { 2 } else { response_code };
        match conn2
            .object_server()
            .interface::<_, PortalRequest>(path.as_str())
            .await
        {
            Ok(iface) => {
                let emitter = iface.signal_emitter();
                if let Err(e) = emit_response(&emitter, response_code, results).await {
                    error!("emitting portal Response failed: {e}");
                }
            }
            Err(e) => error!("request object {path} vanished before response: {e}"),
        }
        let _ = conn2
            .object_server()
            .remove::<PortalRequest, _>(path.as_str())
            .await;
    });
    Ok(())
}

async fn emit_response(
    emitter: &SignalEmitter<'_>,
    response: u32,
    results: HashMap<String, Value<'_>>,
) -> zbus::Result<()> {
    PortalRequest::response(emitter, response, results).await
}

/// Blocking IPC `GetConfig` → (accent, mode).
fn ipc_config() -> Result<(String, String), String> {
    use std::io::{BufReader, BufWriter};
    let sock = cosmos_ipc::socket_path();
    let stream = std::os::unix::net::UnixStream::connect(&sock)
        .map_err(|e| format!("connect {}: {e}", sock.display()))?;
    let mut writer = BufWriter::new(
        stream
            .try_clone()
            .map_err(|e| format!("clone stream: {e}"))?,
    );
    let mut reader = BufReader::new(stream);
    cosmos_ipc::write_message(&mut writer, &cosmos_ipc::Request::GetConfig)
        .map_err(|e| format!("send get_config: {e}"))?;
    loop {
        match cosmos_ipc::read_message::<cosmos_ipc::Event, _>(&mut reader) {
            Ok(Some(cosmos_ipc::Event::Config(map))) => {
                let get = |k: &str| {
                    map.get(k)
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string()
                };
                return Ok((get("accent"), get("mode")));
            }
            Ok(Some(cosmos_ipc::Event::Error { message })) => return Err(message),
            Ok(Some(_)) => continue,
            Ok(None) => return Err("compositor closed IPC".to_string()),
            Err(e) => return Err(format!("read IPC reply: {e}")),
        }
    }
}

/// Blocking IPC round-trip: ask the compositor for a screenshot,
/// wait for the reply, copy the PNG next to it under a stable name.
fn ipc_screenshot() -> Result<String, String> {
    use std::io::{BufReader, BufWriter};
    let sock = cosmos_ipc::socket_path();
    let stream = std::os::unix::net::UnixStream::connect(&sock)
        .map_err(|e| format!("connect {}: {e}", sock.display()))?;
    let mut writer = BufWriter::new(
        stream
            .try_clone()
            .map_err(|e| format!("clone stream: {e}"))?,
    );
    let mut reader = BufReader::new(stream);

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dir = format!(
        "{}/cosmos-screenshots",
        std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string())
    );
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {dir}: {e}"))?;
    let raw = format!("{dir}/.shot-{ts}.png");
    let path = format!("{dir}/screenshot-{ts}.png");

    cosmos_ipc::write_message(
        &mut writer,
        &cosmos_ipc::Request::Screenshot { path: raw.clone() },
    )
    .map_err(|e| format!("send screenshot request: {e}"))?;

    loop {
        match cosmos_ipc::read_message::<cosmos_ipc::Event, _>(&mut reader) {
            Ok(Some(cosmos_ipc::Event::Screenshot { .. })) => {
                std::fs::rename(&raw, &path).map_err(|e| format!("rename: {e}"))?;
                return Ok(format!("file://{path}"));
            }
            Ok(Some(cosmos_ipc::Event::Error { message })) => return Err(message),
            Ok(Some(_)) => continue,
            Ok(None) => return Err("compositor closed IPC".to_string()),
            Err(e) => return Err(format!("read IPC reply: {e}")),
        }
    }
}

struct ScreenshotPortal;

#[interface(name = "org.freedesktop.impl.portal.Screenshot")]
impl ScreenshotPortal {
    async fn screenshot(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        handle: ObjectPath<'_>,
        app_id: String,
        parent_window: String,
        options: HashMap<String, Value<'_>>,
    ) -> zbus::fdo::Result<()> {
        info!("screenshot request from {app_id} ({parent_window}) {options:?}");
        run_request(conn, handle, || async {
            let uri = tokio::task::spawn_blocking(ipc_screenshot)
                .await
                .map_err(|e| format!("capture task: {e}"))??;
            let mut results = HashMap::new();
            results.insert("uri".to_string(), Value::new(uri));
            Ok((0, results))
        })
        .await
        .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn pick_color(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        handle: ObjectPath<'_>,
        app_id: String,
        parent_window: String,
        options: HashMap<String, Value<'_>>,
    ) -> zbus::fdo::Result<()> {
        info!("pick_color request from {app_id} ({parent_window}) {options:?}");
        run_request(conn, handle, || async {
            // No interactive picker yet — hand back the configured
            // accent as the "picked" color so callers get a truthful,
            // well-formed answer rather than a cancelled request.
            let (accent, mode) = ipc_config()
                .unwrap_or_else(|_| (cosmos_ipc::DEFAULT_ACCENT.to_string(), "dark".to_string()));
            let [r, g, b] = cosmos_ipc::accent_rgb(&accent, mode != "light");
            let mut results = HashMap::new();
            results.insert(
                "color".to_string(),
                // spec: color is a `(ddd)` structure, not an array
                Value::Structure((r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0).into()),
            );
            Ok((0, results))
        })
        .await
        .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
}

#[tokio::main]
async fn main() -> zbus::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let conn = connection::Builder::session()?
        .name("org.freedesktop.impl.portal.desktop.cosmos")?
        .build()
        .await?;

    conn.object_server()
        .at("/org/freedesktop/portal/desktop", ScreenshotPortal)
        .await?;
    conn.object_server()
        .at("/org/freedesktop/portal/desktop", FileChooserPortal)
        .await?;

    info!("cosmos-portal: org.freedesktop.impl.portal.desktop.cosmos up");

    std::future::pending::<()>().await;
    Ok(())
}

/// `COSMOS_CHOOSER_OPTS` encoded from portal `options`.
#[derive(Debug, Clone)]
struct ChooserOpts {
    opts: String,
    folder: Option<String>,
}

/// Pull the interesting bits out of a FileChooser `options` dict:
/// `multiple`, `directory`, `filters` (glob patterns only), and
/// `current_folder`/`current_name` for save dialogs.
fn chooser_opts(options: &HashMap<String, Value<'_>>, mode: &str) -> ChooserOpts {
    let get_bool = |k: &str| matches!(options.get(k), Some(Value::Bool(true)));
    let mut parts = vec![format!("mode={mode}")];
    if get_bool("multiple") {
        parts.push("multiple=1".into());
    }
    if get_bool("directory") {
        parts.push("directory=1".into());
    }

    // filters: a(ssas) → collect the glob strings (filtertype 0)
    if let Some(Value::Array(filters)) = options.get("filters") {
        let mut globs = Vec::new();
        for f in filters.iter() {
            if let Value::Structure(s) = f {
                let fields = s.fields();
                if let Some(Value::Array(kinds)) = fields.get(1) {
                    for k in kinds.iter() {
                        if let Value::Structure(kd) = k {
                            let kf = kd.fields();
                            // (u filtertype, s pattern) — 0 = glob
                            if matches!(kf.first(), Some(Value::U32(0))) {
                                if let Some(Value::Str(pat)) = kf.get(1) {
                                    globs.push(pat.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        if !globs.is_empty() {
            parts.push(format!("filters={}", globs.join(",")));
        }
    }

    if let Some(Value::Str(name)) = options.get("current_name") {
        parts.push(format!("name={name}"));
    }

    let folder = options.get("current_folder").and_then(|v| {
        // current_folder is ay — bytes, NUL-terminated
        match v {
            Value::Array(b) => {
                let bytes: Vec<u8> = b
                    .iter()
                    .filter_map(|x| match x {
                        Value::U8(n) => Some(*n),
                        _ => None,
                    })
                    .collect();
                let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
                String::from_utf8(bytes[..end].to_vec()).ok()
            }
            Value::Str(s) => Some(s.to_string()),
            _ => None,
        }
    });

    ChooserOpts {
        opts: parts.join(";"),
        folder,
    }
}

/// Run `cosmos-files --chooser` and turn its contract into portal
/// results: stdout lines become `uris`, exit 3 = user cancelled.
async fn run_chooser(co: ChooserOpts) -> Result<(u32, HashMap<String, Value<'static>>), String> {
    let mut cmd = tokio::process::Command::new("cosmos-files");
    cmd.arg("--chooser").env("COSMOS_CHOOSER_OPTS", co.opts);
    if let Some(folder) = co.folder {
        cmd.env("COSMOS_CHOOSER_FOLDER", folder);
    }
    let out = cmd
        .output()
        .await
        .map_err(|e| format!("spawn cosmos-files --chooser: {e}"))?;

    let mut results = HashMap::new();
    match out.status.code() {
        Some(0) => {
            let text = String::from_utf8_lossy(&out.stdout);
            let uris: Vec<String> = text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(|l| format!("file://{l}"))
                .collect();
            results.insert("uris".to_string(), Value::new(uris));
            Ok((0, results))
        }
        // 3 = Cancel pressed
        Some(3) => Ok((1, results)),
        other => Err(format!("chooser exited {other:?}")),
    }
}

struct FileChooserPortal;

#[interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl FileChooserPortal {
    async fn open_file(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        handle: ObjectPath<'_>,
        app_id: String,
        parent_window: String,
        title: String,
        options: HashMap<String, Value<'_>>,
    ) -> zbus::fdo::Result<()> {
        info!("open_file request from {app_id} ({parent_window}) {title} {options:?}");
        let co = chooser_opts(&options, "open");
        run_request(conn, handle, move || run_chooser(co))
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn save_file(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        handle: ObjectPath<'_>,
        app_id: String,
        parent_window: String,
        title: String,
        options: HashMap<String, Value<'_>>,
    ) -> zbus::fdo::Result<()> {
        info!("save_file request from {app_id} ({parent_window}) {title} {options:?}");
        let co = chooser_opts(&options, "save");
        run_request(conn, handle, move || run_chooser(co))
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn save_files(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        handle: ObjectPath<'_>,
        app_id: String,
        parent_window: String,
        title: String,
        options: HashMap<String, Value<'_>>,
    ) -> zbus::fdo::Result<()> {
        info!("save_files request from {app_id} ({parent_window}) {title} {options:?}");
        let co = chooser_opts(&options, "save");
        run_request(conn, handle, move || run_chooser(co))
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
}
