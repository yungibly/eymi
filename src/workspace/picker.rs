//! Searchable choices with geometry-gated acceptance and a separate query document.
use crate::ui;
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use eymi::Document;
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Clear, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(super) enum Action {
    None,
    Cancel,
    Quit,
    Preview(usize),
    Accept(usize),
}

pub(super) struct Picker {
    query: Document,
    entries: Vec<(String, String)>,
    selected: usize,
    title: &'static str,
    footer: &'static str,
    ready: bool,
    hits: Vec<(Rect, usize)>,
    fuzzy: bool,
    details: bool,
    empty_message: &'static str,
    search_names: Vec<String>,
}
impl Picker {
    pub fn new(
        title: &'static str,
        footer: &'static str,
        entries: Vec<(String, String)>,
        selected: usize,
    ) -> Self {
        Self {
            query: Document::new(""),
            entries,
            selected,
            title,
            footer,
            ready: false,
            hits: vec![],
            fuzzy: false,
            details: false,
            empty_message: "No matches",
            search_names: vec![],
        }
    }
    pub fn navigation(mut self) -> Self {
        self.fuzzy = true;
        self.details = true;
        self
    }
    pub fn empty_message(mut self, message: &'static str) -> Self {
        self.empty_message = message;
        self
    }
    pub fn search_names(mut self, names: Vec<String>) -> Self {
        self.search_names = names;
        self
    }
    fn matches(&self) -> Vec<usize> {
        if self.fuzzy {
            return super::matching::ranked(
                self.entries
                    .iter()
                    .enumerate()
                    .map(|(index, (name, hint))| {
                        (
                            self.search_names.get(index).unwrap_or(name).as_str(),
                            hint.as_str(),
                        )
                    }),
                self.query.text(),
            );
        }
        let query = self.query.text().to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(index, (name, hint))| {
                let name = format!("{name} {hint}").to_lowercase();
                query
                    .split_whitespace()
                    .all(|token| name.contains(token))
                    .then_some(index)
            })
            .collect()
    }
    pub fn invalidate(&mut self) {
        self.ready = false;
        self.hits.clear();
    }
    pub fn handle(&mut self, event: Event) -> Action {
        let previous = self.matches().get(self.selected).copied();
        let revision = self.query.revision();
        match event {
            Event::Resize(..) => self.invalidate(),
            Event::Paste(text) => self.insert_query(&text),
            Event::Key(key) => {
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                match key.code {
                    KeyCode::Esc => return Action::Cancel,
                    KeyCode::Char('q') if ctrl => return Action::Quit,
                    KeyCode::Char('a') if ctrl => self.query.select_all(),
                    KeyCode::Char('u') if ctrl => {
                        self.query.select_all();
                        self.query.insert("");
                    }
                    KeyCode::Up => self.selected = self.selected.saturating_sub(1),
                    KeyCode::Down => {
                        self.selected =
                            (self.selected + 1).min(self.matches().len().saturating_sub(1))
                    }
                    KeyCode::PageUp => self.selected = self.selected.saturating_sub(10),
                    KeyCode::PageDown => {
                        self.selected =
                            (self.selected + 10).min(self.matches().len().saturating_sub(1))
                    }
                    KeyCode::Enter if self.ready => {
                        if let Some(index) = self.matches().get(self.selected) {
                            return Action::Accept(*index);
                        }
                    }
                    KeyCode::Left => self.query.move_left(shift),
                    KeyCode::Right => self.query.move_right(shift),
                    KeyCode::Home => {
                        self.query.set_caret(0).expect("query start");
                    }
                    KeyCode::End => {
                        self.query
                            .set_caret(self.query.text().len())
                            .expect("query end");
                    }
                    KeyCode::Backspace => {
                        self.query.backspace();
                    }
                    KeyCode::Delete => {
                        self.query.delete_forward();
                    }
                    KeyCode::Char(c)
                        if !ctrl
                            && !key.modifiers.contains(KeyModifiers::ALT)
                            && !c.is_control() =>
                    {
                        self.insert_query(&c.to_string());
                    }
                    _ => {}
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) if self.ready => {
                    if let Some((_, index)) = self
                        .hits
                        .iter()
                        .find(|(rect, _)| rect.contains((mouse.column, mouse.row).into()))
                    {
                        return Action::Accept(*index);
                    }
                }
                MouseEventKind::ScrollUp => self.selected = self.selected.saturating_sub(3),
                MouseEventKind::ScrollDown => {
                    self.selected = (self.selected + 3).min(self.matches().len().saturating_sub(1))
                }
                _ => {}
            },
            _ => {}
        }
        if self.query.revision() != revision {
            self.selected = 0;
            self.invalidate();
        }
        let current = self.matches().get(self.selected).copied();
        if current != previous {
            self.invalidate();
            if let Some(index) = current {
                return Action::Preview(index);
            }
        }
        Action::None
    }
    fn insert_query(&mut self, text: &str) {
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        let remaining = 1024usize
            .saturating_sub(self.query.text().len() - self.query.selection().range().len());
        let mut end = 0;
        for grapheme in clean.graphemes(true) {
            if end + grapheme.len() > remaining {
                break;
            }
            end += grapheme.len();
        }
        if end > 0 {
            self.query.insert(&clean[..end]);
        }
    }
    pub fn draw(&mut self, frame: &mut Frame) {
        self.invalidate();
        let area = frame.area();
        if area.width < 24 || area.height < 7 {
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new("Resize to choose\nEsc: cancel").style(ui::surface()),
                area,
            );
            return;
        }
        let width = area.width.saturating_sub(4).clamp(24, 76).min(area.width);
        let max_height = area.height.saturating_sub(2).clamp(7, 20).min(area.height);
        let matches = self.matches();
        self.selected = self.selected.min(matches.len().saturating_sub(1));
        let height = matches
            .len()
            .saturating_add(4)
            .clamp(7, usize::from(max_height)) as u16;
        let rect = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - max_height) / 3,
            width,
            height,
        );
        let count = format!("{}/{}", matches.len(), self.entries.len());
        let inner = ui::panel(frame, rect, self.title, Some(&count));
        let field = Rect::new(inner.x + 1, inner.y, inner.width - 2, 1);
        let caret = ui::prompt(frame, field, &self.query, "Type to filter…");
        frame.set_cursor_position(caret);
        ui::divider(frame, rect, rect.y + 2);
        // The selected entry's full detail rides on the divider.
        if self.details
            && let Some(&index) = matches.get(self.selected)
            && !self.entries[index].1.is_empty()
        {
            let detail = format!(
                " {} ",
                clipped_suffix(&self.entries[index].1, usize::from(width - 8))
            );
            frame
                .buffer_mut()
                .set_string(rect.x + 2, rect.y + 2, detail, ui::muted());
        }
        let available = usize::from(height - 4);
        let first = self.selected.saturating_sub(available.saturating_sub(1));
        let query = self.query.text().to_owned();
        for (row, &index) in matches.iter().skip(first).take(available).enumerate() {
            let row_rect = Rect::new(inner.x, rect.y + 3 + row as u16, inner.width, 1);
            let (name, hint) = &self.entries[index];
            let hint = if self.details { "" } else { hint.as_str() };
            ui::item(
                frame,
                row_rect,
                name,
                hint,
                first + row == self.selected,
                &query,
            );
            self.hits.push((row_rect, index));
        }
        if matches.is_empty() {
            frame.render_widget(
                Paragraph::new(format!(
                    "  {}",
                    if self.entries.is_empty() {
                        self.empty_message
                    } else {
                        "No matches"
                    }
                ))
                .style(ui::muted()),
                Rect::new(inner.x, rect.y + 3, inner.width, 1),
            );
        }
        ui::hints(frame, rect, self.footer);
        self.ready = !self.hits.is_empty();
    }
}

