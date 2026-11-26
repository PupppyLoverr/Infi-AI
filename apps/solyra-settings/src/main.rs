//! Solyra Settings. It edits the system's real configuration (the shell's
//! `shell.conf`, `defaults.toml`, PipeWire through `wpctl`) and every page
//! keeps the original tool one click away, so nothing here can lock you out
//! of the standard Linux way of doing the same thing.
//!
//! `--self-test` opens the window for two seconds and exits 0 (CI).

use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Duration};

use zgx::{compose::prelude::*, reactive::Signal};
use zgx_components::segmented;
use zgx_desktop::{Application, FileDialogOptions, FileFilter, WindowOptions};

mod apps;
mod system;
mod wallpaper;

use apps::Role;
use solyra_conf::look::Look;

const PAGES: [&str; 10] = [
    "Appearance",
    "Desktop",
    "Lock Screen",
    "Background",
    "Sound",
    "Network",
    "Default apps",
    "Agents",
    "Display",
    "About",
];

/// `Signal::set` returns whether the value changed; settings code only cares
/// that it was set.
trait Put<T> {
    fn put(&self, value: T);
}

impl<T: PartialEq + 'static> Put<T> for Signal<T> {
    fn put(&self, value: T) {
        self.set(value);
    }
}

fn conf_path() -> Option<PathBuf> {
    solyra_conf::config_path("shell.conf")
}

fn defaults_path() -> Option<PathBuf> {
    solyra_conf::config_path("defaults.toml")
}

fn read(path: Option<PathBuf>) -> String {
    path.and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default()
}

/// Write `pairs` into shell.conf and report the result in `status`.
fn save_conf(pairs: &[(&str, Option<String>)], status: &Signal<String>) {
    let Some(path) = conf_path() else {
        status.put("Cannot find your config folder (HOME is not set).".into());
        return;
    };
    match solyra_conf::update(&path, pairs) {
        Ok(()) => {
            let look = solyra_conf::look::Look::from_conf(&read(conf_path()));
            let _ = look.write_glass_kdl();
            status.put(String::new());
        }
        Err(e) => status.put(format!("Could not save {}: {e}", path.display())),
    }
}

fn heading(t: &str) -> View {
    text(t.to_string()).text_size(20.).font_bold().mb(4.)
}

fn caption(t: impl Into<String>) -> View {
    text(t.into()).text_size(12.).opacity(0.7).text_wrap(true).w_full()
}

fn action(label: &str, on_click: impl Fn() + 'static) -> View {
    button()
        .px(14.)
        .h(34.)
        .rounded(8.)
        .items_center()
        .child(text(label.to_string()))
        .on_click(on_click)
}

/// A button that opens the original tool for this page, with a visible reason
/// when it is not installed.
fn original_tool(label: &str, cmd: &'static [&'static str], status: Signal<String>) -> View {
    let installed = system::on_path(cmd[0]);
    let view = action(&format!("Open {label} (original)"), move || {
        if let Err(e) = system::launch(cmd) {
            status.put(format!("Could not open the original tool: {e}"));
        }
    });
    if installed { view } else { view.opacity(0.4) }
}

/// A segmented choice bound to a `shell.conf` key: shows the stored value and
/// writes the new one when the person picks another.
fn conf_choice(
    cx: &mut zgx::compose::Context,
    status: &Signal<String>,
    key: &'static str,
    label: &str,
    options: &'static [(&'static str, &'static str)],
    current: &str,
) -> View {
    let rt = cx.runtime();
    let sel = rt.signal(options.iter().position(|(v, _)| *v == current).unwrap_or(0));
    let (s, st) = (sel.clone(), status.clone());
    let last = Rc::new(RefCell::new(None::<usize>));
    cx.retain(rt.effect(move || {
        let v = s.get();
        if last.borrow_mut().replace(v).is_some_and(|p| p != v) {
            save_conf(&[(key, Some(options[v.min(options.len() - 1)].0.into()))], &st);
        }
    }));
    column()
        .gap(6.)
        .child(text(label.to_string()))
        .child(row().child(segmented(label.to_string(), sel, options.iter().map(|(_, l)| *l))))
}

