use crate::{
    clipboard::{self, Clipboard},
    file_io::FileState,
    projection::{Affinity, Parsed, Projection, safe_text},
    search::{
        Action as SearchAction, Button as SearchButton, FieldDrag, FieldGlyph, FieldMap, Focus,
        Geometry as SearchGeometry, Search, contains,
    },
};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use marklane::{Document, Selection};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use std::{io, path::PathBuf};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, PartialEq, Eq)]
enum Overlay {
    None,
    Help,
    Quit,
    SaveAs { path: String, quit_after: bool },
    Search { replace: bool },
}

pub struct App {
    pub document: Document,
    file: Option<FileState>,
    pub live: bool,
    markdown: bool,
    parsed: Parsed,
    pub projection: Projection,
    pub viewport: Rect,
    pub scroll: usize,
    preferred_column: Option<usize>,
    affinity: Affinity,
    dragging: bool,
    follow_cursor: bool,
    anchor_screen_row: Option<usize>,
    clipboard: Clipboard,
    search: Search,
    search_geometry: SearchGeometry,
    field_drag: Option<FieldDrag>,
    overlay: Overlay,
    pub should_exit: bool,
    message: String,
    layout_key: Option<(u64, Selection, usize, bool)>,
    terminal_height: u16,
}

impl App {
    pub fn open(path: Option<PathBuf>) -> io::Result<Self> {
        let markdown = path.as_ref().is_none_or(|p| is_markdown(p));
        let (text, file) = match path {
            Some(path) => {
                let (text, file) = FileState::open(path)?;
                (text, Some(file))
            }
            None => (String::new(), None),
        };
        Ok(Self::new(text, file, markdown))
    }

    fn new(text: String, file: Option<FileState>, markdown: bool) -> Self {
        let document = Document::new(text);
        let parsed = Parsed::new(document.text(), document.markdown(), markdown);
        let projection =
            Projection::build(document.text(), &parsed, document.selection(), 80, markdown);
        Self {
            document,
            file,
            live: markdown,
            markdown,
            parsed,
            projection,
            viewport: Rect::default(),
            scroll: 0,
            preferred_column: None,
            affinity: Affinity::Downstream,
            dragging: false,
            follow_cursor: true,
            anchor_screen_row: None,
            clipboard: Clipboard::internal(),
            search: Search::default(),
            search_geometry: SearchGeometry::default(),
            field_drag: None,
            overlay: Overlay::None,
            should_exit: false,
            message: "F1 Help · Internal clipboard · Terminal paste supported".into(),
            layout_key: None,
            terminal_height: 24,
        }
    }

