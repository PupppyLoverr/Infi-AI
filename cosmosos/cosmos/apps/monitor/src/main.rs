//! cosmos-monitor — the CosmosOS system monitor.
//! All numbers are real: /proc/stat CPU deltas, /proc/meminfo, /proc/loadavg,
//! /proc/<pid> process table with per-process CPU and RSS.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::time::Instant;

use cosmos_kit::controls::{search_field, segmented_with};
use cosmos_kit::layout::{
    card, status_text, toolbar_button, toolbar_spacer, toolbar_title, AppWindow, ColAlign, Column,
    Table,
};
use cosmos_kit::{Icon, Kit, State};
use cosmos_theme::space;

struct Proc {
    pid: u32,
    name: String,
    state: char,
    cpu: f64, // percent over the sample window
    rss_kib: u64,
}

struct Stats {
    cpu_pct: f64,
    cpus: Vec<f64>,
    prev_cpu: Option<Vec<u64>>, // [user+nice+system+idle total, idle] per cpuN
    mem_total: u64,
    mem_avail: u64,
    swap_total: u64,
    swap_free: u64,
    load: [f64; 3],
    uptime_s: u64,
    procs: Vec<Proc>,
    prev_proc_jiffies: HashMap<u32, u64>,
    prev_uptime_jiffies: u64,
    cpu_hist: VecDeque<f64>,
    mem_hist: VecDeque<f64>,
    uptime_hz: u64,
    net_prev: Option<(u64, u64, Instant)>,
    /// Bytes per second, [received, sent].
    net_rate: [f64; 2],
    net_hist: VecDeque<f64>,
    disk_prev: Option<(u64, u64, Instant)>,
    /// Bytes per second, [read, written].
    disk_rate: [f64; 2],
    disk_hist: VecDeque<f64>,
}

/// One sample per second: 60 s of history.
const HIST: usize = 60;

impl Stats {
    fn new() -> Self {
        Stats {
            cpu_pct: 0.0,
            cpus: Vec::new(),
            prev_cpu: None,
            mem_total: 0,
            mem_avail: 0,
            swap_total: 0,
            swap_free: 0,
            load: [0.0; 3],
            uptime_s: 0,
            procs: Vec::new(),
            prev_proc_jiffies: HashMap::new(),
            prev_uptime_jiffies: 0,
            cpu_hist: VecDeque::new(),
            mem_hist: VecDeque::new(),
            uptime_hz: 100,
            net_prev: None,
            net_rate: [0.0; 2],
            net_hist: VecDeque::new(),
            disk_prev: None,
            disk_rate: [0.0; 2],
            disk_hist: VecDeque::new(),
        }
    }

    fn tick(&mut self) {
        self.uptime_hz = unsafe { libc_clk() }.max(1);
        self.read_uptime();
        self.read_cpu();
        self.read_mem();
        self.read_load();
        self.read_procs();
        if let Ok(t) = std::fs::read_to_string("/proc/net/dev") {
            self.net_rate = rate(&mut self.net_prev, net_totals(&t));
        }
        if let Ok(t) = std::fs::read_to_string("/proc/diskstats") {
            self.disk_rate = rate(&mut self.disk_prev, disk_totals(&t, is_disk));
        }
        push(&mut self.cpu_hist, self.cpu_pct);
        push(
            &mut self.mem_hist,
            100.0 * (1.0 - self.mem_avail as f64 / self.mem_total.max(1) as f64),
        );
        push(&mut self.net_hist, self.net_rate[0] + self.net_rate[1]);
        push(&mut self.disk_hist, self.disk_rate[0] + self.disk_rate[1]);
    }

