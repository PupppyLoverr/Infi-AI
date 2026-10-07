//! Cosmos IPC server: a unix socket inside the compositor's calloop.
//!
//! Shell (panel/launcher) and system apps connect to
//! [`cosmos_ipc::socket_path`], send [`cosmos_ipc::Request`]s and receive
//! [`cosmos_ipc::Event`]s — either as direct replies or as broadcasts on any
//! window/workspace/config change.

use std::{
    io::Read,
    net::Shutdown,
    os::unix::net::{UnixListener, UnixStream},
    sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
};

use calloop::{generic::Generic, Interest, Mode, PostAction};
use tracing::{debug, error, info, warn};

use crate::{
    cosmos::{IpcClient, WORKSPACE_COUNT},
    state::{AnvilState, Backend},
};

/// Monotonic client ids — indices into `IpcState::clients` are unstable
/// because `ipc_broadcast` compacts dead clients.
static NEXT_CLIENT_ID: AtomicU64 = AtomicU64::new(1);

/// Register the IPC socket on the event loop. Called once from `init`.
pub fn init_ipc<BackendData: Backend + 'static>(state: &mut AnvilState<BackendData>) {
    let path = cosmos_ipc::socket_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }

    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(err) => {
            error!(?path, "failed to bind cosmos ipc socket: {err}");
            return;
        }
    };
    if let Err(err) = listener.set_nonblocking(true) {
        error!("failed to set ipc listener nonblocking: {err}");
        return;
    }

    info!(?path, "cosmos ipc listening");
    state.cosmos.ipc_socket_path = Some(path);

    let handle = state.handle.clone();
    if let Err(err) = handle.insert_source(
        Generic::new(listener, Interest::READ, Mode::Level),
        move |_, listener, state| {
            match listener.accept() {
                Ok((stream, _)) => accept_client(state, stream),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(err) => warn!("ipc accept failed: {err}"),
            }
            Ok(PostAction::Continue)
        },
    ) {
        error!("failed to register ipc listener: {err}");
    }
}

fn client_index<BackendData: Backend>(state: &AnvilState<BackendData>, id: u64) -> Option<usize> {
    state.cosmos.ipc.clients.iter().position(|c| c.id == id)
}

fn accept_client<BackendData: Backend + 'static>(
    state: &mut AnvilState<BackendData>,
    stream: UnixStream,
) {
    if let Err(err) = stream.set_nonblocking(true) {
        warn!("ipc client set_nonblocking failed: {err}");
        return;
    }
    debug!("ipc client connected");

    let id = NEXT_CLIENT_ID.fetch_add(1, AtomicOrdering::Relaxed);
    let reader_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(err) => {
            warn!("ipc stream clone failed: {err}");
            return;
        }
    };

    state.cosmos.ipc.clients.push(IpcClient {
        id,
        stream,
        subscribed: false,
        dead: false,
        read_buf: Vec::new(),
        outbox: Vec::new(),
    });

    let handle = state.handle.clone();
    // Per-client read source: when data arrives we read through the stored
    // stream, append to that client's buffer, and dispatch complete lines.
    let source = Generic::new(reader_stream, Interest::READ, Mode::Level);
    let res = handle.insert_source(source, move |_, _reader, state| client_readable(state, id));
    if let Err(err) = res {
        warn!("failed to register ipc client source: {err}");
    }
}

fn client_readable<BackendData: Backend>(
    state: &mut AnvilState<BackendData>,
    id: u64,
) -> std::io::Result<PostAction> {
    let Some(idx) = client_index(state, id) else {
        return Ok(PostAction::Remove);
    };

    let mut buf = [0u8; 4096];
    match state.cosmos.ipc.clients[idx].stream.read(&mut buf) {
        Ok(0) => {
            tracing::info!(client = id, "ipc client eof");
            mark_dead(state, id);
            Ok(PostAction::Remove)
        }
        Ok(n) => {
            state.cosmos.ipc.clients[idx]
                .read_buf
                .extend_from_slice(&buf[..n]);
            let requests = drain_lines(&mut state.cosmos.ipc.clients[idx].read_buf);
            for req in requests {
                if let Some(ev) = dispatch_request(state, id, req) {
                    let Some(idx) = client_index(state, id) else {
                        return Ok(PostAction::Remove);
                    };
                    AnvilState::<BackendData>::ipc_push(&mut state.cosmos.ipc.clients[idx], &ev);
                    if state.cosmos.ipc.clients[idx].dead {
                        return Ok(PostAction::Remove);
                    }
                }
            }
            state.ipc_flush();
            Ok(PostAction::Continue)
        }
        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => Ok(PostAction::Continue),
        Err(err) => {
            warn!(client = id, "ipc client read failed: {err}");
            mark_dead(state, id);
            Ok(PostAction::Remove)
        }
    }
}