/// A checkbox bound to a `shell.conf` flag (`1`/`0`).
fn conf_toggle(cx: &mut zgx::compose::Context, status: &Signal<String>, key: &'static str, label: &str, on: bool) -> View {
    let rt = cx.runtime();
    let val = rt.signal(on);
    let (v2, st) = (val.clone(), status.clone());
    let last = Rc::new(RefCell::new(None::<bool>));
    cx.retain(rt.effect(move || {
        let v = v2.get();
        if last.borrow_mut().replace(v).is_some_and(|p| p != v) {
            save_conf(&[(key, Some(if v { "1" } else { "0" }.into()))], &st);
        }
    }));
    checkbox(label.to_string(), val)
}

fn scrolling(cx: &mut zgx::compose::Context, body: View) -> View {
    let offset = cx.runtime().signal(0f32);
    scroll(offset).grow().w_full().child(body.w_full().pr(12.))
}

fn appearance_page(status: Signal<String>) -> View {
    component(move |cx| {
        let conf = read(conf_path());
        let look = Look::from_conf(&conf);
        let theme_now = solyra_conf::get(&conf, "theme").unwrap_or("dark").to_string();
        let app_glass = solyra_ui::current().glass || look.opaque;
        static MATERIALS: [(&str, &str); 4] = [
            ("solid", "Solid"),
            ("clear", "See-through"),
            ("glass", "Glass"),
            ("acrylic", "Acrylic"),
        ];
        static ACCENTS: [(&str, &str); 7] = [
            ("blue", "Blue"),
            ("teal", "Teal"),
            ("graphite", "Graphite"),
            ("green", "Green"),
            ("amber", "Amber"),
            ("rose", "Rose"),
            ("mauve", "Mauve"),
        ];
        let body = column()
            .gap(16.)
            .child(heading("Appearance"))
            .child(conf_choice(cx, &status, "theme", "Theme", &[("dark", "Dark"), ("light", "Light")], &theme_now))
            .child(conf_choice(cx, &status, "accent", "Accent colour", &ACCENTS, &look.accent))
            .child(text("Materials").font_bold())
            .child(caption("What the bar, dock and notch are made of. Glass blurs what is behind; Acrylic also takes the wallpaper's colour and adds a fine grain; See-through stays sharp."))
            .child(conf_choice(cx, &status, "bar_material", "Menu bar", &MATERIALS, look.bar_material.as_str()))
            .child(conf_choice(cx, &status, "dock_material", "Dock", &MATERIALS, look.dock_material.as_str()))
            .child(conf_choice(cx, &status, "notch_material", "Notch", &MATERIALS, look.notch_material.as_str()))
            .child(conf_toggle(cx, &status, "app_glass", "Frosted glass in Solyra apps", app_glass))
            .child(conf_toggle(cx, &status, "opaque", "Solid surfaces everywhere (overrides the materials above)", look.opaque))
            .child(conf_toggle(cx, &status, "reduced_motion", "Reduce motion", look.reduced_motion))
            .child(conf_toggle(cx, &status, "increased_contrast", "Increase contrast on glass", look.increased_contrast))
            .child(conf_choice(
                cx,
                &status,
                "refraction_intensity",
                "Refraction intensity",
                &[("0", "Off"), ("50", "Low"), ("100", "Normal"), ("150", "High")],
                &look.refraction_intensity.to_string(),
            ))
            .child(conf_choice(
                cx,
                &status,
                "blur_intensity",
                "Blur intensity",
                &[("50", "Soft"), ("100", "Normal"), ("150", "Heavy")],
                &look.blur_intensity.to_string(),
            ))
            .child(caption("Changes apply to the shell at once and to Solyra apps the next time they open."));
        scrolling(cx, body)
    })
}