fn clipped_suffix(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut remaining = width - 1;
    let mut start = text.len();
    for (index, grapheme) in text.grapheme_indices(true).rev() {
        let cells = UnicodeWidthStr::width(grapheme);
        if cells > remaining {
            break;
        }
        remaining -= cells;
        start = index;
    }
    format!("…{}", &text[start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_path_details_keep_the_distinguishing_tail_on_grapheme_boundaries() {
        let path = "/shared/very-long-project-parent/alpha/docs/界e\u{301}.md";
        let text = clipped_suffix(path, 24);
        assert!(text.ends_with("alpha/docs/界e\u{301}.md"));
        assert!(UnicodeWidthStr::width(text.as_str()) <= 24);
        assert_eq!(clipped_suffix(path, 0), "");
    }

    #[test]
    fn large_entry_counts_stay_within_terminal_geometry_and_accept_visible_results() {
        use ratatui::{Terminal, backend::TestBackend};
        let entries = (0..65_536)
            .map(|index| (format!("Heading {index}"), String::new()))
            .collect();
        let mut picker = Picker::new("Headings", "Enter Choose", entries, 65_535).navigation();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| picker.draw(frame)).unwrap();
        assert!(picker.ready);
        assert!(
            picker
                .hits
                .iter()
                .all(|(rect, _)| rect.right() <= 80 && rect.bottom() <= 24)
        );
        assert!(matches!(
            picker.handle(Event::Key(crossterm::event::KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::NONE
            ))),
            Action::Accept(65_535)
        ));
    }
    #[test]
    fn bounded_query_can_replace_a_full_selection_without_splitting_graphemes() {
        let mut picker = Picker::new("Test", "", vec![], 0);
        picker.insert_query(&"x".repeat(1023));
        picker.insert_query("界");
        assert_eq!(picker.query.text().len(), 1023);
        picker.insert_query("ab");
        assert_eq!(picker.query.text().len(), 1024);
        picker.query.select_all();
        picker.insert_query("\0\r\n");
        assert!(!picker.query.selection().range().is_empty());
        picker.insert_query("e\u{301}👩🏽‍💻");
        assert_eq!(picker.query.text(), "e\u{301}👩🏽‍💻");
    }
}
