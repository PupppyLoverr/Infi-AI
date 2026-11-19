//! Level-1 sandbox: bubblewrap around one command, a snapshot of the task's
//! workspace taken first, and an undo that restores exactly what changed.
//!
//! What this is: a mount and namespace sandbox. The command sees a read-only
//! `/usr` and `/etc`, a private `/tmp`, the workspace you name (writable) and
//! optionally some read-only folders; the network is off unless the policy
//! allows it.
//!
//! What this is not: a security boundary against a determined attacker (the
//! kernel is shared, `/etc` is readable), and it does not roll back the
//! operating system (that is bootc's job). Undo covers files inside the
//! workspace only, and only for runs started through `solyra-agent run`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, PartialEq, Eq)]
pub struct Policy {
    pub workspace: PathBuf,
    pub network: bool,
    pub read_only: Vec<PathBuf>,
    pub env: Vec<String>,
}

pub fn parse_policy(text: &str) -> Result<Policy, String> {
    let t: toml::Table = text.parse().map_err(|e| format!("policy is not valid TOML: {e}"))?;
    let s = t
        .get("sandbox")
        .and_then(|v| v.as_table())
        .ok_or("policy needs a [sandbox] table")?;
    let workspace = s
        .get("workspace")
        .and_then(|v| v.as_str())
        .ok_or("[sandbox] needs workspace = \"/path\"")?;
    let strs = |k: &str| -> Vec<String> {
        s.get(k)
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    Ok(Policy {
        workspace: PathBuf::from(workspace),
        network: s.get("network").and_then(|v| v.as_bool()).unwrap_or(false),
        read_only: strs("read_only").into_iter().map(PathBuf::from).collect(),
        env: strs("env"),
    })
}

/// A workspace must be a real, specific directory: never `/`, never a home
/// directory itself, never a system directory.
pub fn check_workspace(ws: &Path) -> Result<PathBuf, String> {
    let canon = ws.canonicalize().map_err(|e| format!("{}: {e}", ws.display()))?;
    if !canon.is_dir() {
        return Err(format!("{} is not a directory", canon.display()));
    }
    let depth = canon.components().count();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let system = ["/usr", "/etc", "/var", "/boot", "/proc", "/sys", "/dev", "/run", "/bin", "/sbin", "/lib", "/lib64", "/opt", "/srv"];
    if depth < 3
        || home.as_deref().is_some_and(|h| h.canonicalize().ok().as_deref() == Some(&canon))
        || system.iter().any(|s| canon == Path::new(s))
    {
        return Err(format!(
            "{} is too broad to be a workspace; choose a project folder",
            canon.display()
        ));
    }
    Ok(canon)
}

/// The full bubblewrap command line (excluding the `bwrap` program name).
pub fn bwrap_args(p: &Policy, ws: &Path, command: &[String]) -> Vec<String> {
    let s = |x: &str| x.to_string();
    let mut a: Vec<String> = vec![
        s("--unshare-all"),
        s("--die-with-parent"),
        s("--new-session"),
        s("--ro-bind"), s("/usr"), s("/usr"),
        s("--ro-bind"), s("/etc"), s("/etc"),
        s("--symlink"), s("usr/bin"), s("/bin"),
        s("--symlink"), s("usr/sbin"), s("/sbin"),
        s("--symlink"), s("usr/lib"), s("/lib"),
        s("--symlink"), s("usr/lib64"), s("/lib64"),
        s("--proc"), s("/proc"),
        s("--dev"), s("/dev"),
        s("--tmpfs"), s("/tmp"),
        s("--clearenv"),
        s("--setenv"), s("HOME"), s("/tmp"),
        s("--setenv"), s("PATH"), s("/usr/local/bin:/usr/bin:/bin"),
    ];
    if p.network {
        a.insert(1, s("--share-net"));
    }
    for name in ["TERM", "LANG", "LC_ALL"].iter().map(|s| s.to_string()).chain(p.env.iter().cloned()) {
        if let Ok(v) = std::env::var(&name) {
            a.extend([s("--setenv"), name, v]);
        }
    }
    for ro in &p.read_only {
        let ro = ro.to_string_lossy().into_owned();
        a.extend([s("--ro-bind"), ro.clone(), ro]);
    }
    let w = ws.to_string_lossy().into_owned();
    a.extend([s("--bind"), w.clone(), w.clone(), s("--chdir"), w, s("--")]);
    a.extend(command.iter().cloned());
    a
}

// ------------------------------------------------------------- snapshots

#[derive(Debug, PartialEq, Eq, Default)]
pub struct Changes {
    pub added: Vec<PathBuf>,
    pub changed: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
}

impl Changes {
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.changed.is_empty() && self.removed.is_empty()
    }
}

fn walk(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        for e in fs::read_dir(root.join(&rel))? {
            let e = e?;
            let r = rel.join(e.file_name());
            if e.file_type()?.is_dir() {
                stack.push(r.clone());
            }
            out.push(r);
        }
    }
    out.sort();
    Ok(out)
}

