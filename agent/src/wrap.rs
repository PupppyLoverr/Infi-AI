//! `solyra-agent wrap -- <cli>`: run a program on a pseudo-terminal that this
//! process owns, so the notch can type into it.
//!
//! The wrapped program sees a normal terminal (it is a real pty); keyboard
//! input and output pass straight through. Text the panel writes to
//! `messages/<id>.jsonl` is typed into the program followed by Enter. Output
//! activity gives a coarse "working / idle" status, labelled as detected.

use std::ffi::CString;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::events::Emitter;

pub struct WrapConfig {
    pub emitter: Emitter,
    pub command: Vec<String>,
    pub agent: String,
    /// No output for this long means the agent is waiting.
    pub idle_after: Duration,
}

static WINCH: AtomicBool = AtomicBool::new(false);

extern "C" fn on_winch(_: libc::c_int) {
    WINCH.store(true, Ordering::Relaxed);
}

struct RawMode {
    fd: RawFd,
    saved: libc::termios,
}

impl RawMode {
    fn enter(fd: RawFd) -> Option<Self> {
        // SAFETY: plain termios calls on a valid fd; failure is reported by -1.
        unsafe {
            if libc::isatty(fd) != 1 {
                return None;
            }
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(fd, &mut t) != 0 {
                return None;
            }
            let saved = t;
            libc::cfmakeraw(&mut t);
            (libc::tcsetattr(fd, libc::TCSANOW, &t) == 0).then_some(Self { fd, saved })
        }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // SAFETY: restoring the attributes saved in `enter`.
        unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.saved) };
    }
}

fn winsize_of(fd: RawFd) -> libc::winsize {
    // SAFETY: TIOCGWINSZ fills a winsize; a non-tty simply fails.
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) != 0 || ws.ws_col == 0 {
            ws.ws_col = 80;
            ws.ws_row = 24;
        }
        ws
    }
}

fn write_all(fd: RawFd, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        // SAFETY: writing from a valid slice to an open fd.
        let n = unsafe { libc::write(fd, data.as_ptr().cast(), data.len()) };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted || e.kind() == io::ErrorKind::WouldBlock {
                continue;
            }
            return Err(e);
        }
        data = &data[n as usize..];
    }
    Ok(())
}

fn read_some(fd: RawFd, buf: &mut [u8]) -> io::Result<usize> {
    // SAFETY: reading into a valid buffer from an open fd.
    let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
    if n < 0 { Err(io::Error::last_os_error()) } else { Ok(n as usize) }
}

/// Run the command; returns its exit code.
pub fn run(cfg: &WrapConfig, stdin_fd: RawFd, stdout_fd: RawFd) -> io::Result<i32> {
    let (program, args) = cfg
        .command
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no command to run"))?;
    CString::new(program.as_str()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in command"))?;

    let ws = winsize_of(stdin_fd);
    let (mut master, mut slave) = (0, 0);
    // SAFETY: openpty fills two fds; checked below.
    if unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), &ws) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: both fds are freshly created and owned here.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };

    let mut cmd = Command::new(program);
    cmd.args(args)
        .env("SOLYRA_AGENT_ID", &cfg.emitter.id)
        .env("SOLYRA_AGENT_WRAPPED", "1")
        .stdin(Stdio::from(slave.try_clone()?))
        .stdout(Stdio::from(slave.try_clone()?))
        .stderr(Stdio::from(slave.try_clone()?));
    // SAFETY: only async-signal-safe calls (setsid, ioctl) run before exec.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(|e| io::Error::new(e.kind(), format!("{program}: {e}")))?;
    // `cmd` still holds duplicates of the slave side in its Stdio settings; the
    // master only sees end-of-file once every copy is closed.
    drop(cmd);
    drop(slave);

    let cwd = std::env::current_dir().ok();
    let place = cwd
        .as_deref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let _ = cfg.emitter.emit(json!({
        "type": "started",
        "name": if place.is_empty() { cfg.agent.clone() } else { format!("{} · {}", cfg.agent, place) },
        "agent": cfg.agent, "interactive": true,
        "environment": null,
    }));

    let _raw = RawMode::enter(stdin_fd);
    // SAFETY: installing a handler that only sets an atomic.
    unsafe { libc::signal(libc::SIGWINCH, on_winch as *const () as libc::sighandler_t) };

    let mfd = master.as_raw_fd();
    let mut offset = std::fs::metadata(cfg.emitter.root().join("messages").join(format!("{}.jsonl", cfg.emitter.id)))
        .map(|m| m.len())
        .unwrap_or(0);
    let mut buf = [0u8; 8192];
    let mut stdin_open = true;
    let mut working = true;
    let mut last_output = Instant::now();

    loop {
        let mut fds = [
            libc::pollfd { fd: if stdin_open { stdin_fd } else { -1 }, events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: mfd, events: libc::POLLIN, revents: 0 },
        ];
        // SAFETY: polling two valid pollfd structs.
        let r = unsafe { libc::poll(fds.as_mut_ptr(), 2, 100) };
        if r < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            break;
        }
        if WINCH.swap(false, Ordering::Relaxed) {
            let ws = winsize_of(stdin_fd);
            // SAFETY: resizing the pty we own.
            unsafe { libc::ioctl(mfd, libc::TIOCSWINSZ, &ws) };
        }
        if fds[0].revents & (libc::POLLIN | libc::POLLHUP) != 0 && stdin_open {
            match read_some(stdin_fd, &mut buf) {
                Ok(0) | Err(_) => {
                    stdin_open = false;
                    // End of input: tell the program (Ctrl-D).
                    let _ = write_all(mfd, &[0x04]);
                }
                Ok(n) => {
                    let _ = write_all(mfd, &buf[..n]);
                }
            }
        }
        if fds[1].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
            match read_some(mfd, &mut buf) {
                Ok(0) => break,
                Err(e) if e.raw_os_error() == Some(libc::EIO) => break,
                Err(_) => {}
                Ok(n) => {
                    let _ = write_all(stdout_fd, &buf[..n]);
                    last_output = Instant::now();
                    if !working {
                        working = true;
                        let _ = cfg.emitter.emit(json!({"type":"status","state":"working","detected":true}));
                    }
                }
            }
        }
        // Messages typed in the notch.
        let (msgs, new_offset) = cfg.emitter.read_messages(offset);
        offset = new_offset;
        for m in msgs {
            let _ = write_all(mfd, m.as_bytes());
            std::thread::sleep(Duration::from_millis(30));
            let _ = write_all(mfd, b"\r");
            let _ = cfg.emitter.emit(json!({"type":"milestone","text":"Message sent from the panel"}));
        }
        if matches!(child.try_wait(), Ok(Some(_))) {
            // Exited: stop once the master has nothing left to read.
            let mut drained = libc::pollfd { fd: mfd, events: libc::POLLIN, revents: 0 };
            // SAFETY: polling one valid pollfd without blocking.
            let pending = unsafe { libc::poll(&mut drained, 1, 0) } > 0 && drained.revents & libc::POLLIN != 0;
            if !pending {
                break;
            }
        }
        if working && last_output.elapsed() >= cfg.idle_after {
            working = false;
            let _ = cfg.emitter.emit(json!({"type":"status","state":"idle","detected":true}));
        }
    }

    // Drain whatever the program printed last.
    loop {
        match read_some(mfd, &mut buf) {
            Ok(n) if n > 0 => {
                let _ = write_all(stdout_fd, &buf[..n]);
            }
            _ => break,
        }
    }
    let status = child.wait()?;
    let code = status.code().unwrap_or(128 + std::os::unix::process::ExitStatusExt::signal(&status).unwrap_or(0));
    if code == 0 {
        let _ = cfg.emitter.emit(json!({"type":"finished","summary":"Exited normally"}));
    } else {
        let _ = cfg.emitter.emit(json!({"type":"failed","error": format!("Exited with status {code}")}));
    }
    Ok(code)
}