fn desktop_page(status: Signal<String>) -> View {
    component(move |cx| {
        let look = Look::from_conf(&read(conf_path()));
        let body = column()
            .gap(16.)
            .child(heading("Desktop"))
            .child(text("Top bar").font_bold())
            .child(conf_choice(
                cx,
                &status,
                "bar_style",
                "Style",
                &[("menubar", "Menu bar"), ("compact", "Compact"), ("hidden", "Hidden")],
                look.bar.as_str(),
            ))
            .child(conf_toggle(cx, &status, "show_workspaces", "Show workspaces in the menu bar", look.show_workspaces))
            .child(conf_toggle(cx, &status, "clock_24h", "24-hour clock", look.clock_24h))
            .child(conf_toggle(cx, &status, "show_seconds", "Show seconds", look.show_seconds))
            .child(conf_toggle(cx, &status, "show_date", "Show the date", look.show_date))
            .child(text("Layout").font_bold())
            .child(conf_choice(
                cx,
                &status,
                "desktop_mode",
                "Desktop mode",
                &[("standard", "Standard"), ("agent", "Agent workspace")],
                look.desktop.as_str(),
            ))
            .child(caption("Agent workspace adds a desk card on the right: a large clock, a three-line project placeholder, and Cloud / Local / Worktree. Local is highlighted."))
            .child(text("Notch").font_bold())
            .child(conf_choice(
                cx,
                &status,
                "notch_style",
                "Live activity area",
                &[("notch", "Notch"), ("island", "Dynamic Island"), ("off", "Off")],
                look.notch.as_str(),
            ))
            .child(caption("Shows approvals, agent progress, notifications and what is playing, and a shelf you can drop files on."))
            .child(conf_toggle(cx, &status, "notch_hover", "Open the notch when the pointer rests on it", look.notch_hover))
            .child(conf_toggle(cx, &status, "spotlight_in_notch", "Spotlight opens inside the notch", look.spotlight_in_notch))
            .child(text("Widgets").font_bold())
            .child(conf_toggle(cx, &status, "desktop_widgets", "Calendar and weather on the desktop", look.desktop_widgets))
            .child(weather_city_row(cx, &status))
            .child(caption("Weather comes from Open-Meteo (free, no account) for the city you enter, every 30 minutes. Leave it empty and nothing is fetched."))
            .child(text("Dock").font_bold())
            .child(conf_choice(
                cx,
                &status,
                "dock_style",
                "Style",
                &[("solyra", "Solyra"), ("macos", "Floating"), ("windows", "Taskbar"), ("minimal", "Minimal")],
                look.dock.as_str(),
            ))
            .child(conf_choice(
                cx,
                &status,
                "dock_position",
                "Position on screen",
                &[("bottom", "Bottom"), ("left", "Left"), ("right", "Right")],
                look.dock_position.as_str(),
            ))
            .child(conf_toggle(cx, &status, "dock_magnify", "Magnify icons under the pointer (bottom dock)", look.dock_magnify))
            .child(conf_toggle(cx, &status, "dock_autohide", "Hide the dock until the pointer reaches the bottom edge", look.dock_autohide));
        scrolling(cx, body)
    })
}

/// The weather card's city, saved as `weather_city` in shell.conf.
fn weather_city_row(cx: &mut zgx::compose::Context, status: &Signal<String>) -> View {
    let now = solyra_conf::get(&read(conf_path()), "weather_city").unwrap_or("").to_string();
    let city = cx.runtime().signal(now);
    let (c2, st) = (city.clone(), status.clone());
    row()
        .gap(8.)
        .items_center()
        .child(text("City"))
        .child(text_input("Weather city", city).w(220.).h(32.).px(10.).rounded(8.))
        .child(action("Save", move || {
            let v = c2.get().trim().replace(['\n', '='], " ");
            save_conf(&[("weather_city", if v.is_empty() { None } else { Some(v) })], &st);
        }))
}