fn mark_dead<BackendData: Backend>(state: &mut AnvilState<BackendData>, id: u64) {
    if let Some(idx) = client_index(state, id) {
        let client = &mut state.cosmos.ipc.clients[idx];
        client.dead = true;
        // EOF wakes the read source which then removes itself.
        let _ = client.stream.shutdown(Shutdown::Both);
    }
}

fn drain_lines(buf: &mut Vec<u8>) -> Vec<cosmos_ipc::Request> {
    let mut out = Vec::new();
    loop {
        let Some(pos) = buf.iter().position(|b| *b == b'\n') else {
            break;
        };
        let line: Vec<u8> = buf.drain(..=pos).collect();
        let Ok(text) = String::from_utf8(line) else {
            continue;
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        match serde_json::from_str::<cosmos_ipc::Request>(text) {
            Ok(req) => out.push(req),
            Err(err) => {
                warn!("bad ipc request `{text}`: {err}");
            }
        }
    }
    // Protocol safety: if the buffer grows unboundedly without newlines,
    // drop it rather than leak memory.
    if buf.len() > 64 * 1024 {
        buf.clear();
    }
    out
}

fn dispatch_request<BackendData: Backend>(
    state: &mut AnvilState<BackendData>,
    id: u64,
    req: cosmos_ipc::Request,
) -> Option<cosmos_ipc::Event> {
    use cosmos_ipc::{Event, Request};

    match req {
        Request::Ping => Some(Event::Pong {
            pong: cosmos_ipc::Pong {
                name: "cosmos-compositor".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
        }),
        Request::Subscribe => {
            if let Some(idx) = client_index(state, id) {
                state.cosmos.ipc.clients[idx].subscribed = true;
            }
            // Send current state immediately so the client doesn't need
            // separate ListWindows/ListWorkspaces calls.
            let windows = Event::Windows {
                windows: state.ipc_windows(),
            };
            let workspaces = Event::Workspaces {
                workspaces: state.ipc_workspaces(),
            };
            let config = Event::Config(state.cosmos.config.as_map());
            if let Some(idx) = client_index(state, id) {
                let stream = &mut state.cosmos.ipc.clients[idx].stream;
                let _ = cosmos_ipc::write_message(stream, &windows);
                let _ = cosmos_ipc::write_message(stream, &workspaces);
            }
            Some(config)
        }
        Request::ListWindows => Some(Event::Windows {
            windows: state.ipc_windows(),
        }),
        Request::ListWorkspaces => Some(Event::Workspaces {
            workspaces: state.ipc_workspaces(),
        }),
        Request::GetConfig => Some(Event::Config(state.cosmos.config.as_map())),
        Request::FocusWindow { id } => {
            let Some(window) = state.window_by_id(id) else {
                return Some(Event::Error {
                    message: format!("no such window: {id}"),
                });
            };
            state.focus_window(&window);
            None
        }
        Request::MoveWindowToWorkspace { id, workspace } => {
            if workspace as usize >= WORKSPACE_COUNT {
                return Some(Event::Error {
                    message: format!("workspace out of range: {workspace}"),
                });
            }
            let Some(window) = state.window_by_id(id) else {
                return Some(Event::Error {
                    message: format!("no such window: {id}"),
                });
            };
            state.move_window_to_workspace(&window, workspace as usize);
            None
        }
        Request::CloseWindow { id } => {
            let Some(window) = state.window_by_id(id) else {
                return Some(Event::Error {
                    message: format!("no such window: {id}"),
                });
            };
            if let Some(toplevel) = window.0.toplevel() {
                toplevel.send_close();
            }
            None
        }
        Request::SwitchWorkspace { workspace } => {
            if workspace as usize >= WORKSPACE_COUNT {
                return Some(Event::Error {
                    message: format!("workspace out of range: {workspace}"),
                });
            }
            state.switch_workspace(workspace as usize);
            None
        }
        Request::ToggleLauncher => {
            state.cosmos.launcher_open = !state.cosmos.launcher_open;
            state.ipc_broadcast(&Event::LauncherToggled {
                open: state.cosmos.launcher_open,
            });
            None
        }
        Request::ToggleHelp => {
            state.cosmos.help_open = !state.cosmos.help_open;
            state.ipc_broadcast(&Event::HelpToggled {
                open: state.cosmos.help_open,
            });
            None
        }
        Request::SnapAssistPick { id } => {
            state.snap_assist_pick(id);
            None
        }
        Request::SnapAssistDismiss => {
            state.snap_assist_dismiss();
            None
        }
        Request::SetConfig { key, value } => match state.cosmos_set_config(&key, value) {
            Ok(()) => None,
            Err(message) => Some(Event::Error { message }),
        },
        Request::QuitSession => {
            info!("ipc: quit session requested");
            state.ipc_broadcast(&Event::SessionEnding);
            state
                .running
                .store(false, std::sync::atomic::Ordering::SeqCst);
            None
        }
    }
}
