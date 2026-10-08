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
    Fut: std::future::Future<Output = Result<HashMap<String, Value<'static>>, String>> + Send,
{
    let closed = Arc::new(Mutex::new(false));
    let request = PortalRequest {
        closed: closed.clone(),
    };
    let path = handle.as_str().to_owned();
    conn.object_server().at(handle, request).await?;

    let conn2 = conn.clone();
    tokio::spawn(async move {
        let results = match work().await {
            Ok(r) => r,
            Err(message) => {
                warn!("portal request failed: {message}");
                let mut r = HashMap::new();
                r.insert("error".to_string(), Value::new(message));
                r
            }
        };
        let is_closed = closed.lock().map(|f| *f).unwrap_or(false);
        let response_code = if is_closed { 2 } else { 0 };
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
            Ok(results)
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
            Ok(results)
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

    info!("cosmos-portal: org.freedesktop.impl.portal.desktop.cosmos up");

    std::future::pending::<()>().await;
    Ok(())
}