fn lock_page(status: Signal<String>) -> View {
    component(move |cx| {
        let look = Look::from_conf(&read(conf_path()));
        let st = status.clone();
        column()
            .gap(16.)
            .child(heading("Lock Screen"))
            .child(conf_choice(
                cx,
                &status,
                "lock_style",
                "Layout",
                &[
                    ("solyra", "Solyra"),
                    ("classic", "Centred"),
                    ("corner", "Clock corner"),
                    ("dashboard", "Dashboard"),
                ],
                look.lock.as_str(),
            ))
            .child(caption("Solyra: time and password in the middle. Centred: date and time at the top, your picture and password below. Clock corner: a large clock in the lower left; any key shows the sign-in card. Dashboard: large clock, nL capsule, avatars, agents and media glance."))
            .child(caption("The lock screen uses your wallpaper and accent colour, and the clock settings from Desktop."))
            .child(action("Lock now", move || {
                if let Err(e) = system::launch(&["solyra-panel", "lock"]) {
                    st.put(format!("Could not lock: {e}"));
                }
            }))
            .child(caption("Super+Ctrl+L locks at any time. If the lock screen ever fails, Super+Alt+L opens swaylock so you can always unlock."))
    })
}

fn background_page(
    status: Signal<String>,
    dialogs: zgx_desktop::FileDialogs,
    tasks: zgx_desktop::TaskSpawner,
) -> View {
    component(move |cx| {
        let rt = cx.runtime();
        let conf = read(conf_path());
        let mode_names = ["fill", "fit", "stretch", "center", "tile"];
        let mode = rt.signal(
            mode_names
                .iter()
                .position(|m| Some(*m) == solyra_conf::get(&conf, "wallpaper_mode"))
                .unwrap_or(0),
        );
        let current = rt.signal(
            solyra_conf::get(&conf, "wallpaper_file")
                .map(str::to_string)
                .unwrap_or_else(|| {
                    format!(
                        "Preset: {}",
                        solyra_conf::get(&conf, "wallpaper").unwrap_or("silk")
                    )
                }),
        );
        let imported = rt.signal(
            wallpaper::store_dir()
                .map(|d| wallpaper::list(&d))
                .unwrap_or_default(),
        );

        let apply_file = {
            let (status, current, mode) = (status.clone(), current.clone(), mode.clone());
            Rc::new(move |path: PathBuf| {
                let m = mode_names[mode.get().min(4)];
                save_conf(
                    &[
                        ("wallpaper_file", Some(path.to_string_lossy().into_owned())),
                        ("wallpaper_mode", Some(m.into())),
                    ],
                    &status,
                );
                current.put(path.to_string_lossy().into_owned());
            })
        };

        {
            // Changing the fit mode re-applies the current image.
            let (mode, current, status) = (mode.clone(), current.clone(), status.clone());
            let last = Rc::new(RefCell::new(None::<usize>));
            cx.retain(rt.effect(move || {
                let v = mode.get();
                if last.borrow_mut().replace(v).is_some_and(|p| p != v)
                    && !current.get().starts_with("Preset:")
                {
                    save_conf(
                        &[("wallpaper_mode", Some(mode_names[v.min(4)].into()))],
                        &status,
                    );
                }
            }));
        }

        let presets = ["silk", "beach", "violet", "ocean", "sunset", "forest", "graphite"]
            .into_iter()
            .fold(row().gap(8.).flex_wrap(), |r, name| {
                let (status, current) = (status.clone(), current.clone());
                r.child(action(&capitalise(name), move || {
                    save_conf(
                        &[("wallpaper", Some(name.into())), ("wallpaper_file", None)],
                        &status,
                    );
                    current.put(format!("Preset: {name}"));
                }))
            });

        let choose = {
            let (status, apply_file, imported, dialogs, tasks) = (
                status.clone(),
                apply_file.clone(),
                imported.clone(),
                dialogs.clone(),
                tasks.clone(),
            );
            action("Choose an image…", move || {
                let (status, apply_file, imported, dialogs) = (
                    status.clone(),
                    apply_file.clone(),
                    imported.clone(),
                    dialogs.clone(),
                );
                tasks.spawn(async move {
                    let opts = FileDialogOptions::new("Choose a background")
                        .filter(FileFilter::new("Images", ["png", "jpg", "jpeg"]));
                    match dialogs.open_file(opts).await {
                        Ok(Some(src)) => {
                            let Some(dir) = wallpaper::store_dir() else {
                                status
                                    .put("Cannot find your data folder (HOME is not set).".into());
                                return;
                            };
                            match wallpaper::import(&src, &dir) {
                                Ok(dest) => {
                                    apply_file(dest);
                                    imported.put(wallpaper::list(&dir));
                                }
                                Err(e) => status.put(e),
                            }
                        }
                        Ok(None) => {}
                        Err(e) => status.put(format!("Could not open the file chooser: {e}")),
                    }
                });
            })
        };

        let list = {
            let (imported, apply_file, status) =
                (imported.clone(), apply_file.clone(), status.clone());
            switch(
                {
                    let imported = imported.clone();
                    move || imported.get()
                },
                move |files, _cx| {
                    if files.is_empty() {
                        return caption(
                            "Images you choose appear here so you can switch back to them.",
                        );
                    }
                    files.into_iter().fold(column().gap(6.), |col, f| {
                        let name = f
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("?")
                            .to_string();
                        let (apply_file, f1) = (apply_file.clone(), f.clone());
                        let (imported, status, f2) = (imported.clone(), status.clone(), f.clone());
                        col.child(
                            row()
                                .gap(8.)
                                .items_center()
                                .child(text(name).grow())
                                .child(action("Use", move || apply_file(f1.clone())))
                                .child(action("Delete", move || {
                                    let Some(dir) = wallpaper::store_dir() else {
                                        return;
                                    };
                                    match wallpaper::remove(&dir, &f2) {
                                        Ok(()) => imported.put(wallpaper::list(&dir)),
                                        Err(e) => status.put(e),
                                    }
                                })),
                        )
                    })
                },
            )
        };

        column()
            .gap(14.)
            .child(heading("Background"))
            .child(text_signal({
                let c = current.clone();
                move || format!("Now: {}", c.get())
            }))
            .child(text("Presets"))
            .child(presets)
            .child(text("Your own image"))
            .child(row().gap(8.).child(choose))
            .child(text("Fit"))
            .child(segmented(
                "Fit",
                mode,
                ["Fill", "Fit", "Stretch", "Center", "Tile"],
            ))
            .child(caption(
                "PNG or JPEG. Images are copied into ~/.local/share/solyra/wallpapers.",
            ))
            .child(list)
    })
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn sound_page(status: Signal<String>) -> View {
    component(move |cx| {
        let rt = cx.runtime();
        let (vol, muted) = system::get_volume().unwrap_or((0, false));
        let available = system::get_volume().is_some();
        let level = rt.signal(f32::from(vol.min(100)));
        let mute = rt.signal(muted);
        {
            let (level, s) = (level.clone(), status.clone());
            let last = Rc::new(RefCell::new(None::<u8>));
            cx.retain(rt.effect(move || {
                let v = level.get().round() as u8;
                if last.borrow_mut().replace(v).is_some_and(|p| p != v) {
                    if let Err(e) = system::set_volume(v) {
                        s.put(e);
                    }
                }
            }));
        }
        {
            let (mute, s) = (mute.clone(), status.clone());
            let last = Rc::new(RefCell::new(None::<bool>));
            cx.retain(rt.effect(move || {
                let v = mute.get();
                if last.borrow_mut().replace(v).is_some_and(|p| p != v) {
                    if let Err(e) = system::set_mute(v) {
                        s.put(e);
                    }
                }
            }));
        }
        let body: View = if available {
            column()
                .gap(10.)
                .child(text_signal({
                    let l = level.clone();
                    move || format!("Output volume: {}%", l.get().round() as u8)
                }))
                .child(slider("Output volume", level, 0.0..=100.0).w(360.))
                .child(checkbox("Muted", mute))
        } else {
            caption(
                "No audio service answered (wpctl). Sound controls are unavailable on this system.",
            )
        };
        column()
            .gap(14.)
            .child(heading("Sound"))
            .child(body)
            .child(original_tool(
                "volume control",
                &["pavucontrol"],
                status.clone(),
            ))
    })
}

fn network_page(status: Signal<String>) -> View {
    let devices = system::network_devices();
    let list: View = match devices {
        None => caption("NetworkManager did not answer (nmcli). Network details are unavailable."),
        Some(d) if d.is_empty() => caption("No network devices found."),
        Some(d) => d.into_iter().fold(column().gap(6.), |col, dev| {
            let line = if dev.connection.is_empty() {
                format!("{}  ·  {}  ·  {}", dev.device, dev.kind, dev.state)
            } else {
                format!(
                    "{}  ·  {}  ·  {}  ·  {}",
                    dev.device, dev.kind, dev.state, dev.connection
                )
            };
            col.child(text(line))
        }),
    };
    let tool: &'static [&'static str] = if system::on_path("nm-connection-editor") {
        &["nm-connection-editor"]
    } else {
        &["foot", "-e", "nmtui"]
    };
    column()
        .gap(14.)
        .child(heading("Network"))
        .child(list)
        .child(caption("Connections are managed by NetworkManager. Use the original tool to add or change them."))
        .child(original_tool("network editor", tool, status))
}