/// Read a pipe/fd fully (test helper).
#[cfg(test)]
pub fn read_to_end(fd: RawFd) -> Vec<u8> {
    // SAFETY: wrapping an fd we own for the duration of the read; forgotten after.
    let mut f = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut v = Vec::new();
    let _ = f.read_to_end(&mut v);
    std::mem::forget(f);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::scratch;
    use std::fs;

    fn pipe() -> (RawFd, RawFd) {
        let mut fds = [0; 2];
        // SAFETY: pipe fills two fds.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        (fds[0], fds[1])
    }

    fn cfg(root: &std::path::Path, cmd: &[&str]) -> WrapConfig {
        WrapConfig {
            emitter: Emitter::new(root.to_path_buf(), "wrap-t"),
            command: cmd.iter().map(|s| s.to_string()).collect(),
            agent: "test".into(),
            idle_after: Duration::from_millis(400),
        }
    }

    #[test]
    fn output_passes_through_and_the_task_is_reported() {
        let root = scratch("w1");
        let (inr, inw) = pipe();
        let (outr, outw) = pipe();
        // SAFETY: closing our write end signals EOF.
        unsafe { libc::close(inw) };
        let code = run(&cfg(&root, &["sh", "-c", "echo hello-from-agent"]), inr, outw).unwrap();
        unsafe { libc::close(outw) };
        assert_eq!(code, 0);
        let out = String::from_utf8_lossy(&read_to_end(outr)).to_string();
        assert!(out.contains("hello-from-agent"), "{out:?}");
        let ev = fs::read_to_string(root.join("tasks/wrap-t.jsonl")).unwrap();
        assert!(ev.contains("\"interactive\":true") && ev.contains("\"type\":\"finished\""), "{ev}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failing_program_is_reported_as_failed() {
        let root = scratch("w2");
        let (inr, inw) = pipe();
        let (_outr, outw) = pipe();
        unsafe { libc::close(inw) };
        let code = run(&cfg(&root, &["sh", "-c", "exit 3"]), inr, outw).unwrap();
        assert_eq!(code, 3);
        let ev = fs::read_to_string(root.join("tasks/wrap-t.jsonl")).unwrap();
        assert!(ev.contains("\"type\":\"failed\"") && ev.contains("status 3"), "{ev}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn panel_messages_are_typed_into_the_program() {
        let root = scratch("w3");
        let (inr, _inw_keep_open) = pipe();
        let (outr, outw) = pipe();
        // The program reads one line and echoes it back with a marker.
        let c = cfg(&root, &["sh", "-c", "read line; echo got:$line"]);
        let rt = root.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(600));
            fs::create_dir_all(rt.join("messages")).unwrap();
            fs::write(rt.join("messages/wrap-t.jsonl"), "{\"text\":\"from-the-notch\"}\n").unwrap();
        });
        let code = run(&c, inr, outw).unwrap();
        unsafe { libc::close(outw) };
        assert_eq!(code, 0);
        let out = String::from_utf8_lossy(&read_to_end(outr)).to_string();
        assert!(out.contains("got:from-the-notch"), "{out:?}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_program_is_an_error_not_a_hang() {
        let root = scratch("w4");
        let (inr, _w) = pipe();
        let (_r, outw) = pipe();
        assert!(run(&cfg(&root, &["/definitely/not/a/program"]), inr, outw).is_err());
        assert!(run(&cfg(&root, &[]), inr, outw).is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
