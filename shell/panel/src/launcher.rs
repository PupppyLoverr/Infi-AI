//! Launcher search: pure ranking and grouping, no drawing, no I/O.
//!
//! The launcher answers one question, "what do you want to open or do?", from
//! sources the shell really has: installed applications, open windows, and
//! shell/compositor commands. It does **not** search files, and nothing here
//! pretends to.
//!
//! Natural-language requests to an agent are a different kind of action from
//! launching an app. They appear as a clearly labelled last group, are never
//! the default selection when anything else matched, and only ever queue a
//! draft request for the runtime (see `agent::submit_request`).

use crate::state::{AppEntry, LaunchTarget, WindowInfo};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Application,
    Window,
    Command,
    Agent,
}

impl Kind {
    /// Section title in the Spotlight list. Title case, the way a menu labels a group.
    pub fn heading(self) -> &'static str {
        match self {
            Self::Application => "Applications",
            Self::Window => "Open Windows",
            Self::Command => "Commands",
            Self::Agent => "Agent",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Action {
    Launch(LaunchTarget),
    FocusWindow(u64),
    /// Queue a draft task request. Carries the user's words verbatim.
    AgentRequest(String),
    /// Shown but not runnable; the item's `subtitle` says why.
    Unavailable,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub kind: Kind,
    pub title: String,
    pub subtitle: String,
    /// Icon lookup key and squircle role.
    pub app_id: String,
    pub role: String,
    pub action: Action,
}

impl Item {
    pub fn is_runnable(&self) -> bool {
        !matches!(self.action, Action::Unavailable)
    }
}

const MAX_APPS: usize = 4;
/// Empty query: Top Hit plus a short Applications list, not a wall of apps.
const EMPTY_APPS: usize = 5;
const MAX_WINDOWS: usize = 3;
const MAX_COMMANDS: usize = 3;
/// A couple of shell actions under the apps when the field is still empty.
const EMPTY_COMMANDS: usize = 2;
/// Fuzzy (subsequence) matching only kicks in this far into a query; below it
/// a stray two-letter query would match nearly everything.
const FUZZY_MIN_LEN: usize = 3;
/// Queries shorter than this do not offer the agent row.
const AGENT_MIN_LEN: usize = 3;

/// Score `query` against one candidate. `None` = no match. Higher is better.
/// Exact beats prefix beats word-prefix beats substring beats fuzzy, so an
/// ordinary exact match is always where the user expects it.
pub fn score(query: &str, title: &str, app_id: &str, subtitle: &str) -> Option<u32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    let t = title.to_lowercase();
    if t == q {
        return Some(1000);
    }
    if t.starts_with(&q) {
        return Some(800);
    }
    if t.split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(&q))
    {
        return Some(600);
    }
    if t.contains(&q) {
        return Some(400);
    }
    if app_id.to_lowercase().contains(&q) {
        return Some(300);
    }
    if subtitle.to_lowercase().contains(&q) {
        return Some(150);
    }
    if q.chars().count() >= FUZZY_MIN_LEN && is_subsequence(&q, &t) {
        return Some(50);
    }
    None
}

fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut it = haystack.chars();
    needle.chars().all(|n| it.any(|h| h == n))
}

fn is_command(app: &AppEntry) -> bool {
    !matches!(app.command, LaunchTarget::Shell(_))
}

fn item_from_app(app: &AppEntry) -> Item {
    Item {
        kind: if is_command(app) {
            Kind::Command
        } else {
            Kind::Application
        },
        title: app.title.clone(),
        subtitle: app.subtitle.clone(),
        app_id: app.app_id.clone(),
        role: app.role.clone(),
        action: Action::Launch(app.command.clone()),
    }
}

fn ranked<T>(
    candidates: impl Iterator<Item = (u32, T)>,
    title: impl Fn(&T) -> &str,
    cap: usize,
) -> Vec<T> {
    let mut v: Vec<(u32, T)> = candidates.collect();
    // Stable sort: equal scores keep catalogue order.
    v.sort_by(|(sa, a), (sb, b)| {
        sb.cmp(sa)
            .then_with(|| title(a).chars().count().cmp(&title(b).chars().count()))
    });
    v.into_iter().take(cap).map(|(_, t)| t).collect()
}

