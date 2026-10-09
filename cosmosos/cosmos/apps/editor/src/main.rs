//! cosmos-editor — a small real text editor for CosmosOS.
//! Open/save real files, Ctrl+S, dirty tracking, word/line counts.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use cosmos_kit::ask;
use cosmos_kit::controls::{button, text_field, ButtonKind};
use cosmos_kit::layout::{
    menu_item, menu_section, menu_separator, sheet, status_text, submenu, toolbar_button,
    toolbar_spacer, toolbar_title, AppWindow,
};
use cosmos_kit::{Icon, Kit};
use cosmos_theme::space;
use egui::text::{LayoutJob, TextFormat};

struct Editor {
    path: Option<PathBuf>,
    text: String,
    dirty: bool,
    status: String,
    open_field: String,
    show_open: bool,
    cursor: (usize, usize),
    /// Selected char range, sorted; `None` when the selection is empty.
    selection: Option<(usize, usize)>,
    /// Ask Cosmos job and the char range a Rewrite would replace.
    ask: Option<(ask::Job, (usize, usize))>,
    ask_pending: Option<(ask::Action, (usize, usize))>,
}

fn byte_at(s: &str, ch: usize) -> usize {
    s.char_indices().nth(ch).map_or(s.len(), |(b, _)| b)
}

fn char_slice(s: &str, (a, b): (usize, usize)) -> &str {
    &s[byte_at(s, a)..byte_at(s, b)]
}

impl Editor {
    fn new() -> Self {
        let mut e = Editor {
            path: None,
            text: String::new(),
            dirty: false,
            status: String::new(),
            open_field: String::new(),
            show_open: false,
            cursor: (1, 1),
            selection: None,
            ask: None,
            ask_pending: None,
        };
        // `cosmos-editor <file>` opens that file directly.
        if let Some(arg) = std::env::args().nth(1) {
            e.load(PathBuf::from(arg));
        }
        e
    }

    fn load(&mut self, path: PathBuf) {
        match fs::read_to_string(&path) {
            Ok(s) => {
                self.text = s;
                self.path = Some(path.clone());
                self.dirty = false;
                self.status = format!("loaded {}", path.display());
            }
            Err(e) => {
                // A missing file becomes a new buffer at that path — standard
                // editor behaviour, not a fake success.
                if e.kind() == std::io::ErrorKind::NotFound {
                    self.text.clear();
                    self.path = Some(path.clone());
                    self.dirty = false;
                    self.status = format!("new file {}", path.display());
                } else {
                    self.status = format!("load failed: {e}");
                }
            }
        }
    }

    fn save(&mut self) {
        let Some(path) = self.path.clone() else {
            self.show_open = true;
            self.status = "choose a path to save".into();
            return;
        };
        match fs::write(&path, &self.text) {
            Ok(()) => {
                self.dirty = false;
                self.status = format!("saved {}", path.display());
            }
            Err(e) => self.status = format!("save failed: {e}"),
        }
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let mut ed = Editor::new();
    if let Err(e) = cosmos_uitk::run("Editor", "cosmos.editor", (720, 520), move |ui| {
        draw(ui, &mut ed)
    }) {
        tracing::error!("cosmos-editor fatal: {e}");
        std::process::exit(1);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    Rust,
    Toml,
    Python,
    Shell,
    Plain,
}

impl Lang {
    fn of(path: Option<&PathBuf>) -> Lang {
        match path.and_then(|p| p.extension()).and_then(|e| e.to_str()) {
            Some("rs") => Lang::Rust,
            Some("toml") | Some("conf") | Some("ini") => Lang::Toml,
            Some("py") => Lang::Python,
            Some("sh") | Some("bash") => Lang::Shell,
            _ => Lang::Plain,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Lang::Rust => "Rust",
            Lang::Toml => "TOML",
            Lang::Python => "Python",
            Lang::Shell => "Shell",
            Lang::Plain => "Plain Text",
        }
    }
    fn keywords(self) -> &'static [&'static str] {
        match self {
            Lang::Rust => &[
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match",
                "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct",
                "super", "trait", "true", "type", "unsafe", "use", "where", "while",
            ],
            Lang::Toml => &["true", "false"],
            Lang::Python => &[
                "and", "as", "assert", "break", "class", "continue", "def", "elif", "else",
                "except", "False", "finally", "for", "from", "if", "import", "in", "is", "lambda",
                "None", "not", "or", "pass", "raise", "return", "True", "try", "while", "with",
                "yield",
            ],
            Lang::Shell => &[
                "case", "do", "done", "elif", "else", "esac", "export", "fi", "for", "function",
                "if", "in", "local", "then", "while",
            ],
            Lang::Plain => &[],
        }
    }
    fn line_comment(self) -> &'static str {
        match self {
            Lang::Rust => "//",
            Lang::Plain => "",
            _ => "#",
        }
    }
}

