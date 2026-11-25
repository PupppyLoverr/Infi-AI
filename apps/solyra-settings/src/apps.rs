//! Default apps: which program answers a role (terminal, files, browser).
//!
//! The panel reads `~/.config/solyra/defaults.toml` (`terminal = "foot.desktop"`).
//! The original program is always offered and always installed: choosing a
//! Solyra app is a switch, never a replacement, and one click goes back.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Terminal,
    Files,
    Browser,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::Terminal, Role::Files, Role::Browser];

    pub fn key(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Files => "files",
            Self::Browser => "browser",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Files => "Files",
            Self::Browser => "Web browser",
        }
    }

    /// The upstream program that ships in the image, first; Solyra's own after.
    pub fn candidates(self) -> &'static [Candidate] {
        match self {
            Self::Terminal => &[
                Candidate {
                    bin: "foot",
                    name: "Foot",
                    original: true,
                },
                Candidate {
                    bin: "solyra-term",
                    name: "Solyra Terminal",
                    original: false,
                },
            ],
            Self::Files => &[
                Candidate {
                    bin: "thunar",
                    name: "Thunar",
                    original: true,
                },
                Candidate {
                    bin: "solyra-files",
                    name: "Solyra Files",
                    original: false,
                },
            ],
            Self::Browser => &[
                Candidate {
                    bin: "firefox",
                    name: "Firefox",
                    original: true,
                },
                Candidate {
                    bin: "solyra-browse",
                    name: "Solyra Browse",
                    original: false,
                },
            ],
        }
    }

    pub fn original(self) -> &'static Candidate {
        &self.candidates()[0]
    }

    /// The factory default. Files uses Solyra's own Finder-style app; the
    /// upstream program remains installed and selectable as the way back.
    pub fn default(self) -> &'static Candidate {
        if self == Self::Files {
            &self.candidates()[1]
        } else {
            self.original()
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Candidate {
    pub bin: &'static str,
    pub name: &'static str,
    pub original: bool,
}

/// Candidates for `role` that are installed. The original is returned even if
/// it is missing, so the list is never empty and the way back is always shown.
pub fn available(role: Role, installed: impl Fn(&str) -> bool) -> Vec<&'static Candidate> {
    role.candidates()
        .iter()
        .filter(|c| c.original || installed(c.bin))
        .collect()
}

/// The binary chosen for `role` in `defaults.toml` text, or the factory default.
pub fn chosen(text: &str, role: Role) -> String {
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == role.key() {
                let bin = v.trim().trim_matches('"').trim_end_matches(".desktop");
                if !bin.is_empty() {
                    return bin.to_string();
                }
            }
        }
    }
    role.default().bin.to_string()
}

/// `defaults.toml` text with `role` set to `bin`. Only known candidates are
/// accepted, so the file can never point a role at an arbitrary program.
pub fn set(text: &str, role: Role, bin: &str) -> Result<String, String> {
    if !role.candidates().iter().any(|c| c.bin == bin) {
        return Err(format!("{bin} is not a choice for {}", role.label()));
    }
    let value = format!("\"{bin}.desktop\"");
    let mut text = if text.contains("[roles]") {
        text.to_string()
    } else {
        format!("[roles]\n{text}")
    };
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok(solyra_conf::merge(&text, &[(role.key(), Some(value))]))
}

/// Every role back to its factory default.
pub fn reset_all(text: &str) -> String {
    let mut text = text.to_string();
    for role in Role::ALL {
        text = set(&text, role, role.default().bin).expect("defaults are always valid");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_is_the_default_and_always_listed() {
        assert_eq!(chosen("", Role::Terminal), "foot");
        assert_eq!(chosen("", Role::Files), "solyra-files");
        let none_installed = |_: &str| false;
        let list = available(Role::Terminal, none_installed);
        assert_eq!(list.len(), 1);
        assert!(list[0].original);
    }

    #[test]
    fn solyra_apps_are_offered_only_when_installed() {
        let have = |b: &str| b == "solyra-term";
        let bins: Vec<_> = available(Role::Terminal, have)
            .iter()
            .map(|c| c.bin)
            .collect();
        assert_eq!(bins, ["foot", "solyra-term"]);
    }

    #[test]
    fn choosing_and_reading_back() {
        let t = set("", Role::Terminal, "solyra-term").unwrap();
        assert!(t.starts_with("[roles]"));
        assert_eq!(chosen(&t, Role::Terminal), "solyra-term");
        assert_eq!(chosen(&t, Role::Files), "solyra-files");
    }

    #[test]
    fn existing_roles_and_comments_survive() {
        let t = "[roles]\nfiles = \"thunar.desktop\"\n# keep\nterminal = \"foot.desktop\"\n";
        let out = set(t, Role::Terminal, "solyra-term").unwrap();
        assert!(out.contains("# keep"));
        assert!(out.contains("files = \"thunar.desktop\""));
        assert_eq!(chosen(&out, Role::Terminal), "solyra-term");
    }

    #[test]
    fn arbitrary_programs_are_refused() {
        assert!(set("", Role::Terminal, "rm -rf /").is_err());
        assert!(set("", Role::Files, "solyra-term").is_err());
    }

    #[test]
    fn reset_returns_every_role_to_the_default() {
        let t = set(
            &set("", Role::Terminal, "solyra-term").unwrap(),
            Role::Browser,
            "solyra-browse",
        )
        .unwrap();
        let back = reset_all(&t);
        for role in Role::ALL {
            assert_eq!(chosen(&back, role), role.default().bin);
        }
    }
}
