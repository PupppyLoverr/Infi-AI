//! Reading real system state. Parsers are pure so they are tested; the
//! commands they read are the system's own tools, the same ones the original
//! utilities use.

use std::process::Command;

/// `wpctl get-volume` output: `Volume: 0.45` or `Volume: 0.45 [MUTED]`.
pub fn parse_wpctl_volume(out: &str) -> Option<(u8, bool)> {
    let rest = out.trim().strip_prefix("Volume:")?.trim();
    let num = rest.split_whitespace().next()?;
    let v: f32 = num.parse().ok()?;
    Some((
        (v * 100.0).round().clamp(0.0, 150.0) as u8,
        rest.contains("[MUTED]"),
    ))
}

pub fn get_volume() -> Option<(u8, bool)> {
    let out = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_wpctl_volume(&String::from_utf8_lossy(&out.stdout))
}

pub fn set_volume(pct: u8) -> Result<(), String> {
    run(
        "wpctl",
        &[
            "set-volume",
            "@DEFAULT_AUDIO_SINK@",
            &format!("{}%", pct.min(100)),
        ],
    )
}

pub fn set_mute(mute: bool) -> Result<(), String> {
    run(
        "wpctl",
        &[
            "set-mute",
            "@DEFAULT_AUDIO_SINK@",
            if mute { "1" } else { "0" },
        ],
    )
}

/// One Wi-Fi/ethernet device from `nmcli -t -f DEVICE,TYPE,STATE,CONNECTION device`.
#[derive(Debug, PartialEq, Eq)]
pub struct NetDevice {
    pub device: String,
    pub kind: String,
    pub state: String,
    pub connection: String,
}

pub fn parse_nmcli_devices(out: &str) -> Vec<NetDevice> {
    out.lines()
        .filter_map(|l| {
            // Terse mode escapes ':' inside fields as '\:'.
            let mut parts: Vec<String> = vec![String::new()];
            let mut chars = l.chars().peekable();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        if let Some(n) = chars.next() {
                            parts.last_mut().unwrap().push(n);
                        }
                    }
                    ':' => parts.push(String::new()),
                    c => parts.last_mut().unwrap().push(c),
                }
            }
            if parts.len() < 4 {
                return None;
            }
            let dev = NetDevice {
                device: parts[0].clone(),
                kind: parts[1].clone(),
                state: parts[2].clone(),
                connection: parts[3].clone(),
            };
            (dev.kind != "loopback" && dev.kind != "bridge" && !dev.device.starts_with("veth"))
                .then_some(dev)
        })
        .collect()
}

pub fn network_devices() -> Option<Vec<NetDevice>> {
    let out = Command::new("nmcli")
        .args(["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "device"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| parse_nmcli_devices(&String::from_utf8_lossy(&out.stdout)))
}

pub fn run(program: &str, args: &[&str]) -> Result<(), String> {
    match Command::new(program).args(args).output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(format!(
            "{program}: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => Err(format!("{program}: {e}")),
    }
}

pub fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

/// Start an original system tool without waiting for it.
pub fn launch(cmd: &[&str]) -> Result<(), String> {
    let (bin, args) = cmd.split_first().ok_or("empty command")?;
    Command::new(bin)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{bin}: {e}"))
}

/// `PRETTY_NAME` from os-release text.
pub fn pretty_name(os_release: &str) -> Option<String> {
    os_release
        .lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim().trim_matches('"').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_parses() {
        assert_eq!(parse_wpctl_volume("Volume: 0.45\n"), Some((45, false)));
        assert_eq!(
            parse_wpctl_volume("Volume: 1.00 [MUTED]"),
            Some((100, true))
        );
        assert_eq!(parse_wpctl_volume("garbage"), None);
    }

    #[test]
    fn nmcli_devices_parse_and_skip_virtual() {
        let out = "wlan0:wifi:connected:Home\\:5G\nlo:loopback:unmanaged:\nveth12:ethernet:unmanaged:\neth0:ethernet:disconnected:\n";
        let devs = parse_nmcli_devices(out);
        assert_eq!(devs.len(), 2);
        assert_eq!(devs[0].connection, "Home:5G");
        assert_eq!(devs[1].state, "disconnected");
    }

    #[test]
    fn os_release_name() {
        assert_eq!(
            pretty_name("NAME=x\nPRETTY_NAME=\"Solyra OS 0.1\"\n").as_deref(),
            Some("Solyra OS 0.1")
        );
    }
}
