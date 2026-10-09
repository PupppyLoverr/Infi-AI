//! cosmos-kit-gallery — every kit component in one window. Run it with
//! `COSMOS_CONFIG` pointing at a config whose `appearance` is "light" or
//! "dark" to review either theme (docs/evidence/kit-gallery-*.png).

use cosmos_kit::controls::{
    button, search_field, segmented, segmented_with, slider, text_field, toggle, ButtonKind,
};
use cosmos_kit::layout::{
    card, dropdown, grid_tile, group, menu_frame, menu_item, menu_separator, popover, sheet,
    sidebar_item, sidebar_section, status_text, tab_strip, toolbar_button, toolbar_spacer,
    toolbar_title, AppWindow, ColAlign, Column, TabEvent, Table,
};
use cosmos_kit::{icons, Icon, Kit};

#[derive(Default)]
struct Body {
    toggle_a: bool,
    toggle_b: bool,
    volume: f32,
    seg: usize,
    name: String,
    password: String,
    choice: usize,
    row: usize,
    tile: usize,
    tab: usize,
    tabs: Vec<String>,
    pop: bool,
    sheet: bool,
}

fn heading(ui: &mut egui::Ui, kit: &Kit, s: &str) {
    ui.add_space(12.0);
    ui.label(egui::RichText::new(s).size(15.0).strong().color(kit.text()));
    ui.add_space(4.0);
}