/// Build the ordered result list for `query`.
///
/// * Empty query: applications (Top Hit, then Applications) and a few commands.
///   No windows and no agent row; there is nothing to act on yet.
/// * Otherwise: Applications, Open windows, Commands, then the Agent row.
pub fn search(
    query: &str,
    catalog: &[AppEntry],
    wins: &[WindowInfo],
    agent_runtime_present: bool,
) -> Vec<Item> {
    let q = query.trim();
    let mut out = Vec::new();

    let apps = ranked(
        catalog
            .iter()
            .filter(|a| !is_command(a))
            .filter_map(|a| score(q, &a.title, &a.app_id, &a.subtitle).map(|s| (s, a))),
        |a| a.title.as_str(),
        if q.is_empty() { EMPTY_APPS } else { MAX_APPS },
    );
    out.extend(apps.into_iter().map(item_from_app));

    if !q.is_empty() {
        let windows = ranked(
            wins.iter()
                .filter_map(|w| score(q, &w.title, &w.app_id, "").map(|s| (s, w))),
            |w| w.title.as_str(),
            MAX_WINDOWS,
        );
        out.extend(windows.into_iter().map(|w| Item {
            kind: Kind::Window,
            title: if w.title.trim().is_empty() {
                crate::state::pretty_app_name(&w.app_id)
            } else {
                w.title.clone()
            },
            subtitle: format!(
                "Switch to window · {}",
                crate::state::pretty_app_name(&w.app_id)
            ),
            app_id: w.app_id.clone(),
            role: crate::state::role_for_app_id(&w.app_id),
            action: Action::FocusWindow(w.id),
        }));
    }

    let commands = ranked(
        catalog
            .iter()
            .filter(|a| is_command(a))
            .filter_map(|a| score(q, &a.title, &a.app_id, &a.subtitle).map(|s| (s, a))),
        |a| a.title.as_str(),
        if q.is_empty() {
            EMPTY_COMMANDS
        } else {
            MAX_COMMANDS
        },
    );
    out.extend(commands.into_iter().map(item_from_app));

    if q.chars().count() >= AGENT_MIN_LEN {
        out.push(Item {
            kind: Kind::Agent,
            title: format!("Ask an agent: {q}"),
            subtitle: if agent_runtime_present {
                "Queues a draft task for the agent runtime to review".to_string()
            } else {
                "No agent runtime detected".to_string()
            },
            app_id: "solyra-agents".into(),
            role: "agent".into(),
            action: if agent_runtime_present {
                Action::AgentRequest(q.to_string())
            } else {
                Action::Unavailable
            },
        });
    }
    out
}

/// True when the query matched something other than the agent row.
pub fn has_direct_match(items: &[Item]) -> bool {
    items.iter().any(|i| i.kind != Kind::Agent)
}

/// One drawable row: an item, and the group heading to draw above it when it
/// starts a group (or is the first visible row).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub index: usize,
    /// Section title drawn above this row, when it starts a group.
    pub heading: Option<&'static str>,
}

pub const HEADING_H: i32 = 22;
pub const ROW_H: i32 = 40;

fn section_label(
    items: &[Item],
    index: usize,
    first: usize,
    spotlight: bool,
) -> Option<&'static str> {
    let kind = items[index].kind;
    // The best match stands alone as Top Hit. The rest of its group keeps the
    // normal section title on the following row.
    if spotlight && index == 0 && kind != Kind::Agent {
        return Some("Top Hit");
    }
    let new_group = index == first || items[index - 1].kind != kind;
    if new_group {
        return Some(kind.heading());
    }
    if spotlight && kind != Kind::Agent && items.first().is_some_and(|i| i.kind == kind) {
        let rest = (1..items.len()).find(|&i| items[i].kind == kind);
        if rest == Some(index) {
            return Some(kind.heading());
        }
    }
    None
}

fn rows_with(items: &[Item], first: usize, avail_h: i32, spotlight: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut used = 0;
    for (index, _) in items.iter().enumerate().skip(first) {
        let heading = section_label(items, index, first, spotlight);
        let need = ROW_H + if heading.is_some() { HEADING_H } else { 0 };
        if used + need > avail_h {
            break;
        }
        used += need;
        rows.push(Row { index, heading });
    }
    rows
}

/// Rows that fit in `avail_h` starting at item `first`.
pub fn rows_from(items: &[Item], first: usize, avail_h: i32) -> Vec<Row> {
    rows_with(items, first, avail_h, false)
}

