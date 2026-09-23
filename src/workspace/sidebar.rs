//! Source-backed outline and keyboard/mouse navigation for the workspace rail.
use super::clipped;
use crate::projection::safe_text;
use crate::{icons, theme::chrome_palette};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    widgets::{Block, Paragraph},
};
use unicode_width::UnicodeWidthStr;

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
        Parser::new_ext(&source[bom..], eymi::markdown::options()).into_offset_iter()
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
    /// Automatic visibility outlines only Markdown; an explicit choice holds
    /// for every document.
    pub fn visible(&self, area: Rect, markdown: bool) -> bool {
        area.width >= 60
            && area.height >= 8
            && self.preference.unwrap_or(markdown && area.width >= 110)
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

    pub fn target(&self) -> Option<Target> {
        self.headings
            .get(self.selected)
            .map(|_| Target::Heading(self.selected))
    }

    pub fn select_current(&mut self, caret: usize) {
        self.selected = self
            .headings
            .iter()
            .rposition(|heading| heading.offset <= caret)
            .unwrap_or(0);
    }

    pub fn scroll_rows(&mut self, delta: isize) {
        self.scroll = self.scroll.saturating_add_signed(delta);
    }

    pub fn move_selection(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.headings.len().saturating_sub(1));
    }

    /// `area` starts on the tab row, which carries the sidebar's header.
    pub fn draw(&mut self, frame: &mut Frame, area: Rect, caret: usize) {
        self.area = area;
        self.hits.clear();
        if area.width < 2 || area.height == 0 {
            return;
        }
        let colors = chrome_palette();
        frame.render_widget(Block::default().style(colors.sidebar.style()), area);
        let border = if self.focused {
            colors.tab_indicator
        } else {
            colors.separator
        };
        for y in area.y..area.bottom() {
            frame.buffer_mut().set_string(
                area.right() - 1,
                y,
                "│",
                colors.sidebar.style().fg(border),
            );
        }
        let icon = icons::current().outline();
        let title = if icon.is_empty() {
            " Outline".to_owned()
        } else {
            format!(" {icon} Outline")
        };
        let inner = area.width - 1;
        frame.render_widget(
            Paragraph::new(clipped(&title, usize::from(inner))).style(
                colors
                    .sidebar
                    .style()
                    .fg(if self.focused {
                        colors.tab_indicator
                    } else {
                        colors.sidebar.foreground
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Rect::new(area.x, area.y, inner, 1),
        );
        let top = area.y + 1;
        let height = usize::from(area.height.saturating_sub(1));
        self.selected = self.selected.min(self.headings.len().saturating_sub(1));
        let current = self
            .headings
            .iter()
            .rposition(|heading| heading.offset <= caret);
        if self.focused && height > 0 {
            if self.selected < self.scroll {
                self.scroll = self.selected;
            }
            if self.selected >= self.scroll + height {
                self.scroll = self.selected + 1 - height;
            }
        }
        self.scroll = self.scroll.min(self.headings.len().saturating_sub(height));
        if self.headings.is_empty() && height > 0 {
            frame.render_widget(
                Paragraph::new(" No headings")
                    .style(colors.sidebar.style().fg(colors.sidebar_muted)),
                Rect::new(area.x, top, inner, 1),
            );
        }
        let guides = tree(&self.headings);
        for (index, heading) in self
            .headings
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(height)
        {
            let selected = if self.focused {
                self.selected == index
            } else {
                current == Some(index)
            };
            let (guide, depth) = &guides[index];
            let base = if selected {
                colors.tab_active.style()
            } else {
                colors.sidebar.style()
            };
            let text = if selected {
                base.add_modifier(Modifier::BOLD)
            } else if *depth == 0 {
                base
            } else {
                base.fg(colors.sidebar_muted)
            };
            let row = Rect::new(area.x, top + (index - self.scroll) as u16, inner, 1);
            frame.render_widget(Block::default().style(base), row);
            let buffer = frame.buffer_mut();
            let marker = if selected { "▎" } else { " " };
            buffer.set_string(row.x, row.y, marker, base.fg(colors.tab_indicator));
            let guide = clipped(guide, usize::from(row.width.saturating_sub(1)));
            buffer.set_string(row.x + 1, row.y, &guide, base.fg(colors.separator));
            let x = row.x + 1 + UnicodeWidthStr::width(guide.as_str()) as u16;
            let room = usize::from(row.right().saturating_sub(x));
            buffer.set_stringn(x, row.y, clipped(&heading.title, room), room, text);
            self.hits.push((row, Target::Heading(index)));
        }
    }
}

/// Tree guides and nesting depth for each heading. Depth follows nesting,
/// so a skipped level (H1 then H3) still reads as one step deeper.
fn tree(headings: &[Heading]) -> Vec<(String, usize)> {
    let mut stack: Vec<u8> = Vec::new();
    let depths: Vec<usize> = headings
        .iter()
        .map(|heading| {
            while stack.last().is_some_and(|level| *level >= heading.level) {
                stack.pop();
            }
            stack.push(heading.level);
            stack.len() - 1
        })
        .collect();
    let has_sibling: Vec<bool> = (0..depths.len())
        .map(|index| {
            depths[index + 1..]
                .iter()
                .take_while(|depth| **depth >= depths[index])
                .any(|depth| *depth == depths[index])
        })
        .collect();
    let mut open: Vec<bool> = Vec::new();
    depths
        .iter()
        .zip(&has_sibling)
        .map(|(depth, sibling)| {
            open.truncate(*depth);
            let mut guide: String = open
                .iter()
                .skip(1)
                .map(|open| if *open { "│ " } else { "  " })
                .collect();
            if *depth > 0 {
                guide.push_str(if *sibling { "├ " } else { "└ " });
            }
            open.push(*sibling);
            (guide, *depth)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_guides_follow_nesting_and_last_siblings() {
        let headings: Vec<_> = [1, 2, 3, 2, 3, 3, 1, 4]
            .into_iter()
            .enumerate()
            .map(|(offset, level)| Heading {
                title: String::new(),
                level,
                offset,
            })
            .collect();
        let guides: Vec<_> = tree(&headings)
            .into_iter()
            .map(|(guide, _)| guide)
            .collect();
        assert_eq!(guides, ["", "├ ", "│ └ ", "└ ", "  ├ ", "  └ ", "", "└ "]);
    }
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
