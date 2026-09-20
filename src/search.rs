//! Search panel state; authoritative matching/replacement stays in Document.
use marklane::{Document, Selection};
use ratatui::layout::Rect;
use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    #[default]
    Query,
    Replacement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Previous,
    Next,
    Replace,
    ReplaceAll,
    Close,
}

#[derive(Clone, Debug)]
pub struct Button {
    pub area: Rect,
    pub action: Action,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct FieldGlyph {
    pub column: u16,
    pub width: u16,
    pub source: Range<usize>,
}

#[derive(Clone, Debug)]
pub struct FieldMap {
    pub area: Rect,
    pub focus: Focus,
    pub glyphs: Vec<FieldGlyph>,
    pub start: usize,
    pub end: usize,
    pub revision: u64,
}

impl FieldMap {
    pub fn hit(&self, column: u16) -> usize {
        if let Some(first) = self.glyphs.first()
            && column < first.column
        {
            return self.start;
        }
        for glyph in &self.glyphs {
            if column < glyph.column + glyph.width {
                return if column.saturating_sub(glyph.column) * 2 >= glyph.width {
                    glyph.source.end
                } else {
                    glyph.source.start
                };
            }
        }
        self.end
    }
}

#[derive(Clone, Debug, Default)]
pub struct Geometry {
    pub panel: Rect,
    pub fields: Vec<FieldMap>,
    pub buttons: Vec<Button>,
}

#[derive(Clone, Debug)]
pub struct FieldDrag {
    pub map: FieldMap,
    pub anchor: usize,
}

pub fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x && column < area.right() && row >= area.y && row < area.bottom()
}

pub struct Search {
    pub query: Document,
    pub replacement: Document,
    pub focus: Focus,
    pub matches: Vec<Range<usize>>,
    pub wrapped: bool,
    pub origin: usize,
    key: Option<(u64, String)>,
}

impl Default for Search {
    fn default() -> Self {
        Self {
            query: Document::new(""),
            replacement: Document::new(""),
            focus: Focus::Query,
            matches: Vec::new(),
            wrapped: false,
            origin: 0,
            key: None,
        }
    }
}

impl Search {
    pub fn refresh(&mut self, document: &Document) {
        let key = (document.revision(), self.query.text().to_string());
        if self.key.as_ref() != Some(&key) {
            self.matches = document.find_matches(self.query.text());
            self.key = Some(key);
            self.wrapped = false;
        }
    }

    pub fn current(&self, selection: Selection) -> Option<usize> {
        if selection.is_empty() {
            return None;
        }
        self.matches
            .iter()
            .position(|range| *range == selection.range())
    }

    pub fn near(&self, offset: usize) -> Option<(usize, bool)> {
        if self.matches.is_empty() {
            return None;
        }
        Some(
            self.matches
                .iter()
                .position(|range| range.start >= offset)
                .map_or((0, true), |index| (index, false)),
        )
    }

    pub fn next(&self, selection: Selection, backwards: bool) -> Option<(usize, bool)> {
        let count = self.matches.len();
        if count == 0 {
            return None;
        }
        if let Some(index) = self.current(selection) {
            return Some(if backwards {
                if index == 0 {
                    (count - 1, true)
                } else {
                    (index - 1, false)
                }
            } else if index + 1 == count {
                (0, true)
            } else {
                (index + 1, false)
            });
        }
        if backwards {
            Some(
                self.matches
                    .iter()
                    .rposition(|range| range.end <= selection.range().start)
                    .map_or((count - 1, true), |index| (index, false)),
            )
        } else {
            self.near(selection.range().end)
        }
    }

    pub fn cycle(&mut self, replacement: bool, backwards: bool) {
        let order = if replacement {
            &[Focus::Query, Focus::Replacement][..]
        } else {
            &[Focus::Query][..]
        };
        let current = order
            .iter()
            .position(|focus| *focus == self.focus)
            .unwrap_or(0);
        let next = if backwards {
            (current + order.len() - 1) % order.len()
        } else {
            (current + 1) % order.len()
        };
        self.focus_keyboard(order[next]);
    }

    pub fn input_mut(&mut self) -> Option<&mut Document> {
        match self.focus {
            Focus::Query => Some(&mut self.query),
            Focus::Replacement => Some(&mut self.replacement),
        }
    }

    pub fn input(&self, focus: Focus) -> &Document {
        match focus {
            Focus::Query => &self.query,
            Focus::Replacement => &self.replacement,
        }
    }

    pub fn focus_keyboard(&mut self, focus: Focus) {
        if self.focus != focus {
            self.focus = focus;
            self.input_mut().unwrap().select_all();
        }
    }
}
