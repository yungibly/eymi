//! Source-backed outline and keyboard/mouse navigation for the workspace rail.
use super::{chrome_active, chrome_muted, chrome_style, clipped};
use crate::projection::safe_text;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Paragraph},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Heading {
    pub title: String,
    pub level: u8,
    pub offset: usize,
}

pub(super) fn headings(source: &str) -> Vec<Heading> {
    let bom = if source.starts_with('\u{feff}') { 3 } else { 0 };
    let mut result = Vec::new();
    let mut heading: Option<Heading> = None;
    for (event, range) in
        Parser::new_ext(&source[bom..], marklane::markdown::options()).into_offset_iter()
    {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                heading = Some(Heading {
                    title: String::new(),
                    level: level as u8,
                    offset: range.start + bom,
                })
            }
            Event::Text(text) | Event::Code(text) if heading.is_some() => {
                heading.as_mut().unwrap().title.push_str(&text);
            }
            Event::SoftBreak | Event::HardBreak if heading.is_some() => {
                heading.as_mut().unwrap().title.push(' ');
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(mut value) = heading.take() {
                    value.title = safe_text(value.title.trim());
                    if value.title.is_empty() {
                        value.title = "Untitled heading".into();
                    }
                    result.push(value);
                }
            }
            _ => {}
        }
    }
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Tab(usize),
    Heading(usize),
}

#[derive(Default)]
pub(super) struct Sidebar {
    pub preference: Option<bool>,
    pub focused: bool,
    pub area: Rect,
    pub hits: Vec<(Rect, Target)>,
    pub headings: Vec<Heading>,
    pub key: Option<(usize, u64, bool)>,
    pub selected: usize,
    scroll: usize,
}

impl Sidebar {
    pub fn visible(&self, area: Rect) -> bool {
        area.width >= 60 && area.height >= 8 && self.preference.unwrap_or(area.width >= 110)
    }

    pub fn invalidate(&mut self) {
        self.area = Rect::default();
        self.hits.clear();
    }

    pub fn refresh(&mut self, number: usize, revision: u64, markdown: bool, source: &str) {
        let key = (number, revision, markdown);
        if self.key != Some(key) {
            if self.key.is_none_or(|old| old.0 != number) {
                self.scroll = 0;
            }
            self.headings = if markdown { headings(source) } else { vec![] };
            self.key = Some(key);
            self.hits.clear();
        }
    }

    pub fn target(&self, tabs: usize) -> Target {
        if self.selected < tabs {
            Target::Tab(self.selected)
        } else {
            Target::Heading(self.selected - tabs)
        }
    }

    pub fn scroll_rows(&mut self, delta: isize) {
        self.scroll = self.scroll.saturating_add_signed(delta);
    }

    pub fn move_selection(&mut self, delta: isize, tabs: usize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(tabs + self.headings.len() - 1);
    }

    pub fn draw(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        labels: &[String],
        active: usize,
        caret: usize,
    ) {
        self.area = area;
        self.hits.clear();
        if area.width == 0 || area.height == 0 {
            return;
        }
        frame.render_widget(Block::default().style(chrome_style()), area);
        for y in area.y..area.bottom() {
            frame
                .buffer_mut()
                .set_string(area.right() - 1, y, "│", chrome_muted());
        }
        let tabs = labels.len();
        self.selected = self.selected.min(tabs + self.headings.len() - 1);
        // Labels are presentation rows; selection only counts actionable entries.
        let mut rows = vec![(" DOCUMENTS".into(), None, false)];
        for (index, label) in labels.iter().enumerate() {
            rows.push((
                format!(" {}", label.trim_end()),
                Some(Target::Tab(index)),
                index == active,
            ));
        }
        rows.push((String::new(), None, false));
        rows.push((" OUTLINE".into(), None, false));
        let current = self
            .headings
            .iter()
            .rposition(|heading| heading.offset <= caret);
        for (index, heading) in self.headings.iter().enumerate() {
            rows.push((
                format!(
                    " {}{}",
                    "  ".repeat(usize::from(heading.level.saturating_sub(1)).min(3)),
                    heading.title
                ),
                Some(Target::Heading(index)),
                current == Some(index),
            ));
        }
        if self.headings.is_empty() {
            rows.push((" No headings".into(), None, false));
        }
        let selected = self.target(tabs);
        let selected_row = rows
            .iter()
            .position(|(_, target, _)| *target == Some(selected))
            .unwrap_or(0);
        let height = usize::from(area.height.saturating_sub(1));
        if self.focused && height > 0 {
            if selected_row < self.scroll {
                self.scroll = selected_row;
            }
            if selected_row >= self.scroll + height {
                self.scroll = selected_row + 1 - height;
            }
        }
        self.scroll = self.scroll.min(rows.len().saturating_sub(height));
        for (row, (label, target, active)) in rows.iter().skip(self.scroll).take(height).enumerate()
        {
            let rect = Rect::new(area.x, area.y + row as u16, area.width.saturating_sub(1), 1);
            let style = if self.focused && *target == Some(selected) || !self.focused && *active {
                chrome_active()
            } else {
                chrome_muted()
            };
            frame.render_widget(
                Paragraph::new(clipped(label, rect.width as usize)).style(style),
                rect,
            );
            if let Some(target) = target {
                self.hits.push((rect, *target));
            }
        }
        frame.render_widget(
            Paragraph::new(clipped(
                if self.focused {
                    " ↑↓ Select · Enter · Esc"
                } else {
                    " F9 Focus · F2 Commands"
                },
                area.width.saturating_sub(1) as usize,
            ))
            .style(chrome_muted()),
            Rect::new(area.x, area.bottom() - 1, area.width.saturating_sub(1), 1),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outline_uses_markdown_semantics_and_original_offsets() {
        let source = "\u{feff}# Café **bold** [link](url)\r\n\r\n```md\r\n# hidden\r\n```\r\n\r\nSetext 界\r\n------\r\n\r\n    # code\r\n\r\n## e\u{301}nd\r\n";
        let items = headings(source);
        assert_eq!(
            items
                .iter()
                .map(|h| (h.level, h.title.as_str()))
                .collect::<Vec<_>>(),
            vec![(1, "Café bold link"), (2, "Setext 界"), (2, "e\u{301}nd")]
        );
        assert_eq!(items[0].offset, 3);
        assert!(source[items[1].offset..].starts_with("Setext"));
        assert!(source[items[2].offset..].starts_with("## e"));
    }
}