fn same(a: &Path, b: &Path) -> io::Result<bool> {
    let (ma, mb) = (fs::symlink_metadata(a)?, fs::symlink_metadata(b)?);
    if ma.file_type().is_symlink() || mb.file_type().is_symlink() {
        return Ok(ma.file_type().is_symlink() == mb.file_type().is_symlink()
            && fs::read_link(a)? == fs::read_link(b)?);
    }
    if ma.is_dir() || mb.is_dir() {
        return Ok(ma.is_dir() == mb.is_dir());
    }
    Ok(ma.len() == mb.len() && fs::read(a)? == fs::read(b)?)
}

/// Compare `workspace` against `snapshot` (paths relative to both).
pub fn diff(workspace: &Path, snapshot: &Path) -> io::Result<Changes> {
    let now: std::collections::BTreeSet<PathBuf> = walk(workspace)?.into_iter().collect();
    let then: std::collections::BTreeSet<PathBuf> = walk(snapshot)?.into_iter().collect();
    let mut c = Changes::default();
    for p in now.difference(&then) {
        c.added.push(p.clone());
    }
    for p in then.difference(&now) {
        c.removed.push(p.clone());
    }
    for p in now.intersection(&then) {
        if !same(&workspace.join(p), &snapshot.join(p))? {
            c.changed.push(p.clone());
        }
    }
    Ok(c)
}

/// Copy `ws` into `dest` (an empty or new directory). Tries a reflink copy
/// first (instant on XFS/btrfs), then a normal copy; returns whether reflink
/// worked.
pub fn snapshot(ws: &Path, dest: &Path) -> io::Result<bool> {
    fs::create_dir_all(dest)?;
    let src = format!("{}/.", ws.display());
    for (mode, reflinked) in [("--reflink=always", true), ("--reflink=never", false)] {
        let st = Command::new("cp").args(["-a", mode]).arg(&src).arg(dest).status()?;
        if st.success() {
            return Ok(reflinked);
        }
    }
    Err(io::Error::other("cp could not copy the workspace"))
}