    fn read_uptime(&mut self) {
        if let Ok(s) = fs::read_to_string("/proc/uptime") {
            self.uptime_s = s
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0) as u64;
        }
    }

    fn read_cpu(&mut self) {
        let Ok(s) = fs::read_to_string("/proc/stat") else {
            return;
        };
        let mut totals: Vec<(u64, u64)> = Vec::new(); // (busy+idle, idle)
        let mut prev: Vec<u64> = Vec::new();
        for line in s.lines() {
            if !line.starts_with("cpu") {
                break;
            }
            let parts: Vec<u64> = line
                .split_whitespace()
                .skip(1)
                .filter_map(|v| v.parse().ok())
                .collect();
            if parts.len() < 4 {
                continue;
            }
            let idle = parts[3] + parts.get(4).copied().unwrap_or(0); // idle + iowait
            let total: u64 = parts.iter().sum();
            totals.push((total, idle));
            prev.push(total);
            prev.push(idle);
        }
        if let Some(old) = &self.prev_cpu {
            let mut pcts = Vec::new();
            for (i, (total, idle)) in totals.iter().enumerate() {
                let ot = old.get(i * 2).copied().unwrap_or(0);
                let oi = old.get(i * 2 + 1).copied().unwrap_or(0);
                let dt = total.saturating_sub(ot);
                let di = idle.saturating_sub(oi);
                let pct = if dt > 0 {
                    100.0 * (1.0 - di as f64 / dt as f64)
                } else {
                    0.0
                };
                pcts.push(pct);
            }
            if !pcts.is_empty() {
                self.cpu_pct = pcts[0]; // aggregate "cpu" line is first
                self.cpus = pcts[1..].to_vec();
            }
        }
        self.prev_cpu = Some(prev);
    }

    fn read_mem(&mut self) {
        let Ok(s) = fs::read_to_string("/proc/meminfo") else {
            return;
        };
        let mut kv = HashMap::new();
        for line in s.lines() {
            let mut it = line.split(':');
            if let (Some(k), Some(v)) = (it.next(), it.next()) {
                if let Some(n) = v.split_whitespace().next().and_then(|n| n.parse().ok()) {
                    kv.insert(k.to_string(), n);
                }
            }
        }
        self.mem_total = *kv.get("MemTotal").unwrap_or(&0);
        self.mem_avail = *kv.get("MemAvailable").unwrap_or(&0);
        self.swap_total = *kv.get("SwapTotal").unwrap_or(&0);
        self.swap_free = *kv.get("SwapFree").unwrap_or(&0);
    }

    fn read_load(&mut self) {
        if let Ok(s) = fs::read_to_string("/proc/loadavg") {
            let mut it = s.split_whitespace();
            for i in 0..3 {
                self.load[i] = it.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            }
        }
    }

    fn read_procs(&mut self) {
        let uptime_ticks = self.uptime_s * self.uptime_hz;
        let delta_uptime = uptime_ticks.saturating_sub(self.prev_uptime_jiffies).max(1);
        self.prev_uptime_jiffies = uptime_ticks;

        let mut out = Vec::new();
        let mut seen = HashMap::new();
        let Ok(rd) = fs::read_dir("/proc") else {
            return;
        };
        for e in rd.flatten() {
            let name = e.file_name();
            let Ok(pid) = name.to_string_lossy().parse::<u32>() else {
                continue;
            };
            let stat = match fs::read_to_string(e.path().join("stat")) {
                Ok(s) => s,
                Err(_) => continue,
            };
            // comm may contain spaces/parens — split after last ')'
            let Some(end) = stat.rfind(')') else { continue };
            let comm = stat
                .find('(')
                .map(|s| &stat[s + 1..end])
                .unwrap_or("")
                .to_string();
            let fields: Vec<&str> = stat[end + 2..].split_whitespace().collect();
            if fields.len() < 22 {
                continue;
            }
            let state = fields[0].chars().next().unwrap_or('?');
            let utime: u64 = fields[11].parse().unwrap_or(0);
            let stime: u64 = fields[12].parse().unwrap_or(0);
            let total = utime + stime;
            let prev = self.prev_proc_jiffies.get(&pid).copied().unwrap_or(0);
            let cpu = 100.0 * total.saturating_sub(prev) as f64 / delta_uptime as f64;
            seen.insert(pid, total);
            let rss = fs::read_to_string(e.path().join("statm"))
                .ok()
                .and_then(|s| {
                    s.split_whitespace()
                        .nth(1)
                        .and_then(|v| v.parse::<u64>().ok())
                })
                .unwrap_or(0)
                * 4; // pages → KiB (4K pages on all target archs)
            out.push(Proc {
                pid,
                name: comm,
                state,
                cpu,
                rss_kib: rss,
            });
        }
        self.prev_proc_jiffies = seen;
        out.sort_by(|a, b| {
            b.cpu
                .partial_cmp(&a.cpu)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        self.procs = out;
    }
}

// sysconf(_SC_CLK_TCK) without pulling in the libc crate's API surface.
fn push(h: &mut VecDeque<f64>, v: f64) {
    h.push_back(v);
    while h.len() > HIST {
        h.pop_front();
    }
}

/// Per-second rates of two monotonic byte counters since the last call.
fn rate(prev: &mut Option<(u64, u64, Instant)>, now: (u64, u64)) -> [f64; 2] {
    let t = Instant::now();
    let r = match *prev {
        Some((a, b, t0)) => {
            let dt = t.duration_since(t0).as_secs_f64().max(1e-3);
            [
                now.0.saturating_sub(a) as f64 / dt,
                now.1.saturating_sub(b) as f64 / dt,
            ]
        }
        None => [0.0; 2],
    };
    *prev = Some((now.0, now.1, t));
    r
}

/// (received, sent) bytes over every interface but loopback.
fn net_totals(dev: &str) -> (u64, u64) {
    dev.lines()
        .filter_map(|l| l.split_once(':'))
        .filter(|(name, _)| name.trim() != "lo")
        .filter_map(|(_, rest)| {
            let f: Vec<u64> = rest
                .split_whitespace()
                .filter_map(|v| v.parse().ok())
                .collect();
            Some((*f.first()?, *f.get(8)?))
        })
        .fold((0, 0), |(r, t), (a, b)| (r + a, t + b))
}

/// Whole physical disks only, so partitions and device-mapper volumes
/// aren't counted twice.
fn is_disk(name: &str) -> bool {
    !["loop", "ram", "zram", "dm-", "sr"]
        .iter()
        .any(|p| name.starts_with(p))
        && std::path::Path::new("/sys/block").join(name).exists()
}

/// (read, written) bytes from /proc/diskstats sector counts.
fn disk_totals(stats: &str, is_disk: impl Fn(&str) -> bool) -> (u64, u64) {
    stats
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if !is_disk(f.get(2)?) {
                return None;
            }
            let sectors = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok());
            Some((sectors(5)? * 512, sectors(9)? * 512))
        })
        .fold((0, 0), |(r, w), (a, b)| (r + a, w + b))
}

