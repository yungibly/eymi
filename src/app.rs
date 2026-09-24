use crate::syntax::Language;
use crate::{
    clipboard::{self, Clipboard},
    file_io::{ExternalChange, FileState},
    projection::{Affinity, Fill, FillKind, Parsed, Projection, safe_text},
    search::{
        Action as SearchAction, Button as SearchButton, FieldDrag, FieldGlyph, FieldMap, Focus,
        Geometry as SearchGeometry, Search, contains,
    },
};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use eymi::{Document, InlineStyle, Selection};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};
use std::{cell::Cell, io, path::PathBuf};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(crate) use crate::theme::document_style;
use crate::theme::{self, Theme, palette};
use crate::ui;

pub(crate) fn chrome_style() -> Style {
    let colors = palette();
    Style::default().fg(colors.chrome_text).bg(colors.chrome)
}

pub(crate) fn chrome_muted() -> Style {
    chrome_style().fg(palette().chrome_muted)
}

pub(crate) fn chrome_active() -> Style {
    chrome_style()
        .fg(palette().accent)
        .bg(palette().active)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}

fn chrome_field() -> Style {
    chrome_style().bg(palette().field)
}

fn chrome_focus() -> Style {
    chrome_style().fg(palette().accent)
}

/// The keyboard reference, grouped the way people look for it.
const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "Writing",
        &[
            ("Ctrl+B", "Bold"),
            ("Alt+I", "Italic"),
            ("Alt+`", "Inline code"),
            ("Ctrl+T", "Toggle task"),
            ("Enter", "Continue a list"),
            ("Alt+Enter", "Literal newline"),
            ("Tab / Shift+Tab", "Indent / outdent lines"),
            ("Ctrl+] / Ctrl+[", "Indent / outdent lines"),
            ("Alt+↑ / Alt+↓", "Move lines"),
            ("Alt+Shift+↑ / ↓", "Duplicate lines"),
            ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
        ],
    ),
    (
        "Selecting",
        &[
            ("Shift+arrows", "Extend the selection"),
            ("Ctrl/Alt+← / →", "Move by word"),
            ("Ctrl/Alt+Backspace", "Delete a word"),
            ("Ctrl+A", "Select all"),
            ("Ctrl+C / X / V", "Copy / cut / paste"),
        ],
    ),
    (
        "Moving around",
        &[
            ("Home / End", "Start / end of row"),
            ("Ctrl+Home / End", "Start / end of document"),
            ("Ctrl+G", "Go to line"),
            ("F11", "Go to heading"),
            ("F9", "Focus or hide the outline"),
        ],
    ),
    (
        "Finding",
        &[
            ("Ctrl+F", "Find"),
            ("Ctrl+R", "Find and replace"),
            ("F3 / Shift+F3", "Next / previous match"),
            ("Alt+R / Alt+A", "Replace one / all"),
            ("Tab", "Switch Find and With"),
        ],
    ),
    (
        "Documents",
        &[
            ("Ctrl+N", "New document"),
            ("Ctrl+O", "Open a file"),
            ("Ctrl+S", "Save"),
            ("F4 / Ctrl+Shift+S", "Save as"),
            ("F5", "Reload from disk"),
            ("Ctrl+W", "Close tab"),
            ("F7 / F8", "Previous / next tab"),
            ("F10", "Switch document"),
            ("Ctrl+Q", "Quit"),
        ],
    ),
    (
        "Viewing",
        &[
            ("Ctrl+E / F6", "Live or source view"),
            ("F2 / Ctrl+P", "Commands, themes, icons"),
            ("F1", "This reference"),
        ],
    ),
];

type LayoutKey = (
    u64,
    Vec<std::ops::Range<usize>>,
    usize,
    bool,
    Theme,
    crate::icons::IconSet,
);