fn default_apps_page(status: Signal<String>) -> View {
    component(move |cx| {
        let rt = cx.runtime();
        let text_now = read(defaults_path());
        let mut col = column().gap(14.).child(heading("Default apps"));
        for role in Role::ALL {
            let options = apps::available(role, system::on_path);
            let current = apps::chosen(&text_now, role);
            let sel = rt.signal(options.iter().position(|c| c.bin == current).unwrap_or(0));
            let labels: Vec<String> = options
                .iter()
                .map(|c| {
                    if c.original {
                        format!("{} (original)", c.name)
                    } else {
                        c.name.to_string()
                    }
                })
                .collect();
            {
                let (sel, s) = (sel.clone(), status.clone());
                let last = Rc::new(RefCell::new(None::<usize>));
                cx.retain(rt.effect(move || {
                    let v = sel.get();
                    if last.borrow_mut().replace(v).is_some_and(|p| p != v) {
                        let bin = options[v.min(options.len() - 1)].bin;
                        let Some(path) = defaults_path() else {
                            s.put("Cannot find your config folder (HOME is not set).".into());
                            return;
                        };
                        let current = std::fs::read_to_string(&path).unwrap_or_default();
                        match apps::set(&current, role, bin) {
                            Ok(new) => {
                                if let Some(dir) = path.parent() {
                                    let _ = std::fs::create_dir_all(dir);
                                }
                                match std::fs::write(&path, new) {
                                    Ok(()) => s.put(String::new()),
                                    Err(e) => {
                                        s.put(format!("Could not save {}: {e}", path.display()))
                                    }
                                }
                            }
                            Err(e) => s.put(e),
                        }
                    }
                }));
            }
            col = col
                .child(text(role.label()))
                .child(segmented(role.label(), sel, labels));
        }
        let reset_status = status.clone();
        col.child(caption("The original app is always installed and always listed. Super+Enter, Super+E and Super+B use these choices."))
            .child(action("Back to the originals", move || {
                let Some(path) = defaults_path() else { return };
                let current = std::fs::read_to_string(&path).unwrap_or_default();
                match std::fs::write(&path, apps::reset_all(&current)) {
                    Ok(()) => reset_status.put("Defaults are back to the original apps. Reopen this page to see them.".into()),
                    Err(e) => reset_status.put(format!("Could not save {}: {e}", path.display())),
                }
            }))
    })
}