const CODE_PT: f32 = 13.0;

/// Single-pass tokenizer: comments, strings, numbers, keywords.
fn highlight(src: &str, lang: Lang, kit: &Kit) -> LayoutJob {
    let font = egui::FontId::monospace(CODE_PT);
    let text = kit.text();
    let comment = kit.text3();
    let keyword = kit.accent();
    let (string, number) = if kit.dark {
        (
            egui::Color32::from_rgb(0x9E, 0xCE, 0x8A),
            egui::Color32::from_rgb(0xE5, 0xB5, 0x7A),
        )
    } else {
        (
            egui::Color32::from_rgb(0x2F, 0x7A, 0x3A),
            egui::Color32::from_rgb(0xA3, 0x5A, 0x00),
        )
    };
    let mut job = LayoutJob::default();
    let lc = lang.line_comment();
    let kws = lang.keywords();
    let mut i = 0;
    while i < src.len() {
        let rest = &src[i..];
        let c = rest.chars().next().unwrap_or(' ');
        let (len, color) = if lang == Lang::Plain {
            (rest.len(), text)
        } else if !lc.is_empty() && rest.starts_with(lc) {
            (rest.find('\n').unwrap_or(rest.len()), comment)
        } else if lang == Lang::Rust && rest.starts_with("/*") {
            (
                rest.find("*/").map(|e| e + 2).unwrap_or(rest.len()),
                comment,
            )
        } else if c == '"' || (c == '\'' && lang != Lang::Rust) {
            let mut end = rest.len();
            let mut esc = false;
            for (j, ch) in rest.char_indices().skip(1) {
                if esc {
                    esc = false;
                } else if ch == '\\' {
                    esc = true;
                } else if ch == c {
                    end = j + 1;
                    break;
                } else if ch == '\n' && lang != Lang::Rust && lang != Lang::Python {
                    end = j;
                    break;
                }
            }
            (end, string)
        } else if c.is_ascii_digit() {
            let n = rest
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '.'))
                .unwrap_or(rest.len());
            (n, number)
        } else if c.is_alphabetic() || c == '_' {
            let n = rest
                .find(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                .unwrap_or(rest.len());
            (
                n,
                if kws.contains(&&rest[..n]) {
                    keyword
                } else {
                    text
                },
            )
        } else {
            (c.len_utf8(), text)
        };
        let len = len.max(c.len_utf8());
        job.append(&rest[..len], 0.0, TextFormat::simple(font.clone(), color));
        i += len;
    }
    job
}

