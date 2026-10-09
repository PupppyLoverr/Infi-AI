//! logind → lock bridge. `loginctl lock-session` (and logind idle/lid
//! policy) emits `Lock` on this session's login1 object; it is forwarded
//! to the compositor, which spawns the locker in the session's PAM
//! context. Every shell Lock button takes the same compositor path.

use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::OwnedObjectPath;
use zbus::MatchRule;

const LOGIN1: &str = "org.freedesktop.login1";

pub fn request_lock() -> Result<(), String> {
    let mut sock = std::os::unix::net::UnixStream::connect(cosmos_ipc::socket_path())
        .map_err(|e| format!("ipc: {e}"))?;
    cosmos_ipc::write_message(&mut sock, &cosmos_ipc::Request::Lock)
        .map_err(|e| format!("ipc: {e}"))
}

pub fn spawn_listener() {
    let spawned = std::thread::Builder::new()
        .name("logind-lock".into())
        .spawn(|| {
            if let Err(e) = listen() {
                tracing::warn!("logind lock bridge stopped: {e}");
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("logind lock bridge thread: {e}");
    }
}

fn session_path(conn: &Connection) -> zbus::Result<OwnedObjectPath> {
    let reply = match std::env::var("XDG_SESSION_ID") {
        Ok(id) => conn.call_method(
            Some(LOGIN1),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "GetSession",
            &(id,),
        )?,
        Err(_) => conn.call_method(
            Some(LOGIN1),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "GetSessionByPID",
            &(std::process::id(),),
        )?,
    };
    reply.body().deserialize()
}

fn listen() -> zbus::Result<()> {
    let conn = Connection::system()?;
    let path = session_path(&conn)?;
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(LOGIN1)?
        .interface("org.freedesktop.login1.Session")?
        .member("Lock")?
        .path(path.clone())?
        .build();
    let signals = MessageIterator::for_match_rule(rule, &conn, Some(8))?;
    tracing::info!(session = path.as_str(), "logind lock bridge: watching");
    for msg in signals {
        msg?;
        tracing::info!("logind: Lock signal — locking");
        if let Err(e) = request_lock() {
            tracing::warn!("logind lock: {e}");
        }
    }
    Ok(())
}