fn agent_conf_path() -> Option<PathBuf> {
    solyra_conf::config_path("agent.conf")
}

fn agents_page(status: Signal<String>) -> View {
    component(move |cx| {
        let rt = cx.runtime();
        let conf = read(agent_conf_path());
        let wrap = rt.signal(solyra_conf::get(&conf, "wrap") != Some("0"));
        {
            let (wrap, s) = (wrap.clone(), status.clone());
            let last = Rc::new(RefCell::new(None::<bool>));
            cx.retain(rt.effect(move || {
                let v = wrap.get();
                if last.borrow_mut().replace(v).is_some_and(|p| p != v) {
                    let Some(path) = agent_conf_path() else {
                        s.put("Cannot find your config folder (HOME is not set).".into());
                        return;
                    };
                    match solyra_conf::update(
                        &path,
                        &[("wrap", Some(if v { "1" } else { "0" }.into()))],
                    ) {
                        Ok(()) => s.put("Open a new terminal for this to take effect.".into()),
                        Err(e) => s.put(format!("Could not save {}: {e}", path.display())),
                    }
                }
            }));
        }
        let preview = rt.signal(String::new());
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(|d| PathBuf::from(d).join("solyra-agent/tasks").is_dir())
            .unwrap_or(false);
        let have_agent = system::on_path("solyra-agent");
        let (p1, s1) = (preview.clone(), status.clone());
        let (p2, s2) = (preview.clone(), status.clone());
        column()
            .gap(14.)
            .child(heading("Agents"))
            .child(text(if runtime {
                "An agent is reporting to Solyra right now."
            } else {
                "No agent has reported yet. Agents appear in the notch when they start."
            }))
            .child(checkbox("Show agents in the notch and let me reply from there", wrap))
            .child(caption("Runs Claude Code, Codex, Gemini, OpenCode and Aider in your terminal through solyra-agent. They behave as before; Solyra only adds a status report and lets you type to them. Bypass once with `command claude`."))
            .child(text("Claude Code approvals"))
            .child(caption("Adds hooks to ~/.claude/settings.json so permission prompts also appear in the notch. If you do not answer, Claude asks in the terminal as usual. A backup is kept."))
            .child(
                row()
                    .gap(8.)
                    .child(action("Preview the change", move || {
                        match system::run_capture("solyra-agent", &["install-hooks", "claude"]) {
                            Ok(t) => {
                                p1.put(t);
                                s1.put(String::new());
                            }
                            Err(e) => s1.put(e),
                        }
                    }))
                    .child(action("Install hooks", move || {
                        match system::run_capture("solyra-agent", &["install-hooks", "claude", "--apply"]) {
                            Ok(t) => {
                                p2.put(t.lines().last().unwrap_or("Done.").to_string());
                                s2.put(String::new());
                            }
                            Err(e) => s2.put(e),
                        }
                    })),
            )
            .child(if have_agent { caption("") } else { caption("solyra-agent is not installed on this system.") })
            .child(text_signal({
                let p = preview.clone();
                move || p.get()
            }).text_size(11.))
    })
}

