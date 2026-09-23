//! A bounded, single-directory browser. It never recursively scans a workspace.
use crate::{
    app::draw_field,
    clipboard::Clipboard,
    projection::safe_text,
    search::{FieldMap, Focus},
    ui,
    workspace::clipped,
};
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use eymi::Document;
use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    widgets::{Clear, Paragraph},
};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};
use unicode_width::UnicodeWidthStr;

const MAX_ENTRIES: usize = 2048;
const SNIFF_BYTES: u64 = 512;

#[derive(Clone, Debug)]
struct Entry {
    path: PathBuf,
    directory: bool,
}

#[derive(Clone, Copy)]
enum Hit {
    Entry(usize),
    Parent,
    Hidden,
    All,
    Close,
    Path,
}

pub enum Action {
    None,
    Close,
    Open(PathBuf),
}

pub struct Browser {
    directory: PathBuf,
    entries: Vec<Entry>,
    selected: usize,
    scroll: usize,
    path: Document,
    path_focus: bool,
    hidden: bool,
    all: bool,
    message: String,
    hits: Vec<(Rect, Hit)>,
    visible_rows: usize,
    path_map: Option<FieldMap>,
    ready: bool,
}

impl Browser {
    pub fn new(directory: PathBuf) -> io::Result<Self> {
        let directory = fs::canonicalize(directory)?;
        let mut browser = Self {
            path: Document::new(directory.display().to_string()),
            directory,
            entries: vec![],
            selected: 0,
            scroll: 0,
            path_focus: false,
            hidden: false,
            all: false,
            message: String::new(),
            hits: vec![],
            visible_rows: 1,
            path_map: None,
            ready: true,
        };
        browser.reload()?;
        Ok(browser)
    }

    pub fn set_error(&mut self, error: impl ToString) {
        self.message = error.to_string();
    }

