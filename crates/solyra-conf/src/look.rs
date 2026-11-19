//! The shell's look and behaviour options, as stored in `shell.conf`.
//!
//! Shared by the panel (which obeys them) and Settings (which edits them), so
//! the two always agree on names and defaults. Unknown or malformed values
//! fall back to the default rather than failing.

use crate::get;

macro_rules! choice {
    ($(#[$m:meta])* $name:ident { $($variant:ident = $key:literal / $label:literal),+ $(,)? } default $def:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name { $($variant),+ }
        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            pub fn as_str(self) -> &'static str { match self { $($name::$variant => $key),+ } }
            pub fn label(self) -> &'static str { match self { $($name::$variant => $label),+ } }
            pub fn parse(raw: &str) -> Option<Self> {
                match raw.trim().to_ascii_lowercase().as_str() { $($key => Some($name::$variant),)+ _ => None }
            }
            pub fn index(self) -> usize { Self::ALL.iter().position(|v| *v == self).unwrap_or(0) }
        }
        impl Default for $name { fn default() -> Self { $name::$def } }
    };
}

choice! {
    /// The top bar.
    BarStyle {
        MenuBar = "menubar" / "Menu bar",
        Compact = "compact" / "Compact",
        Hidden = "hidden" / "Hidden",
    } default MenuBar
}

choice! {
    /// The black shape at the top centre that carries live activity.
    NotchStyle {
        Notch = "notch" / "Notch",
        Island = "island" / "Dynamic Island",
        Off = "off" / "Off",
    } default Notch
}

choice! {
    /// Dock layout.
    DockKind {
        Solyra = "solyra" / "Solyra",
        Floating = "macos" / "Floating",
        Taskbar = "windows" / "Taskbar",
        Minimal = "minimal" / "Minimal",
    } default Solyra
}

choice! {
    /// The lock screen's layout.
    LockStyle {
        Solyra = "solyra" / "Solyra",
        Classic = "classic" / "Centred",
        Corner = "corner" / "Clock corner",
    } default Solyra
}

/// Everything appearance-related the shell reads from `shell.conf`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Look {
    pub bar: BarStyle,
    pub notch: NotchStyle,
    pub dock: DockKind,
    pub dock_autohide: bool,
    pub dock_magnify: bool,
    pub show_workspaces: bool,
    pub clock_24h: bool,
    pub show_date: bool,
    pub show_seconds: bool,
    pub reduced_motion: bool,
    pub opaque: bool,
    pub lock: LockStyle,
    /// Accent name (`solyra_tokens::Accent::parse` understands it).
    pub accent: String,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            bar: BarStyle::default(),
            notch: NotchStyle::default(),
            dock: DockKind::default(),
            dock_autohide: false,
            dock_magnify: false,
            show_workspaces: true,
            clock_24h: true,
            show_date: true,
            show_seconds: false,
            reduced_motion: false,
            opaque: false,
            lock: LockStyle::default(),
            accent: "mauve".into(),
        }
    }
}

fn flag(text: &str, key: &str, default: bool) -> bool {
    match get(text, key).map(|v| v.to_ascii_lowercase()) {
        Some(v) if matches!(v.as_str(), "1" | "true" | "yes" | "on") => true,
        Some(v) if matches!(v.as_str(), "0" | "false" | "no" | "off") => false,
        _ => default,
    }
}

