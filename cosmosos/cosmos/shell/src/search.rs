//! Unified launcher rows — "Search or Ask": app matches, file hits,
//! a live calculator, `>` shell commands and `?` Ask mode (opencode).

use crate::{desktop::AppEntry, fileindex, ShellState};

#[derive(Debug, Clone)]
pub enum Kind {
    App(AppEntry),
    /// Evaluated expression — Enter copies the result.
    Calc(String),
    /// `>cmd` — run in a terminal.
    Cmd(String),
    /// File path — Enter opens it.
    File(String),
    /// `?question` — ask opencode in a terminal.
    Ask(String),
    /// Settings pane id (`cosmos-settings --pane <id>`).
    Setting(&'static str),
}

/// Result groups, in display order after the Top Hit.
const GROUPS: [&str; 6] = ["Apps", "Files", "Settings", "Calculator", "Commands", "Ask"];

pub fn group(kind: &Kind) -> &'static str {
    match kind {
        Kind::App(_) => "Apps",
        Kind::File(_) => "Files",
        Kind::Setting(_) => "Settings",
        Kind::Calc(_) => "Calculator",
        Kind::Cmd(_) => "Commands",
        Kind::Ask(_) => "Ask",
    }
}

/// Settings panes searchable by title and keyword.
const PANES: [(&str, &str, &str); 5] = [
    (
        "appearance",
        "Appearance",
        "dark light mode theme accent colour color scale display zoom reduce motion",
    ),
    ("wallpaper", "Wallpaper", "background desktop picture image"),
    (
        "dock",
        "Desktop & Dock",
        "dock widgets weather clock position left right bottom menubar",
    ),
    ("apps", "Default Apps", "terminal launcher rows apps"),
    (
        "session",
        "Session",
        "log out logout lock sign out restart shut down power",
    ),
];

fn settings_rows(q: &str) -> Vec<Row> {
    let q = q.to_lowercase();
    if q.chars().count() < 2 {
        return Vec::new();
    }
    PANES
        .iter()
        .filter(|(_, title, words)| {
            title
                .to_lowercase()
                .split(|c: char| !c.is_alphanumeric())
                .chain(words.split(' '))
                .any(|w| w.starts_with(&q))
                || title.to_lowercase().starts_with(&q)
        })
        .map(|(id, title, _)| Row {
            kind: Kind::Setting(id),
            title: (*title).to_string(),
            hint: "Settings",
            icon: "cosmos-settings".into(),
            sub: "System Settings".into(),
        })
        .collect()
}

/// Spotlight order: one Top Hit (an app whose name starts with the
/// query, else the first result), then the rest grouped by `GROUPS`.
fn order(mut rows: Vec<Row>, q: &str) -> Vec<Row> {
    if rows.is_empty() {
        return rows;
    }
    let ql = q.to_lowercase();
    let top = rows
        .iter()
        .position(|r| matches!(r.kind, Kind::App(_)) && r.title.to_lowercase().starts_with(&ql))
        .unwrap_or(0);
    let hit = rows.remove(top);
    rows.sort_by_key(|r| GROUPS.iter().position(|g| *g == group(&r.kind)));
    rows.insert(0, hit);
    rows
}

#[derive(Debug, Clone)]
pub struct Row {
    pub kind: Kind,
    /// Primary text shown in the row.
    pub title: String,
    /// Right-side hint label ("App", "File", ...).
    pub hint: &'static str,
    /// icons.rs key.
    pub icon: String,
    /// Dim subtitle (file dir, cmd preview).
    pub sub: String,
}

/// Rows for the current launcher query — apps first, then the special
/// result kinds Spotlight-style.
pub fn results(state: &ShellState) -> Vec<Row> {
    let q = state.launcher_query.trim();
    if let Some(cmd) = q.strip_prefix('>') {
        let cmd = cmd.trim();
        return if cmd.is_empty() {
            Vec::new()
        } else {
            vec![Row {
                kind: Kind::Cmd(cmd.to_string()),
                title: cmd.to_string(),
                hint: "Run",
                icon: "cosmos-terminal".into(),
                sub: "Run in terminal".into(),
            }]
        };
    }
    if let Some(ask) = q.strip_prefix('?') {
        let ask = ask.trim();
        return if ask.is_empty() {
            Vec::new()
        } else {
            vec![Row {
                kind: Kind::Ask(ask.to_string()),
                title: ask.to_string(),
                hint: "Ask",
                icon: "search-ask".into(),
                sub: "Ask Cosmos (opencode)".into(),
            }]
        };
    }

    let mut out = Vec::new();
    if !q.is_empty() {
        out.extend(settings_rows(q));
        if let Some(v) = eval_calc(q) {
            out.push(Row {
                kind: Kind::Calc(v.clone()),
                title: format!("= {v}"),
                hint: "Calculate",
                icon: "search-calc".into(),
                sub: "Copy result".into(),
            });
        }
        for (name, path) in fileindex::query(&state.file_index, &q.to_lowercase(), 4) {
            let dir = path
                .rsplit_once('/')
                .map(|(d, _)| d.trim_start_matches("/home/").to_string())
                .unwrap_or_default();
            out.push(Row {
                kind: Kind::File(path.clone()),
                title: name.clone(),
                hint: "File",
                icon: "search-doc".into(),
                sub: dir,
            });
        }
    }
    for app in state.filtered_apps() {
        out.push(Row {
            kind: Kind::App(app.clone()),
            title: app.name.clone(),
            hint: if app.action.is_some() {
                "Action"
            } else if app.terminal {
                "Terminal"
            } else {
                "App"
            },
            icon: app.id.clone(),
            sub: app.comment.clone(),
        });
    }
    if q.is_empty() {
        out
    } else {
        order(out, q)
    }
}

