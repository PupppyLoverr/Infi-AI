//! org.freedesktop.Notifications D-Bus server — real notifications from real
//! clients land here and get drawn as layer-shell popups, with action buttons
//! (Approve/Deny, "Diagnose", …) round-tripping back to the sender.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicU32, Ordering},
};

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
    /// action key → label pairs from the caller (flat strings in twos).
    pub actions: Vec<(String, String)>,
    /// urgency=2 hint: bypasses Focus (approval cards must never be silenced).
    pub critical: bool,
    pub arrived: std::time::Instant,
}

/// What the daemon thread reports to the shell loop.
#[derive(Debug)]
pub enum NotifyEvent {
    Raised(Notification),
    Closed {
        id: u32,
        #[allow(dead_code)]
        reason: u32,
    },
}

struct NotifyDaemon {
    tx: Sender<NotifyEvent>,
    next_id: AtomicU32,
    /// (app_name, summary) → (id, count): identical cards merge into
    /// one — the badge count rides in the summary text. Reusing the id
    /// also lets every waiting caller resolve on the same ActionInvoked.
    pending: std::sync::Mutex<HashMap<(String, String), (u32, u32)>>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl NotifyDaemon {
    async fn get_capabilities(&self) -> Vec<String> {
        vec![
            "actions".to_string(),
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
        actions: Vec<String>,
        hints: HashMap<String, zvariant::Value<'_>>,
        expire_timeout: i32,
    ) -> u32 {
        let mut count = 1u32;
        let id = if replaces_id != 0 {
            replaces_id
        } else {
            let key = (app_name.to_string(), summary.to_string());
            let mut pending = self.pending.lock().unwrap();
            match pending.get_mut(&key) {
                Some((id, n)) => {
                    *n += 1;
                    count = *n;
                    *id
                }
                None => {
                    let id = self.next_id.fetch_add(1, Ordering::Relaxed);
                    pending.insert(key, (id, 1));
                    id
                }
            }
        };
        let summary = if count > 1 {
            format!("{} (×{})", summary, count)
        } else {
            summary.to_string()
        };
        let summary: &str = &summary;
        let pairs: Vec<(String, String)> = actions
            .chunks(2)
            .filter_map(|c| match c {
                [k, l] if !k.is_empty() && !l.is_empty() => Some((k.clone(), l.clone())),
                _ => None,
            })
            .collect();
        let critical = hints
            .get("urgency")
            .and_then(|v| u8::try_from(v.clone()).ok())
            == Some(2);
        let _ = self.tx.send(NotifyEvent::Raised(Notification {
            id,
            app_name: app_name.to_string(),
            summary: summary.to_string(),
            body: body.to_string(),
            timeout_ms: expire_timeout,
            actions: pairs,
            critical,
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
        pending: std::sync::Mutex::new(HashMap::new()),
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

/// Emit ActionInvoked + NotificationClosed(2) back to the notifying client.
pub fn emit_action(conn: &zbus::blocking::Connection, id: u32, key: &str) {
    let _ = conn.emit_signal(
        None::<&str>,
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "ActionInvoked",
        &(id, key),
    );
    emit_closed(conn, id, 2);
}

/// Emit NotificationClosed (reason: 1 expired, 2 dismissed, 3 closed by request).
pub fn emit_closed(conn: &zbus::blocking::Connection, id: u32, reason: u32) {
    let _ = conn.emit_signal(
        None::<&str>,
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "NotificationClosed",
        &(id, reason),
    );
}