fn display_page(status: Signal<String>) -> View {
    let outputs = std::process::Command::new("niri")
        .args(["msg", "outputs"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let body: View = match outputs {
        Some(t) if !t.is_empty() => t.lines().fold(column().gap(2.), |c, l| {
            c.child(text(l.to_string()).text_size(12.))
        }),
        _ => caption("The compositor did not report displays (niri msg outputs)."),
    };
    column()
        .gap(14.)
        .child(heading("Display"))
        .child(body)
        .child(caption("Resolution, scale and arrangement are set in ~/.config/solyra/comp.kdl (outputs). A graphical editor is planned."))
        .child(original_tool("config folder in Files", &["thunar", "--", ".config/solyra"], status))
}

fn about_page() -> View {
    let name = system::pretty_name(&std::fs::read_to_string("/etc/os-release").unwrap_or_default())
        .unwrap_or_else(|| "Unknown system".into());
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    column()
        .gap(8.)
        .child(heading("About"))
        .child(text(name))
        .child(text(format!("Kernel {}", kernel.trim())))
        .child(text(format!("Computer name {}", host.trim())))
        .child(caption(format!(
            "Solyra Settings {}",
            env!("CARGO_PKG_VERSION")
        )))
}

/// `--page <name>` opens a page directly (the menu bar's "About This Computer"
/// uses `--page about`). Names match the sidebar, case-insensitively, with
/// spaces or dashes.
fn start_page() -> usize {
    let args: Vec<String> = std::env::args().collect();
    let Some(name) = args.iter().position(|a| a == "--page").and_then(|i| args.get(i + 1)) else {
        return 0;
    };
    page_index(name)
}

fn page_index(name: &str) -> usize {
    let want = name.to_ascii_lowercase().replace(['-', '_'], " ");
    PAGES
        .iter()
        .position(|p| p.to_ascii_lowercase() == want)
        .unwrap_or(0)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let self_test = std::env::args().any(|a| a == "--self-test");
    let style = solyra_ui::current();
    let theme = style.theme;
    Application::new()
        .window(WindowOptions {
            title: "Settings".into(),
            width: 880.,
            height: 620.,
            min_size: Some((640., 420.)),
            transparent: style.glass,
            ..Default::default()
        })
        .run(move |cx| {
            let rt = cx.ui.runtime.clone();
            let page = rt.signal(start_page());
            let status = rt.signal(String::new());
            let dialogs = cx.dialogs.clone();
            let tasks = cx.tasks.clone();

            let sidebar = PAGES.iter().enumerate().fold(
                column()
                    .gap(2.)
                    .p(12.)
                    .pt(18.)
                    .w(200.)
                    .h_full()
                    .bg(style.sidebar_bg())
                    .border_edges(zgx::scene::Insets { top: 0., right: 1., bottom: 0., left: 0. })
                    .border_color(style.hairline())
                    .child(
                        text("Settings")
                            .text_size(13.)
                            .font_bold()
                            .text_color(solyra_ui::color(theme.text_dim))
                            .px(12.)
                            .mb(8.),
                    ),
                |col, (i, name)| col.child(solyra_ui::sidebar_item(&style, name, i, page.clone())),
            );

            let st = status.clone();
            let content = switch(
                {
                    let page = page.clone();
                    move || page.get()
                },
                move |i, _cx| match i {
                    0 => appearance_page(st.clone()),
                    1 => desktop_page(st.clone()),
                    2 => lock_page(st.clone()),
                    3 => background_page(st.clone(), dialogs.clone(), tasks.clone()),
                    4 => sound_page(st.clone()),
                    5 => network_page(st.clone()),
                    6 => default_apps_page(st.clone()),
                    7 => agents_page(st.clone()),
                    8 => display_page(st.clone()),
                    _ => about_page(),
                },
            );

            cx.render(solyra_ui::scope(
                &style,
                {
                    let w_close = cx.window.clone();
                    let w_zoom = cx.window.clone();
                    let maxed = std::rc::Rc::new(std::cell::Cell::new(false));
                    let ctl = solyra_ui::chrome::WindowCtl {
                        close: std::rc::Rc::new(move || w_close.close()),
                        zoom: std::rc::Rc::new(move || {
                            let m = !maxed.get();
                            maxed.set(m);
                            w_zoom.set_maximized(m);
                        }),
                    };
                    let bar = solyra_ui::chrome::titlebar(&cx.ui.runtime.clone(), &style, "System Settings", ctl, None);
                    column()
                        .w_full()
                        .h_full()
                        .child(bar)
                        .child(
                row()
                    .w_full()
                    .h_full()
                    .text_color(solyra_ui::color(theme.text))
                    .child(sidebar)
                    .child(
                        column()
                            .grow()
                            .h_full()
                            .bg(style.content_bg())
                            .p(28.)
                            .gap(12.)
                            .child(content.grow().w_full())
                            .child(
                                text_signal({
                                    let s = status.clone();
                                    move || s.get()
                                })
                                .text_color(solyra_ui::color(theme.danger)),
                            ),
                    ).grow())
                },
            ));

            if self_test {
                let window = cx.window.clone();
                cx.tasks.spawn(async move {
                    zgx::timer::sleep(Duration::from_secs(2)).await;
                    window.close();
                });
            }
        })
}
