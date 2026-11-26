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
    } default Island
}

choice! {
    /// Dock layout.
    DockKind {
        Solyra = "solyra" / "Solyra",
        Floating = "macos" / "Floating",
        Taskbar = "windows" / "Taskbar",
        Minimal = "minimal" / "Minimal",
    } default Floating
}

choice! {
    /// The lock screen's layout (Dashboard = large clock, avatars, media glance).
    LockStyle {
        Solyra = "solyra" / "Solyra",
        Classic = "classic" / "Centred",
        Corner = "corner" / "Clock corner",
        Dashboard = "dashboard" / "Dashboard",
    } default Classic
}

choice! {
    /// How the desktop is arranged for humans vs agent-forward work.
    DesktopMode {
        Standard = "standard" / "Standard",
        Agent = "agent" / "Agent workspace",
    } default Standard
}

choice! {
    /// What a shell surface is made of. Solid is an opaque fill; See-through
    /// is a light tint with the desktop sharp behind it; Glass blurs what is
    /// behind it and adds a lit edge; Acrylic blurs, takes its tint from the
    /// wallpaper and adds a fine grain. Blur and grain are drawn by the
    /// compositor (layer rules keyed on the surface's namespace suffix).
    Material {
        Solid = "solid" / "Solid",
        Clear = "clear" / "See-through",
        Glass = "glass" / "Glass",
        Acrylic = "acrylic" / "Acrylic",
    } default Glass
}

/// Everything appearance-related the shell reads from `shell.conf`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Look {
    pub bar: BarStyle,
    pub notch: NotchStyle,
    pub dock: DockKind,
    pub dock_autohide: bool,
    pub dock_magnify: bool,
    /// The notch opens when the pointer rests on it (pearOS), not only on click.
    pub notch_hover: bool,
    /// Spotlight opens inside the notch / Dynamic Island (MacBook Pro style)
    /// instead of as a separate capsule mid-screen.
    pub spotlight_in_notch: bool,
    /// Calendar and weather cards at the top left of the desktop.
    pub desktop_widgets: bool,
    pub show_workspaces: bool,
    pub clock_24h: bool,
    pub show_date: bool,
    pub show_seconds: bool,
    pub reduced_motion: bool,
    pub opaque: bool,
    pub lock: LockStyle,
    pub desktop: DesktopMode,
    pub bar_material: Material,
    pub dock_material: Material,
    pub notch_material: Material,
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
            dock_magnify: true,
            notch_hover: true,
            spotlight_in_notch: true,
            desktop_widgets: true,
            show_workspaces: false,
            clock_24h: true,
            show_date: true,
            show_seconds: false,
            reduced_motion: false,
            opaque: false,
            lock: LockStyle::default(),
            desktop: DesktopMode::default(),
            bar_material: Material::Glass,
            dock_material: Material::Glass,
            notch_material: Material::Glass,
            accent: "blue".into(),
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
            notch_hover: flag(text, "notch_hover", d.notch_hover),
            spotlight_in_notch: flag(text, "spotlight_in_notch", d.spotlight_in_notch),
            desktop_widgets: flag(text, "desktop_widgets", d.desktop_widgets),
            show_workspaces: flag(text, "show_workspaces", d.show_workspaces),
            clock_24h: flag(text, "clock_24h", d.clock_24h),
            show_date: flag(text, "show_date", d.show_date),
            show_seconds: flag(text, "show_seconds", d.show_seconds),
            reduced_motion: flag(text, "reduced_motion", d.reduced_motion),
            opaque: flag(text, "opaque", d.opaque),
            lock: get(text, "lock_style")
                .and_then(LockStyle::parse)
                .unwrap_or(d.lock),
            desktop: get(text, "desktop_mode")
                .and_then(DesktopMode::parse)
                .unwrap_or(d.desktop),
            bar_material: get(text, "bar_material")
                .and_then(Material::parse)
                .unwrap_or(d.bar_material),
            dock_material: get(text, "dock_material")
                .and_then(Material::parse)
                .unwrap_or(d.dock_material),
            notch_material: get(text, "notch_material")
                .and_then(Material::parse)
                .unwrap_or(d.notch_material),
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
            ("notch_hover", b(self.notch_hover)),
            ("spotlight_in_notch", b(self.spotlight_in_notch)),
            ("desktop_widgets", b(self.desktop_widgets)),
            ("show_workspaces", b(self.show_workspaces)),
            ("clock_24h", b(self.clock_24h)),
            ("show_date", b(self.show_date)),
            ("show_seconds", b(self.show_seconds)),
            ("reduced_motion", b(self.reduced_motion)),
            ("opaque", b(self.opaque)),
            ("lock_style", Some(self.lock.as_str().into())),
            ("desktop_mode", Some(self.desktop.as_str().into())),
            ("bar_material", Some(self.bar_material.as_str().into())),
            ("dock_material", Some(self.dock_material.as_str().into())),
            ("notch_material", Some(self.notch_material.as_str().into())),
            ("accent", Some(self.accent.clone())),
        ]
    }

    /// The material a surface really uses: "Solid surfaces" (the
    /// accessibility switch) overrides every per-surface choice.
    pub fn material(&self, chosen: Material) -> Material {
        if self.opaque { Material::Solid } else { chosen }
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
        l.bar_material = Material::Acrylic;
        l.dock_material = Material::Clear;
        l.notch_material = Material::Glass;
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
        assert_eq!(
            Look::from_conf("lock_style=dashboard").lock,
            LockStyle::Dashboard
        );
        for v in DesktopMode::ALL {
            assert_eq!(DesktopMode::parse(v.as_str()), Some(*v));
        }
        for v in Material::ALL {
            assert_eq!(Material::parse(v.as_str()), Some(*v));
        }
    }

    #[test]
    fn solid_surfaces_switch_overrides_materials() {
        let mut l = Look::default();
        assert_eq!(l.material(l.bar_material), Material::Glass);
        l.opaque = true;
        assert_eq!(l.material(l.bar_material), Material::Solid);
    }
}
