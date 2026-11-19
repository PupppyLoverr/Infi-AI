//! First-boot setup: what the wizard may ask for, and exactly which commands
//! applying it runs.
//!
//! The wizard (unprivileged) writes a request file; `solyra-setup-apply`
//! (root, started by systemd only while `/var/lib/solyra/setup-done` is
//! absent) validates it again from scratch, so a bad request file cannot do
//! more than a bad wizard could. Nothing here trusts its input.

use serde_json::Value;

pub const REQUEST_PATH: &str = "/var/lib/solyra/setup-request.json";
pub const DONE_PATH: &str = "/var/lib/solyra/setup-done";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub username: String,
    pub full_name: String,
    pub password: String,
    pub hostname: String,
}

const RESERVED: &[&str] = &[
    "root", "bin", "daemon", "sys", "adm", "sync", "shutdown", "halt", "mail", "nobody", "greetd",
    "solyra-agent", "systemd-network", "polkitd", "dbus", "sshd",
];

pub fn validate_username(u: &str) -> Result<(), String> {
    let mut chars = u.chars();
    let ok_first = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    if !ok_first || u.len() > 31 || !u.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-') {
        return Err("Use 1 to 31 lowercase letters, digits, - or _, starting with a letter.".into());
    }
    if RESERVED.contains(&u) || u.starts_with("systemd") {
        return Err(format!("\"{u}\" is reserved for the system."));
    }
    Ok(())
}

pub fn validate_full_name(n: &str) -> Result<(), String> {
    if n.chars().count() > 64 || n.chars().any(|c| c.is_control() || c == ':' || c == ',') {
        return Err("The name may have up to 64 characters and no : or , .".into());
    }
    Ok(())
}

pub fn validate_password(p: &str) -> Result<(), String> {
    let n = p.chars().count();
    if !(8..=128).contains(&n) {
        return Err("The password needs 8 to 128 characters.".into());
    }
    if p.chars().any(char::is_control) {
        return Err("The password cannot contain control characters.".into());
    }
    Ok(())
}

pub fn validate_hostname(h: &str) -> Result<(), String> {
    let ok = !h.is_empty()
        && h.len() <= 63
        && !h.starts_with('-')
        && !h.ends_with('-')
        && h.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok { Ok(()) } else { Err("Use lowercase letters, digits and - (up to 63), not starting or ending with -.".into()) }
}

pub fn validate(r: &Request) -> Result<(), String> {
    validate_username(&r.username)?;
    validate_full_name(&r.full_name)?;
    validate_password(&r.password)?;
    validate_hostname(&r.hostname)
}

pub fn parse_request(json: &str) -> Result<Request, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("not valid JSON: {e}"))?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string).ok_or(format!("missing {k}"));
    let r = Request { username: s("username")?, full_name: s("full_name").unwrap_or_default(), password: s("password")?, hostname: s("hostname")? };
    validate(&r)?;
    Ok(r)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Cmd {
    pub program: &'static str,
    pub args: Vec<String>,
    /// Sent on stdin, never on the command line (so it is not in `ps`).
    pub stdin: Option<String>,
}

/// The exact commands that apply a validated request, in order.
pub fn plan(r: &Request) -> Vec<Cmd> {
    vec![
        Cmd {
            program: "useradd",
            args: ["-m", "-s", "/bin/bash", "-G", "wheel", "-c", r.full_name.as_str(), "--", r.username.as_str()].map(String::from).to_vec(),
            stdin: None,
        },
        Cmd { program: "chpasswd", args: vec![], stdin: Some(format!("{}:{}\n", r.username, r.password)) },
        Cmd { program: "hostnamectl", args: vec!["set-hostname".into(), r.hostname.clone()], stdin: None },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Request {
        Request { username: "ada".into(), full_name: "Ada Lovelace".into(), password: "correct horse".into(), hostname: "ada-laptop".into() }
    }

    #[test]
    fn a_good_request_validates() {
        assert!(validate(&good()).is_ok());
    }

    #[test]
    fn usernames() {
        for ok in ["a", "ada", "a_b-c9"] {
            assert!(validate_username(ok).is_ok(), "{ok}");
        }
        for bad in ["", "Ada", "1ada", "-a", "a b", "a;rm", "root", "greetd", "systemd-x", &"a".repeat(32), "añ"] {
            assert!(validate_username(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn passwords_names_and_hostnames() {
        assert!(validate_password("short").is_err());
        assert!(validate_password("has\nnewline1").is_err());
        assert!(validate_password(&"x".repeat(129)).is_err());
        assert!(validate_password("a perfectly fine passphrase").is_ok());
        assert!(validate_full_name("Ada: Countess").is_err());
        assert!(validate_full_name("Ada, Countess").is_err());
        assert!(validate_full_name("").is_ok());
        for bad in ["", "-a", "a-", "A", "a_b", "a.b", &"a".repeat(64)] {
            assert!(validate_hostname(bad).is_err(), "{bad:?}");
        }
        assert!(validate_hostname("solyra-1").is_ok());
    }

    #[test]
    fn the_plan_never_puts_the_password_on_a_command_line() {
        let r = good();
        let p = plan(&r);
        assert_eq!(p[0].program, "useradd");
        assert_eq!(p[0].args.last().unwrap(), "ada");
        assert!(p[0].args.contains(&"--".to_string()), "option parsing is ended before the name");
        for c in &p {
            assert!(c.args.iter().all(|a| !a.contains(&r.password)), "{c:?}");
        }
        assert_eq!(p[1].stdin.as_deref(), Some("ada:correct horse\n"));
    }

    #[test]
    fn parsing_validates_and_rejects_shapes() {
        let j = r#"{"username":"ada","full_name":"Ada","password":"correct horse","hostname":"h1"}"#;
        assert_eq!(parse_request(j).unwrap().username, "ada");
        assert!(parse_request("nope").is_err());
        assert!(parse_request(r#"{"username":"ada"}"#).unwrap_err().contains("password"));
        assert!(parse_request(r#"{"username":"root","password":"correct horse","hostname":"h"}"#).is_err());
    }
}