/// 1-based line/column of a char index.
fn line_col(text: &str, char_idx: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for ch in text.chars().take(char_idx) {
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn draw(ui: &mut egui::Ui, ed: &mut Editor) {
    let kit = Kit::get(ui.ctx());
    if ui
        .ctx()
        .input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
    {
        ed.save();
    }
    if ui
        .ctx()
        .input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O))
    {
        ed.open_field = ed
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        ed.show_open = true;
    }
    let lang = Lang::of(ed.path.as_ref());
    let name = ed
        .path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into());
    let title = name;
    let cell = std::cell::RefCell::new(&mut *ed);
    AppWindow::new()
        .toolbar(|ui| {
            let mut ed = cell.borrow_mut();
            toolbar_title(ui, &title);
            if ed.dirty {
                let (r, resp) =
                    ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(r.center(), 3.5, cosmos_kit::Kit::get(ui.ctx()).text2());
                resp.on_hover_text("Unsaved changes");
            }
            toolbar_spacer(ui, 2.0 * 28.0 + 4.0);
            ui.spacing_mut().item_spacing.x = 4.0;
            if toolbar_button(ui, Icon::FolderOpen, "Open… (Ctrl+O)", false).clicked() {
                ed.open_field = ed
                    .path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                ed.show_open = true;
            }
            if toolbar_button(ui, Icon::Save, "Save (Ctrl+S)", false).clicked() {
                ed.save();
            }
        })
        .status(|ui| {
            let ed = cell.borrow();
            let lines = ed.text.lines().count().max(1);
            let words = ed.text.split_whitespace().count();
            status_text(ui, &format!("Ln {}, Col {}", ed.cursor.0, ed.cursor.1));
            status_text(ui, &format!("· {lines} lines · {words} words"));
            if !ed.status.is_empty() {
                status_text(ui, &format!("· {}", ed.status));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                status_text(ui, "UTF-8");
                status_text(ui, lang.name());
            });
        })
        .show(ui, |ui| {
            let mut ed = cell.borrow_mut();
            let ed = &mut **ed;
            let gutter_w = 48.0;
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let (gutter, _) = ui.allocate_exact_size(
                            egui::vec2(gutter_w, ui.available_height()),
                            egui::Sense::hover(),
                        );
                        let mut layouter = |ui: &egui::Ui,
                                            buf: &dyn egui::TextBuffer,
                                            _w: f32|
                         -> Arc<egui::Galley> {
                            let job = highlight(buf.as_str(), lang, &kit);
                            ui.fonts_mut(|f| f.layout_job(job))
                        };
                        let before = ed.text.len();
                        let out = egui::TextEdit::multiline(&mut ed.text)
                            .font(egui::FontId::monospace(CODE_PT))
                            .frame(egui::Frame::NONE)
                            .margin(egui::vec2(space::S8, space::S8))
                            .desired_width(f32::INFINITY)
                            .desired_rows(30)
                            .lock_focus(true)
                            .layouter(&mut layouter)
                            .show(ui);
                        if out.response.changed() || ed.text.len() != before {
                            ed.dirty = true;
                        }
                        if let Some(r) = out.cursor_range {
                            ed.cursor = line_col(&ed.text, r.primary.index.into());
                            let (a, b): (usize, usize) =
                                (r.primary.index.into(), r.secondary.index.into());
                            ed.selection = (a != b).then(|| (a.min(b), a.max(b)));
                        }
                        out.response.context_menu(|ui| text_menu(ui, ed));
                        let painter = ui.painter();
                        let mut n = 1;
                        let mut new_line = true;
                        for row in &out.galley.rows {
                            if new_line {
                                let y = out.galley_pos.y + row.rect().center().y;
                                painter.text(
                                    egui::pos2(gutter.right() - space::S8, y),
                                    egui::Align2::RIGHT_CENTER,
                                    n.to_string(),
                                    egui::FontId::monospace(CODE_PT - 1.0),
                                    if n == ed.cursor.0 {
                                        kit.text2()
                                    } else {
                                        kit.text3()
                                    },
                                );
                                n += 1;
                            }
                            new_line = row.ends_with_newline;
                        }
                        painter.vline(
                            gutter.right() - 0.5,
                            gutter.y_range(),
                            egui::Stroke::new(1.0, kit.hairline()),
                        );
                    });
                });
        });

    let ctx = ui.ctx().clone();
    if let Some((a, range)) = ed.ask_pending.take() {
        let sel = char_slice(&ed.text, range).to_string();
        ed.ask = Some((ask::Job::start(&ctx, a, ask::Subject::Text(sel)), range));
    }
    if let Some((job, range)) = &ed.ask {
        match ask::result_sheet(&ctx, job, true) {
            ask::SheetResult::Open => {}
            ask::SheetResult::Closed => ed.ask = None,
            ask::SheetResult::Replace(t) => {
                let (a, b) = (byte_at(&ed.text, range.0), byte_at(&ed.text, range.1));
                ed.text.replace_range(a..b, &t);
                ed.dirty = true;
                ed.selection = None;
                ed.ask = None;
            }
        }
    }

    if ed.show_open {
        let mut done = false;
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let open = sheet(
            &ctx,
            egui::Id::new("editor-open"),
            "Open or Save As",
            |ui| {
                text_field(
                    ui,
                    &mut ed.open_field,
                    "/home/cosmos/file.txt",
                    380.0,
                    false,
                )
                .request_focus();
                ui.add_space(space::S16);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let p = ed.open_field.trim().to_string();
                    if (button(ui, ButtonKind::Primary, "Open").clicked() || enter) && !p.is_empty()
                    {
                        ed.load(PathBuf::from(p.clone()));
                        done = true;
                    }
                    if button(ui, ButtonKind::Secondary, "Save As").clicked() && !p.is_empty() {
                        ed.path = Some(PathBuf::from(p));
                        ed.save();
                        done = true;
                    }
                    if button(ui, ButtonKind::Plain, "Cancel").clicked() {
                        done = true;
                    }
                });
            },
        );
        if !open || done {
            ed.show_open = false;
        }
    }
}