#[derive(Debug, PartialEq, Eq)]
enum Overlay {
    None,
    Help,
    Quit,
    Reload,
    SaveAs { path: String, quit_after: bool },
    Search { replace: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MessageOrigin {
    General,
    Clipboard,
    Disk,
}

pub struct App {
    pub document: Document,
    file: Option<FileState>,
    pub live: bool,
    markdown: bool,
    /// Detected for other text files; Markdown highlights fenced code itself.
    language: Option<&'static Language>,
    parsed: Parsed,
    pub projection: Projection,
    pub viewport: Rect,
    pub scroll: usize,
    preferred_column: Option<usize>,
    affinity: Affinity,
    dragging: bool,
    follow_cursor: bool,
    /// Set by a navigation jump: the next draw places the caret's row at a
    /// reading position instead of just inside the viewport.
    reveal: bool,
    anchor_screen_row: Option<usize>,
    clipboard: Clipboard,
    search: Search,
    search_geometry: SearchGeometry,
    field_drag: Option<FieldDrag>,
    overlay: Overlay,
    help_scroll: usize,
    reload_prompt_visible: Cell<bool>,
    last_disk_change: Option<ExternalChange>,
    word_count: Cell<Option<(u64, usize)>>,
    line_count: Cell<Option<(u64, usize)>>,
    pub should_exit: bool,
    message: String,
    message_is_error: bool,
    message_origin: MessageOrigin,
    layout_key: Option<LayoutKey>,
    editor_area: Rect,
    terminal_height: u16,
    terminal_width: u16,
}

impl App {
    pub(crate) fn open_bounded(path: PathBuf, max_bytes: usize) -> io::Result<Self> {
        let markdown = is_markdown(&path);
        let language = (!markdown).then(|| Language::for_path(&path)).flatten();
        let (text, file) = FileState::open_bounded(path, max_bytes)?;
        if text.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "This file contains binary NUL bytes; choose a UTF-8 text file",
            ));
        }
        let mut app = Self::new(text, Some(file), markdown);
        if language.is_some() {
            app.language = language;
            app.parsed = app.analysis();
        }
        Ok(app)
    }

    /// Restore recovery content in a detached, explicitly unsaved tab. Selection
    /// is clamped to complete graphemes; even an empty buffer remains dirty until
    /// Save As succeeds. Restoring never writes to its former disk path.
    pub(crate) fn recovered(
        text: String,
        selection: Selection,
        live: bool,
        markdown: bool,
    ) -> Self {
        let mut app = Self::new(text, None, markdown);
        app.document = Document::unsaved(app.document.text());
        let selection = Selection {
            anchor: app.document.floor_grapheme_boundary(selection.anchor),
            head: app.document.floor_grapheme_boundary(selection.head),
        };
        app.document
            .set_selection(selection)
            .expect("clamped recovery selection");
        app.live = live && markdown;
        app
    }

    /// Poll advisory disk status without changing document text, selection or
    /// typing groups. Returns whether the visible status needs a redraw.
    pub(crate) fn check_external_change(&mut self) -> bool {
        let Some(file) = &mut self.file else {
            return false;
        };
        let status = file.external_change();
        if self.last_disk_change.as_ref() == Some(&status) {
            return false;
        }
        self.last_disk_change = Some(status.clone());
        let message = match status {
            ExternalChange::Unchanged => {
                if self.message_origin == MessageOrigin::Disk {
                    self.message.clear();
                    self.message_is_error = false;
                    self.message_origin = MessageOrigin::General;
                    return true;
                }
                return false;
            }
            ExternalChange::Changed => {
                "Disk file changed. F5: reload · F4: Save As; your text is intact".into()
            }
            ExternalChange::Missing => {
                "Disk file is missing. Your text is intact; use F4: Save As or restore the file"
                    .into()
            }
            ExternalChange::Unreadable(error) => format!(
                "Cannot read disk file: {error}. Your text is intact; retry F5 or use F4: Save As"
            ),
        };
        self.set_message(message);
        self.message_origin = MessageOrigin::Disk;
        true
    }

    /// Reload clean files immediately; dirty buffers require a visible Y/N
    /// confirmation. Read and history-capacity errors always retain local text.
    pub(crate) fn request_reload(&mut self) {
        self.document.break_undo_group();
        if self.has_modal() {
            return;
        }
        if self.file.is_none() {
            self.set_message("This buffer has no disk file. Use F4: Save As first");
        } else if self.document.is_dirty() {
            self.overlay = Overlay::Reload;
            self.reload_prompt_visible.set(false);
        } else {
            self.reload_from_disk();
        }
    }

    fn reload_from_disk(&mut self) {
        let result = self
            .file
            .as_ref()
            .ok_or_else(|| io::Error::other("This buffer has no disk file"))
            .and_then(|file| file.reload_bounded(8 * 1024 * 1024));
        match result {
            Ok((text, file)) => match self.document.reload_saved(text) {
                Ok(changed) => {
                    self.file = Some(file);
                    self.last_disk_change = None;
                    self.deactivate();
                    self.parsed = self.analysis();
                    self.layout_key = None;
                    self.preferred_column = None;
                    self.affinity = Affinity::Downstream;
                    self.follow_cursor = true;
                    self.message = if changed {
                        "Reloaded disk version · Ctrl+Z restores previous text"
                    } else {
                        "Disk version already matches your text"
                    }
                    .into();
                    self.message_is_error = false;
                    self.message_origin = MessageOrigin::General;
                }
                Err(error) => {
                    self.overlay = Overlay::None;
                    self.reload_prompt_visible.set(false);
                    self.set_message(error.to_string());
                }
            },
            Err(error) => {
                self.overlay = Overlay::None;
                self.reload_prompt_visible.set(false);
                self.set_message(format!("Reload failed: {error}. Your text is intact"));
            }
        }
    }

    pub(crate) fn path(&self) -> Option<&std::path::Path> {
        self.file.as_ref().map(|file| file.path.as_path())
    }

    pub(crate) fn has_modal(&self) -> bool {
        matches!(
            self.overlay,
            Overlay::Help | Overlay::SaveAs { .. } | Overlay::Quit | Overlay::Reload
        )
    }

    pub(crate) fn workspace_commands_allowed(&self) -> bool {
        !matches!(
            self.overlay,
            Overlay::SaveAs { .. } | Overlay::Quit | Overlay::Reload
        )
    }

    pub(crate) fn pending_save_path(&self) -> Option<PathBuf> {
        match &self.overlay {
            Overlay::SaveAs { path, .. } if !path.is_empty() => Some(PathBuf::from(path)),
            _ => None,
        }
    }

    pub(crate) fn saving_as(&self) -> bool {
        matches!(self.overlay, Overlay::SaveAs { .. })
    }

    pub(crate) fn set_message(&mut self, message: impl Into<String>) {
        self.message = message.into();
        self.message_is_error = true;
        self.message_origin = MessageOrigin::General;
    }

    fn inform(&mut self, message: impl Into<String>) {
        if !self.message_is_error {
            self.message = message.into();
            self.message_origin = MessageOrigin::General;
        }
    }

    fn clipboard_feedback(&mut self, message: String, failed: bool) {
        if failed {
            self.set_message(message);
            self.message_origin = MessageOrigin::Clipboard;
        } else {
            if self.message_origin == MessageOrigin::Clipboard {
                self.message_is_error = false;
            }
            self.inform(message);
        }
    }

    pub(crate) fn deactivate(&mut self) {
        self.document.break_undo_group();
        self.reload_prompt_visible.set(false);
        self.overlay = Overlay::None;
        self.search = Search::default();
        self.search_geometry = SearchGeometry::default();
        self.field_drag = None;
        self.dragging = false;
        self.anchor_screen_row = None;
    }

    pub(crate) fn save_for_workspace(&mut self) {
        self.save(false);
    }

    pub(crate) fn workspace_event(&mut self, event: Event, clipboard: &mut Clipboard) {
        // Workspace owns the one session clipboard; per-document placeholders
        // never create a native worker and cannot retain a stale internal copy.
        std::mem::swap(&mut self.clipboard, clipboard);
        self.handle_event(event);
        std::mem::swap(&mut self.clipboard, clipboard);
    }

    pub(crate) fn enable_workspace_clipboard(&mut self, clipboard: &mut Clipboard) {
        std::mem::swap(&mut self.clipboard, clipboard);
        self.enable_system_clipboard();
        std::mem::swap(&mut self.clipboard, clipboard);
    }

    pub fn open(path: Option<PathBuf>) -> io::Result<Self> {
        match path {
            Some(path) => Self::open_bounded(path, 8 * 1024 * 1024),
            None => Ok(Self::new(String::new(), None, true)),
        }
    }

    pub(crate) fn is_markdown(&self) -> bool {
        self.markdown
    }

    pub(crate) fn language(&self) -> Option<&'static Language> {
        self.language
    }

    fn analysis(&self) -> Parsed {
        Parsed::new(
            self.document.text(),
            self.document.markdown(),
            self.markdown,
            self.language,
        )
    }

    fn new(text: String, file: Option<FileState>, markdown: bool) -> Self {
        let document = Document::new(text);
        let parsed = Parsed::new(document.text(), document.markdown(), markdown, None);
        let projection =
            Projection::build(document.text(), &parsed, document.selection(), 80, markdown);
        Self {
            document,
            file,
            live: markdown,
            markdown,
            language: None,
            parsed,
            projection,
            viewport: Rect::default(),
            scroll: 0,
            preferred_column: None,
            affinity: Affinity::Downstream,
            dragging: false,
            follow_cursor: true,
            reveal: false,
            anchor_screen_row: None,
            clipboard: Clipboard::internal(),
            search: Search::default(),
            search_geometry: SearchGeometry::default(),
            field_drag: None,
            overlay: Overlay::None,
            help_scroll: 0,
            reload_prompt_visible: Cell::new(false),
            last_disk_change: None,
            word_count: Cell::new(None),
            line_count: Cell::new(None),
            should_exit: false,
            message: String::new(),
            message_is_error: false,
            message_origin: MessageOrigin::General,
            layout_key: None,
            editor_area: Rect::new(0, 0, 80, 24),
            terminal_height: 24,
            terminal_width: 80,
        }
    }

    pub fn handle_event(&mut self, event: Event) {
        // Only direct text entry may continue a typing group. UI commands,
        // failed saves, view changes and pointer gestures must end it too.
        let typing = self.overlay == Overlay::None
            && matches!(&event, Event::Key(key)
                if key.kind != KeyEventKind::Release
                    && matches!(key.code, KeyCode::Char(_))
                    && !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT));
        if !typing && !matches!(&event, Event::Key(key) if key.kind == KeyEventKind::Release) {
            self.document.break_undo_group();
        }
        if !self.message_is_error
            && (matches!(&event, Event::Key(key) if key.kind != KeyEventKind::Release)
                || matches!(&event, Event::Paste(text) if !text.is_empty())
                || matches!(&event, Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left)))
        {
            self.message.clear();
        }
        let before = (self.document.revision(), self.document.selection().head);
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                self.search_geometry = SearchGeometry::default();
                self.field_drag = None;
                self.key(key);
            }
            Event::Paste(text) if !text.is_empty() => {
                self.search_geometry = SearchGeometry::default();
                self.field_drag = None;
                if matches!(self.overlay, Overlay::Search { .. }) {
                    self.insert_search_text(&text);
                } else if let Overlay::SaveAs { path, .. } = &mut self.overlay {
                    path.push_str(&text.replace(['\n', '\r', '\0'], ""));
                } else if self.overlay == Overlay::None {
                    self.finish_gesture();
                    self.document.insert(&text);
                }
            }
            Event::Mouse(event) if matches!(self.overlay, Overlay::Search { .. }) => {
                self.search_mouse(event)
            }
            Event::Mouse(event) if self.overlay == Overlay::None => self.mouse(event),
            Event::Resize(width, height) => {
                self.reload_prompt_visible.set(false);
                self.terminal_height = height;
                self.terminal_width = width;
                self.dragging = false;
                self.layout_key = None;
                self.follow_cursor = true;
                self.search_geometry = SearchGeometry::default();
                self.field_drag = None;
            }
            _ => {}
        }
        if self.document.revision() != before.0 && self.document.selection().head != before.1 {
            self.affinity = Affinity::Downstream;
        }
    }

    /// Switch all UI surfaces on this thread; cached layouts refresh at draw.
    pub(crate) fn set_theme(&mut self, theme: Theme) {
        theme::set_theme(theme);
        self.layout_key = None;
    }

    /// Reveal a source location without retaining a transient search or drag.
    pub(crate) fn jump_to_source(&mut self, offset: usize) {
        self.deactivate();
        self.preferred_column = None;
        self.move_to(offset.min(self.document.text().len()), false);
        self.follow_cursor = true;
        self.reveal = true;
        self.layout_key = None;
    }

    pub fn caret_position(&self) -> (usize, usize) {
        self.projection
            .cursor_with_affinity(self.document.selection().head, self.affinity)
    }

    fn key(&mut self, key: KeyEvent) {
        if matches!(self.overlay, Overlay::Search { .. }) {
            self.search_key(key);
            return;
        }
        if self.overlay != Overlay::None {
            self.overlay_key(key);
            return;
        }
        self.finish_gesture();
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if control {
            match key.code {
                KeyCode::Char('q') => {
                    if self.document.is_dirty() {
                        self.overlay = Overlay::Quit;
                    } else {
                        self.should_exit = true;
                    }
                }
                KeyCode::Char('s' | 'S') => {
                    if shift {
                        self.start_save_as(false);
                    } else {
                        self.save(false);
                    }
                }
                KeyCode::Char('e') => self.toggle_view(),
                KeyCode::Char('z' | 'Z') if shift => {
                    self.document.redo();
                }
                KeyCode::Char('z') => {
                    self.document.undo();
                }
                KeyCode::Char('y') => {
                    self.document.redo();
                }
                KeyCode::Char('a') => self.document.select_all(),
                KeyCode::Char('f') => self.open_search(false),
                KeyCode::Char('r') => self.open_search(true),
                KeyCode::Char('c') => self.copy(false),
                KeyCode::Char('x') => self.copy(true),
                KeyCode::Char('v') => {
                    let (text, message) = self.clipboard.paste();
                    self.clipboard_feedback(
                        message,
                        text.is_none() || self.clipboard.uses_fallback(),
                    );
                    if let Some(text) = text {
                        self.document.insert(&text);
                    }
                }
                KeyCode::Char('t') if self.markdown => {
                    self.document.toggle_task_at_caret();
                }
                KeyCode::Left => {
                    self.document.move_word_left(shift);
                    self.affinity = Affinity::Downstream;
                }
                KeyCode::Right => {
                    self.document.move_word_right(shift);
                    self.affinity = Affinity::Downstream;
                }
                KeyCode::Backspace => {
                    self.document.delete_word_backward();
                }
                KeyCode::Delete => {
                    self.document.delete_word_forward();
                }
                KeyCode::Char('b' | 'B') if self.markdown => {
                    self.document.toggle_inline(InlineStyle::Bold);
                }
                KeyCode::Char('i' | 'I') if self.markdown => {
                    self.document.toggle_inline(InlineStyle::Italic);
                }
                KeyCode::Char(']') => {
                    self.document.indent_lines();
                }
                KeyCode::Char('[') => {
                    self.document.outdent_lines();
                }
                KeyCode::Home => self.move_to(0, shift),
                KeyCode::End => self.move_to(self.document.text().len(), shift),
                _ => {}
            }
            self.preferred_column = None;
            return;
        }
        if alt {
            match key.code {
                KeyCode::Up => {
                    if shift {
                        self.document.duplicate_lines_up();
                    } else {
                        self.document.move_lines_up();
                    }
                }
                KeyCode::Down => {
                    if shift {
                        self.document.duplicate_lines_down();
                    } else {
                        self.document.move_lines_down();
                    }
                }
                KeyCode::Left => self.document.move_word_left(shift),
                KeyCode::Right => self.document.move_word_right(shift),
                KeyCode::Backspace => {
                    self.document.delete_word_backward();
                }
                KeyCode::Delete => {
                    self.document.delete_word_forward();
                }
                KeyCode::Char('i' | 'I') if self.markdown => {
                    self.document.toggle_inline(InlineStyle::Italic);
                }
                KeyCode::Char('`') if self.markdown => {
                    self.document.toggle_inline(InlineStyle::Code);
                }
                _ => {}
            }
            if !matches!(key.code, KeyCode::Enter) {
                self.affinity = Affinity::Downstream;
                self.preferred_column = None;
                return;
            }
        }
        match key.code {
            KeyCode::F(1) => self.open_help(),
            KeyCode::F(3) => {
                if self.search.query.text().is_empty() {
                    self.open_search(false);
                } else {
                    self.find_next(shift);
                }
            }
            KeyCode::F(4) => self.start_save_as(false),
            KeyCode::F(5) => self.request_reload(),
            KeyCode::F(6) => self.toggle_view(),
            KeyCode::Esc => {
                let _ = self.document.set_caret(self.document.selection().head);
                self.message.clear();
                self.message_is_error = false;
                self.message_origin = MessageOrigin::General;
            }
            KeyCode::Left => {
                self.document.move_left(shift);
                self.affinity = Affinity::Downstream;
                self.preferred_column = None;
            }
            KeyCode::Right => {
                self.document.move_right(shift);
                self.affinity = Affinity::Downstream;
                self.preferred_column = None;
            }
            KeyCode::Up => self.vertical(-1, shift),
            KeyCode::Down => self.vertical(1, shift),
            KeyCode::PageUp => self.vertical(-(self.viewport.height.max(1) as isize), shift),
            KeyCode::PageDown => self.vertical(self.viewport.height.max(1) as isize, shift),
            KeyCode::Home | KeyCode::End => {
                let (row, _) = self.caret_position();
                let row = &self.projection.rows[row];
                self.move_to(
                    if key.code == KeyCode::Home {
                        row.start
                    } else {
                        row.end
                    },
                    shift,
                );
                if key.code == KeyCode::End {
                    self.affinity = Affinity::Upstream;
                }
                self.preferred_column = None;
            }
            KeyCode::Enter => {
                if shift || alt || !self.markdown {
                    self.document.literal_newline();
                } else {
                    self.document.enter();
                }
                self.preferred_column = None;
            }
            KeyCode::Backspace => {
                self.document.backspace();
                self.preferred_column = None;
            }
            KeyCode::Delete => {
                self.document.delete_forward();
                self.preferred_column = None;
            }
            KeyCode::Tab => {
                if shift {
                    self.document.outdent_lines();
                } else if self.document.selection().is_empty() {
                    self.document.insert("\t");
                } else {
                    self.document.indent_lines();
                }
                self.preferred_column = None;
            }
            KeyCode::BackTab => {
                self.document.outdent_lines();
                self.preferred_column = None;
            }
            KeyCode::Char(c) if !alt => {
                self.document.type_text(&c.to_string());
                self.preferred_column = None;
            }
            _ => {}
        }
    }

    fn finish_gesture(&mut self) {
        self.dragging = false;
        self.follow_cursor = true;
    }

    fn open_help(&mut self) {
        self.help_scroll = 0;
        self.overlay = Overlay::Help;
    }

    fn move_to(&mut self, offset: usize, extend: bool) {
        self.affinity = Affinity::Downstream;
        let offset = self.document.floor_grapheme_boundary(offset);
        let anchor = if extend {
            self.document.selection().anchor
        } else {
            offset
        };
        if let Err(error) = self.document.set_selection(Selection {
            anchor,
            head: offset,
        }) {
            self.set_message(error.to_string());
        }
    }

    fn vertical(&mut self, delta: isize, extend: bool) {
        let (row, col) = self.caret_position();
        let col = *self.preferred_column.get_or_insert(col);
        let target_row = self.projection.navigable_row(row, delta);
        let hit = self.projection.hit(target_row, col);
        self.move_to(hit.offset, extend);
        self.affinity = hit.affinity;
        if target_row >= self.scroll && target_row < self.scroll + self.viewport.height as usize {
            self.anchor_screen_row = Some(target_row - self.scroll);
        }
    }

    fn mouse(&mut self, event: MouseEvent) {
        if matches!(
            event.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            if !contains(self.editor_area, event.column, event.row) {
                return;
            }
            self.scroll = if event.kind == MouseEventKind::ScrollUp {
                self.scroll.saturating_sub(3)
            } else {
                (self.scroll + 3).min(
                    self.projection
                        .rows
                        .len()
                        .saturating_sub(self.viewport.height as usize),
                )
            };
            self.follow_cursor = false;
            return;
        }
        let inside = event.column >= self.viewport.x
            && event.column < self.viewport.right()
            && event.row >= self.viewport.y
            && event.row < self.viewport.bottom();
        if !inside {
            if event.kind == MouseEventKind::Up(MouseButton::Left) {
                self.dragging = false;
            }
            return;
        }
        let row = usize::from(event.row - self.viewport.y) + self.scroll;
        let col = usize::from(event.column - self.viewport.x);
        let hit = self.projection.hit(row, col);
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(task) = self
                    .projection
                    .pointer_task(&self.parsed, row, col, usize::from(self.viewport.width))
                    .cloned()
                {
                    match self.document.toggle_task(&task) {
                        Ok(_) => self.inform("Task toggled · Ctrl+Z undo"),
                        Err(error) => self.set_message(error.to_string()),
                    }
                    self.follow_cursor = false;
                    return;
                }
                self.move_to(hit.offset, event.modifiers.contains(KeyModifiers::SHIFT));
                self.affinity = hit.affinity;
                self.dragging = true; // Keep exactly the current hit geometry until button-up.
                self.preferred_column = None;
                self.follow_cursor = false;
                self.anchor_screen_row = Some(row.saturating_sub(self.scroll));
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                self.move_to(hit.offset, true);
                self.affinity = hit.affinity;
                self.anchor_screen_row = Some(row.saturating_sub(self.scroll));
            }
            MouseEventKind::Up(MouseButton::Left) if self.dragging => {
                self.move_to(hit.offset, true);
                self.affinity = hit.affinity;
                self.dragging = false;
                self.follow_cursor = true;
                self.anchor_screen_row = Some(row.saturating_sub(self.scroll));
            }
            _ => {}
        }
    }

    fn toggle_view(&mut self) {
        if self.markdown {
            self.live = !self.live;
        }
    }

    fn copy(&mut self, cut: bool) {
        if !self.document.selection().is_empty() {
            let message = self.clipboard.copy(self.document.selected_text());
            self.clipboard_feedback(message, self.clipboard.uses_fallback());
            if cut {
                self.document.insert("");
            }
        }
    }

    pub fn enable_system_clipboard(&mut self) {
        if clipboard::remote_session() {
            self.clipboard = Clipboard::remote();
        } else {
            self.clipboard = Clipboard::system();
        }
    }

    fn open_search(&mut self, replace: bool) {
        self.finish_gesture();
        self.search.origin = self.document.selection().range().start;
        if !self.document.selection().is_empty() {
            self.search.query.select_all();
            self.search.query.insert(self.document.selected_text());
        }
        self.search.query.select_all();
        self.search.focus = Focus::Query;
        self.overlay = Overlay::Search { replace };
        self.update_query();
    }

    fn update_query(&mut self) {
        self.search.refresh(&self.document);
        if let Some((index, wrapped)) = self.search.near(self.search.origin) {
            self.select_match(index, wrapped);
        }
    }

    fn select_match(&mut self, index: usize, wrapped: bool) {
        let range = self.search.matches[index].clone();
        if let Err(error) = self.document.set_selection(Selection {
            anchor: range.start,
            head: range.end,
        }) {
            self.set_message(error.to_string());
            return;
        }
        self.search.wrapped = wrapped;
        self.follow_cursor = true;
        self.dragging = false;
        self.anchor_screen_row = None;
        self.affinity = Affinity::Downstream;
        self.preferred_column = None;
    }

    fn find_next(&mut self, backwards: bool) {
        self.search.refresh(&self.document);
        if let Some((index, wrapped)) = self.search.next(self.document.selection(), backwards) {
            self.select_match(index, wrapped);
        }
    }

    fn insert_search_text(&mut self, text: &str) {
        if !self.search_ready() {
            self.search_too_short();
            return;
        }
        if let Some(input) = self.search.input_mut() {
            input.insert(text);
        }
        if self.search.focus == Focus::Query {
            self.update_query();
        }
    }

    fn replace_current(&mut self) {
        self.search.refresh(&self.document);
        let Some(index) = self.search.current(self.document.selection()) else {
            self.find_next(false);
            if !self.search.matches.is_empty() {
                self.inform("Match selected · Replace again to change it");
            }
            return;
        };
        let range = self.search.matches[index].clone();
        let next = range.start + self.search.replacement.text().len();
        match self
            .document
            .replace_range(range, self.search.replacement.text())
        {
            Ok(changed) => {
                self.search.refresh(&self.document);
                if let Some((index, wrapped)) = self.search.near(next) {
                    self.select_match(index, wrapped);
                }
                self.inform(if changed {
                    "Replaced one · Esc then Ctrl+Z to undo"
                } else {
                    "Match unchanged: replacement is identical"
                });
                self.follow_cursor = true;
            }
            Err(error) => self.set_message(error.to_string()),
        }
    }

    fn replace_all(&mut self) {
        let identical = self.search.query.text() == self.search.replacement.text();
        let count = self
            .document
            .replace_all(self.search.query.text(), self.search.replacement.text());
        self.search.refresh(&self.document);
        self.follow_cursor = true;
        self.inform(if count == 0 {
            "No matches to replace".into()
        } else if identical {
            format!("{count} matches unchanged: replacement is identical")
        } else {
            format!("Replaced {count} matches · One undo step")
        });
    }

    fn search_key(&mut self, key: KeyEvent) {
        let Overlay::Search { replace } = self.overlay else {
            return;
        };
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        if key.code == KeyCode::Esc {
            self.overlay = Overlay::None;
            return;
        }
        if control && matches!(key.code, KeyCode::Char('s' | 'S' | 'q')) {
            self.overlay = Overlay::None;
            self.key(key);
            return;
        }
        if control && matches!(key.code, KeyCode::Char('f' | 'r')) {
            self.overlay = Overlay::Search {
                replace: key.code == KeyCode::Char('r'),
            };
            self.search.focus = Focus::Query;
            self.search.query.select_all();
            return;
        }
        if key.code == KeyCode::F(1) {
            self.open_help();
            return;
        }
        if key.code == KeyCode::F(4) {
            self.start_save_as(false);
            return;
        }
        if key.code == KeyCode::F(5) {
            self.request_reload();
            return;
        }
        if !self.search_ready() {
            self.search_too_short();
            return;
        }
        if key.modifiers.contains(KeyModifiers::ALT) && !control {
            match key.code {
                KeyCode::Char('r' | 'R') if replace => self.replace_current(),
                KeyCode::Char('a' | 'A') if replace => self.replace_all(),
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::F(3) => {
                self.find_next(shift);
                return;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.search
                    .cycle(replace, shift || key.code == KeyCode::BackTab);
                return;
            }
            KeyCode::Up if replace => {
                self.search.focus_keyboard(Focus::Query);
                return;
            }
            KeyCode::Down if replace => {
                self.search.focus_keyboard(Focus::Replacement);
                return;
            }
            KeyCode::Enter => {
                match self.search.focus {
                    Focus::Replacement if !shift => self.replace_current(),
                    _ => self.find_next(shift),
                }
                return;
            }
            _ => {}
        }
        let mut clipboard_feedback = None;
        let Some(input) = self.search.input_mut() else {
            return;
        };
        let before = input.revision();
        if control {
            match key.code {
                KeyCode::Char('a') => input.select_all(),
                KeyCode::Char('c' | 'x') => {
                    if !input.selection().is_empty() {
                        let message = self.clipboard.copy(input.selected_text());
                        clipboard_feedback = Some((message, self.clipboard.uses_fallback()));
                        if key.code == KeyCode::Char('x') {
                            input.insert("");
                        }
                    }
                }
                KeyCode::Char('v') => {
                    let (text, message) = self.clipboard.paste();
                    clipboard_feedback =
                        Some((message, text.is_none() || self.clipboard.uses_fallback()));
                    if let Some(text) = text {
                        input.insert(&text);
                    }
                }
                KeyCode::Char('z') => {
                    input.undo();
                }
                KeyCode::Char('y') => {
                    input.redo();
                }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Left => input.move_left(shift),
                KeyCode::Right => input.move_right(shift),
                KeyCode::Home | KeyCode::End => {
                    let head = if key.code == KeyCode::Home {
                        0
                    } else {
                        input.text().len()
                    };
                    let anchor = if shift {
                        input.selection().anchor
                    } else {
                        head
                    };
                    let _ = input.set_selection(Selection { anchor, head });
                }
                KeyCode::Backspace => {
                    input.backspace();
                }
                KeyCode::Delete => {
                    input.delete_forward();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::ALT) => {
                    input.insert(&c.to_string());
                }
                _ => {}
            }
        }
        let changed = before != input.revision();
        if changed && self.search.focus == Focus::Query {
            self.update_query();
        }
        if let Some((message, failed)) = clipboard_feedback {
            self.clipboard_feedback(message, failed);
        }
    }

    fn search_mouse(&mut self, event: MouseEvent) {
        if matches!(
            event.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            if contains(self.viewport, event.column, event.row) {
                self.mouse(event);
            }
            return;
        }
        if let Some(drag) = self.field_drag.clone() {
            match event.kind {
                MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => {
                    if self.search_ready()
                        && self.search.input(drag.map.focus).revision() == drag.map.revision
                    {
                        self.search.focus = drag.map.focus;
                        let head = drag.map.hit(event.column);
                        let _ = self.search.input_mut().unwrap().set_selection(Selection {
                            anchor: drag.anchor,
                            head,
                        });
                    }
                    if event.kind == MouseEventKind::Up(MouseButton::Left) {
                        self.field_drag = None;
                    }
                    return;
                }
                _ => {}
            }
        }
        if event.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }
        self.field_drag = None;
        if !contains(self.search_geometry.panel, event.column, event.row) {
            return;
        }
        if let Some(button) = self
            .search_geometry
            .buttons
            .iter()
            .find(|button| contains(button.area, event.column, event.row))
            .cloned()
        {
            self.search_geometry = SearchGeometry::default();
            if button.enabled {
                match button.action {
                    SearchAction::Previous => self.find_next(true),
                    SearchAction::Next => self.find_next(false),
                    SearchAction::Replace => self.replace_current(),
                    SearchAction::ReplaceAll => self.replace_all(),
                    SearchAction::Close => {
                        self.overlay = Overlay::None;
                        self.search_geometry = SearchGeometry::default();
                    }
                }
            }
            return;
        }
        if !self.search_ready() {
            return;
        }
        if let Some(map) = self
            .search_geometry
            .fields
            .iter()
            .find(|field| contains(field.area, event.column, event.row))
            .cloned()
        {
            if self.search.input(map.focus).revision() != map.revision {
                return;
            }
            self.search.focus = map.focus;
            let head = map.hit(event.column);
            let anchor = if event.modifiers.contains(KeyModifiers::SHIFT) {
                self.search.input(map.focus).selection().anchor
            } else {
                head
            };
            let _ = self
                .search
                .input_mut()
                .unwrap()
                .set_selection(Selection { anchor, head });
            self.field_drag = Some(FieldDrag { map, anchor });
        }
    }

    fn start_save_as(&mut self, quit_after: bool) {
        self.overlay = Overlay::SaveAs {
            path: String::new(),
            quit_after,
        };
    }

    fn save(&mut self, quit_after: bool) {
        if let Some(file) = &mut self.file {
            match file.save(self.document.text()) {
                Ok(()) => {
                    self.document.mark_saved();
                    self.last_disk_change = None;
                    self.message = format!("Saved {}", safe_text(&file.path.display().to_string()));
                    self.message_is_error = false;
                    self.message_origin = MessageOrigin::General;
                    self.overlay = Overlay::None;
                    self.should_exit = quit_after;
                }
                Err(error) => {
                    self.set_message(error.to_string());
                    self.overlay = Overlay::None;
                }
            }
        } else {
            self.start_save_as(quit_after);
        }
    }

    fn overlay_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.overlay = Overlay::None;
            self.reload_prompt_visible.set(false);
            return;
        }
        if self.overlay == Overlay::Reload {
            if key.kind == KeyEventKind::Press
                && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT)
            {
                match key.code {
                    KeyCode::Char('n' | 'N') => {
                        self.overlay = Overlay::None;
                        self.reload_prompt_visible.set(false);
                    }
                    KeyCode::Char('y' | 'Y') if self.reload_prompt_visible.get() => {
                        self.reload_from_disk()
                    }
                    _ => {}
                }
            }
            return;
        }
        match &mut self.overlay {
            Overlay::Help => {
                match key.code {
                    KeyCode::F(1) | KeyCode::Enter => self.overlay = Overlay::None,
                    KeyCode::Up => self.help_scroll = self.help_scroll.saturating_sub(1),
                    KeyCode::Down => self.help_scroll += 1,
                    KeyCode::PageUp => self.help_scroll = self.help_scroll.saturating_sub(8),
                    KeyCode::PageDown => self.help_scroll += 8,
                    KeyCode::Home => self.help_scroll = 0,
                    KeyCode::End => self.help_scroll = usize::MAX,
                    _ => {}
                }
                self.help_scroll = self.help_scroll.min(help_sheet(1).len() - 1);
            }
            Overlay::Quit => match key.code {
                KeyCode::Char('y' | 'Y') => self.save(true),
                KeyCode::Char('n' | 'N') => self.should_exit = true,
                _ => {}
            },
            Overlay::SaveAs { path, quit_after } => match key.code {
                KeyCode::Enter if !path.is_empty() => {
                    let target = PathBuf::from(path.clone());
                    let quit_after = *quit_after;
                    match FileState::new_target(target).and_then(|mut file| {
                        file.save(self.document.text())?;
                        Ok(file)
                    }) {
                        Ok(file) => {
                            self.markdown = is_markdown(&file.path);
                            self.live &= self.markdown;
                            self.message =
                                format!("Saved {}", safe_text(&file.path.display().to_string()));
                            self.message_is_error = false;
                            self.message_origin = MessageOrigin::General;
                            self.file = Some(file);
                            self.last_disk_change = None;
                            self.document.mark_saved();
                            self.overlay = Overlay::None;
                            self.should_exit = quit_after;
                            self.parsed = self.analysis();
                            self.layout_key = None;
                        }
                        Err(error) => self.set_message(error.to_string()),
                    }
                }
                KeyCode::Backspace => {
                    if let Some((start, _)) = path.grapheme_indices(true).next_back() {
                        path.truncate(start);
                    }
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    path.push(c)
                }
                _ => {}
            },
            Overlay::None | Overlay::Search { .. } | Overlay::Reload => {}
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        self.draw_with_cursor(frame, true);
    }

    pub(crate) fn draw_with_cursor(&mut self, frame: &mut Frame, show_cursor: bool) {
        self.draw_in(frame, frame.area(), show_cursor);
    }

    /// Draw inside an absolute editor slice, preserving the reserved header rows.
    pub(crate) fn draw_in(&mut self, frame: &mut Frame, area: Rect, show_cursor: bool) {
        let area = area.intersection(frame.area());
        if self.editor_area != area {
            self.dragging = false;
            self.field_drag = None;
            self.search_geometry = SearchGeometry::default();
        }
        self.editor_area = area;
        frame.render_widget(Block::default().style(document_style()), area);
        if self.terminal_height != area.height || self.terminal_width != area.width {
            self.field_drag = None;
        }
        self.terminal_height = area.height;
        self.terminal_width = area.width;
        if matches!(self.overlay, Overlay::Search { .. }) {
            self.search.refresh(&self.document);
        }
        if matches!(self.overlay, Overlay::Search { .. }) && !self.search_ready() {
            self.search_too_short();
        }
        let search_height = self.search_height(area.height);
        let body_top = body_top(area.height);
        // Source and code views number their lines; live prose is a column.
        let digits = self.gutter_digits(area.width);
        let (margin, prose_width) = if self.live {
            // Two cells each side leave room for heading marks and code
            // surfaces that hang into the margin.
            let gutter = if area.width >= 48 { 4 } else { 2 };
            let width = area.width.saturating_sub(gutter).min(88);
            (area.width.saturating_sub(width) / 2, width)
        } else {
            let margin = if digits > 0 { digits + 2 } else { 1 };
            (margin, area.width.saturating_sub(margin + 1))
        };
        self.viewport = Rect::new(
            area.x.saturating_add(margin),
            area.y.saturating_add(body_top),
            prose_width,
            area.height.saturating_sub(body_top + 1 + search_height),
        );
        if self.parsed.snapshot.revision != self.document.revision()
            || self.parsed.theme != theme::current_theme()
            || self.parsed.icons != crate::icons::current()
        {
            self.parsed = self.analysis();
        }
        // Layout depends on the selection only through what it discloses, so
        // caret movement within a block or in source view reuses it.
        let disclosed = if self.live {
            self.parsed
                .disclosed(self.document.text(), self.document.selection())
        } else {
            Vec::new()
        };
        let key = (
            self.document.revision(),
            disclosed,
            self.viewport.width as usize,
            self.live,
            theme::current_theme(),
            crate::icons::current(),
        );
        if !self.dragging {
            if self.layout_key.as_ref() != Some(&key) {
                self.projection = Projection::layout(
                    self.document.text(),
                    &self.parsed,
                    &key.1,
                    key.2,
                    self.live,
                );
                self.layout_key = Some(key);
                if let Some(screen_row) = self.anchor_screen_row.take() {
                    self.scroll = self.caret_position().0.saturating_sub(screen_row);
                }
            } else {
                self.anchor_screen_row = None;
            }
        }
        let (cursor_row, cursor_col) = self.caret_position();
        let height = self.viewport.height as usize;
        // A jump lands a third of the way down, with what follows it in view,
        // unless the document ends first.
        if std::mem::take(&mut self.reveal) && height > 0 {
            self.scroll = cursor_row
                .saturating_sub(height / 3)
                .min(self.projection.rows.len().saturating_sub(height));
        }
        if self.follow_cursor && height > 0 {
            if cursor_row < self.scroll {
                self.scroll = cursor_row;
            }
            if cursor_row >= self.scroll + height {
                self.scroll = cursor_row + 1 - height;
            }
        }
        self.scroll = self
            .scroll
            .min(self.projection.rows.len().saturating_sub(1));
        let filename = self.file.as_ref().map_or_else(
            || "Untitled.md".into(),
            |f| safe_text(&f.path.display().to_string()),
        );
        let title = format!(
            " {}{} ",
            filename,
            if self.document.is_dirty() { " *" } else { "" }
        );
        frame.render_widget(
            Paragraph::new(title).style(chrome_active()),
            Rect::new(area.x, area.y, area.width, area.height.min(1)),
        );
        let selection = self.document.selection().range();
        let search_open = matches!(self.overlay, Overlay::Search { .. });
        let matches = if search_open {
            self.search.matches.as_slice()
        } else {
            &[]
        };
        let active = if search_open {
            self.search
                .current(self.document.selection())
                .and_then(|index| matches.get(index))
        } else {
            None
        };
        let colors = palette();
        let rows = &self.projection.rows;
        let line_of = |row: usize| {
            rows[..=row.min(rows.len() - 1)]
                .iter()
                .rev()
                .find_map(|r| r.line)
        };
        let caret_line = line_of(cursor_row);
        let mut line = if self.scroll > 0 {
            line_of(self.scroll - 1)
        } else {
            None
        };
        let guides = if digits > 0 {
            indent_guides(rows, self.scroll, height)
        } else {
            None
        };
        for (screen_row, row) in rows.iter().skip(self.scroll).take(height).enumerate() {
            let y = self.viewport.y + screen_row as u16;
            line = row.line.or(line);
            let current = digits > 0 && line == caret_line;
            if let Some(number) = row.line.filter(|_| digits > 0) {
                let style = if current {
                    document_style()
                        .fg(colors.foreground)
                        .add_modifier(Modifier::BOLD)
                } else {
                    document_style().fg(colors.faint)
                };
                let text = format!("{number:>width$}", width = usize::from(digits));
                frame
                    .buffer_mut()
                    .set_stringn(area.x + 1, y, &text, usize::from(digits), style);
            }
            let mut surface = None;
            if current {
                let line = Rect::new(self.viewport.x, y, self.viewport.width, 1);
                frame
                    .buffer_mut()
                    .set_style(line, Style::default().bg(colors.cursorline));
                surface = Some((0, colors.cursorline));
            }
            if let Some(fill) = &row.fill {
                draw_fill(frame, area, self.viewport, fill, y);
                if fill.kind == FillKind::Band {
                    surface = Some((fill.from, fill.color));
                }
            }
            for glyph in &row.glyphs {
                let selected =
                    glyph.source.start < selection.end && selection.start < glyph.source.end;
                let mut base = glyph.style;
                // Glyphs on a surface take its color unless they carry their own.
                if let Some((from, color)) = surface
                    && glyph.column as isize >= from
                    && base.bg == Some(colors.background)
                {
                    base = base.bg(color);
                }
                let style = crate::search_highlight::style_match(
                    base,
                    &glyph.source,
                    selected,
                    matches,
                    active,
                );
                let x = self.viewport.x + glyph.column as u16;
                if x < self.viewport.right() {
                    frame.buffer_mut().set_stringn(
                        x,
                        y,
                        &glyph.text,
                        usize::from(self.viewport.right() - x),
                        style,
                    );
                }
            }
            if let Some((unit, indents)) = &guides {
                let buffer = frame.buffer_mut();
                for column in (0..indents[screen_row]).step_by(*unit) {
                    let x = self.viewport.x + column as u16;
                    if x < self.viewport.right() && buffer[(x, y)].symbol() == " " {
                        buffer[(x, y)].set_symbol("│").set_fg(colors.faint);
                    }
                }
            }
        }
        if self.document.text().is_empty() && self.markdown && height > 0 {
            let width = usize::from(self.viewport.width);
            let buffer = frame.buffer_mut();
            buffer.set_stringn(
                self.viewport.x,
                self.viewport.y,
                "Start writing…",
                width,
                document_style()
                    .fg(colors.muted)
                    .add_modifier(Modifier::ITALIC),
            );
            // Only an empty page offers a way in; written pages stay quiet.
            if height > 2 {
                let mut x = self.viewport.x;
                for (key, action) in [("F2", "commands"), ("Ctrl+O", "open"), ("F1", "help")] {
                    let room = usize::from(self.viewport.right().saturating_sub(x));
                    let label = format!("{key} {action}   ");
                    if UnicodeWidthStr::width(label.as_str()) > room {
                        break;
                    }
                    let quiet = document_style().fg(colors.muted);
                    buffer.set_string(
                        x,
                        self.viewport.y + 2,
                        key,
                        quiet.add_modifier(Modifier::BOLD),
                    );
                    let after = x + UnicodeWidthStr::width(key) as u16 + 1;
                    buffer.set_string(after, self.viewport.y + 2, action, quiet);
                    x += UnicodeWidthStr::width(label.as_str()) as u16;
                }
            }
        }
        self.draw_scrollbar(frame, area, height);
        if show_cursor
            && self.overlay == Overlay::None
            && height > 0
            && cursor_row >= self.scroll
            && cursor_row < self.scroll + height
            && cursor_col < self.viewport.width as usize
        {
            frame.set_cursor_position((
                self.viewport.x + cursor_col as u16,
                self.viewport.y + (cursor_row - self.scroll) as u16,
            ));
        }
        if matches!(self.overlay, Overlay::Search { .. }) {
            self.draw_search(frame, area, search_height, show_cursor);
        }
        self.draw_footer(frame, area);
        self.draw_overlay(frame);
    }

    /// Number width for the gutter, or zero when it is hidden.
    fn gutter_digits(&self, width: u16) -> u16 {
        if self.live || width < 24 {
            return 0;
        }
        (self.line_count().to_string().len() as u16).max(3)
    }

    fn line_count(&self) -> usize {
        let revision = self.document.revision();
        if let Some((cached, count)) = self.line_count.get()
            && cached == revision
        {
            return count;
        }
        let bytes = self.document.text().as_bytes();
        let count = 1 + bytes
            .iter()
            .enumerate()
            .filter(|(index, byte)| {
                **byte == b'\n' || (**byte == b'\r' && bytes.get(index + 1) != Some(&b'\n'))
            })
            .count();
        self.line_count.set(Some((revision, count)));
        count
    }

    /// A thin thumb on the editor's right edge, only when content overflows.
    /// While Find is open, the track also marks where matches are.
    fn draw_scrollbar(&self, frame: &mut Frame, area: Rect, height: usize) {
        let total = self.projection.rows.len();
        if height == 0 || total <= height || area.width < 3 {
            return;
        }
        let colors = palette();
        let thumb = (height * height / total).clamp(1, height);
        let top = (self.scroll * height).div_ceil(total).min(height - thumb);
        let x = area.right() - 1;
        let buffer = frame.buffer_mut();
        for y in top..top + thumb {
            buffer.set_string(
                x,
                self.viewport.y + y as u16,
                "▐",
                document_style().fg(colors.faint),
            );
        }
        if !matches!(self.overlay, Overlay::Search { .. }) {
            return;
        }
        let current = self.search.current(self.document.selection());
        for (index, range) in self.search.matches.iter().enumerate() {
            let y = self.projection.cursor(range.start).0 * height / total;
            let color = if current == Some(index) {
                colors.search_active
            } else {
                colors.search
            };
            buffer.set_string(
                x,
                self.viewport.y + y as u16,
                "▐",
                document_style().fg(color),
            );
        }
    }

    fn search_count(&self) -> String {
        if self.search.query.text().is_empty() {
            String::new()
        } else if self.search.matches.is_empty() {
            "No matches".into()
        } else if let Some(index) = self.search.current(self.document.selection()) {
            format!("{}/{}", index + 1, self.search.matches.len())
        } else {
            format!("{} matches", self.search.matches.len())
        }
    }

    fn search_inline(&self) -> bool {
        // Two margins, a bounded field with at least 24 input cells, a count,
        // two gaps, and the full Previous/Next/Close labels must all fit.
        let count = UnicodeWidthStr::width(self.search_count().as_str()).max(10);
        usize::from(self.terminal_width) >= 2 + 8 + 24 + count + 2 + 21
    }

    fn search_rows(&self) -> u16 {
        match self.overlay {
            Overlay::Search { replace } => {
                1 + u16::from(replace) + u16::from(!self.search_inline())
            }
            _ => 0,
        }
    }

    fn search_height(&self, height: u16) -> u16 {
        self.search_rows()
            .min(height.saturating_sub(body_top(height) + 1))
    }

    fn search_ready(&self) -> bool {
        !matches!(self.overlay, Overlay::Search { .. })
            || (self.terminal_width >= 11
                && self.terminal_height >= body_top(self.terminal_height) + self.search_rows() + 2)
    }

    fn search_too_short(&mut self) {
        self.search.focus = Focus::Query;
    }

    fn draw_search_field(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        focus: Focus,
        show_cursor: bool,
    ) {
        let focused = self.search.focus == focus;
        let frozen_start = self
            .field_drag
            .as_ref()
            .filter(|drag| drag.map.focus == focus)
            .map(|drag| drag.map.start);
        // A six-cell label chip and a gap precede the input surface.
        let label = match focus {
            Focus::Query => " Find  ",
            Focus::Replacement => " With  ",
        };
        frame.render_widget(
            Block::default().style(chrome_field()),
            Rect::new(area.x + 7, area.y, area.width.saturating_sub(7), 1),
        );
        let mut map = draw_field(
            frame,
            Rect::new(area.x, area.y, area.width.saturating_sub(1), 1),
            label,
            self.search.input(focus),
            focus,
            focused && show_cursor,
            frozen_start,
        );
        let colors = theme::chrome_palette();
        let chip = if focused {
            colors.status_accent.style().add_modifier(Modifier::BOLD)
        } else {
            colors.status_secondary.style()
        };
        frame
            .buffer_mut()
            .set_style(Rect::new(area.x, area.y, area.width.min(6), 1), chip);
        if area.width > 6 {
            frame
                .buffer_mut()
                .set_style(Rect::new(area.x + 6, area.y, 1, 1), chrome_style());
        }
        map.area = area;
        self.search_geometry.fields.push(map);
    }

    fn draw_search(&mut self, frame: &mut Frame, area: Rect, height: u16, show_cursor: bool) {
        self.search_geometry = SearchGeometry::default();
        let Overlay::Search { replace } = self.overlay else {
            return;
        };
        if height == 0 {
            return;
        }
        let panel = Rect::new(area.x, area.bottom() - 1 - height, area.width, height);
        self.search_geometry.panel = panel;
        frame.render_widget(Clear, panel);
        frame.render_widget(Block::default().style(chrome_style()), panel);
        let row = |offset: u16| {
            Rect::new(
                panel.x + 1,
                panel.y + offset,
                panel.width.saturating_sub(2),
                1,
            )
        };
        if !self.search_ready() {
            self.search_geometry.buttons = draw_search_buttons(
                frame,
                Rect::new(panel.x, panel.y, panel.width, 1),
                &[(SearchAction::Close, true)],
            );
            return;
        }
        let has_matches = !self.search.matches.is_empty();
        let navigation = [
            (SearchAction::Previous, has_matches),
            (SearchAction::Next, has_matches),
            (SearchAction::Close, true),
        ];
        let replacements = [
            (
                SearchAction::Replace,
                self.search.current(self.document.selection()).is_some(),
            ),
            (SearchAction::ReplaceAll, has_matches),
        ];
        let counts = self.search_count();
        let count_width = UnicodeWidthStr::width(counts.as_str()).max(10) as u16;
        let query_row = row(0);
        if self.search_inline() {
            let field_width = query_row.width - count_width - 23;
            self.draw_search_field(
                frame,
                Rect::new(query_row.x, query_row.y, field_width, 1),
                Focus::Query,
                show_cursor,
            );
            frame.render_widget(
                Paragraph::new(counts).style(chrome_muted()),
                Rect::new(query_row.x + field_width + 1, query_row.y, count_width, 1),
            );
            self.search_geometry.buttons = draw_search_buttons(
                frame,
                Rect::new(query_row.right() - 21, query_row.y, 21, 1),
                &navigation,
            );
            if replace {
                let with_row = row(1);
                self.draw_search_field(
                    frame,
                    Rect::new(with_row.x, with_row.y, field_width, 1),
                    Focus::Replacement,
                    show_cursor,
                );
                self.search_geometry.buttons.extend(draw_search_buttons(
                    frame,
                    Rect::new(with_row.right() - 25, with_row.y, 25, 1),
                    &replacements,
                ));
            }
        } else {
            // A narrow dock puts actions on their own row. Keep at least eight
            // input cells before giving the count its own horizontal space.
            let count_fits = query_row.width >= 8 + 8 + 1 + count_width;
            let field_width = if count_fits {
                query_row.width - count_width - 1
            } else {
                query_row.width
            };
            self.draw_search_field(
                frame,
                Rect::new(query_row.x, query_row.y, field_width, 1),
                Focus::Query,
                show_cursor,
            );
            if count_fits {
                frame.render_widget(
                    Paragraph::new(counts.as_str()).style(chrome_muted()),
                    Rect::new(query_row.right() - count_width, query_row.y, count_width, 1),
                );
            }
            if replace {
                self.draw_search_field(frame, row(1), Focus::Replacement, show_cursor);
            }
            let mut actions = navigation[..2].to_vec();
            if replace {
                actions.extend(replacements);
            }
            actions.push((SearchAction::Close, true));
            self.search_geometry.buttons =
                draw_search_buttons(frame, row(if replace { 2 } else { 1 }), &actions);
        }
    }

    fn source_word_count(&self) -> usize {
        let revision = self.document.revision();
        if let Some((cached_revision, count)) = self.word_count.get()
            && cached_revision == revision
        {
            return count;
        }
        let count = self.document.text().unicode_words().count();
        self.word_count.set(Some((revision, count)));
        count
    }

    /// Context takes priority over passive metadata. Search geometry belongs to
    /// the editor slice even when Workspace redraws this footer at full width.
    fn footer_context(&self, width: u16) -> Option<String> {
        if !self.message.is_empty() {
            return Some(safe_text(&self.message));
        }
        let Overlay::Search { replace } = self.overlay else {
            return None;
        };
        if !self.search_ready() {
            return Some(
                if width >= 29 {
                    "Resize to search · Esc Close"
                } else {
                    "Esc Close"
                }
                .into(),
            );
        }
        let count = self.search_count();
        let count_in_footer = !self.search_inline()
            && self.terminal_width.saturating_sub(2)
                < 8 + 8 + 1 + UnicodeWidthStr::width(count.as_str()).max(10) as u16
            && !self.search.query.text().is_empty();
        let guidance = if count_in_footer {
            if usize::from(width) >= UnicodeWidthStr::width(count.as_str()) + 13 {
                format!("{count} · Esc Close")
            } else if usize::from(width) >= UnicodeWidthStr::width(count.as_str()) + 6 {
                format!("{count} Esc")
            } else {
                count
            }
        } else if replace && self.search.focus == Focus::Replacement {
            if width < 60 {
                "Enter: replace · Tab Find · Esc Close".into()
            } else {
                "Enter: replace + next · Tab Find · Esc Close".into()
            }
        } else if replace {
            "Enter Next · Tab With · Esc Close".into()
        } else {
            "Enter Next · ^R Replace · Esc Close".into()
        };
        Some(if self.search.wrapped {
            format!("Wrapped · {guidance}")
        } else {
            guidance
        })
    }

    /// The badge naming what is being edited: Markdown's view, or the language.
    fn format_badge(&self) -> (String, &'static str) {
        let icons = crate::icons::current();
        let nerd = icons == crate::icons::IconSet::Nerd;
        match (self.markdown, self.language) {
            (true, _) if self.live => ("MARKDOWN".into(), icons.file(true)),
            (true, _) => ("SOURCE".into(), icons.file(true)),
            (false, Some(language)) => (
                language.name().to_uppercase(),
                if nerd { language.icon() } else { "" },
            ),
            (false, None) => ("TEXT".into(), icons.file(false)),
        }
    }

    fn caret_line_column(&self) -> (usize, usize) {
        let text = self.document.text();
        let head = self.document.selection().head;
        // CRLF is one grapheme, so count LF and lone CR bytes.
        let bytes = &text.as_bytes()[..head];
        let line = bytes
            .iter()
            .enumerate()
            .filter(|(index, byte)| {
                **byte == b'\n' || (**byte == b'\r' && bytes.get(index + 1) != Some(&b'\n'))
            })
            .count()
            + 1;
        let line_start = text[..head].rfind(['\n', '\r']).map_or(0, |i| i + 1);
        (line, text[line_start..head].graphemes(true).count() + 1)
    }

    /// Document length, in lines for code and words for prose, and how much
    /// of it a selection covers.
    fn extent(&self) -> String {
        if self.language.is_some() {
            let lines = self.line_count();
            let selected = self.document.selected_text().lines().count();
            let unit = if lines == 1 { "line" } else { "lines" };
            return if selected > 0 {
                format!("{selected} of {lines} {unit}")
            } else {
                format!("{lines} {unit}")
            };
        }
        let words = self.source_word_count();
        let unit = if words == 1 { "word" } else { "words" };
        if self.document.selection().is_empty() {
            format!("{words} {unit}")
        } else {
            let selected = self.document.selected_text().unicode_words().count();
            format!("{selected} of {words} {unit}")
        }
    }

    /// Render one segmented status line. The caller may pass the full frame
    /// after painting other panes; this does not alter editor or search geometry.
    pub(crate) fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        let area = area.intersection(frame.area());
        if area.height < 2 || area.width == 0 {
            return;
        }
        let footer = Rect::new(area.x, area.bottom() - 1, area.width, 1);
        let colors = theme::chrome_palette();
        frame.render_widget(Clear, footer);
        frame.render_widget(Block::default().style(colors.status.style()), footer);
        let (line, col) = self.caret_line_column();
        let location = format!("Ln {line}, Col {col}");
        let mut bar = StatusBar::new(footer, colors.status.style());
        if let Some(context) = self.footer_context(area.width) {
            let style = if self.message_is_error {
                colors.status_warning.style()
            } else {
                colors.status.style()
            };
            let text = format!(" {context}");
            // Position yields to the whole message; errors take the full row.
            if self.message_is_error {
                frame.render_widget(Block::default().style(style), footer);
            } else if UnicodeWidthStr::width(text.as_str()) + bar.cells(&location, true)
                < usize::from(footer.width)
            {
                bar.push(Side::Right, &location, colors.status_accent.style(), true);
            }
            bar.draw(frame);
            let width = bar.left_edge().saturating_sub(footer.x);
            frame.render_widget(
                Paragraph::new(text).style(style),
                Rect::new(footer.x, footer.y, width, 1),
            );
            return;
        }
        let (label, icon) = self.format_badge();
        let label = label.as_str();
        let badge = if icon.is_empty() {
            label.to_owned()
        } else {
            format!("{icon} {label}")
        };
        let badge_style = if label == "SOURCE" {
            colors.status_alternate.style()
        } else {
            colors.status_accent.style()
        }
        .add_modifier(Modifier::BOLD);
        let secondary = colors.status_secondary.style();
        let length = self.document.text().len();
        let head = self.document.selection().head;
        let percent = head.saturating_mul(100).checked_div(length).unwrap_or(100);
        let newline = match eymi::editing::preferred_newline(self.document.text(), 0) {
            "\r\n" => "CRLF",
            "\r" => "CR",
            _ => "LF",
        };
        let bom = self.document.text().starts_with('\u{feff}');
        let encoding = format!("UTF-8{} · {newline}", if bom { " BOM" } else { "" });
        // Higher priority segments claim space first; each side keeps its order.
        let fits_badge = bar.fits(&badge, true) && {
            let pill = bar.cells(&badge, true) + bar.cells(&location, true);
            pill <= usize::from(footer.width)
        };
        if fits_badge {
            bar.push(Side::Left, &badge, badge_style, true);
        }
        if !bar.push(Side::Right, &location, colors.status_accent.style(), true) {
            bar.push(
                Side::Right,
                &format!("{line}:{col}"),
                colors.status_accent.style(),
                false,
            );
        }
        if fits_badge {
            bar.push(Side::Left, &self.extent(), secondary, false);
            bar.push(Side::Right, &format!("{percent}%"), secondary, false);
            bar.push(Side::Right, &encoding, secondary, false);
        }
        bar.draw(frame);
    }

    pub(crate) fn draw_overlay(&self, frame: &mut Frame) {
        self.reload_prompt_visible.set(false);
        let text = |line: &'static str| Line::from(line);
        match &self.overlay {
            Overlay::None | Overlay::Search { .. } => {}
            Overlay::Help => self.draw_help(frame),
            Overlay::Reload => {
                let shown = ui::dialog(
                    frame,
                    "Reload from disk",
                    &[
                        text("Replace local edits with disk text?"),
                        Line::from(Span::styled(
                            "Undo can restore your local edits.",
                            ui::muted(),
                        )),
                    ],
                    &[("Y", "Reload"), ("N", "Keep editing")],
                    60,
                );
                self.reload_prompt_visible.set(shown);
            }
            Overlay::Quit => {
                ui::dialog(
                    frame,
                    "Unsaved changes",
                    &[text("Save before quitting?")],
                    &[
                        ("Y", "Save and quit"),
                        ("N", "Discard"),
                        ("Esc", "Keep editing"),
                    ],
                    64,
                );
            }
            Overlay::SaveAs { path, .. } => {
                let colors = palette();
                let mut body = vec![
                    Line::from(vec![
                        Span::styled(
                            "❯ ",
                            ui::surface().fg(colors.accent).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(safe_text(path)),
                        Span::styled("▏", ui::surface().fg(colors.accent)),
                    ]),
                    Line::from(Span::styled(
                        "Existing files are protected; enter a new path.",
                        ui::muted(),
                    )),
                ];
                if !self.message.is_empty() {
                    body.push(Line::from(Span::styled(
                        safe_text(&self.message),
                        ui::surface().fg(colors.warning),
                    )));
                }
                ui::dialog(
                    frame,
                    "Save As · new filename",
                    &body,
                    &[("⏎", "Save"), ("Esc", "Cancel")],
                    78,
                );
            }
        }
    }

    /// A two-column keyboard reference when there is room, one otherwise.
    fn draw_help(&self, frame: &mut Frame) {
        let area = frame.area();
        let width = area.width.saturating_sub(4).min(100);
        let height = area.height.saturating_sub(2);
        if width < 24 || height < 5 {
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new("Resize for help\nEsc: close").style(ui::surface()),
                area,
            );
            return;
        }
        let columns = if width >= 80 { 2 } else { 1 };
        let sheet = help_sheet(columns);
        let height = (sheet.len() as u16 + 2).min(height);
        let rect = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 3,
            width,
            height,
        );
        let inner = ui::panel(frame, rect, "Eymi · Help", None);
        let visible = usize::from(inner.height);
        let scroll = self.help_scroll.min(sheet.len().saturating_sub(visible));
        let column_width = inner.width.saturating_sub(2) / columns as u16;
        for (row, line) in sheet.iter().skip(scroll).take(visible).enumerate() {
            for (column, cell) in line.iter().enumerate() {
                let x = inner.x + 1 + column as u16 * column_width;
                draw_help_cell(
                    frame,
                    Rect::new(x, inner.y + row as u16, column_width, 1),
                    cell,
                );
            }
        }
        let hint = if sheet.len() > visible {
            "↑↓ scroll · esc close"
        } else {
            "esc close"
        };
        ui::hints(frame, rect, hint);
    }
}