/// Chart ceiling for a byte-rate history: the next power of two at or
/// above its peak, at least 64 KiB/s so an idle link stays flat.
fn rate_ceiling(hist: &VecDeque<f64>) -> f64 {
    let peak = hist.iter().copied().fold(64.0 * 1024.0, f64::max);
    2f64.powi(peak.log2().ceil() as i32)
}

fn fmt_rate(b: f64) -> String {
    match b {
        b if b >= 1e9 => format!("{:.1} GB/s", b / 1e9),
        b if b >= 1e6 => format!("{:.1} MB/s", b / 1e6),
        b if b >= 1e3 => format!("{:.0} KB/s", b / 1e3),
        b => format!("{b:.0} B/s"),
    }
}

unsafe fn libc_clk() -> u64 {
    extern "C" {
        fn sysconf(name: i32) -> i64;
    }
    const SC_CLK_TCK: i32 = 2;
    sysconf(SC_CLK_TCK) as u64
}

#[derive(Default)]
struct View {
    /// 0 = by CPU, 1 = by memory.
    sort: usize,
    query: String,
    selected: Option<u32>,
    status: String,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let mut stats = Stats::new();
    let mut view = View::default();
    let mut last_tick = std::time::Instant::now() - std::time::Duration::from_secs(2);
    if let Err(e) = cosmos_uitk::run("Activity", "cosmos.monitor", (720, 520), move |ui| {
        if last_tick.elapsed() >= std::time::Duration::from_secs(1) {
            stats.tick();
            last_tick = std::time::Instant::now();
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
        draw(ui, &stats, &mut view);
    }) {
        tracing::error!("cosmos-monitor fatal: {e}");
        std::process::exit(1);
    }
}

/// Filled-area history graph over 0..max: accent line, accent fill
/// fading from 35% at the top of the chart to 0% at its base.
fn sparkline(ui: &mut egui::Ui, kit: &Kit, hist: &VecDeque<f64>, max: f64, h: f32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, kit.fill(State::Rest));
    if hist.len() < 2 {
        return;
    }
    let n = (HIST - 1) as f32;
    let off = HIST.saturating_sub(hist.len()) as f32;
    let pts: Vec<egui::Pos2> = hist
        .iter()
        .enumerate()
        .map(|(i, v)| {
            egui::pos2(
                rect.min.x + rect.width() * (off + i as f32) / n,
                rect.max.y
                    - 2.0
                    - (rect.height() - 4.0) * (*v / max.max(1e-9)).clamp(0.0, 1.0) as f32,
            )
        })
        .collect();
    let base = rect.max.y - 1.0;
    let a = kit.accent();
    let span = (base - rect.min.y).max(1.0);
    let tint = |y: f32| {
        let k = ((base - y) / span).clamp(0.0, 1.0);
        egui::Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), (0.35 * 255.0 * k) as u8)
    };
    let mut mesh = egui::Mesh::default();
    for w in pts.windows(2) {
        let i = mesh.vertices.len() as u32;
        for p in [
            w[0],
            w[1],
            egui::pos2(w[1].x, base),
            egui::pos2(w[0].x, base),
        ] {
            mesh.colored_vertex(p, tint(p.y));
        }
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, a)));
}

