//! Shell integration: zeron has no shell-integration code to reuse (its
//! engine only spawns the shell), so Solyra ships its own: OSC 7 reports the
//! working directory (tab titles, new panes open where you are) and OSC 133
//! marks prompts and exit codes. The user's own rc files load first.

use std::path::PathBuf;

const BASH: &str = include_str!("../assets/shell/bashrc");
const ZSH: &str = include_str!("../assets/shell/zshrc");
const FISH: &str = include_str!("../assets/shell/config.fish");

fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    let d = base.join("solyra-term/shell");
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

fn write(d: &std::path::Path, name: &str, body: &str) -> Option<PathBuf> {
    let p = d.join(name);
    if std::fs::read_to_string(&p).ok().as_deref() != Some(body) {
        std::fs::write(&p, body).ok()?;
    }
    Some(p)
}

/// The command line and environment that start the user's shell with the
/// integration loaded. Unknown shells run unchanged.
pub fn launch() -> (Option<String>, Vec<(String, String)>) {
    let mut env = vec![("TERM_PROGRAM".to_string(), "solyra-term".to_string())];
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
    let name = shell.rsplit('/').next().unwrap_or("bash").to_string();
    let Some(d) = dir() else { return (None, env) };
    let cmd = match name.as_str() {
        "bash" => write(&d, "bashrc", BASH).map(|p| format!("{shell} --rcfile {} -i", p.display())),
        "zsh" => {
            let z = d.join("zsh");
            let _ = std::fs::create_dir_all(&z);
            write(&z, ".zshrc", ZSH).map(|_| {
                env.push(("ZDOTDIR".into(), z.display().to_string()));
                format!("{shell} -i")
            })
        }
        "fish" => write(&d, "config.fish", FISH).map(|p| format!("{shell} -i -C 'source {}'", p.display())),
        _ => None,
    };
    (cmd, env)
}

#[cfg(test)]
mod tests {
    #[test]
    fn integration_scripts_emit_osc7_and_osc133() {
        for s in [super::BASH, super::ZSH, super::FISH] {
            assert!(s.contains("]7;file://"), "OSC 7 cwd");
            assert!(s.contains("]133;A") && s.contains("]133;D"), "OSC 133 prompt marks");
        }
    }
}