    fn reload(&mut self) -> io::Result<()> {
        let mut entries = vec![];
        let mut truncated = false;
        let mut skipped = 0;
        for (index, item) in fs::read_dir(&self.directory)?.enumerate() {
            if index >= MAX_ENTRIES {
                truncated = true;
                break;
            }
            let item = match item {
                Ok(item) => item,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            if !self.hidden && item.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = item.path();
            let metadata = match fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    // Keep inaccessible entries reachable so opening can report the cause.
                    entries.push(Entry {
                        path,
                        directory: false,
                    });
                    continue;
                }
            };
            let directory = metadata.is_dir();
            if directory || self.all || (metadata.is_file() && text_candidate(&path)) {
                entries.push(Entry { path, directory });
            }
        }
        entries.sort_by(|a, b| {
            b.directory
                .cmp(&a.directory)
                .then_with(|| a.path.file_name().cmp(&b.path.file_name()))
        });
        self.entries = entries;
        self.selected = 0;
        self.scroll = 0;
        self.hits.clear();
        self.message = if truncated {
            format!("Listing limited to {MAX_ENTRIES} entries; use Path to open omitted names.")
        } else if skipped > 0 {
            format!("{skipped} unreadable entries skipped; direct paths remain available.")
        } else {
            String::new()
        };
        Ok(())
    }

    fn navigate(&mut self, directory: PathBuf) {
        let previous = self.directory.clone();
        self.directory = directory;
        match self.reload() {
            Ok(()) => {
                self.path = Document::new(self.directory.display().to_string());
                self.path_focus = false;
            }
            Err(error) => {
                self.directory = previous;
                self.set_error(error);
            }
        }
    }

    fn activate_path(&mut self, path: PathBuf) -> Action {
        match fs::metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {
                match fs::canonicalize(path) {
                    Ok(path) => self.navigate(path),
                    Err(error) => self.set_error(error),
                }
                Action::None
            }
            Ok(_) => Action::Open(path),
            Err(error) => {
                self.set_error(error);
                Action::None
            }
        }
    }

    fn enter(&mut self) -> Action {
        if self.path_focus {
            let typed = PathBuf::from(self.path.text());
            let path = if typed.is_absolute() {
                typed
            } else {
                self.directory.join(typed)
            };
            self.activate_path(path)
        } else if let Some(entry) = self.entries.get(self.selected) {
            self.activate_path(entry.path.clone())
        } else {
            Action::None
        }
    }

    fn parent(&mut self) {
        if let Some(parent) = self.directory.parent() {
            self.navigate(parent.to_owned());
        }
    }

    fn focus_path(&mut self) {
        self.path_focus = true;
        self.path.select_all();
    }

    fn move_selection(&mut self, delta: isize) {
        self.path_focus = false;
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.entries.len().saturating_sub(1));
    }

    fn toggle(&mut self, hidden: bool) {
        if hidden {
            self.hidden = !self.hidden;
        } else {
            self.all = !self.all;
        }
        if let Err(error) = self.reload() {
            self.set_error(error);
        }
    }

    fn insert_path_text(&mut self, text: &str) {
        let text = text.replace(['\n', '\r', '\0'], "");
        if !text.is_empty() {
            self.path.insert(&text);
        }
    }

    pub fn handle_event(&mut self, event: Event, clipboard: &mut Clipboard) -> Action {
        if let Event::Resize(width, height) = &event {
            self.ready = *width >= 12 && *height >= 9;
        }
        if !self.ready
            && !matches!(
                &event,
                Event::Resize(..)
                    | Event::Key(crossterm::event::KeyEvent {
                        code: KeyCode::Esc,
                        ..
                    })
            )
        {
            return Action::None;
        }
        let mut action = Action::None;
        match event {
            Event::Resize(..) => {}
            Event::Paste(text) if self.path_focus => {
                self.insert_path_text(&text);
            }
            Event::Key(key) => {
                let control = key.modifiers.contains(KeyModifiers::CONTROL);
                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                match key.code {
                    KeyCode::Esc => action = Action::Close,
                    KeyCode::Tab | KeyCode::BackTab => {
                        if self.path_focus {
                            self.path_focus = false;
                        } else {
                            self.focus_path();
                        }
                    }
                    KeyCode::F(2) => self.toggle(true),
                    KeyCode::F(5) => self.toggle(false),
                    KeyCode::Enter => action = self.enter(),
                    KeyCode::Up => self.move_selection(-1),
                    KeyCode::Down => self.move_selection(1),
                    KeyCode::PageUp => self.move_selection(-(self.visible_rows as isize)),
                    KeyCode::PageDown => self.move_selection(self.visible_rows as isize),
                    KeyCode::Backspace if !self.path_focus => self.parent(),
                    KeyCode::Char('/') if !self.path_focus => self.focus_path(),
                    KeyCode::Char('a') if control && self.path_focus => self.path.select_all(),
                    KeyCode::Char('c' | 'x') if control && self.path_focus => {
                        if !self.path.selection().is_empty() {
                            self.message = clipboard.copy(self.path.selected_text());
                            if key.code == KeyCode::Char('x') {
                                self.path.insert("");
                            }
                        }
                    }
                    KeyCode::Char('v') if control && self.path_focus => {
                        let (text, message) = clipboard.paste();
                        self.message = message;
                        if let Some(text) = text {
                            self.insert_path_text(&text);
                        }
                    }
                    KeyCode::Char('z') if control && self.path_focus => {
                        self.path.undo();
                    }
                    KeyCode::Char('y') if control && self.path_focus => {
                        self.path.redo();
                    }
                    KeyCode::Left if self.path_focus => self.path.move_left(shift),
                    KeyCode::Right if self.path_focus => self.path.move_right(shift),
                    KeyCode::Home if self.path_focus => {
                        let head = 0;
                        let anchor = if shift {
                            self.path.selection().anchor
                        } else {
                            head
                        };
                        let _ = self.path.set_selection(eymi::Selection { anchor, head });
                    }
                    KeyCode::End if self.path_focus => {
                        let head = self.path.text().len();
                        let anchor = if shift {
                            self.path.selection().anchor
                        } else {
                            head
                        };
                        let _ = self.path.set_selection(eymi::Selection { anchor, head });
                    }
                    KeyCode::Backspace if self.path_focus => {
                        self.path.backspace();
                    }
                    KeyCode::Delete if self.path_focus => {
                        self.path.delete_forward();
                    }
                    KeyCode::Char(c)
                        if self.path_focus
                            && !key
                                .modifiers
                                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        self.path.insert(&c.to_string());
                    }
                    _ => {}
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    let hit = self.hits.iter().find_map(|(rect, hit)| {
                        rect.contains((mouse.column, mouse.row).into())
                            .then_some(*hit)
                    });
                    match hit {
                        Some(Hit::Entry(index)) => {
                            self.selected = index;
                            self.path_focus = false;
                            action = self.enter();
                        }
                        Some(Hit::Parent) => self.parent(),
                        Some(Hit::Hidden) => self.toggle(true),
                        Some(Hit::All) => self.toggle(false),
                        Some(Hit::Close) => action = Action::Close,
                        Some(Hit::Path) => {
                            self.path_focus = true;
                            if let Some(map) = &self.path_map {
                                let head = map.hit(mouse.column);
                                let anchor = if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                                    self.path.selection().anchor
                                } else {
                                    head
                                };
                                let _ = self.path.set_selection(eymi::Selection { anchor, head });
                            }
                        }
                        None => {}
                    }
                }
                MouseEventKind::ScrollDown => self.move_selection(3),
                MouseEventKind::ScrollUp => self.move_selection(-3),
                _ => {}
            },
            _ => {}
        }
        self.hits.clear();
        self.path_map = None;
        action
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        self.hits.clear();
        let area = frame.area();
        self.ready = area.width >= 12 && area.height >= 9;
        if !self.ready {
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new("Open: enlarge window. Esc cancels.").style(ui::surface()),
                area,
            );
            return;
        }
        let width = (area.width - 2).min(96);
        let popup = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + 1,
            width,
            area.height - 2,
        );
        let count = format!(
            "{} {}",
            self.entries.len(),
            if self.entries.len() == 1 {
                "item"
            } else {
                "items"
            }
        );
        let inner = ui::panel(frame, popup, "Open file", Some(&count));
        let colors = crate::theme::palette();
        let chrome = crate::theme::chrome_palette();
        let field = Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(2), 1);
        self.path_map = Some(draw_field(
            frame,
            field,
            "❯ ",
            &self.path,
            Focus::Query,
            self.path_focus,
            None,
        ));
        self.hits.push((field, Hit::Path));
        let toggle = |on: bool, name: &str| format!("{} {name}", if on { "●" } else { "○" });
        let controls = [
            ("↑ Up".to_owned(), Hit::Parent, false),
            (toggle(self.hidden, "Hidden"), Hit::Hidden, self.hidden),
            (toggle(self.all, "All files"), Hit::All, self.all),
            ("Close".to_owned(), Hit::Close, false),
        ];
        let mut x = inner.x + 1;
        for (label, hit, on) in controls {
            let label = format!(" {label} ");
            let cells = UnicodeWidthStr::width(label.as_str()) as u16;
            if x + cells > inner.right() {
                break;
            }
            let style = if on {
                chrome.tab_active.style().add_modifier(Modifier::BOLD)
            } else {
                ui::surface().bg(chrome.tab_inactive.background)
            };
            frame.buffer_mut().set_string(x, inner.y + 1, &label, style);
            self.hits.push((Rect::new(x, inner.y + 1, cells, 1), hit));
            x += cells + 1;
        }
        self.visible_rows = usize::from(inner.height.saturating_sub(3));
        if self.selected < self.scroll {
            self.scroll = self.selected;
        }
        if self.selected >= self.scroll + self.visible_rows {
            self.scroll = (self.selected + 1).saturating_sub(self.visible_rows);
        }
        let icons = crate::icons::current();
        for (row, (index, entry)) in self
            .entries
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(self.visible_rows)
            .enumerate()
        {
            let rect = Rect::new(inner.x, inner.y + 2 + row as u16, inner.width, 1);
            let selected = index == self.selected && !self.path_focus;
            let base = if selected {
                chrome.tab_active.style()
            } else {
                ui::surface()
            };
            let buffer = frame.buffer_mut();
            buffer.set_style(rect, base);
            if selected {
                buffer.set_string(rect.x, rect.y, "▌", base.fg(chrome.tab_indicator));
            }
            let name = safe_text(&entry.path.file_name().unwrap_or_default().to_string_lossy());
            let icon = if icons == crate::icons::IconSet::Plain {
                ""
            } else if entry.directory {
                "\u{f024b}"
            } else {
                icons.path(&entry.path)
            };
            let label = format!(
                "{}{name}{}",
                if icon.is_empty() {
                    String::new()
                } else {
                    format!("{icon} ")
                },
                if entry.directory { "/" } else { "" }
            );
            let style = if entry.directory {
                base.fg(colors.accent)
            } else {
                base
            };
            let style = if selected {
                style.add_modifier(Modifier::BOLD)
            } else {
                style
            };
            let room = usize::from(rect.width.saturating_sub(3));
            buffer.set_stringn(rect.x + 2, rect.y, clipped(&label, room), room, style);
            self.hits.push((rect, Hit::Entry(index)));
        }
        if inner.height >= 4 {
            frame.render_widget(
                Paragraph::new(clipped(
                    &safe_text(&self.message),
                    usize::from(inner.width.saturating_sub(2)),
                ))
                .style(ui::muted()),
                Rect::new(
                    inner.x + 1,
                    inner.bottom() - 1,
                    inner.width.saturating_sub(2),
                    1,
                ),
            );
        }
        ui::hints(
            frame,
            popup,
            "⏎ open · ⌫ up · tab path · F2 hidden · F5 all · esc close",
        );
    }
}