/// Right-click menu in the text area: Copy, then the Ask Cosmos group
/// for the current selection.
fn text_menu(ui: &mut egui::Ui, ed: &mut Editor) {
    let Some(range) = ed.selection else {
        menu_section(ui, "Ask Cosmos");
        ui.add_enabled_ui(false, |ui| {
            menu_item(ui, Some(Icon::Sparkle), "Select text to ask", None)
        });
        return;
    };
    if menu_item(ui, Some(Icon::Clipboard), "Copy", Some("Ctrl+C")).clicked() {
        ui.ctx().copy_text(char_slice(&ed.text, range).to_string());
        ui.close();
    }
    menu_separator(ui);
    menu_section(ui, "Ask Cosmos");
    if !ask::provider_configured() {
        if menu_item(ui, Some(Icon::Sparkle), "Set up an agent…", None).clicked() {
            if let Err(e) = ask::open_onboarding() {
                ed.status = format!("can't open Agents: {e}");
            }
            ui.close();
        }
        return;
    }
    submenu(ui, Some(Icon::Sparkle), "Ask Cosmos", |ui| {
        for a in [
            ask::Action::Summarize,
            ask::Action::Explain,
            ask::Action::Rewrite,
        ] {
            if menu_item(ui, None, a.label(), None).clicked() {
                ed.ask_pending = Some((a, range));
                ui.close();
            }
        }
        menu_separator(ui);
        if menu_item(ui, Some(Icon::Terminal), "Open in Agent", None).clicked() {
            let dir = ed
                .path
                .clone()
                .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()));
            if let Err(e) = ask::open_in_agent(&dir) {
                ed.status = format!("can't open the agent: {e}");
            }
            ui.close();
        }
    });
}

#[cfg(test)]
mod ask_tests {
    use super::*;

    #[test]
    fn char_ranges_are_utf8_safe() {
        let s = "héllo wörld";
        assert_eq!(char_slice(s, (6, 11)), "wörld");
        assert_eq!(byte_at(s, 99), s.len());
    }
}