impl Look {
    pub fn from_conf(text: &str) -> Self {
        let d = Look::default();
        Self {
            bar: get(text, "bar_style")
                .and_then(BarStyle::parse)
                .unwrap_or(d.bar),
            notch: get(text, "notch_style")
                .and_then(NotchStyle::parse)
                .unwrap_or(d.notch),
            dock: get(text, "dock_style")
                .and_then(|v| match v {
                    "hybrid" => Some(DockKind::Solyra),
                    "mac" => Some(DockKind::Floating),
                    "win" | "taskbar" => Some(DockKind::Taskbar),
                    other => DockKind::parse(other),
                })
                .unwrap_or(d.dock),
            dock_autohide: flag(text, "dock_autohide", d.dock_autohide),
            dock_magnify: flag(text, "dock_magnify", d.dock_magnify),
            show_workspaces: flag(text, "show_workspaces", d.show_workspaces),
            clock_24h: flag(text, "clock_24h", d.clock_24h),
            show_date: flag(text, "show_date", d.show_date),
            show_seconds: flag(text, "show_seconds", d.show_seconds),
            reduced_motion: flag(text, "reduced_motion", d.reduced_motion),
            opaque: flag(text, "opaque", d.opaque),
            lock: get(text, "lock_style")
                .and_then(LockStyle::parse)
                .unwrap_or(d.lock),
            accent: get(text, "accent").map(str::to_string).unwrap_or(d.accent),
        }
    }

    /// Key/value pairs for `crate::update`, every look key included.
    pub fn pairs(&self) -> Vec<(&'static str, Option<String>)> {
        let b = |v: bool| Some(if v { "1" } else { "0" }.to_string());
        vec![
            ("bar_style", Some(self.bar.as_str().into())),
            ("notch_style", Some(self.notch.as_str().into())),
            ("dock_style", Some(self.dock.as_str().into())),
            ("dock_autohide", b(self.dock_autohide)),
            ("dock_magnify", b(self.dock_magnify)),
            ("show_workspaces", b(self.show_workspaces)),
            ("clock_24h", b(self.clock_24h)),
            ("show_date", b(self.show_date)),
            ("show_seconds", b(self.show_seconds)),
            ("reduced_motion", b(self.reduced_motion)),
            ("opaque", b(self.opaque)),
            ("lock_style", Some(self.lock.as_str().into())),
            ("accent", Some(self.accent.clone())),
        ]
    }

    /// strftime pattern for the clock.
    pub fn clock_format(&self) -> &'static str {
        match (self.clock_24h, self.show_seconds) {
            (true, false) => "%H:%M",
            (true, true) => "%H:%M:%S",
            (false, false) => "%-I:%M %p",
            (false, true) => "%-I:%M:%S %p",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_empty_or_garbage() {
        assert_eq!(Look::from_conf(""), Look::default());
        assert_eq!(
            Look::from_conf("bar_style=sideways\nclock_24h=maybe\n"),
            Look::default()
        );
    }

    #[test]
    fn round_trips_through_the_file_format() {
        let mut l = Look::default();
        l.bar = BarStyle::Compact;
        l.notch = NotchStyle::Island;
        l.dock = DockKind::Minimal;
        l.dock_autohide = true;
        l.clock_24h = false;
        l.lock = LockStyle::Corner;
        l.accent = "teal".into();
        let text = crate::merge("theme=dark\n", &l.pairs());
        assert_eq!(Look::from_conf(&text), l);
        assert!(text.starts_with("theme=dark\n"), "foreign keys are kept");
    }

    #[test]
    fn old_dock_names_still_work() {
        assert_eq!(Look::from_conf("dock_style=hybrid").dock, DockKind::Solyra);
        assert_eq!(Look::from_conf("dock_style=macos").dock, DockKind::Floating);
        assert_eq!(
            Look::from_conf("dock_style=windows").dock,
            DockKind::Taskbar
        );
    }

    #[test]
    fn clock_formats() {
        let mut l = Look::default();
        assert_eq!(l.clock_format(), "%H:%M");
        l.clock_24h = false;
        l.show_seconds = true;
        assert_eq!(l.clock_format(), "%-I:%M:%S %p");
    }

    #[test]
    fn every_choice_parses_its_own_name() {
        for v in BarStyle::ALL {
            assert_eq!(BarStyle::parse(v.as_str()), Some(*v));
        }
        for v in NotchStyle::ALL {
            assert_eq!(NotchStyle::parse(v.as_str()), Some(*v));
        }
        for v in DockKind::ALL {
            assert_eq!(DockKind::parse(v.as_str()), Some(*v));
        }
        for v in LockStyle::ALL {
            assert_eq!(LockStyle::parse(v.as_str()), Some(*v));
        }
    }
}
