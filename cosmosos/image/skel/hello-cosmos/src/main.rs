use std::fs;

/// The PRETTY_NAME line of an os-release file.
fn pretty_name(os_release: &str) -> Option<String> {
    os_release
        .lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim_matches('"').to_string())
}

/// "2 h 5 min" from the first field of /proc/uptime.
fn uptime(proc_uptime: &str) -> Option<String> {
    let secs = proc_uptime.split_whitespace().next()?.parse::<f64>().ok()? as u64;
    let (h, m) = (secs / 3600, secs % 3600 / 60);
    Some(if h > 0 {
        format!("{h} h {m} min")
    } else {
        format!("{m} min")
    })
}

fn main() {
    let user = std::env::var("USER").unwrap_or_else(|_| "there".into());
    println!("Hello, {user}! Welcome to CosmosOS.");
    if let Some(name) = fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|s| pretty_name(&s))
    {
        println!("System:  {name}");
    }
    if let Some(up) = fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| uptime(&s))
    {
        println!("Up for:  {up}");
    }
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    println!("CPUs:    {cpus}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pretty_name() {
        let s = "NAME=\"CosmosOS\"\nPRETTY_NAME=\"CosmosOS (Debian trixie)\"\n";
        assert_eq!(pretty_name(s).as_deref(), Some("CosmosOS (Debian trixie)"));
    }

    #[test]
    fn formats_uptime() {
        assert_eq!(uptime("7500.42 12000.10").as_deref(), Some("2 h 5 min"));
        assert_eq!(uptime("59.0 1.0").as_deref(), Some("0 min"));
    }
}
