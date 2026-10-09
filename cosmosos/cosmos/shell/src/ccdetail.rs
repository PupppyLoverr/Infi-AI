//! Control Centre detail views (spec v5 §2.4): the Wi-Fi network list
//! (NetworkManager over D-Bus) and the sound output picker (WirePlumber
//! via `wpctl`). Probes are blocking and run when a detail view opens.

use std::collections::HashMap;

use zbus::blocking::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const PROPS: &str = "org.freedesktop.DBus.Properties";
/// NM_DEVICE_TYPE_WIFI.
const DEVICE_WIFI: u32 = 2;

#[derive(Debug, Clone, PartialEq)]
pub struct Network {
    pub ssid: String,
    /// 0..=100.
    pub strength: u8,
    pub secured: bool,
    pub active: bool,
    /// AP object path, used to activate it.
    pub ap: String,
    /// Wi-Fi device object path the AP belongs to.
    pub device: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sink {
    pub id: u32,
    pub name: String,
    pub default: bool,
}

fn get(conn: &Connection, path: &str, iface: &str, prop: &str) -> Option<OwnedValue> {
    let reply = conn
        .call_method(Some(NM), path, Some(PROPS), "Get", &(iface, prop))
        .ok()?;
    reply.body().deserialize::<OwnedValue>().ok()
}

fn as_u32(v: &OwnedValue) -> Option<u32> {
    match &**v {
        Value::U32(n) => Some(*n),
        _ => None,
    }
}

fn as_path(v: &OwnedValue) -> Option<String> {
    match &**v {
        Value::ObjectPath(p) => Some(p.as_str().to_string()),
        _ => None,
    }
}

/// Every visible network on every Wi-Fi device, strongest first, one row
/// per SSID. Empty when NM or a Wi-Fi device is absent.
pub fn wifi_networks() -> Vec<Network> {
    let Ok(conn) = Connection::system() else {
        return Vec::new();
    };
    let Ok(reply) = conn.call_method(Some(NM), NM_PATH, Some(NM), "GetDevices", &()) else {
        return Vec::new();
    };
    let devices: Vec<OwnedObjectPath> = reply.body().deserialize().unwrap_or_default();
    let mut out: Vec<Network> = Vec::new();
    for dev in devices {
        let dev = dev.as_str();
        let kind = get(
            &conn,
            dev,
            "org.freedesktop.NetworkManager.Device",
            "DeviceType",
        );
        if kind.as_ref().and_then(as_u32) != Some(DEVICE_WIFI) {
            continue;
        }
        let wifi = "org.freedesktop.NetworkManager.Device.Wireless";
        let active = get(&conn, dev, wifi, "ActiveAccessPoint")
            .as_ref()
            .and_then(as_path)
            .unwrap_or_default();
        let Ok(reply) = conn.call_method(Some(NM), dev, Some(wifi), "GetAllAccessPoints", &())
        else {
            continue;
        };
        let aps: Vec<OwnedObjectPath> = reply.body().deserialize().unwrap_or_default();
        for ap in aps {
            let ap = ap.as_str();
            let all = conn
                .call_method(
                    Some(NM),
                    ap,
                    Some(PROPS),
                    "GetAll",
                    &("org.freedesktop.NetworkManager.AccessPoint"),
                )
                .ok()
                .and_then(|r| r.body().deserialize::<HashMap<String, OwnedValue>>().ok());
            let Some(p) = all else {
                continue;
            };
            let ssid = match p.get("Ssid").map(|v| &**v) {
                Some(Value::Array(a)) => {
                    let bytes: Vec<u8> = a
                        .iter()
                        .filter_map(|b| match b {
                            Value::U8(x) => Some(*x),
                            _ => None,
                        })
                        .collect();
                    String::from_utf8_lossy(&bytes).into_owned()
                }
                _ => String::new(),
            };
            if ssid.is_empty() {
                continue;
            }
            let strength = match p.get("Strength").map(|v| &**v) {
                Some(Value::U8(s)) => *s,
                _ => 0,
            };
            let flag = |k: &str| p.get(k).and_then(as_u32).unwrap_or(0);
            let secured = flag("WpaFlags") != 0 || flag("RsnFlags") != 0 || flag("Flags") & 1 != 0;
            out.push(Network {
                ssid,
                strength,
                secured,
                active: ap == active,
                ap: ap.to_string(),
                device: dev.to_string(),
            });
        }
    }
    dedupe(out)
}

/// One row per SSID (the active AP, else the strongest), active first,
/// then by strength.
pub fn dedupe(mut nets: Vec<Network>) -> Vec<Network> {
    nets.sort_by(|a, b| b.active.cmp(&a.active).then(b.strength.cmp(&a.strength)));
    let mut seen = std::collections::HashSet::new();
    nets.retain(|n| seen.insert(n.ssid.clone()));
    nets
}

/// Join `net`: NM reuses a saved profile for the SSID or creates one.
/// Secured networks without a saved profile need a password, which NM
/// asks for through its secret agent; returns false if NM refuses.
pub fn connect(net: &Network) -> bool {
    let Ok(conn) = Connection::system() else {
        return false;
    };
    let settings: HashMap<&str, HashMap<&str, Value>> = HashMap::new();
    let (Ok(dev), Ok(ap)) = (
        zbus::zvariant::ObjectPath::try_from(net.device.as_str()),
        zbus::zvariant::ObjectPath::try_from(net.ap.as_str()),
    ) else {
        return false;
    };
    conn.call_method(
        Some(NM),
        NM_PATH,
        Some(NM),
        "AddAndActivateConnection",
        &(settings, dev, ap),
    )
    .is_ok()
}

/// Parse the `Sinks:` block of `wpctl status`.
pub fn parse_sinks(status: &str) -> Vec<Sink> {
    let mut out = Vec::new();
    let mut in_audio = false;
    let mut in_sinks = false;
    for line in status.lines() {
        let t = line.trim_start_matches(|c: char| c.is_whitespace() || "│├└─".contains(c));
        if line.starts_with("Audio") {
            in_audio = true;
            continue;
        }
        if line.starts_with("Video") || line.starts_with("Settings") {
            in_audio = false;
        }
        if !in_audio {
            continue;
        }
        if t.starts_with("Sinks:") {
            in_sinks = true;
            continue;
        }
        if t.ends_with(':') && !t.is_empty() {
            in_sinks = false;
            continue;
        }
        if !in_sinks || t.is_empty() {
            continue;
        }
        let default = t.starts_with('*');
        let t = t.trim_start_matches('*').trim_start();
        let Some((id, rest)) = t.split_once('.') else {
            continue;
        };
        let Ok(id) = id.trim().parse::<u32>() else {
            continue;
        };
        let name = rest.split(" [").next().unwrap_or("").trim().to_string();
        if !name.is_empty() {
            out.push(Sink { id, name, default });
        }
    }
    out
}

pub fn sinks() -> Vec<Sink> {
    std::process::Command::new("wpctl")
        .arg("status")
        .output()
        .ok()
        .map(|o| parse_sinks(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// An open Control Centre detail view and its rows.
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Wifi(Vec<Network>),
    Sound(Vec<Sink>),
}

pub fn set_default_sink(id: u32) -> bool {
    std::process::Command::new("wpctl")
        .args(["set-default", &id.to_string()])
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "PipeWire 'pipewire-0' [1.4.2, cosmos@cosmos, cookie:1]
 └─ Clients:
        31. WirePlumber                         [1.4.2, cosmos@cosmos, pid:610]

Audio
 ├─ Devices:
 │      42. Built-in Audio                      [alsa]
 │  
 ├─ Sinks:
 │  *   46. Built-in Audio Analog Stereo        [vol: 0.40]
 │      51. HDMI / DisplayPort 1 Output         [vol: 1.00 MUTED]
 │  
 ├─ Sources:
 │  *   47. Built-in Audio Analog Stereo        [vol: 1.00]
 │  
 └─ Streams:

Video
 ├─ Devices:
 ├─ Sinks:
 │      70. Fake Video Sink
";

    #[test]
    fn parses_audio_sinks_only() {
        let s = parse_sinks(STATUS);
        assert_eq!(
            s,
            vec![
                Sink {
                    id: 46,
                    name: "Built-in Audio Analog Stereo".into(),
                    default: true
                },
                Sink {
                    id: 51,
                    name: "HDMI / DisplayPort 1 Output".into(),
                    default: false
                },
            ]
        );
    }

    #[test]
    fn dedupe_keeps_active_then_strongest() {
        let n = |ssid: &str, strength, active| Network {
            ssid: ssid.into(),
            strength,
            secured: true,
            active,
            ap: String::new(),
            device: String::new(),
        };
        let out = dedupe(vec![
            n("a", 40, false),
            n("b", 90, false),
            n("a", 70, false),
            n("c", 20, true),
        ]);
        let names: Vec<_> = out.iter().map(|x| (x.ssid.as_str(), x.strength)).collect();
        assert_eq!(names, vec![("c", 20), ("b", 90), ("a", 70)]);
    }
}