    pub fn handle_event(&mut self, event: Event) {
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
            Event::Resize(_, height) => {
                self.terminal_height = height;
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
                    self.message = message;
                    if let Some(text) = text {
                        self.document.insert(&text);
                    }
                }
                KeyCode::Char('t') if self.markdown => {
                    self.document.toggle_task_at_caret();
                }
                KeyCode::Home => self.move_to(0, shift),
                KeyCode::End => self.move_to(self.document.text().len(), shift),
                _ => {}
            }
            self.preferred_column = None;
            return;
        }
        match key.code {
            KeyCode::F(1) => self.overlay = Overlay::Help,
            KeyCode::F(3) => {
                if self.search.query.text().is_empty() {
                    self.open_search(false);
                } else {
                    self.find_next(shift);
                }
            }
            KeyCode::F(4) => self.start_save_as(false),
            KeyCode::F(6) => self.toggle_view(),
            KeyCode::Esc => {
                let _ = self.document.set_caret(self.document.selection().head);
                self.message.clear();
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
                self.document.insert("\t");
                self.preferred_column = None;
            }
            KeyCode::Char(c) if !alt => {
                self.document.insert(&c.to_string());
                self.preferred_column = None;
            }
            _ => {}
        }
    }

    fn finish_gesture(&mut self) {
        self.dragging = false;
        self.follow_cursor = true;
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
            self.message = error.to_string();
        }
    }

    fn vertical(&mut self, delta: isize, extend: bool) {
        let (row, col) = self.caret_position();
        let col = *self.preferred_column.get_or_insert(col);
        let target_row = row
            .saturating_add_signed(delta)
            .min(self.projection.rows.len() - 1);
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
                        Ok(_) => self.message = "Task toggled · Ctrl+Z undo".into(),
                        Err(error) => self.message = error.to_string(),
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
            self.message = if self.live {
                "Live view · Active block shows source"
            } else {
                "Source view · Markdown helpers remain enabled"
            }
            .into();
        } else {
            self.message = "Plain text file · Source view".into();
        }
    }

    fn copy(&mut self, cut: bool) {
        if !self.document.selection().is_empty() {
            self.message = self.clipboard.copy(self.document.selected_text());
            if cut {
                self.document.insert("");
            }
        }
    }

    pub fn enable_system_clipboard(&mut self) {
        if clipboard::remote_session() {
            self.clipboard = Clipboard::remote();
            self.message = "SSH session · Internal clipboard · Use terminal paste".into();
        } else {
            self.clipboard = Clipboard::system();
            self.message = "Ctrl+C/X/V uses system clipboard · Terminal paste supported".into();
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
        } else {
            self.message = if self.search.query.text().is_empty() {
                "Type a literal search query"
            } else {
                "No matches"
            }
            .into();
        }
    }

    fn select_match(&mut self, index: usize, wrapped: bool) {
        let range = self.search.matches[index].clone();
        if let Err(error) = self.document.set_selection(Selection {
            anchor: range.start,
            head: range.end,
        }) {
            self.message = error.to_string();
            return;
        }
        self.search.wrapped = wrapped;
        self.follow_cursor = true;
        self.dragging = false;
        self.anchor_screen_row = None;
        self.affinity = Affinity::Downstream;
        self.preferred_column = None;
        self.message = format!(
            "Match {}/{}{}",
            index + 1,
            self.search.matches.len(),
            if wrapped { " · wrapped" } else { "" }
        );
    }

    fn find_next(&mut self, backwards: bool) {
        self.search.refresh(&self.document);
        if let Some((index, wrapped)) = self.search.next(self.document.selection(), backwards) {
            self.select_match(index, wrapped);
        } else {
            self.message = if self.search.query.text().is_empty() {
                "Type a literal search query"
            } else {
                "No matches"
            }
            .into();
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
                self.message = "Match selected; activate Replace again to change it".into();
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
                self.message = if changed {
                    "Replaced one match · Ctrl+Z after closing search to undo"
                } else {
                    "Match unchanged: replacement is identical"
                }
                .into();
                self.follow_cursor = true;
            }
            Err(error) => self.message = error.to_string(),
        }
    }

    fn replace_all(&mut self) {
        let identical = self.search.query.text() == self.search.replacement.text();
        let count = self
            .document
            .replace_all(self.search.query.text(), self.search.replacement.text());
        self.search.refresh(&self.document);
        self.follow_cursor = true;
        self.message = if count == 0 {
            "No matches to replace".into()
        } else if identical {
            format!("{count} matches unchanged: replacement is identical")
        } else {
            format!("Replaced {count} matches · One undo step")
        };
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
            self.overlay = Overlay::Help;
            return;
        }
        if key.code == KeyCode::F(4) {
            self.start_save_as(false);
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
        let Some(input) = self.search.input_mut() else {
            return;
        };
        let before = input.revision();
        if control {
            match key.code {
                KeyCode::Char('a') => input.select_all(),
                KeyCode::Char('c' | 'x') => {
                    if !input.selection().is_empty() {
                        self.message = self.clipboard.copy(input.selected_text());
                        if key.code == KeyCode::Char('x') {
                            input.insert("");
                        }
                    }
                }
                KeyCode::Char('v') => {
                    let (text, message) = self.clipboard.paste();
                    self.message = message;
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
        let clipboard_feedback = (control && matches!(key.code, KeyCode::Char('c' | 'x' | 'v')))
            .then(|| self.message.clone());
        if changed && self.search.focus == Focus::Query {
            self.update_query();
        }
        if let Some(message) = clipboard_feedback {
            self.message = message;
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
                    self.message = format!("Saved {}", safe_text(&file.path.display().to_string()));
                    self.overlay = Overlay::None;
                    self.should_exit = quit_after;
                }
                Err(error) => {
                    self.message = error.to_string();
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
            return;
        }
        match &mut self.overlay {
            Overlay::Help => {
                if matches!(key.code, KeyCode::F(1) | KeyCode::Enter) {
                    self.overlay = Overlay::None;
                }
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
                            self.file = Some(file);
                            self.document.mark_saved();
                            self.overlay = Overlay::None;
                            self.should_exit = quit_after;
                            self.parsed = Parsed::new(
                                self.document.text(),
                                self.document.markdown(),
                                self.markdown,
                            );
                            self.layout_key = None;
                        }
                        Err(error) => self.message = error.to_string(),
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
            Overlay::None | Overlay::Search { .. } => {}
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        if self.terminal_height != area.height || self.search_geometry.panel.width != area.width {
            self.field_drag = None;
        }
        self.terminal_height = area.height;
        if matches!(self.overlay, Overlay::Search { .. }) && !self.search_ready() {
            self.search_too_short();
        }
        let search_height = self.search_height(area.height);
        if matches!(self.overlay, Overlay::Search { .. }) {
            self.search.refresh(&self.document);
        }
        self.viewport = Rect::new(
            area.x.saturating_add(1),
            area.y.saturating_add(2),
            area.width.saturating_sub(2),
            area.height.saturating_sub(5 + search_height),
        );
        if self.parsed.snapshot.revision != self.document.revision() {
            self.parsed = Parsed::new(
                self.document.text(),
                self.document.markdown(),
                self.markdown,
            );
        }
        let key = (
            self.document.revision(),
            self.document.selection(),
            self.viewport.width as usize,
            self.live,
        );
        if !self.dragging && self.layout_key != Some(key) {
            self.projection = Projection::build(
                self.document.text(),
                &self.parsed,
                self.document.selection(),
                self.viewport.width as usize,
                self.live,
            );
            self.layout_key = Some(key);
            if let Some(screen_row) = self.anchor_screen_row.take() {
                self.scroll = self.caret_position().0.saturating_sub(screen_row);
            }
        }
        let (cursor_row, cursor_col) = self.caret_position();
        let height = self.viewport.height as usize;
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
            " Marklane  {}{}  │ {} ",
            filename,
            if self.document.is_dirty() { " *" } else { "" },
            if self.live { "LIVE" } else { "SOURCE" }
        );
        frame.render_widget(
            Paragraph::new(title).style(Style::default().add_modifier(Modifier::REVERSED)),
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
        for (screen_row, row) in self
            .projection
            .rows
            .iter()
            .skip(self.scroll)
            .take(height)
            .enumerate()
        {
            for glyph in &row.glyphs {
                let selected =
                    glyph.source.start < selection.end && selection.start < glyph.source.end;
                let style = crate::search_highlight::style_match(
                    glyph.style,
                    &glyph.source,
                    selected,
                    matches,
                    active,
                );
                let x = self.viewport.x + glyph.column as u16;
                let y = self.viewport.y + screen_row as u16;
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
        }
        if area.height >= 3 {
            let head = self.document.selection().head;
            let line = self.document.text()[..head]
                .graphemes(true)
                .filter(|g| matches!(*g, "\r" | "\n" | "\r\n"))
                .count()
                + 1;
            let line_start = self.document.text()[..head]
                .rfind(['\n', '\r'])
                .map_or(0, |i| i + 1);
            let col = self.document.text()[line_start..head]
                .graphemes(true)
                .count()
                + 1;
            let status = format!(" Ln {line}, Col {col}  │ {}", safe_text(&self.message));
            frame.render_widget(
                Paragraph::new(status).style(Style::default().fg(Color::Cyan)),
                Rect::new(area.x, area.bottom() - 2, area.width, 1),
            );
            frame.render_widget(
                Paragraph::new(" F1 Help  ^F Find  ^S Save  ^E View  ^Q Quit")
                    .style(Style::default().add_modifier(Modifier::DIM)),
                Rect::new(area.x, area.bottom() - 1, area.width, 1),
            );
        }
        if self.overlay == Overlay::None
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
            self.draw_search(frame, search_height);
        } else {
            self.draw_overlay(frame);
        }
    }

    fn search_height(&self, height: u16) -> u16 {
        match self.overlay {
            Overlay::Search { replace } => {
                (if replace { 6 } else { 5 }).min(height.saturating_sub(6))
            }
            _ => 0,
        }
    }

    fn search_ready(&self) -> bool {
        match self.overlay {
            Overlay::Search { replace } => self.terminal_height >= if replace { 12 } else { 11 },
            _ => true,
        }
    }

    fn search_too_short(&mut self) {
        self.search.focus = Focus::Query;
        self.message = "Search needs a taller terminal · Resize or Esc to close".into();
    }

    fn draw_search(&mut self, frame: &mut Frame, height: u16) {
        self.search_geometry = SearchGeometry::default();
        let Overlay::Search { replace } = self.overlay else {
            return;
        };
        if height == 0 {
            return;
        }
        let area = frame.area();
        let panel = Rect::new(area.x, area.bottom() - 2 - height, area.width, height);
        self.search_geometry.panel = panel;
        frame.render_widget(Clear, panel);
        frame.render_widget(
            Block::default().borders(Borders::TOP).title(if replace {
                " Find / Replace · literal · case sensitive "
            } else {
                " Find · literal · case sensitive "
            }),
            panel,
        );
        if panel.height < 2 {
            return;
        }
        let row = |offset: u16| Rect::new(panel.x, panel.y + offset, panel.width, 1);
        if !self.search_ready() {
            self.search_geometry.buttons =
                draw_search_buttons(frame, row(1), &[(SearchAction::Close, true)], true);
            if panel.height > 2 {
                frame.render_widget(
                    Paragraph::new("Resize taller to search")
                        .style(Style::default().fg(Color::Yellow)),
                    row(2),
                );
            }
            return;
        }
        for (focus, offset, label) in [(Focus::Query, 1, "Find "), (Focus::Replacement, 2, "With ")]
        {
            if focus == Focus::Replacement && !replace {
                continue;
            }
            let frozen_start = self
                .field_drag
                .as_ref()
                .filter(|drag| drag.map.focus == focus)
                .map(|drag| drag.map.start);
            let map = draw_field(
                frame,
                row(offset),
                label,
                self.search.input(focus),
                focus,
                self.search.focus == focus,
                frozen_start,
            );
            self.search_geometry.fields.push(map);
        }
        let has_matches = !self.search.matches.is_empty();
        let mut actions = vec![
            (SearchAction::Previous, has_matches),
            (SearchAction::Next, has_matches),
        ];
        if replace {
            actions.push((
                SearchAction::Replace,
                self.search.current(self.document.selection()).is_some(),
            ));
            actions.push((SearchAction::ReplaceAll, has_matches));
        }
        actions.push((SearchAction::Close, true));
        self.search_geometry.buttons =
            draw_search_buttons(frame, row(if replace { 3 } else { 2 }), &actions, false);
        let counts = if self.search.query.text().is_empty() {
            "Type to search".into()
        } else if !has_matches {
            "No matches".into()
        } else if let Some(index) = self.search.current(self.document.selection()) {
            format!(
                "{}/{} matches{}",
                index + 1,
                self.search.matches.len(),
                if self.search.wrapped {
                    " · wrapped"
                } else {
                    ""
                }
            )
        } else {
            format!("{} matches", self.search.matches.len())
        };
        frame.render_widget(
            Paragraph::new(counts).style(Style::default().fg(Color::Cyan)),
            row(if replace { 4 } else { 3 }),
        );
        let hint = if replace && self.search.focus == Focus::Replacement {
            if panel.width < 24 {
                "Enter: one · Tab: Find"
            } else {
                "Enter: replace + next · Tab/Up: Find · Alt+R: one · Alt+A: all"
            }
        } else if replace {
            "Enter: next · Shift+Enter: previous · Tab/Down: With · Alt+R/A: one/all"
        } else {
            "Enter: next · Shift+Enter: previous · Ctrl+R: replace · Esc: close"
        };
        frame.render_widget(
            Paragraph::new(hint).style(Style::default().add_modifier(Modifier::DIM)),
            row(if replace { 5 } else { 4 }),
        );
    }

    fn draw_overlay(&self, frame: &mut Frame) {
        let (title, body) = match &self.overlay {
            Overlay::None | Overlay::Search { .. } => return,
            Overlay::Help => (" Marklane · Help ", "Type normally. Shift+arrows or drag selects source.\nHome/End: visual row · Ctrl+Home/End: document\nCtrl+S: save · F4 / Ctrl+Shift+S: Save As (new filename)\nCtrl+E / F6: live/source · Ctrl+Z / Ctrl+Y: undo/redo\nCtrl+A: select all · Ctrl+C/X/V: system copy/cut/paste\nClipboard errors or SSH use internal fallback, shown in status.\nTerminal paste is literal and undoable.\nCtrl+F: find · Ctrl+R: replace · F3 / Shift+F3: next/previous\nSearch: Tab/Shift+Tab switches Find/With; Esc closes.\nFind Enter: next; With Enter: replace one. Alt+R/A: one/all.\nEnter continues lists · Alt+Enter: literal newline\nCtrl+T: toggle task · Ctrl+Q: quit with unsaved prompt\n\nLive view reveals active source. Small UTF-8 files only.\nDisk conflicts require Save As. No clipboard polling or OSC52.\nPress Esc, Enter, or F1 to close.".to_string()),
            Overlay::Quit => (" Unsaved changes ", "Save before quitting?\n\nY: Save and quit\nN: Discard edits and quit\nEsc: Keep editing".into()),
            Overlay::SaveAs { path, .. } => (" Save As · new filename ", format!("{}▏\n\nEnter: Save  ·  Esc: Cancel\nExisting files are protected; enter a new path.\n\n{}", safe_text(path), safe_text(&self.message))),
        };
        let area = frame.area();
        let width = area.width.saturating_sub(4).min(78);
        let height = area
            .height
            .saturating_sub(2)
            .min(if self.overlay == Overlay::Help { 19 } else { 9 });
        if width < 4 || height < 3 {
            return;
        }
        let popup = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(
                body.lines()
                    .map(|line| Line::from(line.to_string()))
                    .collect::<Vec<_>>(),
            )
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(title)),
            popup,
        );
    }
}

fn draw_search_buttons(
    frame: &mut Frame,
    area: Rect,
    actions: &[(SearchAction, bool)],
    compact_message: bool,
) -> Vec<SearchButton> {
    let label = |action, compact| match (action, compact) {
        (SearchAction::Previous, false) => "[Prev]",
        (SearchAction::Previous, true) => "[<]",
        (SearchAction::Next, false) => "[Next]",
        (SearchAction::Next, true) => "[>]",
        (SearchAction::Replace, false) => "[Replace]",
        (SearchAction::Replace, true) => "[One]",
        (SearchAction::ReplaceAll, false) => "[Replace all]",
        (SearchAction::ReplaceAll, true) => "[All]",
        (SearchAction::Close, false) => "[Close]",
        (SearchAction::Close, true) => "[x]",
    };
    let required = actions
        .iter()
        .map(|(action, _)| label(*action, false).len() + 1)
        .sum::<usize>()
        .saturating_sub(1);
    let compact = required > usize::from(area.width);
    let mut buttons = Vec::new();
    let mut column = 0;
    for (action, enabled) in actions {
        let mut text = label(*action, compact);
        if *action == SearchAction::Close && area.width < 3 {
            text = "x";
        }
        let width = text.len();
        let reserve = if *action == SearchAction::Close {
            0
        } else {
            label(SearchAction::Close, compact).len() + 1
        };
        if column + width + reserve > usize::from(area.width) {
            continue;
        }
        let rect = Rect::new(area.x + column as u16, area.y, width as u16, 1);
        let style = if *enabled {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM)
        };
        frame.render_widget(Paragraph::new(text).style(style), rect);
        buttons.push(SearchButton {
            area: rect,
            action: *action,
            enabled: *enabled,
        });
        column += width + 1;
    }
    if compact_message && column < usize::from(area.width) {
        frame.render_widget(
            Paragraph::new("Resize taller").style(Style::default().fg(Color::Yellow)),
            Rect::new(
                area.x + column as u16,
                area.y,
                area.width - column as u16,
                1,
            ),
        );
    }
    buttons
}