/// Same list as [`rows_from`], with the first direct match labelled Top Hit.
pub fn spotlight_rows(items: &[Item], first: usize, avail_h: i32) -> Vec<Row> {
    rows_with(items, first, avail_h, true)
}

fn first_with(items: &[Item], selected: usize, avail_h: i32, spotlight: bool) -> usize {
    let selected = selected.min(items.len().saturating_sub(1));
    for first in 0..=selected {
        if rows_with(items, first, avail_h, spotlight)
            .iter()
            .any(|r| r.index == selected)
        {
            return first;
        }
    }
    selected
}

/// Smallest `first` that keeps `selected` fully visible. Deterministic, so the
/// list needs no scroll state of its own.
pub fn first_visible(items: &[Item], selected: usize, avail_h: i32) -> usize {
    first_with(items, selected, avail_h, false)
}

/// [`first_visible`] for the Spotlight section layout.
pub fn spotlight_first(items: &[Item], selected: usize, avail_h: i32) -> usize {
    first_with(items, selected, avail_h, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ShellCmd;

    fn app(title: &str, id: &str, sub: &str) -> AppEntry {
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: id.into(),
            role: id.into(),
            title: title.into(),
            subtitle: sub.into(),
            command: LaunchTarget::Shell(id.into()),
        }
    }

    fn cmd(title: &str, id: &str) -> AppEntry {
        AppEntry {
            command: LaunchTarget::Internal(ShellCmd::ToggleControl),
            ..app(title, id, "")
        }
    }

    fn win(id: u64, app_id: &str, title: &str) -> WindowInfo {
        WindowInfo {
            id,
            app_id: app_id.into(),
            title: title.into(),
            workspace_id: 1,
            is_focused: false,
        }
    }

    fn catalog() -> Vec<AppEntry> {
        vec![
            app("Files", "thunar", "Browse storage"),
            app("Terminal", "foot", "Command line shell"),
            app("Text Editor", "gedit", "Edit text"),
            app("Firefox", "firefox", "Web browser"),
            cmd("Control Centre", "solyra-control"),
            cmd("Workspace Overview", "solyra-overview"),
        ]
    }

    fn titles(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn exact_beats_prefix_beats_substring() {
        assert!(score("files", "Files", "", "") > score("fil", "Files", "", ""));
        assert!(score("fil", "Files", "", "") > score("ile", "Files", "", ""));
        assert!(score("ile", "Files", "", "") > score("ile", "Other", "", "mobile"));
    }

    #[test]
    fn word_prefix_matches() {
        assert_eq!(score("edit", "Text Editor", "", ""), Some(600));
    }

    #[test]
    fn fuzzy_is_gated_by_length() {
        assert_eq!(
            score("tm", "Terminal", "", ""),
            None,
            "two letters never fuzzy"
        );
        assert!(score("tml", "Terminal", "", "").is_some());
        assert_eq!(score("zzz", "Terminal", "", ""), None);
    }

    #[test]
    fn exact_match_is_first_and_predictable() {
        let items = search("terminal", &catalog(), &[], true);
        assert_eq!(items[0].title, "Terminal");
        assert_eq!(items[0].kind, Kind::Application);
    }

    #[test]
    fn exact_outranks_fuzzy_noise() {
        let mut c = catalog();
        c.push(app("Tiny Metal Library", "tml", ""));
        let items = search("terminal", &c, &[], true);
        assert_eq!(items[0].title, "Terminal");
    }

    #[test]
    fn groups_are_ordered_apps_windows_commands_agent() {
        let wins = [win(7, "foot", "foot — control notes")];
        let items = search("control", &catalog(), &wins, true);
        let kinds: Vec<Kind> = items.iter().map(|i| i.kind).collect();
        let mut sorted = kinds.clone();
        sorted.sort();
        assert_eq!(kinds, sorted, "groups out of order: {kinds:?}");
        assert_eq!(kinds.last(), Some(&Kind::Agent));
        assert!(kinds.contains(&Kind::Window));
        assert!(kinds.contains(&Kind::Command));
    }

    #[test]
    fn empty_query_offers_apps_and_commands_only() {
        let wins = [win(1, "foot", "shell")];
        let items = search("", &catalog(), &wins, true);
        assert!(items
            .iter()
            .all(|i| matches!(i.kind, Kind::Application | Kind::Command)));
        assert!(!items.is_empty());
    }

    #[test]
    fn agent_row_is_last_and_needs_a_runtime() {
        let with = search("sort my downloads", &catalog(), &[], true);
        let last = with.last().unwrap();
        assert_eq!(last.kind, Kind::Agent);
        assert!(last.is_runnable());
        assert!(matches!(&last.action, Action::AgentRequest(q) if q == "sort my downloads"));

        let without = search("sort my downloads", &catalog(), &[], false);
        let last = without.last().unwrap();
        assert!(!last.is_runnable());
        assert!(last.subtitle.contains("No agent runtime"));
    }

    #[test]
    fn agent_row_never_precedes_a_direct_match() {
        let items = search("files", &catalog(), &[], true);
        assert_ne!(items[0].kind, Kind::Agent);
        assert!(has_direct_match(&items));
    }

    #[test]
    fn no_direct_match_leaves_only_the_agent_row() {
        let items = search("qwertyuiop", &catalog(), &[], true);
        assert!(!has_direct_match(&items));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, Kind::Agent);
    }

    #[test]
    fn short_queries_do_not_offer_the_agent() {
        assert!(search("zz", &catalog(), &[], true).is_empty());
    }

    #[test]
    fn windows_are_searched_by_title_and_app() {
        let wins = [win(3, "firefox", "Release notes"), win(4, "foot", "build")];
        let items = search("release", &catalog(), &wins, false);
        let w: Vec<_> = items.iter().filter(|i| i.kind == Kind::Window).collect();
        assert_eq!(w.len(), 1);
        assert!(matches!(w[0].action, Action::FocusWindow(3)));
    }

    #[test]
    fn caps_apply_per_group() {
        let c: Vec<AppEntry> = (0..20)
            .map(|i| app(&format!("App {i}"), &format!("a{i}"), ""))
            .collect();
        let items = search("app", &c, &[], false);
        assert_eq!(
            items.iter().filter(|i| i.kind == Kind::Application).count(),
            MAX_APPS
        );
    }

    #[test]
    fn ties_prefer_shorter_titles_then_catalogue_order() {
        let c = vec![
            app("Notes Pro Max", "a", ""),
            app("Notes", "b", ""),
            app("Notes+", "c", ""),
        ];
        let items = search("notes", &c, &[], false);
        assert_eq!(titles(&items)[..3], ["Notes", "Notes+", "Notes Pro Max"]);
    }

    #[test]
    fn rows_scroll_to_keep_selection_visible() {
        let c: Vec<AppEntry> = (0..4)
            .map(|i| app(&format!("App {i}"), &format!("a{i}"), ""))
            .collect();
        let mut items = search("app", &c, &[], false);
        items.extend(search("control", &catalog(), &[], true));
        let avail = HEADING_H + 2 * ROW_H;
        for sel in 0..items.len() {
            let first = first_visible(&items, sel, avail);
            let rows = rows_from(&items, first, avail);
            assert!(
                rows.iter().any(|r| r.index == sel),
                "sel {sel} hidden (first {first})"
            );
            assert_eq!(
                rows[0].heading,
                Some(items[first].kind.heading()),
                "first row carries its heading"
            );
        }
        assert_eq!(first_visible(&items, 0, avail), 0);
    }

    #[test]
    fn empty_query_leads_with_top_hit_then_applications() {
        let items = search("", &catalog(), &[], false);
        assert!(items.iter().any(|i| i.kind == Kind::Application));
        let rows = spotlight_rows(&items, 0, 480);
        assert_eq!(rows[0].heading, Some("Top Hit"));
        assert_eq!(rows[0].index, 0);
        assert!(
            rows.iter().any(|r| r.heading == Some("Applications")),
            "remaining apps keep their section: {rows:?}"
        );
        let first = spotlight_first(&items, items.len() - 1, HEADING_H + 2 * ROW_H);
        let visible = spotlight_rows(&items, first, HEADING_H + 2 * ROW_H);
        assert!(visible.iter().any(|r| r.index == items.len() - 1));
    }

    #[test]
    fn a_heading_alone_never_overflows() {
        let items = search("files", &catalog(), &[], true);
        let rows = rows_from(&items, 0, HEADING_H + ROW_H);
        assert_eq!(rows.len(), 1);
        let rows = rows_from(&items, 0, ROW_H - 1);
        assert!(rows.is_empty());
    }
}