/// Make `ws` match `snap` again: delete what was added, put back what was
/// changed or removed. Returns what it did.
pub fn restore(ws: &Path, snap: &Path) -> io::Result<Changes> {
    let c = diff(ws, snap)?;
    // Deepest first so files go before their directories.
    let mut added = c.added.clone();
    added.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for p in &added {
        let full = ws.join(p);
        match fs::symlink_metadata(&full) {
            Ok(m) if m.is_dir() => {
                let _ = fs::remove_dir(&full);
            }
            Ok(_) => fs::remove_file(&full)?,
            Err(_) => {}
        }
    }
    let mut put_back: Vec<&PathBuf> = c.changed.iter().chain(c.removed.iter()).collect();
    put_back.sort_by_key(|p| p.components().count());
    for p in put_back {
        let (from, to) = (snap.join(p), ws.join(p));
        let meta = fs::symlink_metadata(&from)?;
        if let Ok(tm) = fs::symlink_metadata(&to) {
            if tm.is_dir() && !meta.is_dir() {
                fs::remove_dir_all(&to)?;
            } else if !tm.is_dir() {
                fs::remove_file(&to)?;
            }
        }
        if meta.is_dir() {
            fs::create_dir_all(&to)?;
        } else if meta.file_type().is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(&from)?, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(c)
}

pub fn snapshots_dir() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_STATE_HOME") {
        Some(x) => PathBuf::from(x),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".local/state"),
    };
    Some(base.join("solyra/agent-snapshots"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::scratch;

    #[test]
    fn policy_parses_and_defaults_to_no_network() {
        let p = parse_policy("[sandbox]\nworkspace = \"/home/u/p\"\nread_only = [\"/home/u/docs\"]\nenv = [\"EDITOR\"]\n").unwrap();
        assert_eq!(p.workspace, PathBuf::from("/home/u/p"));
        assert!(!p.network);
        assert_eq!(p.read_only, vec![PathBuf::from("/home/u/docs")]);
        assert!(parse_policy("nonsense").is_err());
        assert!(parse_policy("[sandbox]\nnetwork = true\n").unwrap_err().contains("workspace"));
    }

    #[test]
    fn broad_workspaces_are_refused() {
        assert!(check_workspace(Path::new("/")).is_err());
        assert!(check_workspace(Path::new("/usr")).is_err());
        assert!(check_workspace(Path::new("/tmp")).is_err(), "depth < 3");
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        assert!(check_workspace(Path::new(&home)).is_err(), "the home directory itself");
        assert!(check_workspace(Path::new("/definitely/missing/dir")).is_err());
        let root = scratch("ws");
        let ws = root.join("project");
        fs::create_dir_all(&ws).unwrap();
        assert!(check_workspace(&ws).is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bwrap_args_isolate_by_default_and_open_only_what_is_asked() {
        let p = Policy { workspace: "/w/p".into(), network: false, read_only: vec!["/w/docs".into()], env: vec![] };
        let a = bwrap_args(&p, Path::new("/w/p"), &["echo".into(), "hi".into()]);
        assert!(a.contains(&"--unshare-all".to_string()));
        assert!(!a.contains(&"--share-net".to_string()), "network stays off");
        assert!(a.windows(3).any(|w| w == ["--bind", "/w/p", "/w/p"]));
        assert!(a.windows(3).any(|w| w == ["--ro-bind", "/w/docs", "/w/docs"]));
        assert!(a.windows(3).any(|w| w == ["--tmpfs", "/tmp", "--clearenv"]) || a.contains(&"--clearenv".to_string()));
        assert_eq!(&a[a.len() - 3..], ["--", "echo", "hi"].map(String::from));
        // The user's home is never bound.
        assert!(!a.iter().any(|x| x == "/home"));
        let mut net = p;
        net.network = true;
        assert!(bwrap_args(&net, Path::new("/w/p"), &["x".into()]).contains(&"--share-net".to_string()));
    }

    fn tree(root: &Path) {
        fs::create_dir_all(root.join("src/deep")).unwrap();
        fs::write(root.join("a.txt"), "alpha").unwrap();
        fs::write(root.join("src/b.rs"), "fn b() {}").unwrap();
        fs::write(root.join("src/deep/c.txt"), "gamma").unwrap();
        std::os::unix::fs::symlink("a.txt", root.join("link")).unwrap();
    }

    #[test]
    fn diff_and_restore_undo_edits_deletes_and_additions() {
        let base = scratch("snap");
        let ws = base.join("ws");
        tree(&ws);
        let snap = base.join("snap");
        let reflinked = snapshot(&ws, &snap).unwrap();
        eprintln!("reflink used: {reflinked}");
        assert!(diff(&ws, &snap).unwrap().is_empty());

        fs::write(ws.join("a.txt"), "CHANGED").unwrap(); // edit
        fs::remove_file(ws.join("src/b.rs")).unwrap(); // delete
        fs::create_dir_all(ws.join("new/dir")).unwrap(); // add dirs + file
        fs::write(ws.join("new/dir/x.txt"), "x").unwrap();
        fs::remove_dir_all(ws.join("src/deep")).unwrap(); // delete a tree

        let c = diff(&ws, &snap).unwrap();
        assert!(c.changed.contains(&PathBuf::from("a.txt")));
        assert!(c.removed.contains(&PathBuf::from("src/b.rs")));
        assert!(c.removed.contains(&PathBuf::from("src/deep/c.txt")));
        assert!(c.added.contains(&PathBuf::from("new/dir/x.txt")));

        restore(&ws, &snap).unwrap();
        assert!(diff(&ws, &snap).unwrap().is_empty(), "workspace matches the snapshot again");
        assert_eq!(fs::read_to_string(ws.join("a.txt")).unwrap(), "alpha");
        assert_eq!(fs::read_to_string(ws.join("src/deep/c.txt")).unwrap(), "gamma");
        assert!(!ws.join("new").exists());
        assert_eq!(fs::read_link(ws.join("link")).unwrap(), PathBuf::from("a.txt"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_file_replaced_by_a_directory_is_restored() {
        let base = scratch("snap2");
        let ws = base.join("ws");
        tree(&ws);
        let snap = base.join("snap");
        snapshot(&ws, &snap).unwrap();
        fs::remove_file(ws.join("a.txt")).unwrap();
        fs::create_dir(ws.join("a.txt")).unwrap();
        fs::write(ws.join("a.txt/inner"), "z").unwrap();
        restore(&ws, &snap).unwrap();
        assert!(diff(&ws, &snap).unwrap().is_empty());
        assert!(ws.join("a.txt").is_file());
        let _ = fs::remove_dir_all(&base);
    }

    fn bwrap_works() -> bool {
        Command::new("bwrap")
            .args(["--unshare-all", "--ro-bind", "/usr", "/usr", "--symlink", "usr/bin", "/bin", "--symlink", "usr/lib", "/lib", "--symlink", "usr/lib64", "/lib64", "--", "/usr/bin/true"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Runs only where user namespaces work (a CI container, most desktops).
    #[test]
    fn the_real_sandbox_hides_home_and_the_network_and_contains_writes() {
        if !bwrap_works() {
            eprintln!("skipped: bubblewrap cannot run here (no bwrap or no user namespaces)");
            return;
        }
        let base = scratch("bw");
        let ws = base.join("ws/project");
        fs::create_dir_all(&ws).unwrap();
        let secret = base.join("secret.txt");
        fs::write(&secret, "top secret").unwrap();
        let p = Policy { workspace: ws.clone(), network: false, read_only: vec![], env: vec![] };
        let script = format!(
            "cat {} 2>/dev/null && echo LEAK; echo ok > inside.txt; ls /sys/class/net | grep -v '^lo$' | head -1 | sed 's/^/NET:/'",
            secret.display()
        );
        let args = bwrap_args(&p, &ws.canonicalize().unwrap(), &["sh".into(), "-c".into(), script]);
        let out = Command::new("bwrap").args(&args).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(!text.contains("LEAK"), "the sandbox read a file outside its workspace: {text}");
        assert!(!text.contains("NET:"), "a network interface is visible: {text}");
        assert_eq!(fs::read_to_string(ws.join("inside.txt")).unwrap().trim(), "ok");
        let _ = fs::remove_dir_all(&base);
    }
}