/// One cell of the help sheet: a section title or a key and its action.
enum HelpCell {
    Blank,
    Title(&'static str),
    Key(&'static str, &'static str),
}

/// Sections flow down the columns, balanced by height.
fn help_sheet(columns: usize) -> Vec<Vec<HelpCell>> {
    let heights: Vec<usize> = HELP.iter().map(|(_, keys)| keys.len() + 2).collect();
    let total: usize = heights.iter().sum();
    let mut assigned = vec![Vec::new(); columns];
    let mut column = 0;
    let mut filled = 0;
    for (index, height) in heights.iter().enumerate() {
        if column + 1 < columns && filled > 0 && filled + height / 2 > total.div_ceil(columns) {
            column += 1;
            filled = 0;
        }
        assigned[column].push(index);
        filled += height;
    }
    let lists: Vec<Vec<HelpCell>> = assigned
        .into_iter()
        .map(|sections| {
            let mut cells = Vec::new();
            for index in sections {
                let (title, keys) = HELP[index];
                if !cells.is_empty() {
                    cells.push(HelpCell::Blank);
                }
                cells.push(HelpCell::Title(title));
                cells.extend(keys.iter().map(|(key, action)| HelpCell::Key(key, action)));
            }
            cells
        })
        .collect();
    let rows = lists.iter().map(Vec::len).max().unwrap_or(0);
    let mut lists: Vec<_> = lists.into_iter().map(Vec::into_iter).collect();
    (0..rows)
        .map(|_| {
            lists
                .iter_mut()
                .map(|cells| cells.next().unwrap_or(HelpCell::Blank))
                .collect()
        })
        .collect()
}

fn draw_help_cell(frame: &mut Frame, area: Rect, cell: &HelpCell) {
    let colors = palette();
    let buffer = frame.buffer_mut();
    let room = usize::from(area.width.saturating_sub(1));
    match cell {
        HelpCell::Blank => {}
        HelpCell::Title(title) => {
            buffer.set_stringn(
                area.x,
                area.y,
                title,
                room,
                ui::surface()
                    .fg(colors.heading)
                    .add_modifier(Modifier::BOLD),
            );
        }
        HelpCell::Key(key, action) => {
            // Keys right-align in a column that yields to actions when narrow.
            let keys = 19.min(room / 2);
            let key = ui::clipped(key, keys);
            let pad = keys.saturating_sub(UnicodeWidthStr::width(key.as_str()));
            let x = area.x + pad as u16;
            buffer.set_stringn(x, area.y, &key, room, ui::surface().fg(colors.accent));
            let start = area.x + (keys + 2).min(room) as u16;
            let space = usize::from(area.right().saturating_sub(start + 1));
            buffer.set_stringn(start, area.y, *action, space, ui::surface());
        }
    }
}

fn body_top(height: u16) -> u16 {
    u16::from(height > 0)
}

/// Indentation for guides on each visible row, with the document's smallest
/// indent as the step. Wrapped rows share their line's indent; blank lines
/// take the indent of the code that follows, so a block's guides run through
/// its blank lines and stop where it closes.
fn indent_guides(
    rows: &[crate::projection::VisualRow],
    scroll: usize,
    height: usize,
) -> Option<(usize, Vec<usize>)> {
    let leading = |row: &crate::projection::VisualRow| {
        row.glyphs
            .iter()
            .find(|glyph| !glyph.source.is_empty() && !glyph.text.trim().is_empty())
            .map(|glyph| glyph.column)
    };
    let unit = rows
        .iter()
        .filter(|row| row.line.is_some())
        .filter_map(leading)
        .filter(|indent| *indent > 0)
        .min()?
        .clamp(2, 8);
    // Resolve each row's indent, carrying a line's indent onto its wrapped rows.
    let resolve = |index: usize| -> Option<usize> {
        let first = rows[..=index].iter().rposition(|row| row.line.is_some())?;
        leading(&rows[first])
    };
    let end = (scroll + height).min(rows.len());
    let mut indents: Vec<Option<usize>> = (scroll..end).map(resolve).collect();
    let mut next = (end..rows.len()).take(200).find_map(resolve).unwrap_or(0);
    for indent in indents.iter_mut().rev() {
        next = *indent.get_or_insert(next);
    }
    Some((unit, indents.into_iter().map(|i| i.unwrap_or(0)).collect()))
}

/// Paint a row surface between the text column's margins; labels hanging
/// into a margin too narrow for them are omitted.
fn draw_fill(frame: &mut Frame, area: Rect, viewport: Rect, fill: &Fill, y: u16) {
    let origin = viewport.x as isize + fill.from;
    let left = origin.max(area.x as isize) as u16;
    let right = viewport.right().min(area.right());
    if left >= right {
        return;
    }
    let page = palette().background;
    let buffer = frame.buffer_mut();
    let (symbol, style) = match fill.kind {
        FillKind::Band => (" ", Style::default().bg(fill.color)),
        FillKind::Lower => ("▄", Style::default().fg(fill.color).bg(page)),
        FillKind::Upper => ("▀", Style::default().fg(fill.color).bg(page)),
        FillKind::Rule => ("─", Style::default().fg(fill.color).bg(page)),
    };
    for x in left..right {
        buffer[(x, y)].set_symbol(symbol).set_style(style);
    }
    if let Some((label, style)) = &fill.label
        && origin >= area.x as isize
    {
        buffer.set_stringn(left, y, label, usize::from(right - left), *style);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

/// Status segments packed from both edges. Badges gain rounded caps when a
/// Nerd Font is in use; plain segments are padded blocks.
struct StatusBar {
    area: Rect,
    base: Style,
    rounded: bool,
    left: Vec<(String, Style, bool)>,
    right: Vec<(String, Style, bool)>,
    used: usize,
}

impl StatusBar {
    fn new(area: Rect, base: Style) -> Self {
        Self {
            area,
            base,
            rounded: crate::icons::current() == crate::icons::IconSet::Nerd,
            left: Vec::new(),
            right: Vec::new(),
            used: 0,
        }
    }

    fn cells(&self, text: &str, badge: bool) -> usize {
        UnicodeWidthStr::width(text) + 2 + 2 * usize::from(badge && self.rounded)
    }

    /// Room for the segment and a one-cell gap from its neighbours.
    fn fits(&self, text: &str, badge: bool) -> bool {
        let gaps = usize::from(!self.left.is_empty()) + usize::from(!self.right.is_empty());
        self.used + gaps + self.cells(text, badge) < usize::from(self.area.width)
    }

    fn push(&mut self, side: Side, text: &str, style: Style, badge: bool) -> bool {
        if !self.fits(text, badge) {
            return false;
        }
        self.used += self.cells(text, badge) + 1;
        let segment = (text.to_owned(), style, badge);
        match side {
            Side::Left => self.left.push(segment),
            Side::Right => self.right.push(segment),
        }
        true
    }

    /// The first column right-hand segments occupy.
    fn left_edge(&self) -> u16 {
        let right: usize = self
            .right
            .iter()
            .map(|(text, _, badge)| self.cells(text, *badge) + 1)
            .sum();
        self.area.right().saturating_sub(right as u16)
    }

    fn draw(&self, frame: &mut Frame) {
        let y = self.area.y;
        let mut x = self.area.x;
        for (text, style, badge) in &self.left {
            x = self.segment(frame, x, y, text, *style, *badge) + 1;
        }
        let mut x = self.left_edge() + 1;
        for (text, style, badge) in self.right.iter().rev() {
            x = self.segment(frame, x, y, text, *style, *badge) + 1;
        }
    }

    fn segment(
        &self,
        frame: &mut Frame,
        x: u16,
        y: u16,
        text: &str,
        style: Style,
        badge: bool,
    ) -> u16 {
        let buffer = frame.buffer_mut();
        let mut x = x;
        let cap = self.base.fg(style.bg.unwrap_or(Color::Reset));
        if badge && self.rounded {
            buffer.set_string(x, y, "\u{e0b6}", cap);
            x += 1;
        }
        let padded = format!(" {text} ");
        buffer.set_string(x, y, &padded, style);
        x += UnicodeWidthStr::width(padded.as_str()) as u16;
        if badge && self.rounded {
            buffer.set_string(x, y, "\u{e0b4}", cap);
            x += 1;
        }
        x
    }
}

fn draw_search_buttons(
    frame: &mut Frame,
    area: Rect,
    actions: &[(SearchAction, bool)],
) -> Vec<SearchButton> {
    let label = |action, compact| match action {
        SearchAction::Previous if compact < 2 => " Prev ",
        SearchAction::Previous => " ‹ ",
        SearchAction::Next if compact < 2 => " Next ",
        SearchAction::Next => " › ",
        SearchAction::Replace if compact == 0 => " Replace 1 ",
        SearchAction::Replace => " One ",
        SearchAction::ReplaceAll if compact == 0 => " Replace all ",
        SearchAction::ReplaceAll => " All ",
        SearchAction::Close if compact < 2 => " Close ",
        SearchAction::Close => " × ",
    };
    let compact = (0..=2)
        .find(|compact| {
            actions
                .iter()
                .map(|(action, _)| label(*action, *compact).len() + 1)
                .sum::<usize>()
                .saturating_sub(1)
                <= usize::from(area.width)
        })
        .unwrap_or(2);
    let mut buttons = Vec::new();
    let mut column = 0;
    for (action, enabled) in actions {
        let mut text = label(*action, compact);
        if *action == SearchAction::Close && area.width < 3 {
            text = "×";
        }
        let width = text.len();
        let reserve = if *action == SearchAction::Close
            || !actions
                .iter()
                .any(|(action, _)| *action == SearchAction::Close)
        {
            0
        } else {
            label(SearchAction::Close, compact).len() + 1
        };
        if column + width + reserve > usize::from(area.width) {
            continue;
        }
        let rect = Rect::new(area.x + column as u16, area.y, width as u16, 1);
        let style = if *enabled {
            chrome_style().bg(theme::chrome_palette().tab_active.background)
        } else {
            chrome_muted()
        };
        frame.render_widget(Paragraph::new(text).style(style), rect);
        buttons.push(SearchButton {
            area: rect,
            action: *action,
            enabled: *enabled,
        });
        column += width + 1;
    }
    buttons
}

pub(crate) fn draw_field(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    input: &Document,
    focus: Focus,
    focused: bool,
    frozen_start: Option<usize>,
) -> FieldMap {
    let mut map = FieldMap {
        area,
        focus,
        glyphs: Vec::new(),
        start: 0,
        end: 0,
        revision: input.revision(),
    };
    let label_width = UnicodeWidthStr::width(label).min(usize::from(area.width));
    frame.render_widget(
        Paragraph::new(label).style(if focused {
            chrome_focus().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        }),
        Rect::new(area.x, area.y, label_width as u16, 1),
    );
    let width = usize::from(area.width).saturating_sub(label_width);
    if width == 0 {
        map.area.width = 0;
        return map;
    }
    let glyphs: Vec<_> = input
        .text()
        .grapheme_indices(true)
        .map(|(start, grapheme)| {
            let mut text = safe_text(grapheme);
            if UnicodeWidthStr::width(text.as_str()) == 0 {
                text.insert(0, '◌');
            }
            let cells = UnicodeWidthStr::width(text.as_str());
            (start, start + grapheme.len(), text, cells)
        })
        .collect();
    let caret = input.selection().head;
    let caret_column: usize = glyphs.iter().filter(|g| g.1 <= caret).map(|g| g.3).sum();
    let mut skip = 0;
    let mut hidden = 0;
    while skip < glyphs.len()
        && if let Some(start) = frozen_start {
            glyphs[skip].0 < start
        } else {
            caret_column.saturating_sub(hidden) >= width
        }
    {
        hidden += glyphs[skip].3;
        skip += 1;
    }
    map.start = glyphs.get(skip).map_or(input.text().len(), |g| g.0);
    map.end = map.start;
    let selected = input.selection().range();
    let mut column = 0;
    for (start, end, text, cells) in glyphs.iter().skip(skip) {
        if column + cells > width {
            break;
        }
        let style = if *start < selected.end && selected.start < *end {
            Style::default()
                .bg(palette().selection)
                .fg(palette().selection_text)
        } else {
            Style::default()
        };
        let x = area.x + (label_width + column) as u16;
        frame
            .buffer_mut()
            .set_stringn(x, area.y, text, width - column, style);
        map.glyphs.push(FieldGlyph {
            column: x,
            width: *cells as u16,
            source: *start..*end,
        });
        map.end = *end;
        column += cells;
    }
    if focused {
        frame.set_cursor_position((
            area.x + (label_width + caret_column.saturating_sub(hidden).min(width - 1)) as u16,
            area.y,
        ));
    }
    map
}

fn is_markdown(path: &std::path::Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "md" | "markdown" | "mdown" | "mkd" | "mdx"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, style::Color};

    #[test]
    fn alt_arrows_move_and_duplicate_source_lines_in_both_views() {
        for live in [false, true] {
            for markdown in [false, true] {
                let mut app = App::new("one\r\né👩🏽‍💻\nthree".into(), None, live);
                app.markdown = markdown;
                app.document.set_caret(7).unwrap();
                draw(&mut app, 50, 12);
                key(&mut app, KeyCode::Up, KeyModifiers::ALT);
                assert_eq!(app.document.text(), "é👩🏽‍💻\r\none\nthree");
                assert_eq!(app.document.selection(), Selection::caret(2));
                key(&mut app, KeyCode::Down, KeyModifiers::ALT);
                assert_eq!(app.document.text(), "one\r\né👩🏽‍💻\nthree");
                key(
                    &mut app,
                    KeyCode::Down,
                    KeyModifiers::ALT | KeyModifiers::SHIFT,
                );
                assert_eq!(app.document.text(), "one\r\né👩🏽‍💻\né👩🏽‍💻\nthree");
                key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
                assert_eq!(app.document.text(), "one\r\né👩🏽‍💻\nthree");
                key(
                    &mut app,
                    KeyCode::Up,
                    KeyModifiers::ALT | KeyModifiers::SHIFT,
                );
                assert_eq!(app.document.text(), "one\r\né👩🏽‍💻\né👩🏽‍💻\nthree");
                assert_eq!(app.document.selection(), Selection::caret(7));
            }
        }
    }

    #[test]
    fn line_shortcuts_ignore_releases_and_stay_inside_overlays() {
        let mut app = App::new("one\ntwo".into(), None, true);
        for code in [KeyCode::Up, KeyCode::Down] {
            app.handle_event(Event::Key(KeyEvent::new_with_kind(
                code,
                KeyModifiers::ALT | KeyModifiers::SHIFT,
                KeyEventKind::Release,
            )));
        }
        assert_eq!(app.document.text(), "one\ntwo");
        app.open_help();
        key(
            &mut app,
            KeyCode::Down,
            KeyModifiers::ALT | KeyModifiers::SHIFT,
        );
        assert_eq!(app.document.text(), "one\ntwo");
        assert!(!app.document.can_undo());
        let keys: Vec<_> = HELP.iter().flat_map(|(_, keys)| keys.iter()).collect();
        for key in ["F10", "F11", "Alt+↑ / Alt+↓", "Alt+Shift+↑ / ↓"] {
            assert!(keys.iter().any(|(name, _)| *name == key), "{key}");
        }
    }

    fn disk_app(source: &str) -> (tempfile::TempDir, PathBuf, App) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("external.md");
        std::fs::write(&path, source).unwrap();
        let app = App::open(Some(path.clone())).unwrap();
        (dir, path, app)
    }

    fn status_text(terminal: &Terminal<TestBackend>, row: u16) -> String {
        (0..terminal.backend().buffer().area.width)
            .map(|x| terminal.backend().buffer()[(x, row)].symbol())
            .collect()
    }

    #[test]
    fn status_segments_use_explicit_theme_pairs_without_body_color_bleed() {
        for theme in [
            Theme::Dark,
            Theme::Light,
            Theme::from_name("Dracula").unwrap(),
            Theme::from_name("Nord").unwrap(),
        ] {
            let mut app = App::new("one two three".into(), None, true);
            app.set_theme(theme);
            let colors = theme.chrome_palette();
            let terminal = draw(&mut app, 100, 24);
            let buffer = terminal.backend().buffer();
            assert_eq!(
                (buffer[(1, 23)].fg, buffer[(1, 23)].bg),
                (
                    colors.status_accent.foreground,
                    colors.status_accent.background
                )
            );
            assert_eq!(
                (buffer[(50, 23)].fg, buffer[(50, 23)].bg),
                (colors.status.foreground, colors.status.background)
            );
            assert_eq!(
                (buffer[(98, 23)].fg, buffer[(98, 23)].bg),
                (
                    colors.status_accent.foreground,
                    colors.status_accent.background
                )
            );
            assert_eq!(
                (buffer[(80, 23)].fg, buffer[(80, 23)].bg),
                (
                    colors.status_secondary.foreground,
                    colors.status_secondary.background
                )
            );
            assert_eq!(buffer[(50, 22)].bg, theme.palette().background);
            app.set_message("Disk conflict; your edits are intact");
            let terminal = draw(&mut app, 100, 24);
            for x in 0..100 {
                let cell = &terminal.backend().buffer()[(x, 23)];
                assert_eq!(
                    (cell.fg, cell.bg),
                    (
                        colors.status_warning.foreground,
                        colors.status_warning.background
                    )
                );
            }
        }
        theme::set_theme(Theme::Dark);
    }

    #[test]
    fn optional_icons_keep_textual_status_labels_and_never_enter_the_document() {
        let source = "one two";
        let mut app = App::new(source.into(), None, true);
        crate::icons::set(crate::icons::IconSet::Plain);
        let plain = status_text(&draw(&mut app, 80, 24), 23);
        crate::icons::set(crate::icons::IconSet::Nerd);
        let nerd = status_text(&draw(&mut app, 80, 24), 23);
        for width in 0..=40 {
            draw(&mut app, width, 4);
        }
        crate::icons::set(crate::icons::IconSet::Plain);
        assert!(plain.starts_with(" MARKDOWN "));
        assert!(
            !plain
                .chars()
                .any(|c| ('\u{e000}'..='\u{f8ff}').contains(&c))
        );
        assert!(nerd.contains(crate::icons::IconSet::Nerd.file(true)));
        assert!(nerd.contains("MARKDOWN"));
        assert!(nerd.contains("Ln 1, Col 1"));
        assert_eq!(app.document.text(), source);
        assert!(!app.document.is_dirty());
    }

    #[test]
    fn one_tab_row_leaves_the_first_document_row_visible_at_every_height() {
        for height in 0..=24 {
            let mut app = App::new("first line\nsecond line".into(), None, true);
            let terminal = draw(&mut app, 80, height);
            assert_eq!(app.viewport.y, u16::from(height > 0));
            assert_eq!(app.viewport.height, height.saturating_sub(2));
            if height >= 3 {
                assert!(status_text(&terminal, 1).contains("first line"));
            }
        }
    }

    #[test]
    fn idle_statusline_has_format_and_position_without_redundant_chrome() {
        let mut app = App::new("one\r\n界e\u{301}".into(), None, true);
        app.document.set_caret(app.document.text().len()).unwrap();
        let terminal = draw(&mut app, 100, 24);
        let footer = status_text(&terminal, 23);
        assert!(footer.starts_with(" MARKDOWN "));
        assert!(footer.contains("3 words"));
        assert!(footer.contains("Ln 2, Col 3"));
        assert!(footer.contains("100%"));
        assert!(footer.contains("UTF-8"));
        for redundant in ["Live", "Source", "Untitled", "Eymi", "F1", "^S", "Commands"] {
            assert!(!footer.contains(redundant), "{footer}");
        }
        key(&mut app, KeyCode::F(6), KeyModifiers::NONE);
        let footer = status_text(&draw(&mut app, 80, 24), 23);
        assert!(footer.starts_with(" SOURCE "));
        assert!(!footer.contains("MARKDOWN"));
        let mut plain = App::new("plain text".into(), None, false);
        let footer = status_text(&draw(&mut plain, 80, 24), 23);
        assert!(footer.starts_with(" TEXT "));
        assert!(!footer.contains("SOURCE"));
    }

    #[test]
    fn statusline_counts_follow_edits_undo_and_redo() {
        let mut app = App::new("one two".into(), None, true);
        assert!(status_text(&draw(&mut app, 80, 24), 23).contains("2 words"));
        app.document.set_caret(app.document.text().len()).unwrap();
        app.document.insert(" three");
        assert!(status_text(&draw(&mut app, 80, 24), 23).contains("3 words"));
        app.document.undo();
        assert!(status_text(&draw(&mut app, 80, 24), 23).contains("2 words"));
        app.document.redo();
        assert!(status_text(&draw(&mut app, 80, 24), 23).contains("3 words"));
        app.document
            .set_selection(Selection { anchor: 0, head: 7 })
            .unwrap();
        assert!(status_text(&draw(&mut app, 80, 24), 23).contains("2 of 3 words"));
    }

    #[test]
    fn statusline_prioritizes_messages_and_errors_over_metadata() {
        let mut app = App::new("one two three".into(), None, true);
        app.inform("Saved note.md");
        let footer = status_text(&draw(&mut app, 80, 24), 23);
        assert!(footer.contains("Saved note.md"));
        assert!(footer.contains("Ln 1, Col 1"));
        assert!(!footer.contains("MARKDOWN"));
        app.set_message(
            "Disk file changed; your local text is intact. Reload or Save As to continue.",
        );
        let footer = status_text(&draw(&mut app, 80, 24), 23);
        assert!(footer.contains("Reload or Save As to continue."));
        for optional in ["MARKDOWN", "words", "Ln 1", "UTF-8", "%"] {
            assert!(!footer.contains(optional), "{footer}");
        }
        key(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);
        assert!(status_text(&draw(&mut app, 80, 24), 23).contains("Disk file changed"));
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(status_text(&draw(&mut app, 80, 24), 23).starts_with(" MARKDOWN "));
    }

    #[test]
    fn full_width_status_keeps_narrow_editor_search_count_and_wrap_feedback() {
        let mut app = App::new("cat cat cat".into(), None, true);
        search(&mut app, "cat", false);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        app.search.wrapped = true;
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        terminal
            .draw(|frame| {
                app.draw_in(frame, Rect::new(56, 0, 24, 12), true);
                app.draw_footer(frame, frame.area());
            })
            .unwrap();
        let footer = status_text(&terminal, 11);
        assert!(footer.contains("2/3"), "{footer}");
        assert!(footer.contains("Wrapped"), "{footer}");
        assert!(footer.contains("Esc Close"), "{footer}");
        let screen: String = (0..12).map(|row| status_text(&terminal, row)).collect();
        assert_eq!(screen.matches("2/3").count(), 1);
        assert_eq!(app.terminal_width, 24);
        assert!(
            app.search_geometry
                .fields
                .iter()
                .all(|field| field.area.x >= 56)
        );
        assert_eq!(app.document.text(), "cat cat cat");
        assert!(!app.document.is_dirty());
    }

    #[test]
    fn footer_clips_to_its_slice_and_never_overwrites_the_only_tab_row() {
        let app = App::new("界e\u{301} 👩🏽‍💻".into(), None, true);
        for width in 0..=80 {
            for height in [0, 1, 2, 4] {
                let mut terminal = Terminal::new(TestBackend::new(100, 12)).unwrap();
                let area = Rect::new(7, 3, width, height);
                terminal
                    .draw(|frame| {
                        frame.render_widget(
                            Block::default().style(Style::default().bg(Color::Magenta)),
                            frame.area(),
                        );
                        app.draw_footer(frame, area);
                    })
                    .unwrap();
                for y in 0..12 {
                    for x in 0..100 {
                        let footer_cell = width > 0
                            && height >= 2
                            && x >= area.x
                            && x < area.right()
                            && y == area.bottom() - 1;
                        if !footer_cell {
                            assert_eq!(
                                terminal.backend().buffer()[(x, y)].bg,
                                Color::Magenta,
                                "{area:?} at{x},{y}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn dialogs_do_not_repeat_their_instructions_in_the_statusline() {
        let mut app = App::new("one two".into(), None, true);
        for overlay in [
            Overlay::Help,
            Overlay::Quit,
            Overlay::Reload,
            Overlay::SaveAs {
                path: String::new(),
                quit_after: false,
            },
        ] {
            app.overlay = overlay;
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            terminal
                .draw(|frame| app.draw_footer(frame, frame.area()))
                .unwrap();
            let footer = status_text(&terminal, 23);
            assert!(footer.starts_with(" MARKDOWN "));
            for duplicate in ["Esc", "Enter", "Reload", "Discard", "Scroll", "Save"] {
                assert!(!footer.contains(duplicate), "{footer}");
            }
        }
    }

    #[test]
    fn identical_reload_does_not_promise_an_undo_step() {
        let (_dir, _path, mut app) = disk_app("unchanged");
        app.request_reload();
        assert!(!app.document.can_undo());
        assert!(!app.document.is_dirty());
        assert!(app.message.contains("already matches"));
        assert!(!app.message.contains("Ctrl+Z"));
    }

    #[test]
    fn save_as_starts_fresh_external_status_for_the_new_path() {
        let (dir, old, mut app) = disk_app("base");
        std::fs::write(&old, "external old").unwrap();
        app.check_external_change();
        let new = dir.path().join("new.md");
        key(&mut app, KeyCode::F(4), KeyModifiers::NONE);
        app.handle_event(Event::Paste(new.display().to_string()));
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        std::fs::write(&new, "external new").unwrap();
        assert!(app.check_external_change());
        assert_eq!(app.message_origin, MessageOrigin::Disk);
        assert!(app.message.contains("Disk file changed"));
        assert_eq!(app.document.text(), "base");
    }

    #[test]
    fn disk_poll_reports_changes_and_recovery_without_mutating_editor() {
        let (_dir, path, mut app) = disk_app("e\u{301} original\r\n");
        app.document
            .set_selection(Selection {
                anchor: app.document.text().len(),
                head: 3,
            })
            .unwrap();
        let selection = app.document.selection();
        assert!(!app.check_external_change());
        std::fs::write(&path, "external edit").unwrap();
        assert!(app.check_external_change());
        assert_eq!(app.message_origin, MessageOrigin::Disk);
        assert!(app.message.contains("F5"));
        assert!(!app.check_external_change());
        assert_eq!(app.document.text(), "e\u{301} original\r\n");
        assert_eq!(app.document.selection(), selection);
        assert!(!app.document.can_undo());
        std::fs::remove_file(&path).unwrap();
        assert!(app.check_external_change());
        assert!(app.message.contains("missing"));
        std::fs::create_dir(&path).unwrap();
        assert!(app.check_external_change());
        assert!(app.message.contains("Cannot read"));
        std::fs::remove_dir(&path).unwrap();
        std::fs::write(&path, app.document.text()).unwrap();
        assert!(app.check_external_change());
        assert!(app.message.is_empty());
        assert!(!app.message_is_error);
        app.set_message("Different actionable error");
        assert!(!app.check_external_change());
        assert_eq!(app.message, "Different actionable error");
    }

    #[test]
    fn periodic_poll_does_not_split_contiguous_typing() {
        let (_dir, _path, mut app) = disk_app("");
        key(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);
        app.check_external_change();
        key(&mut app, KeyCode::Char('b'), KeyModifiers::NONE);
        app.check_external_change();
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.document.text(), "");
    }

    #[test]
    fn clean_reload_resets_transient_state_and_keeps_undo_dirty_against_new_disk() {
        let source = "old original source";
        let (_dir, path, mut app) = disk_app(source);
        search(&mut app, "original", false);
        app.document
            .set_selection(Selection {
                anchor: source.len(),
                head: 2,
            })
            .unwrap();
        let selection = app.document.selection();
        app.preferred_column = Some(15);
        std::fs::write(&path, "e\u{301}界\r\n").unwrap();
        key(&mut app, KeyCode::F(5), KeyModifiers::NONE);
        assert_eq!(app.document.text(), "e\u{301}界\r\n");
        assert_eq!(
            app.document.selection(),
            Selection {
                anchor: app.document.text().len(),
                head: 0
            }
        );
        assert!(!app.document.is_dirty());
        assert!(!app.has_modal());
        assert_eq!(app.overlay, Overlay::None);
        assert!(app.search.query.text().is_empty());
        assert!(app.preferred_column.is_none());
        assert_eq!(app.parsed.snapshot.revision, app.document.revision());
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.document.text(), source);
        assert_eq!(app.document.selection(), selection);
        assert!(app.document.is_dirty());
        key(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL);
        assert!(!app.document.is_dirty());
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
        assert!(!app.document.is_dirty());
    }

    #[test]
    fn dirty_reload_requires_a_visible_unmodified_nonrepeat_confirmation() {
        let (_dir, path, mut app) = disk_app("base");
        app.document.insert("local ");
        let local = app.document.text().to_owned();
        let selection = app.document.selection();
        std::fs::write(&path, "disk").unwrap();
        app.request_reload();
        assert_eq!(app.overlay, Overlay::Reload);
        assert!(app.has_modal());
        assert!(!app.workspace_commands_allowed());
        key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
        assert_eq!(app.document.text(), local);
        let terminal = draw(&mut app, 80, 24);
        let screen: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(screen.contains("Replace local edits with disk text?"));
        assert!(screen.contains(" Y  Reload   N  Keep editing"), "{screen}");
        for modifiers in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
            key(&mut app, KeyCode::Char('y'), modifiers);
            assert_eq!(app.document.text(), local);
        }
        for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
            app.handle_event(Event::Key(KeyEvent::new_with_kind(
                KeyCode::Char('y'),
                KeyModifiers::NONE,
                kind,
            )));
            assert_eq!(app.document.text(), local);
        }
        key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
        assert_eq!(app.document.selection(), selection);
        assert_eq!(app.overlay, Overlay::None);
        app.request_reload();
        draw(&mut app, 80, 24);
        std::fs::write(&path, "newest disk version").unwrap();
        key(&mut app, KeyCode::Char('Y'), KeyModifiers::SHIFT);
        assert_eq!(app.document.text(), "newest disk version");
        assert!(!app.document.is_dirty());
        app.document.undo();
        assert_eq!(app.document.text(), local);
        assert_eq!(app.document.selection(), selection);
        assert!(app.document.is_dirty());
        app.document.redo();
        assert!(!app.document.is_dirty());
    }

    #[test]
    fn tiny_resized_or_cancelled_reload_prompts_cannot_discard_text() {
        let (_dir, path, mut app) = disk_app("base");
        app.document.insert("local ");
        std::fs::write(&path, "disk").unwrap();
        let local = app.document.text().to_owned();
        for (width, height) in [(12, 4), (43, 20), (80, 8)] {
            app.request_reload();
            draw(&mut app, width, height);
            key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
            assert_eq!(app.document.text(), local);
            assert_eq!(app.overlay, Overlay::Reload);
            key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
            assert_eq!(app.overlay, Overlay::None);
        }
        app.request_reload();
        draw(&mut app, 80, 24);
        app.handle_event(Event::Resize(12, 4));
        key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
        assert_eq!(app.document.text(), local);
        draw(&mut app, 12, 4);
        key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
        assert_eq!(app.document.text(), local);
        app.handle_event(Event::Resize(80, 24));
        key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
        assert_eq!(app.document.text(), local);
        draw(&mut app, 80, 24);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
        assert!(app.document.text().contains("base"));
        assert!(app.document.is_dirty());
    }

    #[test]
    fn reload_errors_and_history_rejection_preserve_local_state_and_baseline() {
        for invalid in [
            Some(vec![0xff]),
            Some(b"binary\0data".to_vec()),
            Some(vec![b'x'; 8 * 1024 * 1024 + 1]),
            None,
        ] {
            let (_dir, path, mut app) = disk_app("base");
            app.document.insert("local ");
            let before = (
                app.document.text().to_owned(),
                app.document.selection(),
                app.document.revision(),
            );
            if let Some(bytes) = invalid {
                std::fs::write(&path, bytes).unwrap();
            } else {
                std::fs::remove_file(&path).unwrap();
            }
            app.request_reload();
            draw(&mut app, 80, 24);
            key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
            assert_eq!(
                (
                    app.document.text(),
                    app.document.selection(),
                    app.document.revision()
                ),
                (before.0.as_str(), before.1, before.2)
            );
            assert!(app.message.contains("Reload failed"));
            assert!(!app.has_modal());
            assert!(app.document.is_dirty());
            std::fs::write(&path, "base").unwrap();
            key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), before.0);
        }
        for dirty in [false, true] {
            let (_dir, path, mut app) = disk_app("base");
            if dirty {
                app.document.insert("local ");
            }
            app.document.set_history_limits(eymi::HistoryLimits {
                max_entries: 0,
                max_bytes: 0,
            });
            let before = app.document.text().to_owned();
            std::fs::write(&path, "disk").unwrap();
            app.request_reload();
            if dirty {
                draw(&mut app, 80, 24);
                key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
            }
            assert_eq!(app.document.text(), before);
            assert!(app.message.contains("history limits"));
            assert_eq!(app.document.is_dirty(), dirty);
            key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
            assert!(app.message_is_error);
            assert_eq!(std::fs::read_to_string(path).unwrap(), "disk");
        }
    }

    #[test]
    fn recovered_empty_and_unicode_content_are_detached_unsaved_and_saveable() {
        for source in ["", "e\u{301}界👩🏽‍💻\r\n"] {
            let mut app = App::recovered(
                source.into(),
                Selection {
                    anchor: usize::MAX,
                    head: 1,
                },
                true,
                true,
            );
            assert_eq!(app.document.text(), source);
            assert!(app.document.is_dirty());
            assert!(app.path().is_none());
            assert!(!app.document.can_undo());
            assert_eq!(
                app.document.selection(),
                Selection {
                    anchor: source.len(),
                    head: 0
                }
            );
            assert!(!app.check_external_change());
            app.request_reload();
            assert_eq!(app.document.text(), source);
            assert!(!app.has_modal());
            key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
            assert!(app.saving_as());
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("recovered.md");
            app.handle_event(Event::Paste(path.display().to_string()));
            key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
            assert_eq!(std::fs::read_to_string(path).unwrap(), source);
            assert!(!app.document.is_dirty());
        }
        let app = App::recovered("text".into(), Selection::caret(0), true, false);
        assert!(!app.live);
    }

    #[test]
    fn typing_groups_end_at_view_changes_help_and_deactivation() {
        let mut app = App::open(None).unwrap();
        for c in "first".chars() {
            key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
        }
        key(&mut app, KeyCode::F(6), KeyModifiers::NONE);
        for c in "second".chars() {
            key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
        }
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.document.text(), "first");
        key(
            &mut app,
            KeyCode::Char('Z'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        assert_eq!(app.document.text(), "firstsecond");
        app.deactivate();
        key(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        key(&mut app, KeyCode::F(1), KeyModifiers::NONE);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.document.text(), "firstsecondx");
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.document.text(), "firstsecond");
    }

    #[test]
    fn tab_keeps_literal_caret_input_and_indents_selected_lines() {
        let mut app = App::new("one\r\ntwo".into(), None, true);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.document.text(), "\tone\r\ntwo");
        key(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!(app.document.text(), "one\r\ntwo");
        app.document.select_all();
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.document.text(), "    one\r\n    two");
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.document.text(), "one\r\ntwo");
        assert_eq!(app.document.selected_text(), "one\r\ntwo");
    }

    #[test]
    fn code_files_name_their_language_and_count_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool.py");
        std::fs::write(&path, "def run():\n    return 1\n").unwrap();
        let mut app = App::open(Some(path)).unwrap();
        let footer = status_text(&draw(&mut app, 80, 24), 23);
        assert!(footer.starts_with(" PYTHON "), "{footer}");
        assert!(footer.contains("3 lines"), "{footer}");
        app.document
            .set_selection(Selection { anchor: 0, head: 5 })
            .unwrap();
        assert!(status_text(&draw(&mut app, 80, 24), 23).contains("1 of 3 lines"));
        let plain = dir.path().join("notes.txt");
        std::fs::write(&plain, "just words here\n").unwrap();
        let mut app = App::open(Some(plain)).unwrap();
        let footer = status_text(&draw(&mut app, 80, 24), 23);
        assert!(
            footer.starts_with(" TEXT ") && footer.contains("3 words"),
            "{footer}"
        );
    }

    #[test]
    fn scroll_track_marks_matches_only_while_find_is_open() {
        let source = (0..60)
            .map(|line| if line % 20 == 5 { "needle\n" } else { "hay\n" })
            .collect::<String>();
        let mut app = App::new(source, None, true);
        let track = |app: &App, terminal: &Terminal<TestBackend>| -> Vec<Color> {
            (app.viewport.y..app.viewport.bottom())
                .map(|y| terminal.backend().buffer()[(79, y)].fg)
                .filter(|fg| *fg != palette().foreground)
                .collect()
        };
        let idle = draw(&mut app, 80, 24);
        assert!(!track(&app, &idle).contains(&palette().search));
        search(&mut app, "needle", false);
        let open = draw(&mut app, 80, 24);
        let marks = track(&app, &open);
        assert_eq!(
            marks
                .iter()
                .filter(|fg| **fg == palette().search_active)
                .count(),
            1
        );
        assert_eq!(
            marks.iter().filter(|fg| **fg == palette().search).count(),
            2
        );
        assert!(!app.document.is_dirty());
    }

    #[test]
    fn code_views_number_lines_and_draw_guides_only_in_indentation() {
        let source = "fn main() {\n    if ok {\n\n        run();\n    }\n}\n";
        let mut app = App::new(source.into(), None, false);
        app.document.set_caret(source.find("run").unwrap()).unwrap();
        let terminal = draw(&mut app, 60, 12);
        let buffer = terminal.backend().buffer();
        let x = app.viewport.x;
        let row = |line: u16| app.viewport.y + line;
        let text = |y: u16| -> String {
            (x..app.viewport.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        };
        assert!(text(row(1)).starts_with("│   if ok {"));
        // A blank line inside a block keeps the guides of the code after it.
        assert!(text(row(2)).starts_with("│   │"));
        assert!(text(row(3)).starts_with("│   │   run();"));
        assert!(text(row(5)).starts_with('}'));
        assert_eq!(buffer[(x - 2, row(3))].symbol(), "4");
        assert!(buffer[(x - 2, row(3))].modifier.contains(Modifier::BOLD));
        assert_eq!(buffer[(x + 10, row(3))].bg, palette().cursorline);
        assert_eq!(app.document.text(), source);
    }

    #[test]
    fn help_scroll_reaches_every_shortcut_without_editing_the_document() {
        let mut app = App::new("Keep this source".into(), None, true);
        key(&mut app, KeyCode::F(1), KeyModifiers::NONE);
        key(&mut app, KeyCode::End, KeyModifiers::NONE);
        let screen = crate::simulation::snapshot(&mut app, 42, 12).unwrap();
        assert!(screen.contains("This reference"), "{screen}");
        key(&mut app, KeyCode::Home, KeyModifiers::NONE);
        assert_eq!(app.help_scroll, 0);
        key(&mut app, KeyCode::PageDown, KeyModifiers::NONE);
        assert_eq!(app.help_scroll, 8);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.document.text(), "Keep this source");
        assert!(!app.document.can_undo());
    }

    fn draw(app: &mut App, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        terminal
    }
    fn key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        app.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
    }
    fn click(app: &mut App, source: usize, release: bool) {
        let (row, col) = app.projection.cursor(source);
        click_at(app, row, col, release);
    }

    fn click_at(app: &mut App, row: usize, col: usize, release: bool) {
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: app.viewport.x + col as u16,
            row: app.viewport.y + (row - app.scroll) as u16,
            modifiers: KeyModifiers::NONE,
        };
        app.handle_event(Event::Mouse(event));
        if release {
            app.handle_event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                ..event
            }));
        }
    }

    #[test]
    fn production_event_path_toggles_task_but_label_places_caret() {
        let source = "intro\n\n- [ ] task text\n";
        let mut app = App::new(source.into(), None, true);
        draw(&mut app, 80, 24);
        click(&mut app, source.find('[').unwrap(), true);
        assert_eq!(app.document.text(), "intro\n\n- [x] task text\n");
        assert_eq!(app.document.selection().head, 0);
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.document.text(), source);
        draw(&mut app, 80, 24);
        click(&mut app, source.find("text").unwrap(), true);
        draw(&mut app, 80, 24);
        assert_eq!(app.document.selection().head, source.find("text").unwrap());
        assert_eq!(app.document.text(), source);
        assert!(app.projection.rows[2].glyphs.iter().any(|g| g.text == "["));
    }

    fn checkbox_position(app: &App) -> (usize, usize) {
        app.projection
            .rows
            .iter()
            .enumerate()
            .find_map(|(row, visual)| {
                visual
                    .glyphs
                    .iter()
                    .find(|glyph| glyph.task.is_some())
                    .map(|glyph| (row, glyph.column))
            })
            .unwrap()
    }

    #[test]
    fn checkbox_center_and_neighbor_clicks_preserve_selection_and_undo() {
        for marker in ["[ ]", "[x]"] {
            for delta in [-1, 0, 1] {
                let source = format!("intro\n\n- {marker} task text\n");
                let mut app = App::new(source.clone(), None, true);
                let selection = Selection { anchor: 3, head: 1 };
                app.document.set_selection(selection).unwrap();
                draw(&mut app, 80, 24);
                let (row, column) = checkbox_position(&app);
                click_at(&mut app, row, column.saturating_add_signed(delta), true);
                let mut expected = source.clone();
                let state = source.find('[').unwrap() + 1;
                expected.replace_range(state..state + 1, if marker == "[ ]" { "x" } else { " " });
                assert_eq!(app.document.text(), expected, "{marker} delta {delta}");
                assert_eq!(app.document.selection(), selection);
                key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
                assert_eq!(app.document.text(), source);
                assert_eq!(app.document.selection(), selection);
                assert!(!app.document.can_undo());
            }
        }
    }

    #[test]
    fn checkbox_padding_does_not_capture_label_or_more_distant_cells() {
        let source = "intro\n\n- [ ] task text\n";
        for delta in [-2, 2, 3] {
            let mut app = App::new(source.into(), None, true);
            draw(&mut app, 80, 24);
            let (row, column) = checkbox_position(&app);
            // The checkbox replaces its bullet, so nothing lies to its left.
            let Some(column) = column.checked_add_signed(delta) else {
                continue;
            };
            let expected = app.projection.hit(row, column).offset;
            click_at(&mut app, row, column, true);
            assert_eq!(app.document.text(), source);
            assert_eq!(app.document.selection(), Selection::caret(expected));
        }
        for live in [false, true] {
            let mut app = App::new(source.into(), None, true);
            app.live = live;
            app.document
                .set_caret(source.find("task").unwrap())
                .unwrap();
            draw(&mut app, 80, 24);
            assert!(
                app.projection
                    .rows
                    .iter()
                    .flat_map(|r| &r.glyphs)
                    .all(|g| g.task.is_none())
            );
            click(&mut app, source.find('[').unwrap(), true);
            assert_eq!(app.document.text(), source);
            assert_eq!(app.document.selection().head, source.find('[').unwrap());
        }
    }

    #[test]
    fn checkbox_padding_is_not_used_for_drag_or_vertical_navigation() {
        let source = "intro\n\n- [ ] task text\n";
        for delta in [-1, 1] {
            let mut app = App::new(source.into(), None, true);
            draw(&mut app, 80, 24);
            let (row, column) = checkbox_position(&app);
            let column = column.saturating_add_signed(delta);
            let target = app.projection.hit(row, column).offset;
            click(&mut app, 0, false);
            let event = MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: app.viewport.x + column as u16,
                row: app.viewport.y + row as u16,
                modifiers: KeyModifiers::NONE,
            };
            app.handle_event(Event::Mouse(event));
            app.handle_event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                ..event
            }));
            assert_eq!(app.document.text(), source);
            assert_eq!(
                app.document.selection(),
                Selection {
                    anchor: 0,
                    head: target
                }
            );

            let mut app = App::new(source.into(), None, true);
            app.document.set_caret(column).unwrap();
            draw(&mut app, 80, 24);
            key(&mut app, KeyCode::Down, KeyModifiers::NONE);
            draw(&mut app, 80, 24);
            key(&mut app, KeyCode::Down, KeyModifiers::NONE);
            assert_eq!(app.document.text(), source);
            assert_eq!(app.document.selection().head, target);
        }
    }

    #[test]
    fn checkbox_padding_stays_on_its_wrapped_row_and_inside_viewport() {
        let source = "intro\n\n- [ ] task text\n";
        // Width 5 wraps the checkbox to column 0; width 6 puts it at row end.
        for width in [5, 6] {
            let mut app = App::new(source.into(), None, true);
            draw(&mut app, width, 30);
            let (row, column) = checkbox_position(&app);
            assert!(
                app.projection
                    .pointer_task(
                        &app.parsed,
                        row,
                        usize::from(app.viewport.width),
                        usize::from(app.viewport.width)
                    )
                    .is_none()
            );
            assert!(
                app.projection
                    .pointer_task(
                        &app.parsed,
                        app.projection.rows.len(),
                        column,
                        usize::from(app.viewport.width)
                    )
                    .is_none()
            );
            let outside = MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: if column == 0 {
                    app.viewport.x - 1
                } else {
                    app.viewport.right()
                },
                row: app.viewport.y + row as u16,
                modifiers: KeyModifiers::NONE,
            };
            app.handle_event(Event::Mouse(outside));
            assert_eq!(app.document.text(), source);
            let adjacent_row = if column == 0 { row - 1 } else { row + 1 };
            let adjacent_col = if column == 0 {
                usize::from(app.viewport.width - 1)
            } else {
                0
            };
            let expected = app.projection.hit(adjacent_row, adjacent_col).offset;
            click_at(&mut app, adjacent_row, adjacent_col, true);
            assert_eq!(app.document.text(), source);
            assert_eq!(app.document.selection().head, expected);

            let mut app = App::new(source.into(), None, true);
            draw(&mut app, width, 30);
            let (row, column) = checkbox_position(&app);
            click_at(&mut app, row, column + 1, true);
            assert_eq!(app.document.text(), "intro\n\n- [x] task text\n");
            assert_eq!(app.document.selection(), Selection::caret(0));
        }
    }

    #[test]
    fn enter_paste_view_switch_and_undo_share_the_document() {
        let mut app = App::new("- [x] Done".into(), None, true);
        app.document.set_caret(app.document.text().len()).unwrap();
        draw(&mut app, 80, 24);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.document.text(), "- [x] Done\n- [ ] ");
        app.handle_event(Event::Paste("one\n- untouched\r\n".into()));
        assert!(app.document.text().ends_with("one\n- untouched\r\n"));
        let selection = app.document.selection();
        key(&mut app, KeyCode::F(6), KeyModifiers::NONE);
        assert_eq!(app.document.selection(), selection);
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.document.text(), "- [x] Done\n- [ ] ");
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.document.text(), "- [x] Done");
    }

    #[test]
    fn drag_freezes_geometry_then_reveals_selected_source() {
        let source = "first\n\n**bold** and more\n\nlast";
        let mut app = App::new(source.into(), None, true);
        draw(&mut app, 24, 20);
        let target = source.find("more").unwrap();
        let (row, col) = app.projection.cursor(target);
        click(&mut app, 0, false);
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: app.viewport.x + col as u16,
            row: app.viewport.y + row as u16,
            modifiers: KeyModifiers::NONE,
        }));
        draw(&mut app, 24, 20);
        assert_eq!(app.projection.cursor(target), (row, col));
        assert_eq!(app.document.selected_text(), &source[..target]);
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: app.viewport.x + col as u16,
            row: app.viewport.y + row as u16,
            modifiers: KeyModifiers::NONE,
        }));
        draw(&mut app, 24, 20);
        assert!(
            app.projection
                .rows
                .iter()
                .flat_map(|r| &r.glyphs)
                .any(|g| g.text == "*")
        );
    }

    #[test]
    fn resize_and_vertical_selection_keep_valid_source_positions() {
        let source = "a界e\u{301}👩‍💻 long wrapped line\nnext";
        let mut app = App::new(source.into(), None, true);
        draw(&mut app, 18, 12);
        key(&mut app, KeyCode::Down, KeyModifiers::SHIFT);
        assert!(
            app.document
                .is_grapheme_boundary(app.document.selection().head)
        );
        let before = app.document.selection();
        app.handle_event(Event::Resize(9, 8));
        draw(&mut app, 9, 8);
        assert_eq!(app.document.selection(), before);
        assert!(!app.document.is_dirty());
        for width in 0..6 {
            draw(&mut app, width, 2);
        }
    }

    #[test]
    fn quit_and_save_conflict_keep_unsaved_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, "base").unwrap();
        let mut app = App::open(Some(path.clone())).unwrap();
        app.document.insert("local");
        std::fs::write(&path, "external").unwrap();
        key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert!(app.document.is_dirty());
        assert!(app.message.contains("changed"));
        key(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert_eq!(app.overlay, Overlay::Quit);
        assert!(!app.should_exit);
        key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
        assert!(!app.should_exit);
        assert!(app.document.is_dirty());
        assert_eq!(app.overlay, Overlay::None);
        assert!(app.message.contains("changed"));
        key(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.overlay, Overlay::None);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "external");
    }

    #[test]
    fn plain_text_enter_does_not_continue_lists() {
        let mut app = App::new("- literal".into(), None, false);
        app.document.set_caret(app.document.text().len()).unwrap();
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.document.text(), "- literal\n");
    }

    #[test]
    fn empty_internal_clipboard_keeps_selected_text_intact() {
        let mut app = App::new("keep this".into(), None, true);
        app.document.select_all();
        let selection = app.document.selection();
        key(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert_eq!(app.document.text(), "keep this");
        assert_eq!(app.document.selection(), selection);
        assert!(!app.document.is_dirty());
        assert!(app.message.contains("empty"));
    }

    #[test]
    fn untitled_save_failure_can_be_recovered_through_save_as() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(String::new(), None, true);
        app.handle_event(Event::Paste("draft\r\n".into()));
        key(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
        key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
        app.handle_event(Event::Paste(
            dir.path()
                .join("missing")
                .join("draft.md")
                .display()
                .to_string(),
        ));
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(!app.should_exit);
        assert!(app.document.is_dirty());
        assert!(matches!(app.overlay, Overlay::SaveAs { .. }));
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut app, KeyCode::F(4), KeyModifiers::NONE);
        let path = dir.path().join("saved.md");
        app.handle_event(Event::Paste(path.display().to_string()));
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(std::fs::read(path).unwrap(), b"draft\r\n");
        assert!(!app.document.is_dirty());
        assert!(!app.should_exit);
        assert_eq!(app.overlay, Overlay::None);
    }

    #[test]
    fn end_and_blank_cell_click_keep_upstream_wrap_affinity() {
        let mut app = App::new("abcdefghijklmnopqrstuvwxyz0123456789".into(), None, false);
        draw(&mut app, 12, 12);
        let first_end = app.projection.rows[0].end;
        key(&mut app, KeyCode::End, KeyModifiers::NONE);
        draw(&mut app, 12, 12);
        assert_eq!(app.document.selection().head, first_end);
        assert_eq!(app.caret_position(), (0, 9));
        key(&mut app, KeyCode::Down, KeyModifiers::SHIFT);
        draw(&mut app, 12, 12);
        assert_eq!(app.caret_position(), (1, 9));
        assert_eq!(app.document.selection().anchor, first_end);
        key(&mut app, KeyCode::Up, KeyModifiers::NONE);
        draw(&mut app, 12, 12);
        assert_eq!(app.caret_position(), (0, 9));
        key(&mut app, KeyCode::Home, KeyModifiers::NONE);
        draw(&mut app, 12, 12);
        assert_eq!(app.caret_position(), (0, 0));
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: app.viewport.x + 9,
            row: app.viewport.y,
            modifiers: KeyModifiers::NONE,
        };
        app.handle_event(Event::Mouse(event));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            ..event
        }));
        let mut terminal = draw(&mut app, 12, 12);
        assert_eq!(app.caret_position(), (0, 9));
        assert_eq!(
            terminal.get_cursor_position().unwrap(),
            (app.viewport.x + 9, app.viewport.y).into()
        );
    }

    #[test]
    fn snapshot_uses_real_rendered_cells_and_cr_status() {
        let mut app = App::new("first\r\n\r- [ ] **task**".into(), None, true);
        let live = crate::simulation::snapshot(&mut app, 80, 24).unwrap();
        assert!(live.contains("□ task"));
        key(&mut app, KeyCode::F(6), KeyModifiers::NONE);
        let source = crate::simulation::snapshot(&mut app, 80, 24).unwrap();
        assert!(source.contains("- [ ] **task**"));
        app.document
            .set_caret(app.document.text().find('-').unwrap())
            .unwrap();
        let moved = crate::simulation::snapshot(&mut app, 80, 24).unwrap();
        assert!(moved.contains("Ln 3, Col 1"));
    }

    fn fake_clipboard(
        app: &mut App,
    ) -> std::sync::Arc<std::sync::Mutex<crate::clipboard::tests::State>> {
        let state = std::sync::Arc::new(std::sync::Mutex::new(
            crate::clipboard::tests::State::default(),
        ));
        app.clipboard =
            Clipboard::with_backend(Box::new(crate::clipboard::tests::Fake(state.clone())));
        state
    }

    fn search(app: &mut App, query: &str, replace: bool) {
        key(
            app,
            KeyCode::Char(if replace { 'r' } else { 'f' }),
            KeyModifiers::CONTROL,
        );
        app.handle_event(Event::Paste(query.into()));
    }

    #[test]
    fn compact_search_keeps_usable_fields_one_count_and_more_document_rows() {
        for (width, height, find_rows, replace_rows, body_rows) in
            [(80, 24, 1, 2, 22), (40, 12, 2, 3, 10)]
        {
            let mut app = App::new("cat cat cat".into(), None, true);
            let idle = crate::simulation::snapshot(&mut app, width, height).unwrap();
            assert_eq!(app.viewport.height, body_rows);
            assert!(!idle.contains("clipboard"));
            search(&mut app, "cat", false);
            key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
            let selected = app.document.selection();
            let snapshot = crate::simulation::snapshot(&mut app, width, height).unwrap();
            assert_eq!(snapshot.matches("2/3").count(), 1);
            assert_eq!(app.search_geometry.panel.height, find_rows);
            assert_eq!(app.viewport.height, body_rows - find_rows);
            assert!(!snapshot.contains("F7/F8"));
            key(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL);
            let mut terminal = draw(&mut app, width, height);
            assert_eq!(app.search.query.text(), "cat");
            assert_eq!(app.document.selection(), selected);
            assert_eq!(app.search_geometry.panel.height, replace_rows);
            assert_eq!(app.viewport.height, body_rows - replace_rows);
            assert_eq!(button(&app, SearchAction::Previous).area.width, 6);
            assert_eq!(button(&app, SearchAction::Next).area.width, 6);
            assert_eq!(button(&app, SearchAction::Close).area.width, 7);
            for field in &app.search_geometry.fields {
                let buffer = terminal.backend().buffer();
                let label: String = (field.area.x..field.area.x + 6)
                    .map(|x| buffer[(x, field.area.y)].symbol())
                    .collect();
                assert!(matches!(label.as_str(), " Find " | " With "), "{label:?}");
                let edge = &buffer[(field.area.right() - 1, field.area.y)];
                assert_eq!(edge.bg, chrome_field().bg.unwrap());
                assert!(field.area.width >= 16);
            }
            let field = app.search_geometry.fields[0].area;
            assert_eq!(
                terminal.backend().buffer()[(field.x, field.y)].bg,
                theme::chrome_palette().status_accent.background
            );
            let cursor = terminal.get_cursor_position().unwrap();
            assert!(contains(field, cursor.x, cursor.y));
            for button in &app.search_geometry.buttons {
                assert!(button.area.right() <= width);
                for field in &app.search_geometry.fields {
                    assert!(button.area.intersection(field.area).is_empty());
                }
            }
            click_button(&mut app, SearchAction::Previous);
            assert_eq!(app.document.selection().range(), 0..3);
            key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
            app.handle_event(Event::Paste("dog".into()));
            draw(&mut app, width, height);
            let snapshot = crate::simulation::snapshot(&mut app, width, height).unwrap();
            assert!(
                snapshot
                    .lines()
                    .nth(usize::from(height - 1))
                    .unwrap()
                    .contains("Esc Close")
            );
            click_button(&mut app, SearchAction::Replace);
            assert_eq!(app.document.text(), "dog cat cat");
            draw(&mut app, width, height);
            click_button(&mut app, SearchAction::Close);
            key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
            assert_eq!(app.document.text(), "cat cat cat");
            assert!(!app.document.is_dirty());
        }
    }

    #[test]
    fn search_layout_breakpoint_preserves_fields_and_has_no_overlapping_targets() {
        let mut app = App::new("cat cat".into(), None, true);
        search(&mut app, "cat", true);
        for width in 0..=100 {
            for height in [0, 1, 2, 3, 4, 5, 8, 12, 24] {
                let mut terminal = draw(&mut app, width, height);
                let geometry = &app.search_geometry;
                for button in &geometry.buttons {
                    assert!(button.area.right() <= width && button.area.bottom() <= height);
                }
                if app.search_ready() {
                    assert_eq!(geometry.fields.len(), 2);
                    for field in &geometry.fields {
                        assert!(field.area.width >= 9);
                        assert!(field.area.right() <= width && field.area.bottom() < height);
                        for button in &geometry.buttons {
                            assert!(field.area.intersection(button.area).is_empty());
                        }
                    }
                    assert!(app.viewport.height >= 1);
                    let cursor = terminal.get_cursor_position().unwrap();
                    assert!(contains(geometry.fields[0].area, cursor.x, cursor.y));
                } else {
                    assert!(geometry.fields.is_empty());
                    assert!(
                        geometry
                            .buttons
                            .iter()
                            .all(|button| button.action == SearchAction::Close)
                    );
                }
            }
        }
        draw(&mut app, 66, 24);
        assert_eq!(app.search_geometry.panel.height, 3);
        draw(&mut app, 67, 24);
        assert_eq!(app.search_geometry.panel.height, 2);
        assert_eq!(app.search_geometry.fields[0].area.width - 8, 24);
    }

    #[test]
    fn file_errors_survive_editing_search_and_redraw_until_dismissed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("draft.md");
        std::fs::write(&path, "cat cat").unwrap();
        let mut app = App::open(Some(path.clone())).unwrap();
        app.document.insert("new ");
        std::fs::write(&path, "external edit").unwrap();
        key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
        let error = app.message.clone();
        assert!(app.message_is_error && error.contains("changed"));
        key(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        search(&mut app, "cat", false);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let terminal = draw(&mut app, 80, 24);
        assert_eq!(app.message, error);
        assert_eq!(terminal.backend().buffer()[(1, 23)].fg, palette().warning);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.message, error);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.message.is_empty() && !app.message_is_error);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "external edit");
    }

    #[test]
    fn chrome_backgrounds_fill_bars_and_pair_field_colors_without_body_bleed() {
        for width in [42, 80] {
            let mut app = App::new("cat cat".into(), None, true);
            search(&mut app, "cat", true);
            key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
            let terminal = draw(&mut app, width, 24);
            let buffer = terminal.backend().buffer();
            for x in 0..width {
                assert_ne!(buffer[(x, 0)].bg, Color::Reset);
                assert_ne!(buffer[(x, 23)].bg, Color::Reset);
                assert_eq!(buffer[(x, app.viewport.y + 1)].bg, palette().background);
                assert_eq!(
                    buffer[(x, app.viewport.bottom() - 1)].bg,
                    palette().background
                );
                for y in app.search_geometry.panel.y..app.search_geometry.panel.bottom() {
                    assert_ne!(buffer[(x, y)].bg, Color::Reset);
                }
            }
            let field = &app.search_geometry.fields[0];
            let selected = &buffer[(field.glyphs[0].column, field.area.y)];
            assert_eq!(selected.bg, palette().selection);
            assert_ne!(selected.fg, Color::Reset);
            assert_ne!(selected.bg, Color::Reset);
            assert_ne!(selected.fg, selected.bg);
            assert_ne!(selected.bg, buffer[(0, field.area.y)].bg);
            key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
            let terminal = draw(&mut app, width, 24);
            let disabled = button(&app, SearchAction::Next);
            assert!(!disabled.enabled);
            let cell = &terminal.backend().buffer()[(disabled.area.x, disabled.area.y)];
            assert_ne!(cell.fg, cell.bg);
            assert!(
                !cell
                    .modifier
                    .intersects(Modifier::DIM | Modifier::UNDERLINED)
            );
            assert_eq!(app.document.text(), "cat cat");
            assert!(!app.document.is_dirty());
        }
    }

    #[test]
    fn native_clipboard_commands_copy_source_and_paste_as_one_literal_edit() {
        let source = "**copy**\r\nrest";
        let mut app = App::new(source.into(), None, true);
        let native = fake_clipboard(&mut app);
        draw(&mut app, 80, 24);
        assert_eq!(native.lock().unwrap().reads, 0);
        assert!(native.lock().unwrap().writes.is_empty());
        app.document
            .set_selection(Selection { anchor: 0, head: 8 })
            .unwrap();
        key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(native.lock().unwrap().writes, ["**copy**"]);
        assert_eq!(app.document.text(), source);
        assert!(!app.document.is_dirty());
        native.lock().unwrap().text = "- literal\r\n- paste".into();
        key(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert_eq!(app.document.text(), "- literal\r\n- paste\r\nrest");
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.document.text(), source);
        assert_eq!(app.document.selection(), Selection { anchor: 0, head: 8 });
        app.handle_event(Event::Paste("terminal\ntext".into()));
        assert_eq!(
            native.lock().unwrap().reads,
            1,
            "terminal paste must not read native clipboard"
        );
    }

    #[test]
    fn clipboard_cut_failure_keeps_a_recoverable_internal_copy() {
        let mut app = App::new("keep **source**".into(), None, true);
        let native = fake_clipboard(&mut app);
        native.lock().unwrap().error = Some(crate::clipboard::Error::Unavailable("busy".into()));
        app.document.select_all();
        key(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert_eq!(app.document.text(), "");
        assert!(app.message.contains("internal clipboard only"));
        native.lock().unwrap().error = None;
        native.lock().unwrap().text = "stale system text".into();
        key(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert_eq!(app.document.text(), "keep **source**");
        assert!(app.message.contains("internal clipboard"));
        assert_eq!(native.lock().unwrap().reads, 0);
    }

    #[test]
    fn clipboard_fallback_warnings_survive_source_and_search_input() {
        let mut app = App::new("recoverable source".into(), None, true);
        let native = fake_clipboard(&mut app);
        native.lock().unwrap().error = Some(crate::clipboard::Error::Unavailable("busy".into()));
        app.set_message("An earlier file error");
        app.document.select_all();
        key(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
        let warning = app.message.clone();
        assert!(warning.contains("internal clipboard only") && app.message_is_error);
        key(&mut app, KeyCode::Right, KeyModifiers::NONE);
        key(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);
        app.handle_event(Event::Paste("new text".into()));
        draw(&mut app, 80, 24);
        assert_eq!(app.message, warning);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.message.is_empty() && !app.message_is_error);

        for focus in [Focus::Query, Focus::Replacement] {
            let mut app = App::new("cat cat".into(), None, true);
            let native = fake_clipboard(&mut app);
            native.lock().unwrap().error =
                Some(crate::clipboard::Error::Unavailable("busy".into()));
            search(&mut app, "cat", true);
            if focus == Focus::Replacement {
                key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
                app.handle_event(Event::Paste("dog".into()));
            }
            key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
            key(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
            let warning = app.message.clone();
            assert!(warning.contains("internal clipboard only") && app.message_is_error);
            key(&mut app, KeyCode::Right, KeyModifiers::NONE);
            key(&mut app, KeyCode::Char('c'), KeyModifiers::NONE);
            app.handle_event(Event::Paste("at".into()));
            draw(&mut app, 80, 24);
            assert_eq!(app.message, warning);
            assert_eq!(app.document.text(), "cat cat");
            assert_eq!(native.lock().unwrap().reads, 0);
        }
    }

    #[test]
    fn empty_clipboard_warning_is_sticky_but_native_success_is_transient() {
        let mut app = App::new("keep".into(), None, true);
        let native = fake_clipboard(&mut app);
        app.document.select_all();
        key(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
        let warning = app.message.clone();
        assert!(warning.contains("no text") && app.message_is_error);
        key(&mut app, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(app.message, warning);
        assert_eq!(app.document.text(), "keep");
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        app.document.select_all();
        key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(!app.message_is_error);
        assert!(app.message.contains("system clipboard"));
        key(&mut app, KeyCode::Right, KeyModifiers::NONE);
        assert!(app.message.is_empty());
        assert_eq!(native.lock().unwrap().writes, ["keep"]);
    }

    #[test]
    fn successful_clipboard_retry_replaces_only_a_clipboard_warning() {
        for field in [None, Some(Focus::Query), Some(Focus::Replacement)] {
            for command in ['c', 'v'] {
                let mut app = App::new("cat dog".into(), None, true);
                let native = fake_clipboard(&mut app);
                if let Some(focus) = field {
                    search(&mut app, "cat", true);
                    if focus == Focus::Replacement {
                        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
                        app.handle_event(Event::Paste("replacement".into()));
                    }
                }
                key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
                key(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
                assert!(app.message_is_error && app.message.contains("no text"));
                native.lock().unwrap().text = "dog".into();
                key(&mut app, KeyCode::Char(command), KeyModifiers::CONTROL);
                assert!(!app.message_is_error);
                assert!(!app.message.contains("no text"));
                assert!(app.message.contains("system clipboard"));
                if command == 'v' {
                    let text = field.map_or_else(
                        || app.document.text(),
                        |focus| app.search.input(focus).text(),
                    );
                    assert_eq!(text, "dog");
                }
                if field.is_some() {
                    assert_eq!(app.document.text(), "cat dog");
                }
                key(&mut app, KeyCode::Right, KeyModifiers::NONE);
                assert!(app.message.is_empty());

                app.set_message("File changed; use Save As");
                key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
                key(&mut app, KeyCode::Char(command), KeyModifiers::CONTROL);
                assert!(app.message_is_error);
                assert_eq!(app.message, "File changed; use Save As");
            }
        }
    }

    #[test]
    fn empty_or_failed_native_paste_does_not_delete_selected_document() {
        for error in [
            None,
            Some(crate::clipboard::Error::Empty),
            Some(crate::clipboard::Error::Unavailable("offline".into())),
        ] {
            let mut app = App::new("keep".into(), None, true);
            let native = fake_clipboard(&mut app);
            native.lock().unwrap().error = error;
            app.document.select_all();
            let selection = app.document.selection();
            key(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
            assert_eq!(app.document.text(), "keep");
            assert_eq!(app.document.selection(), selection);
            assert!(!app.document.is_dirty());
        }
    }

    #[test]
    fn empty_terminal_paste_preserves_source_and_search_selection() {
        let mut app = App::new("keep text".into(), None, true);
        app.document.select_all();
        let source_selection = app.document.selection();
        let revision = app.document.revision();
        app.handle_event(Event::Paste(String::new()));
        assert_eq!(app.document.text(), "keep text");
        assert_eq!(app.document.selection(), source_selection);
        assert_eq!(app.document.revision(), revision);
        search(&mut app, "keep", false);
        key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
        let selection = app.search.query.selection();
        let revision = app.search.query.revision();
        app.handle_event(Event::Paste(String::new()));
        assert_eq!(app.search.query.text(), "keep");
        assert_eq!(app.search.query.selection(), selection);
        assert_eq!(app.search.query.revision(), revision);
        assert_eq!(app.document.text(), "keep text");
    }

    #[test]
    fn find_reveals_hidden_source_wraps_and_keeps_selection_when_closed() {
        let source = "intro\n\n[label](hidden/path)\n\nhidden/path tail";
        let mut app = App::new(source.into(), None, true);
        search(&mut app, "hidden/path", false);
        let matches = app.document.find_matches("hidden/path");
        assert_eq!(app.document.selection().range(), matches[0]);
        let snapshot = crate::simulation::snapshot(&mut app, 34, 16).unwrap();
        assert!(snapshot.contains("[label](hidden/path)"));
        assert!(snapshot.contains("1/2"));
        assert!(
            app.caret_position().0 >= app.scroll
                && app.caret_position().0 < app.scroll + usize::from(app.viewport.height)
        );
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.document.selection().range(), matches[1]);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.document.selection().range(), matches[0]);
        assert!(app.search.wrapped);
        key(&mut app, KeyCode::Enter, KeyModifiers::SHIFT);
        assert_eq!(app.document.selection().range(), matches[1]);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.document.selection().range(), matches[1]);
        key(&mut app, KeyCode::F(3), KeyModifiers::NONE);
        assert_eq!(app.document.selection().range(), matches[0]);
        key(&mut app, KeyCode::F(3), KeyModifiers::SHIFT);
        assert_eq!(app.document.selection().range(), matches[1]);
        assert_eq!(app.document.text(), source);
        assert!(!app.document.is_dirty());
        assert!(!app.document.can_undo());
    }

    #[test]
    fn search_fields_edit_graphemes_and_receive_paste_without_editing_document() {
        let source = "unchanged source";
        let mut app = App::new(source.into(), None, true);
        search(&mut app, "a界e\u{301}👩‍💻", false);
        key(&mut app, KeyCode::Left, KeyModifiers::SHIFT);
        assert_eq!(app.search.query.selected_text(), "👩‍💻");
        key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        key(&mut app, KeyCode::Home, KeyModifiers::NONE);
        key(&mut app, KeyCode::Delete, KeyModifiers::NONE);
        key(&mut app, KeyCode::End, KeyModifiers::NONE);
        key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(app.search.query.text(), "界");
        key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
        app.handle_event(Event::Paste("no matches".into()));
        assert!(app.search.matches.is_empty());
        key(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        app.handle_event(Event::Paste("line one\nline two".into()));
        assert_eq!(app.search.replacement.text(), "line one\nline two");
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.document.text(), source);
        key(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
        key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
        key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        assert!(app.search.query.text().is_empty());
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.search.matches.is_empty());
        assert!(!app.document.is_dirty());
    }

    #[test]
    fn search_query_clipboard_feedback_survives_match_refresh() {
        let mut app = App::new("cat dog".into(), None, true);
        let native = fake_clipboard(&mut app);
        search(&mut app, "cat", false);
        key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
        native.lock().unwrap().text = "dog".into();
        key(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert_eq!(app.search.query.text(), "dog");
        assert!(app.message.contains("Pasted from system clipboard"));
        assert_eq!(app.document.selected_text(), "dog");
        key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
        native.lock().unwrap().text.clear();
        key(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert_eq!(app.search.query.text(), "dog");
        assert!(app.message.contains("no text"));
        native.lock().unwrap().error = Some(crate::clipboard::Error::Unavailable("offline".into()));
        key(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert_eq!(app.search.query.text(), "");
        assert!(app.message.contains("internal clipboard only"));
        key(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert_eq!(app.search.query.text(), "dog");
        assert!(app.message.contains("Pasted from internal clipboard"));
        assert_eq!(app.document.text(), "cat dog");
    }

    #[test]
    fn replace_actions_are_explicit_and_each_source_edit_has_one_undo() {
        let source = "cat cat **cat**";
        let mut app = App::new(source.into(), None, true);
        search(&mut app, "cat", true);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        app.handle_event(Event::Paste("dog".into()));
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            app.document.text(),
            "dog cat **cat**",
            "Enter in With replaces exactly one match"
        );
        assert_eq!(app.document.selection().range(), 4..7);
        assert_eq!(app.document.selected_text(), "cat");
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.document.text(), source);
        assert_eq!(app.document.selection().range(), 0..3);
        assert!(!app.document.can_undo());
        search(&mut app, "cat", true);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
        app.handle_event(Event::Paste("$1\\n".into()));
        key(&mut app, KeyCode::Char('a'), KeyModifiers::ALT);
        assert_eq!(app.document.text(), "$1\\n $1\\n **$1\\n**");
        assert!(app.search.matches.is_empty());
        assert!(app.message.contains("Replaced 3 matches"));
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.document.text(), source);
        assert!(!app.document.can_undo());
    }

    #[test]
    fn find_refreshes_after_document_edits_and_identical_replace_does_not_dirty() {
        let mut app = App::new("cat x cat".into(), None, true);
        search(&mut app, "cat", false);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        app.handle_event(Event::Paste("elephant".into()));
        key(&mut app, KeyCode::F(3), KeyModifiers::NONE);
        assert_eq!(app.document.selected_text(), "cat");
        assert_eq!(app.search.matches, vec![11..14]);
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        key(&mut app, KeyCode::F(3), KeyModifiers::NONE);
        assert_eq!(app.search.matches, vec![0..3, 6..9]);
        search(&mut app, "cat", true);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        app.handle_event(Event::Paste("cat".into()));
        key(&mut app, KeyCode::Char('a'), KeyModifiers::ALT);
        assert!(!app.document.is_dirty());
        assert!(!app.document.can_undo());
        assert_eq!(app.search.matches.len(), 2);
        assert!(app.message.contains("unchanged"));
    }

    #[test]
    fn search_short_window_suppresses_hidden_actions_and_narrow_fields_show_cursor() {
        let mut app = App::new("cat cat".into(), None, true);
        let native = fake_clipboard(&mut app);
        search(&mut app, "cat", true);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        app.handle_event(Event::Paste("a very long 界 replacement".into()));
        let mut terminal = draw(&mut app, 12, 16);
        let cursor = terminal.get_cursor_position().unwrap();
        assert!(cursor.x < 12 && cursor.y > app.viewport.bottom());
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.search.focus, Focus::Replacement);
        let narrow = crate::simulation::snapshot(&mut app, 12, 16).unwrap();
        assert!(narrow.contains('×'));
        app.handle_event(Event::Resize(80, 4));
        let short = crate::simulation::snapshot(&mut app, 80, 4).unwrap();
        assert!(short.contains("Resize to search"));
        assert_eq!(app.search.focus, Focus::Query);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
        app.handle_event(Event::Paste("hidden edit".into()));
        assert_eq!(app.document.text(), "cat cat");
        assert_eq!(app.search.query.text(), "cat");
        assert_eq!(native.lock().unwrap().reads, 0);
        assert_eq!(app.search.focus, Focus::Query);
        draw(&mut app, 80, 24);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.document.text(), "cat cat");
        for height in 0..10 {
            draw(&mut app, 8, height);
        }
    }

    #[test]
    fn save_quit_and_save_as_keep_global_meaning_from_search() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, "cat").unwrap();
        let mut app = App::open(Some(path.clone())).unwrap();
        app.document.insert("new ");
        search(&mut app, "cat", false);
        key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "new cat");
        assert_eq!(app.overlay, Overlay::None);
        assert!(!app.document.is_dirty());
        app.document.insert("dog");
        search(&mut app, "dog", true);
        key(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert_eq!(app.overlay, Overlay::Quit);
        assert!(!app.should_exit);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        search(&mut app, "dog", false);
        key(&mut app, KeyCode::F(4), KeyModifiers::NONE);
        assert!(matches!(app.overlay, Overlay::SaveAs { .. }));
    }

    fn pointer(
        app: &mut App,
        kind: MouseEventKind,
        column: u16,
        row: u16,
        modifiers: KeyModifiers,
    ) {
        app.handle_event(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers,
        }));
    }

    fn button(app: &App, action: SearchAction) -> SearchButton {
        app.search_geometry
            .buttons
            .iter()
            .find(|button| button.action == action)
            .unwrap()
            .clone()
    }

    fn click_button(app: &mut App, action: SearchAction) {
        let button = button(app, action);
        pointer(
            app,
            MouseEventKind::Down(MouseButton::Left),
            button.area.x,
            button.area.y,
            KeyModifiers::NONE,
        );
        pointer(
            app,
            MouseEventKind::Up(MouseButton::Left),
            button.area.x,
            button.area.y,
            KeyModifiers::NONE,
        );
    }

    fn source_cell<'a>(
        app: &App,
        terminal: &'a Terminal<TestBackend>,
        source: usize,
    ) -> &'a ratatui::buffer::Cell {
        let (row, column) = app.projection.cursor(source);
        &terminal.backend().buffer()[(
            app.viewport.x + column as u16,
            app.viewport.y + (row - app.scroll) as u16,
        )]
    }

    #[test]
    fn search_buttons_are_real_single_fire_and_disable_without_matches() {
        let source = "cat cat cat";
        let mut app = App::new(source.into(), None, true);
        search(&mut app, "cat", true);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        app.handle_event(Event::Paste("dog".into()));
        draw(&mut app, 80, 24);
        click_button(&mut app, SearchAction::Next);
        assert_eq!(app.document.selection().range(), 4..7);
        draw(&mut app, 80, 24);
        click_button(&mut app, SearchAction::Previous);
        assert_eq!(app.document.selection().range(), 0..3);
        assert_eq!(app.document.text(), source);
        assert!(!app.document.is_dirty());
        draw(&mut app, 80, 24);
        let target = button(&app, SearchAction::Replace);
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            target.area.x,
            target.area.y,
            KeyModifiers::NONE,
        );
        assert_eq!(app.document.text(), "dog cat cat");
        let revision = app.document.revision();
        pointer(
            &mut app,
            MouseEventKind::Drag(MouseButton::Left),
            target.area.x,
            target.area.y,
            KeyModifiers::NONE,
        );
        pointer(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            target.area.x,
            target.area.y,
            KeyModifiers::NONE,
        );
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            target.area.x,
            target.area.y,
            KeyModifiers::NONE,
        );
        assert_eq!(
            app.document.revision(),
            revision,
            "stale pre-redraw button geometry is inert"
        );
        draw(&mut app, 80, 24);
        click_button(&mut app, SearchAction::ReplaceAll);
        assert_eq!(app.document.text(), "dog dog dog");
        let terminal = draw(&mut app, 80, 24);
        for action in [
            SearchAction::Previous,
            SearchAction::Next,
            SearchAction::Replace,
            SearchAction::ReplaceAll,
        ] {
            let target = button(&app, action);
            assert!(!target.enabled);
            let cell = &terminal.backend().buffer()[(target.area.x, target.area.y)];
            assert_eq!(cell.fg, chrome_muted().fg.unwrap());
            assert!(!cell.modifier.contains(Modifier::UNDERLINED));
        }
        let revision = app.document.revision();
        click_button(&mut app, SearchAction::ReplaceAll);
        assert_eq!(app.document.revision(), revision);
        draw(&mut app, 80, 24);
        let retained = app.document.selection();
        click_button(&mut app, SearchAction::Close);
        assert_eq!(app.overlay, Overlay::None);
        assert_eq!(app.document.selection(), retained);
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(
            app.document.text(),
            "dog cat cat",
            "Replace All is one undo transaction"
        );
    }

    #[test]
    fn search_field_pointer_uses_scrolled_wide_graphemes_and_frozen_drag_geometry() {
        let source = "document stays untouched";
        let query = "prefixabcdef 界e\u{301}👩‍💻 suffix";
        let mut app = App::new(source.into(), None, true);
        search(&mut app, query, true);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        app.handle_event(Event::Paste("replacement".into()));
        draw(&mut app, 24, 18);
        let map = app
            .search_geometry
            .fields
            .iter()
            .find(|map| map.focus == Focus::Query)
            .unwrap()
            .clone();
        assert!(map.start > 0);
        let wide = map
            .glyphs
            .iter()
            .find(|glyph| glyph.width == 2)
            .unwrap()
            .clone();
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            wide.column,
            map.area.y,
            KeyModifiers::NONE,
        );
        assert_eq!(app.search.focus, Focus::Query);
        assert_eq!(
            app.search.query.selection(),
            Selection::caret(wide.source.start)
        );
        draw(&mut app, 24, 18);
        let frozen = app
            .search_geometry
            .fields
            .iter()
            .find(|field| field.focus == Focus::Query)
            .unwrap();
        assert_eq!(frozen.start, map.start);
        pointer(
            &mut app,
            MouseEventKind::Drag(MouseButton::Left),
            wide.column + wide.width - 1,
            map.area.y,
            KeyModifiers::NONE,
        );
        assert_eq!(app.search.query.selection().range(), wide.source);
        pointer(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            wide.column + wide.width - 1,
            map.area.y,
            KeyModifiers::NONE,
        );
        draw(&mut app, 24, 18);
        let map = app
            .search_geometry
            .fields
            .iter()
            .find(|field| field.focus == Focus::Query)
            .unwrap()
            .clone();
        let target = map.glyphs.first().unwrap().clone();
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            target.column,
            map.area.y,
            KeyModifiers::SHIFT,
        );
        assert_eq!(app.search.query.selection().anchor, wide.source.start);
        assert_eq!(app.search.query.selection().head, target.source.start);
        pointer(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            target.column,
            map.area.y,
            KeyModifiers::SHIFT,
        );
        key(&mut app, KeyCode::Home, KeyModifiers::NONE);
        draw(&mut app, 24, 18);
        let map = app
            .search_geometry
            .fields
            .iter()
            .find(|field| field.focus == Focus::Query)
            .unwrap()
            .clone();
        assert!(map.end < query.len());
        let end = map.hit(map.area.right() - 1);
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            map.area.right() - 1,
            map.area.y,
            KeyModifiers::NONE,
        );
        assert_eq!(app.search.query.selection().head, end);
        assert!(app.search.query.is_grapheme_boundary(end));
        assert_eq!(app.search.query.text(), query);
        assert_eq!(app.document.text(), source);
        assert!(!app.document.is_dirty());
    }

    #[test]
    fn keyboard_focus_selects_fields_and_never_tabs_into_replace_all() {
        let mut app = App::new("cat cat cat".into(), None, true);
        search(&mut app, "cat", true);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        app.handle_event(Event::Paste("dog".into()));
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.search.focus, Focus::Query);
        assert_eq!(app.search.query.selected_text(), "cat");
        key(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!(app.search.focus, Focus::Replacement);
        assert_eq!(app.search.replacement.selected_text(), "dog");
        key(&mut app, KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(app.search.query.selected_text(), "cat");
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.search.replacement.selected_text(), "dog");
        key(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(app.search.replacement.text(), "x");
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.search.replacement.text(), "dog");
        assert_eq!(app.document.text(), "cat cat cat");
        let snapshot = crate::simulation::snapshot(&mut app, 80, 24).unwrap();
        assert!(snapshot.contains("Enter: replace + next"));
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.document.text(), "dog cat cat");
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            app.document.text(),
            "dog cat cat",
            "Find Enter only navigates"
        );
        key(&mut app, KeyCode::Char('a'), KeyModifiers::ALT);
        assert_eq!(app.document.text(), "dog dog dog");
        assert_eq!(
            app.search.query.text(),
            "cat",
            "Alt action must not type in field"
        );
    }

    #[test]
    fn search_pointer_bounds_are_exact_and_stale_resize_or_overlay_targets_are_inert() {
        let mut app = App::new("cat cat".into(), None, true);
        search(&mut app, "cat", true);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        app.handle_event(Event::Paste("dog".into()));
        draw(&mut app, 80, 24);
        let next = button(&app, SearchAction::Next);
        let all = button(&app, SearchAction::ReplaceAll);
        let selected = app.document.selection();
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            next.area.right(),
            next.area.y,
            KeyModifiers::NONE,
        );
        assert_eq!(
            app.document.selection(),
            selected,
            "blank gap is not a button"
        );
        let doc_x = app.viewport.x;
        let doc_y = app.viewport.y;
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            doc_x,
            doc_y,
            KeyModifiers::NONE,
        );
        assert_eq!(app.document.text(), "cat cat");
        assert_eq!(app.document.selection(), selected);
        app.handle_event(Event::Resize(80, 4));
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            all.area.x,
            all.area.y,
            KeyModifiers::NONE,
        );
        draw(&mut app, 80, 4);
        assert!(app.search_geometry.fields.is_empty());
        assert!(
            app.search_geometry
                .buttons
                .iter()
                .all(|button| button.action == SearchAction::Close)
        );
        key(&mut app, KeyCode::Char('a'), KeyModifiers::ALT);
        assert_eq!(app.document.text(), "cat cat");
        draw(&mut app, 80, 4);
        let close = button(&app, SearchAction::Close);
        click_button(&mut app, SearchAction::Close);
        key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            close.area.x,
            close.area.y,
            KeyModifiers::NONE,
        );
        assert!(matches!(app.overlay, Overlay::Search { .. }));
        for width in 0..30 {
            draw(&mut app, width, 16);
            for button in &app.search_geometry.buttons {
                assert!(button.area.right() <= width && button.area.width > 0);
            }
        }
        assert_eq!(app.document.text(), "cat cat");
    }

    #[test]
    fn real_cells_highlight_all_matches_move_active_and_clear_after_close() {
        let mut app = App::new("cat cat cat plain".into(), None, true);
        search(&mut app, "cat", false);
        let terminal = draw(&mut app, 80, 24);
        let active = source_cell(&app, &terminal, 0).bg;
        let passive = source_cell(&app, &terminal, 4).bg;
        let ordinary = source_cell(&app, &terminal, 12).bg;
        assert_ne!(active, passive);
        assert_ne!(passive, ordinary);
        assert_eq!(source_cell(&app, &terminal, 8).bg, passive);
        assert!(
            source_cell(&app, &terminal, 0)
                .modifier
                .contains(Modifier::BOLD | Modifier::UNDERLINED)
        );
        click_button(&mut app, SearchAction::Next);
        let terminal = draw(&mut app, 80, 24);
        assert_eq!(source_cell(&app, &terminal, 0).bg, passive);
        assert_eq!(source_cell(&app, &terminal, 4).bg, active);
        assert_eq!(source_cell(&app, &terminal, 8).bg, passive);
        let selected = app.document.selection();
        click_button(&mut app, SearchAction::Close);
        let terminal = draw(&mut app, 80, 24);
        assert_eq!(app.document.selection(), selected);
        for offset in [0, 8] {
            assert_eq!(source_cell(&app, &terminal, offset).bg, ordinary);
        }
        assert_eq!(source_cell(&app, &terminal, 4).bg, palette().selection);
        assert_eq!(app.document.text(), "cat cat cat plain");
        assert!(!app.document.is_dirty());
    }

    #[test]
    fn real_match_cells_refresh_after_replace_undo_and_query_changes() {
        let mut app = App::new("cat cat plain".into(), None, true);
        search(&mut app, "cat", true);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        app.handle_event(Event::Paste("dog".into()));
        let terminal = draw(&mut app, 80, 24);
        let ordinary = source_cell(&app, &terminal, 8).bg;
        let active = source_cell(&app, &terminal, 0).bg;
        click_button(&mut app, SearchAction::Replace);
        let terminal = draw(&mut app, 80, 24);
        assert_eq!(source_cell(&app, &terminal, 0).bg, ordinary);
        assert_eq!(source_cell(&app, &terminal, 4).bg, active);
        assert_eq!(app.search.matches, vec![4..7]);
        click_button(&mut app, SearchAction::Close);
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        search(&mut app, "cat", true);
        let terminal = draw(&mut app, 80, 24);
        assert_eq!(app.search.matches.len(), 2);
        assert_ne!(source_cell(&app, &terminal, 4).bg, ordinary);
        key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
        app.handle_event(Event::Paste("plain".into()));
        let terminal = draw(&mut app, 80, 24);
        assert_eq!(source_cell(&app, &terminal, 0).bg, ordinary);
        assert_eq!(source_cell(&app, &terminal, 4).bg, ordinary);
        assert_eq!(source_cell(&app, &terminal, 8).bg, active);
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        let terminal = draw(&mut app, 80, 24);
        assert_eq!(app.search.matches.len(), 2);
        assert_ne!(source_cell(&app, &terminal, 4).bg, ordinary);
        key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
        app.handle_event(Event::Paste("missing".into()));
        let terminal = draw(&mut app, 80, 24);
        assert!(app.search.matches.is_empty());
        // Without matches, only the retained selection keeps a surface.
        for offset in [0, 4, 8] {
            let expected = if app.document.selection().range().contains(&offset) {
                palette().selection
            } else {
                ordinary
            };
            assert_eq!(source_cell(&app, &terminal, offset).bg, expected);
        }
        assert_eq!(app.document.text(), "cat cat plain");
    }

    #[test]
    fn wheel_over_document_inspects_passive_matches_and_next_restores_active_follow() {
        let source = (0..40)
            .map(|index| format!("cat {index}\n\n"))
            .collect::<String>();
        let mut app = App::new(source.clone(), None, true);
        search(&mut app, "cat", false);
        draw(&mut app, 50, 20);
        let selection = app.document.selection();
        let x = app.viewport.x;
        let y = app.viewport.y;
        for _ in 0..4 {
            pointer(
                &mut app,
                MouseEventKind::ScrollDown,
                x,
                y,
                KeyModifiers::NONE,
            );
        }
        let terminal = draw(&mut app, 50, 20);
        assert!(app.scroll > app.caret_position().0);
        assert_eq!(app.document.selection(), selection);
        let visible = app
            .search
            .matches
            .iter()
            .find(|range| {
                let row = app.projection.cursor(range.start).0;
                row >= app.scroll && row < app.scroll + usize::from(app.viewport.height)
            })
            .unwrap()
            .start;
        assert_eq!(source_cell(&app, &terminal, visible).bg, palette().search);
        let scroll = app.scroll;
        let field = app.search_geometry.fields[0].area;
        pointer(
            &mut app,
            MouseEventKind::ScrollDown,
            field.x + 5,
            field.y,
            KeyModifiers::NONE,
        );
        assert_eq!(app.scroll, scroll);
        key(&mut app, KeyCode::F(3), KeyModifiers::NONE);
        let terminal = draw(&mut app, 50, 20);
        assert!(
            app.caret_position().0 >= app.scroll
                && app.caret_position().0 < app.scroll + usize::from(app.viewport.height)
        );
        assert_eq!(
            source_cell(&app, &terminal, app.document.selection().range().start).bg,
            palette().search_active
        );
        assert_eq!(app.document.text(), source);
        assert!(!app.document.is_dirty());
    }

    #[test]
    fn editor_slice_contains_search_cursor_and_document_clicks() {
        let text = "intro\n\n- [ ] task cat\n\ncat cat";
        let mut app = App::new(text.into(), None, true);
        let area = Rect::new(26, 0, 94, 36);
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        let render = |app: &mut App, terminal: &mut Terminal<TestBackend>| {
            terminal
                .draw(|frame| {
                    frame.render_widget(
                        Block::default().style(Style::default().bg(Color::Magenta)),
                        frame.area(),
                    );
                    app.draw_in(frame, area, true);
                })
                .unwrap();
        };
        render(&mut app, &mut terminal);
        assert_eq!(app.viewport.width, 88);
        assert_eq!(app.viewport.x, 29);
        assert_eq!(terminal.backend().buffer()[(25, 20)].bg, Color::Magenta);
        assert_eq!(
            terminal.backend().buffer()[(26, 20)].bg,
            palette().background
        );
        click(&mut app, text.find('[').unwrap(), true);
        assert!(app.document.text().contains("[x]"));
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        search(&mut app, "cat", true);
        render(&mut app, &mut terminal);
        for field in &app.search_geometry.fields {
            assert_eq!(field.area.intersection(area), field.area);
        }
        for button in &app.search_geometry.buttons {
            assert_eq!(button.area.intersection(area), button.area);
        }
        let cursor = terminal.get_cursor_position().unwrap();
        assert!(app.search_geometry.fields[0].area.contains(cursor));
        assert_eq!(terminal.backend().buffer()[(25, 35)].bg, Color::Magenta);
        click_button(&mut app, SearchAction::Next);
        assert_eq!(
            app.document.selection().range(),
            text.rfind("cat cat").unwrap()..text.rfind("cat cat").unwrap() + 3
        );
        assert_eq!(app.document.text(), text);
    }

    #[test]
    fn source_jump_clears_transient_state_and_follows_a_grapheme_boundary() {
        let text = format!("intro\n{}界e\u{301} target", "line\n".repeat(60));
        let mut app = App::new(text.clone(), None, true);
        search(&mut app, "line", true);
        app.preferred_column = Some(12);
        app.dragging = true;
        app.anchor_screen_row = Some(3);
        let target = text.find('界').unwrap();
        app.jump_to_source(target + 1);
        draw(&mut app, 80, 24);
        assert_eq!(app.document.selection(), Selection::caret(target));
        assert_eq!(app.overlay, Overlay::None);
        assert!(app.search.query.text().is_empty());
        assert!(!app.dragging);
        assert!(app.preferred_column.is_none());
        assert!(app.anchor_screen_row.is_none());
        assert!(app.caret_position().0 >= app.scroll);
        assert!(app.caret_position().0 < app.scroll + usize::from(app.viewport.height));
        app.jump_to_source(usize::MAX);
        assert_eq!(app.document.selection(), Selection::caret(text.len()));
        assert_eq!(app.document.text(), text);
        assert!(!app.document.is_dirty());
    }

    #[test]
    fn source_jumps_land_a_third_of_the_way_down() {
        let filler = "A paragraph.\n\n".repeat(40);
        let text = format!("# Top\n\n{filler}## Middle\n\n{filler}## End\n\nlast\n");
        let mut app = App::new(text.clone(), None, true);
        draw(&mut app, 80, 24);
        let height = usize::from(app.viewport.height);
        let reading = height / 3;
        let middle = text.find("## Middle").unwrap();
        app.jump_to_source(middle);
        draw(&mut app, 80, 24);
        assert_eq!(app.caret_position().0 - app.scroll, reading);
        // A target already on screen moves to the same place.
        let visible = middle + "## Middle\n\n".len() + "A paragraph.\n\n".len() * 5;
        assert!(app.projection.cursor(visible).0 < app.scroll + height);
        app.jump_to_source(visible);
        draw(&mut app, 80, 24);
        assert_eq!(app.caret_position().0 - app.scroll, reading);
        // Ordinary movement still scrolls only as far as it must.
        let scroll = app.scroll;
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
        draw(&mut app, 80, 24);
        assert_eq!(app.scroll, scroll);
        // The document's end stays at the bottom rather than rising above it.
        app.jump_to_source(text.find("## End").unwrap());
        draw(&mut app, 80, 24);
        assert_eq!(app.scroll, app.projection.rows.len() - height);
        assert!(app.caret_position().0 - app.scroll > reading);
        app.jump_to_source(text.find("A paragraph").unwrap());
        draw(&mut app, 80, 24);
        assert_eq!(app.scroll, 0);
        assert_eq!(app.document.text(), text);
    }

    #[test]
    fn theme_switch_rebuilds_markdown_without_touching_selection_or_history() {
        let text = "intro\n\n# Heading\n\nA [link](target) and `code`.";
        let mut app = App::new(text.into(), None, true);
        app.document
            .set_selection(Selection { anchor: 3, head: 1 })
            .unwrap();
        let selection = app.document.selection();
        for theme in [Theme::Light, Theme::Dark] {
            app.set_theme(theme);
            let terminal = draw(&mut app, 120, 36);
            assert_eq!(
                terminal.backend().buffer()[(0, 20)].bg,
                theme.palette().background
            );
            assert_eq!(
                source_cell(&app, &terminal, text.find("Heading").unwrap()).fg,
                theme.palette().headings[0]
            );
            assert_eq!(
                source_cell(&app, &terminal, text.find("link").unwrap()).fg,
                theme.palette().link
            );
            assert_eq!(
                source_cell(&app, &terminal, text.find("code").unwrap()).fg,
                theme.palette().code
            );
            assert_eq!(app.document.selection(), selection);
            assert_eq!(app.document.text(), text);
            assert!(!app.document.can_undo());
        }
    }

    #[test]
    fn offset_slice_survives_tiny_layouts_and_ignores_other_panes_wheel() {
        for width in [0, 1, 2, 8, 12, 42, 94, 134] {
            for height in [0, 1, 2, 4, 8, 24, 45] {
                let mut app = App::new("first\ncat cat\n".repeat(20), None, true);
                let mut terminal =
                    Terminal::new(TestBackend::new(width + 26, height.max(1))).unwrap();
                let area = Rect::new(26, 0, width, height);
                for replace in [false, true] {
                    search(&mut app, "cat", replace);
                    terminal
                        .draw(|frame| app.draw_in(frame, area, true))
                        .unwrap();
                    let geometry = &app.search_geometry;
                    for field in &geometry.fields {
                        assert_eq!(field.area.intersection(area), field.area);
                    }
                    for button in &geometry.buttons {
                        assert_eq!(button.area.intersection(area), button.area);
                    }
                }
                app.deactivate();
                let scroll = app.scroll;
                app.handle_event(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    column: 10,
                    row: 3,
                    modifiers: KeyModifiers::NONE,
                }));
                assert_eq!(app.scroll, scroll);
            }
        }
    }

    #[test]
    fn command_line_open_uses_browser_text_and_size_guards() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("binary.md");
        std::fs::write(&path, "before\0after").unwrap();
        assert!(
            matches!(App::open(Some(path)), Err(error) if error.kind() == io::ErrorKind::InvalidData)
        );
        let path = dir.path().join("large.md");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(8 * 1024 * 1024 + 1)
            .unwrap();
        assert!(
            matches!(App::open(Some(path)), Err(error) if error.to_string().contains("8") || error.to_string().contains("limit"))
        );
        assert!(App::open(None).unwrap().is_markdown());
        let path = dir.path().join("plain.txt");
        std::fs::write(&path, "text\r\n").unwrap();
        let app = App::open(Some(path)).unwrap();
        assert!(!app.is_markdown());
        assert_eq!(app.document.text(), "text\r\n");
    }

    #[test]
    fn modal_overlay_can_be_redrawn_above_workspace_chrome() {
        let mut app = App::new("source".into(), None, true);
        assert!(!app.has_modal());
        search(&mut app, "source", false);
        assert!(!app.has_modal());
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut app, KeyCode::F(1), KeyModifiers::NONE);
        assert!(app.has_modal());
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal
            .draw(|frame| {
                app.draw_in(frame, Rect::new(26, 0, 94, 36), false);
                frame.render_widget(
                    Block::default().style(Style::default().bg(Color::Magenta)),
                    Rect::new(0, 0, 26, 36),
                );
                app.draw_overlay(frame);
            })
            .unwrap();
        // The modal straddles the sidebar boundary and must win at that cell.
        assert_eq!(terminal.backend().buffer()[(23, 9)].bg, palette().chrome);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut app, KeyCode::F(4), KeyModifiers::NONE);
        assert!(app.has_modal());
    }
}