fn text_candidate(path: &Path) -> bool {
    if let Some(extension) = path.extension().and_then(|s| s.to_str()) {
        match extension.to_ascii_lowercase().as_str() {
            "md" | "markdown" | "mdown" | "mkd" | "mdx" | "txt" | "rs" | "toml" | "json"
            | "yaml" | "yml" | "js" | "ts" | "tsx" | "jsx" | "html" | "css" | "py" | "sh"
            | "csv" | "xml" | "log" | "ini" | "conf" | "sql" | "go" | "c" | "h" | "cpp"
            | "java" | "rb" | "svg" => return true,
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "pdf" | "zip" | "gz" | "xz" | "mp3"
            | "mp4" | "mov" | "exe" | "dll" | "so" | "dylib" | "woff" | "woff2" | "ttf" | "bin"
            | "ico" => return false,
            _ => {}
        }
    }
    sniff_text(path).unwrap_or(true)
}

fn sniff_text(path: &Path) -> io::Result<bool> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Ok(false);
    }
    let mut bytes = vec![];
    file.take(SNIFF_BYTES).read_to_end(&mut bytes)?;
    if bytes.contains(&0) {
        return Ok(false);
    }
    Ok(match std::str::from_utf8(&bytes) {
        Ok(_) => true,
        Err(error) => error.error_len().is_none(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, MouseEvent};
    use ratatui::{Terminal, backend::TestBackend};

    fn key(browser: &mut Browser, code: KeyCode) -> Action {
        browser.handle_event(
            Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
            &mut Clipboard::internal(),
        )
    }
    fn draw(browser: &mut Browser, width: u16, height: u16) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| browser.draw(frame)).unwrap();
    }
    fn names(browser: &Browser) -> Vec<String> {
        browser
            .entries
            .iter()
            .map(|entry| {
                entry
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    #[test]
    fn lists_directories_first_sniffs_extensionless_and_exposes_hidden_and_all_toggles() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("z-folder")).unwrap();
        fs::write(dir.path().join("a.md"), "# hello").unwrap();
        fs::write(dir.path().join("LICENSE"), "text\n").unwrap();
        fs::write(dir.path().join("opaque"), [0, 255]).unwrap();
        fs::write(dir.path().join("picture.png"), [1, 2]).unwrap();
        fs::write(dir.path().join(".agents.md"), "hidden").unwrap();
        let mut browser = Browser::new(dir.path().to_owned()).unwrap();
        assert_eq!(names(&browser), ["z-folder", "LICENSE", "a.md"]);
        key(&mut browser, KeyCode::F(2));
        assert!(names(&browser).contains(&".agents.md".into()));
        key(&mut browser, KeyCode::F(5));
        assert!(names(&browser).contains(&"opaque".into()));
        assert!(names(&browser).contains(&"picture.png".into()));
    }

    #[test]
    fn keyboard_parent_and_direct_paths_preserve_failed_navigation() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("child")).unwrap();
        fs::write(dir.path().join("child/note.md"), "body").unwrap();
        let mut browser = Browser::new(dir.path().to_owned()).unwrap();
        key(&mut browser, KeyCode::Enter);
        assert!(browser.directory.ends_with("child"));
        key(&mut browser, KeyCode::Backspace);
        assert_eq!(browser.directory, fs::canonicalize(dir.path()).unwrap());
        key(&mut browser, KeyCode::Tab);
        browser.handle_event(
            Event::Paste("child/note.md".into()),
            &mut Clipboard::internal(),
        );
        let Action::Open(path) = key(&mut browser, KeyCode::Enter) else {
            panic!("expected direct open")
        };
        assert!(path.ends_with("child/note.md"));
        browser.path.select_all();
        browser.path.insert("missing/no.md");
        assert!(matches!(key(&mut browser, KeyCode::Enter), Action::None));
        assert_eq!(browser.directory, fs::canonicalize(dir.path()).unwrap());
        assert!(!browser.entries.is_empty());
    }

    #[test]
    fn rendered_controls_and_entries_are_clickable_and_resize_invalidates_hits() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "a").unwrap();
        let mut browser = Browser::new(dir.path().to_owned()).unwrap();
        draw(&mut browser, 80, 24);
        let rect = browser
            .hits
            .iter()
            .find(|(_, hit)| matches!(hit, Hit::Hidden))
            .unwrap()
            .0;
        let mouse = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        };
        browser.handle_event(Event::Mouse(mouse), &mut Clipboard::internal());
        assert!(browser.hidden);
        browser.handle_event(
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                ..mouse
            }),
            &mut Clipboard::internal(),
        );
        assert!(browser.hidden);
        draw(&mut browser, 80, 24);
        let row = browser
            .hits
            .iter()
            .find(|(_, hit)| matches!(hit, Hit::Entry(0)))
            .unwrap()
            .0;
        browser.handle_event(Event::Resize(12, 7), &mut Clipboard::internal());
        assert!(matches!(
            browser.handle_event(
                Event::Mouse(MouseEvent {
                    column: row.x,
                    row: row.y,
                    ..mouse
                }),
                &mut Clipboard::internal()
            ),
            Action::None
        ));
        draw(&mut browser, 80, 24);
        assert!(matches!(
            browser.handle_event(
                Event::Mouse(MouseEvent {
                    column: row.x,
                    row: row.y,
                    ..mouse
                }),
                &mut Clipboard::internal()
            ),
            Action::Open(_)
        ));
        for (w, h) in [(1, 1), (8, 5), (12, 7), (30, 12)] {
            draw(&mut browser, w, h);
        }
    }

    #[test]
    fn path_field_uses_grapheme_geometry_and_shared_clipboard() {
        let dir = tempfile::tempdir().unwrap();
        let mut browser = Browser::new(dir.path().to_owned()).unwrap();
        let mut clipboard = Clipboard::internal();
        clipboard.copy("long-path/界👩🏽‍💻/note.md");
        key(&mut browser, KeyCode::Tab);
        browser.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL)),
            &mut clipboard,
        );
        assert_eq!(browser.path.text(), "long-path/界👩🏽‍💻/note.md");
        draw(&mut browser, 24, 12);
        let map = browser.path_map.as_ref().unwrap();
        assert!(map.start > 0);
        let glyph = map.glyphs.first().unwrap();
        let offset = glyph.source.start;
        let mouse = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: glyph.column,
            row: map.area.y,
            modifiers: KeyModifiers::NONE,
        };
        browser.handle_event(Event::Mouse(mouse), &mut clipboard);
        assert_eq!(browser.path.selection().head, offset);
        assert!(browser.path.is_grapheme_boundary(offset));
    }

    #[test]
    fn directory_scan_is_bounded_and_sniff_handles_utf8_split_at_sample_edge() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..MAX_ENTRIES + 1 {
            fs::write(dir.path().join(format!("{index}.md")), "").unwrap();
        }
        let browser = Browser::new(dir.path().to_owned()).unwrap();
        assert!(browser.entries.len() <= MAX_ENTRIES);
        assert!(browser.message.contains("limited"));
        let sample = dir.path().join("sample");
        fs::write(
            &sample,
            format!("{}界", "a".repeat(SNIFF_BYTES as usize - 1)),
        )
        .unwrap();
        assert!(sniff_text(&sample).unwrap());
        fs::write(&sample, "abc\0def").unwrap();
        assert!(!sniff_text(&sample).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn fifo_candidates_are_never_opened_for_sniffing() {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe");
        let name = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(!sniff_text(&fifo).unwrap());
        let mut browser = Browser::new(dir.path().to_owned()).unwrap();
        assert!(browser.entries.is_empty());
        key(&mut browser, KeyCode::F(5));
        assert_eq!(browser.entries.len(), 1);
    }
    #[test]
    fn empty_or_sanitized_empty_path_paste_preserves_selection() {
        let dir = tempfile::tempdir().unwrap();
        let mut browser = Browser::new(dir.path().to_owned()).unwrap();
        let mut clipboard = Clipboard::internal();
        key(&mut browser, KeyCode::Tab);
        let text = browser.path.text().to_owned();
        let selection = browser.path.selection();
        for paste in ["", "\n\r\0"] {
            browser.handle_event(Event::Paste(paste.into()), &mut clipboard);
            assert_eq!(browser.path.text(), text);
            assert_eq!(browser.path.selection(), selection);
        }
        clipboard.copy("\n\r\0");
        browser.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL)),
            &mut clipboard,
        );
        assert_eq!(browser.path.text(), text);
        assert_eq!(browser.path.selection(), selection);
        assert!(!browser.path.can_undo());
    }

    #[test]
    fn tiny_browser_cannot_activate_an_unseen_row_or_mutate_path() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("one.md"), "one").unwrap();
        let mut browser = Browser::new(dir.path().to_owned()).unwrap();
        for (width, height) in [(80, 7), (80, 8), (11, 24)] {
            draw(&mut browser, width, height);
            assert!(!browser.ready);
            assert!(browser.hits.is_empty());
            assert!(matches!(key(&mut browser, KeyCode::Enter), Action::None));
            assert!(matches!(key(&mut browser, KeyCode::Esc), Action::Close));
        }
        browser.handle_event(Event::Resize(80, 24), &mut Clipboard::internal());
        draw(&mut browser, 80, 24);
        assert!(matches!(key(&mut browser, KeyCode::Enter), Action::Open(_)));
        browser.handle_event(Event::Resize(80, 8), &mut Clipboard::internal());
        assert!(matches!(key(&mut browser, KeyCode::Enter), Action::None));
    }
}
