//! Root helper: apply the first-boot request, once.
//!
//! Started by systemd (`solyra-setup-apply.service`) only while
//! `/var/lib/solyra/setup-done` does not exist. It refuses a request file that
//! is a symlink, not a regular file, group- or world-readable, or not owned by
//! the setup user, validates it again, deletes it before doing anything (it
//! holds a password), runs the planned commands, and writes the done marker
//! only if all of them succeeded. `--dry-run` prints the plan instead.

use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

use solyra_setup_core::{parse_request, plan, DONE_PATH, REQUEST_PATH};

fn fail(msg: impl AsRef<str>) -> ExitCode {
    eprintln!("solyra-setup-apply: {}", msg.as_ref());
    ExitCode::from(1)
}

fn main() -> ExitCode {
    let dry = std::env::args().any(|a| a == "--dry-run");
    let path = std::env::var("SOLYRA_SETUP_REQUEST").unwrap_or_else(|_| REQUEST_PATH.to_string());
    let done = std::env::var("SOLYRA_SETUP_DONE").unwrap_or_else(|_| DONE_PATH.to_string());
    if Path::new(&done).exists() {
        return fail("setup was already completed; refusing to run again");
    }
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) => return fail(format!("{path}: {e}")),
    };
    if !meta.file_type().is_file() {
        return fail("the request must be a regular file, not a link");
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return fail("the request file must not be readable by group or others (mode 0600)");
    }
    // The wizard runs as the `solyra` setup user; anything else wrote it elsewhere.
    let expected = std::env::var("SOLYRA_SETUP_UID").ok().and_then(|v| v.parse::<u32>().ok());
    if let Some(uid) = expected {
        if meta.uid() != uid {
            return fail("the request file is not owned by the setup user");
        }
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => return fail(format!("{path}: {e}")),
    };
    let request = match parse_request(&text) {
        Ok(r) => r,
        Err(e) => return fail(format!("invalid request: {e}")),
    };
    if !dry {
        // It holds a password: gone before anything else happens.
        if let Err(e) = std::fs::remove_file(&path) {
            return fail(format!("cannot remove the request: {e}"));
        }
    }
    for cmd in plan(&request) {
        if dry {
            println!("{} {}", cmd.program, cmd.args.join(" "));
            continue;
        }
        let mut child = match Command::new(cmd.program)
            .args(&cmd.args)
            .stdin(if cmd.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .spawn()
        {
            Ok(c) => c,
            Err(e) => return fail(format!("{}: {e}", cmd.program)),
        };
        if let (Some(input), Some(mut pipe)) = (cmd.stdin, child.stdin.take()) {
            let _ = pipe.write_all(input.as_bytes());
        }
        match child.wait() {
            Ok(s) if s.success() => {}
            Ok(s) => return fail(format!("{} failed ({s}); setup is not marked done", cmd.program)),
            Err(e) => return fail(format!("{}: {e}", cmd.program)),
        }
    }
    if !dry {
        if let Some(dir) = Path::new(&done).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&done, b"done\n") {
            return fail(format!("cannot write {done}: {e}"));
        }
    }
    ExitCode::SUCCESS
}