fn main() -> anyhow::Result<()> {
    let mut side = 0usize;
    let mut view = 0usize;
    let mut query = String::new();
    let mut b = Body {
        toggle_a: true,
        volume: 0.6,
        name: "Ada".into(),
        tabs: vec!["main.rs".into(), "kit.md".into(), "Cargo.toml".into()],
        ..Default::default()
    };
    cosmos_uitk::run(
        "Kit Gallery",
        "cosmos-kit-gallery",
        (1100, 760),
        move |ui| {
            let kit = Kit::get(ui.ctx());
            let (side, view, query, b) = (&mut side, &mut view, &mut query, &mut b);
            AppWindow::new()
                .toolbar(|ui| {
                    toolbar_button(ui, Icon::Back, "Back", false);
                    toolbar_button(ui, Icon::Forward, "Forward", false);
                    toolbar_title(ui, "Kit Gallery");
                    toolbar_spacer(ui, 64.0 + 12.0 + 200.0 + 12.0 + 28.0);
                    segmented_with(ui, view, 2, 32.0, |ui, i, r, c| {
                        let icon = [Icon::List, Icon::Grid][i];
                        icons::paint(
                            ui,
                            icon,
                            egui::Rect::from_center_size(r.center(), egui::vec2(16.0, 16.0)),
                            c,
                        );
                    });
                    ui.add_space(12.0);
                    search_field(ui, query, "Search", 200.0);
                    ui.add_space(12.0);
                    toolbar_button(ui, Icon::FolderPlus, "New folder", false);
                })
                .sidebar(|ui| {
                    sidebar_section(ui, "FAVOURITES");
                    for (i, (icon, l)) in [
                        (Icon::Home, "Home"),
                        (Icon::Desktop, "Desktop"),
                        (Icon::Document, "Documents"),
                        (Icon::Download, "Downloads"),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        if sidebar_item(ui, icon, l, *side == i).clicked() {
                            *side = i;
                        }
                    }
                    sidebar_section(ui, "LOCATIONS");
                    for (i, (icon, l)) in [(Icon::Disk, "CosmosOS"), (Icon::Trash, "Trash")]
                        .into_iter()
                        .enumerate()
                    {
                        if sidebar_item(ui, icon, l, *side == 4 + i).clicked() {
                            *side = 4 + i;
                        }
                    }
                })
                .status(|ui| status_text(ui, "Every cosmos-kit component · 44 icons"))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Frame::new().inner_margin(20.0).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                heading(ui, &kit, "Buttons");
                                ui.horizontal(|ui| {
                                    button(ui, ButtonKind::Primary, "Save");
                                    button(ui, ButtonKind::Secondary, "Cancel");
                                    button(ui, ButtonKind::Plain, "Learn more");
                                    button(ui, ButtonKind::Destructive, "Move to Trash");
                                    ui.add_enabled_ui(false, |ui| {
                                        button(ui, ButtonKind::Primary, "Disabled")
                                    });
                                });
                                heading(ui, &kit, "Controls");
                                ui.horizontal(|ui| {
                                    toggle(ui, &mut b.toggle_a);
                                    toggle(ui, &mut b.toggle_b);
                                    ui.add_space(16.0);
                                    slider(ui, &mut b.volume, 0.0..=1.0, 200.0);
                                    ui.add_space(16.0);
                                    segmented(ui, &mut b.seg, &["Light", "Dark", "Auto"]);
                                    ui.add_space(16.0);
                                    dropdown(
                                        ui,
                                        "accent",
                                        &mut b.choice,
                                        &["Wallpaper", "Violet", "Ocean", "Coral"],
                                        160.0,
                                    );
                                });
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    text_field(ui, &mut b.name, "Name", 200.0, false);
                                    text_field(ui, &mut b.password, "Password", 200.0, true);
                                    let r = button(ui, ButtonKind::Secondary, "Popover");
                                    if r.clicked() {
                                        b.pop = !b.pop;
                                    }
                                    popover(
                                        ui.ctx(),
                                        egui::Id::new("gallery-pop"),
                                        r.rect,
                                        &mut b.pop,
                                        |ui| {
                                            ui.set_width(220.0);
                                            ui.label("Popovers use the menu material.");
                                        },
                                    );
                                    if button(ui, ButtonKind::Secondary, "Sheet").clicked() {
                                        b.sheet = true;
                                    }
                                });
                                if b.sheet {
                                    b.sheet = sheet(
                                        ui.ctx(),
                                        egui::Id::new("gallery-sheet"),
                                        "Rename item",
                                        |ui| {
                                            text_field(ui, &mut b.name, "Name", 320.0, false);
                                            ui.add_space(12.0);
                                            ui.horizontal(|ui| {
                                                button(ui, ButtonKind::Secondary, "Cancel");
                                                button(ui, ButtonKind::Primary, "Rename");
                                            });
                                        },
                                    );
                                }
                                heading(ui, &kit, "Grouped settings");
                                group(ui, None, |g| {
                                    g.row("Dark appearance", |ui| {
                                        toggle(ui, &mut b.toggle_a);
                                    });
                                    g.row_detail(
                                        "Reduce motion",
                                        Some("Cross-fade instead of zoom"),
                                        |ui| {
                                            toggle(ui, &mut b.toggle_b);
                                        },
                                    );
                                    g.row("Volume", |ui| {
                                        slider(ui, &mut b.volume, 0.0..=1.0, 180.0);
                                    });
                                });
                                heading(ui, &kit, "Tabs");
                                match tab_strip(ui, b.tab, &b.tabs, true) {
                                    Some(TabEvent::Select(i)) => b.tab = i,
                                    Some(TabEvent::Close(i)) if b.tabs.len() > 1 => {
                                        b.tabs.remove(i);
                                        b.tab = b.tab.min(b.tabs.len() - 1);
                                    }
                                    Some(TabEvent::New) => {
                                        b.tabs.push(format!("untitled-{}", b.tabs.len() + 1));
                                        b.tab = b.tabs.len() - 1;
                                    }
                                    _ => {}
                                }
                                heading(ui, &kit, "Table");
                                card(ui, |ui| {
                                    let cols = [
                                        Column {
                                            title: "Name",
                                            width: 0.0,
                                            align: ColAlign::Left,
                                        },
                                        Column {
                                            title: "Size",
                                            width: 90.0,
                                            align: ColAlign::Right,
                                        },
                                        Column {
                                            title: "Kind",
                                            width: 140.0,
                                            align: ColAlign::Left,
                                        },
                                    ];
                                    let t = Table { cols: &cols };
                                    t.header(ui);
                                    let rows = [
                                        ("Projects", "—", "Folder", true),
                                        ("notes.md", "4 KB", "Markdown", false),
                                        ("photo.png", "1.2 MB", "PNG image", false),
                                    ];
                                    for (i, (n, s, k, dir)) in rows.into_iter().enumerate() {
                                        let lead = move |ui: &egui::Ui, r: egui::Rect| {
                                            let c = if dir { kit.accent() } else { kit.text2() };
                                            icons::paint(
                                                ui,
                                                if dir { Icon::Folder } else { Icon::Document },
                                                r,
                                                c,
                                            );
                                        };
                                        if t.row(ui, b.row == i, Some(&lead), &[n, s, k]).clicked()
                                        {
                                            b.row = i;
                                        }
                                    }
                                });
                                heading(ui, &kit, "Grid");
                                ui.horizontal_wrapped(|ui| {
                                    for (i, n) in [
                                        "Projects",
                                        "Wallpapers",
                                        "a-long-file-name-that-wraps.txt",
                                        "music.flac",
                                    ]
                                    .iter()
                                    .enumerate()
                                    {
                                        let icon = [
                                            Icon::Folder,
                                            Icon::Image,
                                            Icon::Document,
                                            Icon::Music,
                                        ][i];
                                        if grid_tile(ui, b.tile == i, n, |ui, r| {
                                            icons::paint(ui, icon, r.shrink(8.0), kit.accent())
                                        })
                                        .clicked()
                                        {
                                            b.tile = i;
                                        }
                                    }
                                });
                                heading(ui, &kit, "Menu");
                                menu_frame(&kit).show(ui, |ui| {
                                    ui.set_width(240.0);
                                    menu_item(ui, Some(Icon::FolderOpen), "Open", Some("Ctrl+O"));
                                    menu_item(ui, Some(Icon::Pencil), "Rename", Some("F2"));
                                    menu_separator(ui);
                                    menu_item(ui, Some(Icon::Trash), "Move to Trash", Some("Del"));
                                });
                                heading(ui, &kit, "Icons");
                                ui.horizontal_wrapped(|ui| {
                                    for &i in Icon::ALL {
                                        icons::show(ui, i, 20.0, kit.text())
                                            .on_hover_text(i.name());
                                        ui.add_space(6.0);
                                    }
                                });
                            });
                        });
                });
        },
    )
}