fn stat_card(
    ui: &mut egui::Ui,
    kit: &Kit,
    w: f32,
    (title, value, detail, hist, max): (&str, &str, &str, &VecDeque<f64>, f64),
) {
    // Called inside a horizontal row: force a top-down layout so the
    // title, value, detail and graph stack instead of running side by side.
    let layout = egui::Layout::top_down(egui::Align::Min);
    ui.allocate_ui_with_layout(egui::vec2(w, 152.0), layout, |ui| {
        ui.set_width(w);
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(128.0);
            ui.label(egui::RichText::new(title).size(12.0).color(kit.text2()));
            ui.label(
                egui::RichText::new(value)
                    .size(22.0)
                    .strong()
                    .color(kit.text()),
            );
            ui.label(egui::RichText::new(detail).size(12.0).color(kit.text3()));
            ui.add_space(4.0);
            sparkline(ui, kit, hist, max, 56.0);
        });
    });
}

fn fmt_uptime(s: u64) -> String {
    let d = s / 86400;
    let h = (s % 86400) / 3600;
    let m = (s % 3600) / 60;
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

fn draw(ui: &mut egui::Ui, st: &Stats, v: &mut View) {
    let kit = Kit::get(ui.ctx());
    let cell = std::cell::RefCell::new(&mut *v);
    AppWindow::new()
        .toolbar(|ui| {
            let mut v = cell.borrow_mut();
            toolbar_title(ui, "Activity");
            let fixed = 2.0 * 72.0 + 28.0 + 3.0 * space::S12;
            let sw = (ui.available_width() - fixed - space::S8).clamp(96.0, 200.0);
            toolbar_spacer(ui, fixed + sw);
            ui.add_enabled_ui(v.selected.is_some(), |ui| {
                if toolbar_button(ui, Icon::Close, "Quit Process", false).clicked() {
                    if let Some(pid) = v.selected {
                        v.status = match std::process::Command::new("kill")
                            .arg(pid.to_string())
                            .status()
                        {
                            Ok(s) if s.success() => format!("sent SIGTERM to {pid}"),
                            Ok(s) => format!("kill {pid}: {s}"),
                            Err(e) => format!("kill {pid}: {e}"),
                        };
                        v.selected = None;
                    }
                }
            });
            ui.add_space(space::S12);
            segmented_with(ui, &mut v.sort, 2, 72.0, |ui, i, r, c| {
                ui.painter().text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    ["CPU", "Memory"][i],
                    egui::FontId::proportional(13.0),
                    c,
                );
            });
            ui.add_space(space::S12);
            search_field(ui, &mut v.query, "Search", sw);
        })
        .status(|ui| {
            let v = cell.borrow();
            status_text(
                ui,
                &format!(
                    "{} processes · load {:.2} {:.2} {:.2} · up {}",
                    st.procs.len(),
                    st.load[0],
                    st.load[1],
                    st.load[2],
                    fmt_uptime(st.uptime_s)
                ),
            );
            if !v.status.is_empty() {
                status_text(ui, &format!("· {}", v.status));
            }
        })
        .show(ui, |ui| {
            let mut v = cell.borrow_mut();
            egui::Frame::new().inner_margin(space::S16).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let mem_used = st.mem_total.saturating_sub(st.mem_avail);
                let swap = if st.swap_total > 0 {
                    format!(" · swap {} MiB", (st.swap_total - st.swap_free) / 1024)
                } else {
                    String::new()
                };
                let cards = [
                    (
                        "CPU",
                        format!("{:.0}%", st.cpu_pct),
                        format!("{} cores", st.cpus.len().max(1)),
                        &st.cpu_hist,
                        100.0,
                    ),
                    (
                        "Memory",
                        format!("{} MiB", mem_used / 1024),
                        format!(
                            "of {} MiB · {:.0}%{swap}",
                            st.mem_total / 1024,
                            100.0 * mem_used as f64 / st.mem_total.max(1) as f64
                        ),
                        &st.mem_hist,
                        100.0,
                    ),
                    (
                        "Network",
                        format!("{} in", fmt_rate(st.net_rate[0])),
                        format!("{} out", fmt_rate(st.net_rate[1])),
                        &st.net_hist,
                        rate_ceiling(&st.net_hist),
                    ),
                    (
                        "Disk",
                        format!("{} read", fmt_rate(st.disk_rate[0])),
                        format!("{} written", fmt_rate(st.disk_rate[1])),
                        &st.disk_hist,
                        rate_ceiling(&st.disk_hist),
                    ),
                ];
                let cols = if ui.available_width() >= 4.0 * 180.0 + 3.0 * space::S12 {
                    4
                } else {
                    2
                };
                let w = (ui.available_width() - (cols - 1) as f32 * space::S12) / cols as f32;
                for (r, row) in cards.chunks(cols).enumerate() {
                    if r > 0 {
                        ui.add_space(space::S12);
                    }
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S12;
                        for (title, value, detail, hist, max) in row {
                            stat_card(ui, &kit, w, (title, value, detail, hist, *max));
                        }
                    });
                }
                ui.add_space(space::S12);
                card(ui, |ui| {
                    let cols = [
                        Column {
                            title: "Process",
                            width: 0.0,
                            align: ColAlign::Left,
                        },
                        Column {
                            title: "PID",
                            width: 72.0,
                            align: ColAlign::Right,
                        },
                        Column {
                            title: "State",
                            width: 64.0,
                            align: ColAlign::Left,
                        },
                        Column {
                            title: "% CPU",
                            width: 72.0,
                            align: ColAlign::Right,
                        },
                        Column {
                            title: "Memory",
                            width: 96.0,
                            align: ColAlign::Right,
                        },
                    ];
                    let t = Table { cols: &cols };
                    t.header(ui);
                    let q = v.query.to_lowercase();
                    let mut rows: Vec<&Proc> = st
                        .procs
                        .iter()
                        .filter(|p| {
                            q.is_empty()
                                || p.name.to_lowercase().contains(&q)
                                || p.pid.to_string() == q
                        })
                        .collect();
                    if v.sort == 0 {
                        rows.sort_by(|a, b| {
                            b.cpu
                                .partial_cmp(&a.cpu)
                                .unwrap_or(std::cmp::Ordering::Equal)
                                .then(b.rss_kib.cmp(&a.rss_kib))
                        });
                    } else {
                        rows.sort_by_key(|p| std::cmp::Reverse(p.rss_kib));
                    }
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for p in rows {
                                let mem = if p.rss_kib >= 1024 {
                                    format!("{:.1} MB", p.rss_kib as f64 / 1024.0)
                                } else {
                                    format!("{} KB", p.rss_kib)
                                };
                                let state = match p.state {
                                    'R' => "Running",
                                    'S' => "Sleeping",
                                    'D' => "Waiting",
                                    'Z' => "Zombie",
                                    'T' | 't' => "Stopped",
                                    'I' => "Idle",
                                    _ => "Other",
                                };
                                let r = t.row(
                                    ui,
                                    v.selected == Some(p.pid),
                                    None,
                                    &[
                                        &p.name,
                                        &p.pid.to_string(),
                                        state,
                                        &format!("{:.1}", p.cpu),
                                        &mem,
                                    ],
                                );
                                if r.clicked() {
                                    v.selected = Some(p.pid);
                                }
                            }
                        });
                });
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn net_totals_skip_loopback() {
        let dev = "Inter-|   Receive |  Transmit
 face |bytes packets errs drop fifo frame compressed multicast|bytes packets
    lo: 900 9 0 0 0 0 0 0 900 9 0 0 0 0 0 0
  eth0: 1000 10 0 0 0 0 0 0 250 5 0 0 0 0 0 0
  wlan0: 24 1 0 0 0 0 0 0 6 1 0 0 0 0 0 0
";
        assert_eq!(net_totals(dev), (1024, 256));
    }

    #[test]
    fn disk_totals_whole_disks_in_bytes() {
        let stats = "   8       0 vda 10 0 4 0 2 0 8 0 0 0 0
   8       1 vda1 10 0 4 0 2 0 8 0 0 0 0
   7       0 loop0 1 0 100 0 0 0 0 0 0 0 0
";
        assert_eq!(disk_totals(stats, |n| n == "vda"), (4 * 512, 8 * 512));
    }

    #[test]
    fn rates_and_ceiling() {
        let mut prev = Some((0, 0, Instant::now() - std::time::Duration::from_secs(2)));
        let [a, b] = rate(&mut prev, (2000, 0));
        assert!((900.0..=1000.0).contains(&a) && b == 0.0);
        let h: VecDeque<f64> = [10.0, 70_000.0].into_iter().collect();
        assert_eq!(rate_ceiling(&h), 131_072.0);
        assert_eq!(rate_ceiling(&VecDeque::new()), 65_536.0);
        assert_eq!(fmt_rate(1_500_000.0), "1.5 MB/s");
    }
}