fn draw_field(
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
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
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
            Style::default().add_modifier(Modifier::REVERSED)
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
    use ratatui::{Terminal, backend::TestBackend};
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
            let column = column.saturating_add_signed(delta);
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
        assert!(live.contains("• ☐ task"));
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
        assert!(snapshot.contains("1/2 matches"));
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
        assert!(narrow.contains("[x]"));
        app.handle_event(Event::Resize(80, 8));
        let short = crate::simulation::snapshot(&mut app, 80, 8).unwrap();
        assert!(short.contains("Resize taller"));
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
            assert!(
                terminal.backend().buffer()[(target.area.x, target.area.y)]
                    .modifier
                    .contains(Modifier::DIM)
            );
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
        app.handle_event(Event::Resize(80, 8));
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            all.area.x,
            all.area.y,
            KeyModifiers::NONE,
        );
        draw(&mut app, 80, 8);
        assert!(app.search_geometry.fields.is_empty());
        assert!(
            app.search_geometry
                .buttons
                .iter()
                .all(|button| button.action == SearchAction::Close)
        );
        key(&mut app, KeyCode::Char('a'), KeyModifiers::ALT);
        assert_eq!(app.document.text(), "cat cat");
        draw(&mut app, 80, 8);
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
        for offset in [0, 4, 8] {
            assert_eq!(source_cell(&app, &terminal, offset).bg, ordinary);
        }
        assert!(
            source_cell(&app, &terminal, 4)
                .modifier
                .contains(Modifier::REVERSED)
        );
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
        for offset in [0, 4, 8] {
            assert_eq!(source_cell(&app, &terminal, offset).bg, ordinary);
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
        assert_eq!(source_cell(&app, &terminal, visible).bg, Color::DarkGray);
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
            Color::LightYellow
        );
        assert_eq!(app.document.text(), source);
        assert!(!app.document.is_dirty());
    }
}
