//! org.freedesktop.Notifications D-Bus server — real notifications from real
//! clients land here and get drawn as layer-shell popups.

use std::{collections::HashMap, sync::atomic::{AtomicU32, Ordering}};

use calloop::channel::Sender;
use zbus::zvariant;

#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    /// Milliseconds; 0 = server default, <0 = persistent.
    pub timeout_ms: i32,
    pub arrived: std::time::Instant,
}

/// What the daemon thread reports to the shell loop.
#[derive(Debug)]
pub enum NotifyEvent {
    Raised(Notification),
    Closed { id: u32, #[allow(dead_code)] reason: u32 },
}

struct NotifyDaemon {
    tx: Sender<NotifyEvent>,
    next_id: AtomicU32,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl NotifyDaemon {
    async fn get_capabilities(&self) -> Vec<String> {
        vec![
            "body".to_string(),
            "body-markup".to_string(),
            "persistence".to_string(),
        ]
    }

    async fn get_server_information(&self) -> (String, String, String, String) {
        (
            "cosmos-shell".to_string(),
            "CosmosOS".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
            "1.2".to_string(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        _app_icon: &str,
        summary: &str,
        body: &str,
        _actions: Vec<String>,
        _hints: HashMap<String, zvariant::Value<'_>>,
        expire_timeout: i32,
    ) -> u32 {
        let id = if replaces_id != 0 {
            replaces_id
        } else {
            self.next_id.fetch_add(1, Ordering::Relaxed)
        };
        let _ = self.tx.send(NotifyEvent::Raised(Notification {
            id,
            app_name: app_name.to_string(),
            summary: summary.to_string(),
            body: body.to_string(),
            timeout_ms: expire_timeout,
            arrived: std::time::Instant::now(),
        }));
        id
    }

    async fn close_notification(&self, id: u32) {
        let _ = self.tx.send(NotifyEvent::Closed { id, reason: 2 });
    }
}

/// Register on the session bus. Returns Err if another daemon owns the name.
pub fn start(tx: Sender<NotifyEvent>) -> Result<zbus::blocking::Connection, String> {
    let daemon = NotifyDaemon {
        tx,
        next_id: AtomicU32::new(1),
    };
    zbus::blocking::connection::Builder::session()
        .map_err(|e| e.to_string())?
        .name("org.freedesktop.Notifications")
        .map_err(|e| e.to_string())?
        .serve_at("/org/freedesktop/Notifications", daemon)
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())
}
