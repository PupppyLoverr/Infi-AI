//! cosmos-monitor — the CosmosOS system monitor.
//! All numbers are real: /proc/stat CPU deltas, /proc/meminfo, /proc/loadavg,
//! /proc/<pid> process table with per-process CPU and RSS.

use std::collections::{HashMap, VecDeque};
use std::fs;

struct Proc {
    pid: u32,
    name: String,
    state: char,
    cpu: f64,  // percent over the sample window
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
}

const HIST: usize = 120;

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
        }
    }

    fn tick(&mut self) {
        self.uptime_hz = unsafe { libc_clk() }.max(1);
        self.read_uptime();
        self.read_cpu();
        self.read_mem();
        self.read_load();
        self.read_procs();
        self.cpu_hist.push_back(self.cpu_pct);
        self.mem_hist
            .push_back(100.0 * (1.0 - self.mem_avail as f64 / self.mem_total.max(1) as f64));
        while self.cpu_hist.len() > HIST {
            self.cpu_hist.pop_front();
        }
        while self.mem_hist.len() > HIST {
            self.mem_hist.pop_front();
        }
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
        let Ok(s) = fs::read_to_string("/proc/stat") else { return };
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
        let Ok(s) = fs::read_to_string("/proc/meminfo") else { return };
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
        let uptime_ticks = (self.uptime_s as u64) * self.uptime_hz;
        let delta_uptime = uptime_ticks.saturating_sub(self.prev_uptime_jiffies).max(1);
        self.prev_uptime_jiffies = uptime_ticks;

        let mut out = Vec::new();
        let mut seen = HashMap::new();
        let Ok(rd) = fs::read_dir("/proc") else { return };
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
            b.cpu.partial_cmp(&a.cpu)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out.truncate(64);
        self.procs = out;
    }
}

// sysconf(_SC_CLK_TCK) without pulling in the libc crate's API surface.
unsafe fn libc_clk() -> u64 {
    extern "C" {
        fn sysconf(name: i32) -> i64;
    }
    const SC_CLK_TCK: i32 = 2;
    sysconf(SC_CLK_TCK) as u64
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let mut stats = Stats::new();
    let mut last_tick = std::time::Instant::now() - std::time::Duration::from_secs(2);
    if let Err(e) = cosmos_uitk::run("System Monitor", "cosmos.monitor", (600, 460), move |ui| {
        if last_tick.elapsed() >= std::time::Duration::from_secs(1) {
            stats.tick();
            last_tick = std::time::Instant::now();
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        draw(ui, &mut stats);
    }) {
        tracing::error!("cosmos-monitor fatal: {e}");
        std::process::exit(1);
    }
}

fn bar(ui: &mut egui::Ui, label: &str, frac: f64, detail: &str) {
    ui.horizontal(|ui| {
        ui.label(format!("{label:>6}"));
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width() - 200.0, 14.0),
            egui::Sense::hover(),
        );
        let painter = ui.painter();
        let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
        let fill = ui.visuals().selection.bg_fill;
        painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
        let mut inner = rect;
        inner.max.x = rect.min.x + rect.width() * frac.clamp(0.0, 1.0) as f32;
        painter.rect_filled(inner, 2.0, fill);
        painter.rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside);
        ui.label(detail);
    });
}

fn sparkline(ui: &mut egui::Ui, hist: &VecDeque<f64>) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
    if hist.len() > 1 {
        let pts: Vec<egui::Pos2> = hist
            .iter()
            .enumerate()
            .map(|(i, v)| {
                egui::pos2(
                    rect.min.x + rect.width() * i as f32 / (hist.len() - 1).max(1) as f32,
                    rect.max.y - rect.height() * (*v as f32 / 100.0).clamp(0.0, 1.0),
                )
            })
            .collect();
        painter.add(egui::Shape::line(
            pts,
            egui::Stroke::new(1.5, ui.visuals().override_text_color.unwrap_or(egui::Color32::WHITE)),
        ));
    }
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

fn draw(ui: &mut egui::Ui, st: &mut Stats) {
    egui::CentralPanel::default().show(ui, |ui| {
        ui.heading("System Monitor");
        ui.horizontal(|ui| {
            ui.label(format!(
                "load {:.2} {:.2} {:.2}",
                st.load[0], st.load[1], st.load[2]
            ));
            ui.separator();
            ui.label(format!("up {}", fmt_uptime(st.uptime_s)));
        });
        ui.add_space(4.0);
        bar(ui, "CPU", st.cpu_pct / 100.0, &format!("{:.0}%", st.cpu_pct));
        sparkline(ui, &st.cpu_hist);
        let mem_used = st.mem_total.saturating_sub(st.mem_avail);
        bar(
            ui,
            "Memory",
            mem_used as f64 / st.mem_total.max(1) as f64,
            &format!("{}/{} MiB", mem_used / 1024, st.mem_total / 1024),
        );
        sparkline(ui, &st.mem_hist);
        if st.swap_total > 0 {
            let su = st.swap_total - st.swap_free;
            bar(
                ui,
                "Swap",
                su as f64 / st.swap_total as f64,
                &format!("{}/{} MiB", su / 1024, st.swap_total / 1024),
            );
        }
        ui.add_space(8.0);
        ui.heading("Processes");
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("procs")
                .num_columns(5)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("PID");
                    ui.strong("Name");
                    ui.strong("State");
                    ui.strong("CPU%");
                    ui.strong("RSS");
                    ui.end_row();
                    for p in &st.procs {
                        ui.label(p.pid.to_string());
                        ui.label(&p.name);
                        ui.label(p.state.to_string());
                        ui.label(format!("{:.0}", p.cpu));
                        ui.label(format!("{} KiB", p.rss_kib));
                        ui.end_row();
                    }
                });
        });
    });
}