/// `sh`-quote a string for embedding in a `cosmos-terminal -e` command.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

// ---------------- tiny calculator ----------------
// Recursive-descent evaluator: + - * / % ^, parens, unary minus. Only
// accepts the query as an expression when it fully parses AND contains
// a digit and an operator — plain words never become calculator rows.

fn eval_calc(q: &str) -> Option<String> {
    let s: Vec<char> = q.chars().filter(|c| !c.is_whitespace()).collect();
    if !s.iter().any(|c| c.is_ascii_digit()) || !s.iter().any(|c| "+-*/%^".contains(*c)) {
        return None;
    }
    let mut p = 0usize;
    let v = expr(&s, &mut p)?;
    if p != s.len() || !v.is_finite() {
        return None;
    }
    let rounded = (v * 1e10).round() / 1e10;
    if rounded == rounded.trunc() && rounded.abs() < 1e15 {
        Some(format!("{}", rounded as i64))
    } else {
        Some(
            format!("{rounded:.4}")
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string(),
        )
    }
}

fn expr(s: &[char], p: &mut usize) -> Option<f64> {
    let mut v = term(s, p)?;
    loop {
        match s.get(*p) {
            Some('+') => {
                *p += 1;
                v += term(s, p)?;
            }
            Some('-') => {
                *p += 1;
                v -= term(s, p)?;
            }
            _ => return Some(v),
        }
    }
}

fn term(s: &[char], p: &mut usize) -> Option<f64> {
    let mut v = power(s, p)?;
    loop {
        match s.get(*p) {
            Some('*') => {
                *p += 1;
                v *= power(s, p)?;
            }
            Some('/') => {
                *p += 1;
                let d = power(s, p)?;
                if d == 0.0 {
                    return None;
                }
                v /= d;
            }
            Some('%') => {
                *p += 1;
                let d = power(s, p)?;
                if d == 0.0 {
                    return None;
                }
                v %= d;
            }
            _ => return Some(v),
        }
    }
}

fn power(s: &[char], p: &mut usize) -> Option<f64> {
    let base = unary(s, p)?;
    if s.get(*p) == Some(&'^') {
        *p += 1;
        let e = power(s, p)?; // right-associative
        return Some(base.powf(e));
    }
    Some(base)
}

fn unary(s: &[char], p: &mut usize) -> Option<f64> {
    match s.get(*p) {
        Some('-') => {
            *p += 1;
            Some(-unary(s, p)?)
        }
        Some('+') => {
            *p += 1;
            unary(s, p)
        }
        _ => atom(s, p),
    }
}

fn atom(s: &[char], p: &mut usize) -> Option<f64> {
    match s.get(*p) {
        Some('(') => {
            *p += 1;
            let v = expr(s, p)?;
            if s.get(*p) != Some(&')') {
                return None;
            }
            *p += 1;
            Some(v)
        }
        Some(c) if c.is_ascii_digit() || *c == '.' => {
            let start = *p;
            while matches!(s.get(*p), Some(c) if c.is_ascii_digit() || *c == '.') {
                *p += 1;
            }
            s[start..*p].iter().collect::<String>().parse().ok()
        }
        _ => None,
    }
}

#[cfg(test)]
mod group_tests {
    use super::*;

    fn row(kind: Kind, title: &str) -> Row {
        Row {
            kind,
            title: title.into(),
            hint: "",
            icon: String::new(),
            sub: String::new(),
        }
    }

    #[test]
    fn settings_match_title_and_keywords() {
        let ids = |q: &str| -> Vec<&str> {
            settings_rows(q)
                .into_iter()
                .filter_map(|r| match r.kind {
                    Kind::Setting(id) => Some(id),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(ids("dark"), ["appearance"]);
        assert_eq!(ids("Wall"), ["wallpaper"]);
        assert_eq!(ids("weather"), ["dock"]);
        assert_eq!(ids("dock"), ["dock"]);
        assert!(ids("d").is_empty());
    }

    #[test]
    fn top_hit_then_groups() {
        let rows = vec![
            row(Kind::Calc("4".into()), "= 4"),
            row(Kind::File("/home/u/files.txt".into()), "files.txt"),
            row(Kind::Setting("dock"), "Desktop & Dock"),
            row(Kind::Cmd("ls".into()), "ls"),
            row(Kind::File("/home/u/a".into()), "a"),
        ];
        let got: Vec<&str> = order(rows.clone(), "fi")
            .iter()
            .map(|r| group(&r.kind))
            .collect();
        // No app prefix match: the first row is the Top Hit.
        assert_eq!(
            got,
            ["Calculator", "Files", "Files", "Settings", "Commands"]
        );
    }
}
