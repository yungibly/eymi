//! A conservative, source-anchored projection. Unrecognized syntax stays
//! visible. Decoration without source — table grids, hanging indents, row
//! surfaces — maps to neighbouring source and is never an insertion target.
mod layout;
mod parse;
#[cfg(test)]
mod tests;

use eymi::{Selection, markdown::Task};
pub use parse::Parsed;
use ratatui::style::{Color, Style};
use std::ops::Range;

#[derive(Clone, Debug)]
pub struct Glyph {
    pub text: String,
    /// Empty for inserted decoration, which never takes selection or matches.
    pub source: Range<usize>,
    pub column: usize,
    pub width: usize,
    pub style: Style,
    pub task: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillKind {
    /// A solid surface behind the row's text.
    Band,
    /// Half-height edges that open and close a code surface.
    Lower,
    Upper,
    /// A thin horizontal line, for thematic breaks.
    Rule,
}

/// A row surface drawn beneath glyphs, from `from` to the right text edge.
#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    /// Relative to the text column; negative values hang into the margin.
    pub from: isize,
    pub kind: FillKind,
    pub color: Color,
    /// Drawn at `from`, such as a code language tab or a heading edge.
    pub label: Option<(String, Style)>,
}

#[derive(Clone, Debug)]
pub struct VisualRow {
    pub glyphs: Vec<Glyph>,
    pub start: usize,
    pub end: usize,
    /// One-based source line, on the first visual row of each line only.
    pub line: Option<usize>,
    pub fill: Option<Fill>,
    /// False for rows of pure decoration, which vertical movement skips.
    pub navigable: bool,
}

impl VisualRow {
    fn new(start: usize, line: Option<usize>) -> Self {
        Self {
            glyphs: Vec::new(),
            start,
            end: start,
            line,
            fill: None,
            navigable: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Projection {
    pub rows: Vec<VisualRow>,
    pub revision: u64,
    positions: Positions,
}

/// Caret cells by source offset. Layout records them in source order, so
/// recording almost always appends or refines the latest entry.
#[derive(Clone, Debug, Default)]
struct Positions(Vec<(usize, (usize, usize))>);

impl Positions {
    fn insert(&mut self, offset: usize, cell: (usize, usize)) {
        match self.0.last_mut() {
            Some((last, value)) if *last == offset => *value = cell,
            Some((last, _)) if *last > offset => {
                match self.0.binary_search_by_key(&offset, |(at, _)| *at) {
                    Ok(index) => self.0[index].1 = cell,
                    Err(index) => self.0.insert(index, (offset, cell)),
                }
            }
            _ => self.0.push((offset, cell)),
        }
    }

    /// The cell recorded at `offset`, or else at the nearest earlier offset.
    fn at_or_before(&self, offset: usize) -> Option<(usize, usize)> {
        let index = self.0.partition_point(|(at, _)| *at <= offset);
        index.checked_sub(1).map(|index| self.0[index].1)
    }

    fn contains(&self, offset: usize) -> bool {
        self.0.binary_search_by_key(&offset, |(at, _)| *at).is_ok()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Affinity {
    Upstream,
    Downstream,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub offset: usize,
    pub task: Option<usize>,
    pub affinity: Affinity,
}

impl Projection {
    pub fn build(
        source: &str,
        parsed: &Parsed,
        selection: Selection,
        width: usize,
        live: bool,
    ) -> Self {
        let disclosed = if live {
            parsed.disclosed(source, selection)
        } else {
            Vec::new()
        };
        Self::layout(source, parsed, &disclosed, width, live)
    }

    /// Lay out with precomputed disclosure, as returned by `Parsed::disclosed`.
    pub fn layout(
        source: &str,
        parsed: &Parsed,
        disclosed: &[Range<usize>],
        width: usize,
        live: bool,
    ) -> Self {
        layout::build(source, parsed, disclosed, width, live)
    }

    pub fn cursor(&self, offset: usize) -> (usize, usize) {
        self.positions.at_or_before(offset).unwrap_or((0, 0))
    }

    pub fn hit(&self, row: usize, column: usize) -> Hit {
        let row = &self.rows[row.min(self.rows.len() - 1)];
        for glyph in &row.glyphs {
            if column < glyph.column + glyph.width {
                let after = column.saturating_sub(glyph.column) * 2 >= glyph.width;
                return Hit {
                    offset: if after {
                        glyph.source.end
                    } else {
                        glyph.source.start
                    },
                    task: glyph.task,
                    affinity: if after {
                        Affinity::Upstream
                    } else {
                        Affinity::Downstream
                    },
                };
            }
        }
        Hit {
            offset: row.end,
            task: None,
            affinity: Affinity::Upstream,
        }
    }

    /// A wrap boundary belongs to the next row unless the caret arrived from
    /// its left; then it stays at the end of the previous row.
    pub fn cursor_with_affinity(&self, offset: usize, affinity: Affinity) -> (usize, usize) {
        let (row, col) = self.cursor(offset);
        if affinity == Affinity::Upstream
            && row > 0
            && self.rows[row].start == offset
            && self.rows[row - 1].end == offset
            && self.rows[row].line.is_none()
        {
            let previous = &self.rows[row - 1];
            return (
                row - 1,
                previous.glyphs.last().map_or(0, |g| g.column + g.width),
            );
        }
        (row, col)
    }

    /// The nearest row in `direction` that vertical movement may enter.
    pub fn navigable_row(&self, from: usize, delta: isize) -> usize {
        let last = self.rows.len() - 1;
        let target = from.saturating_add_signed(delta).min(last);
        let step = if delta < 0 { -1 } else { 1 };
        let walk = |step: isize| {
            std::iter::successors(Some(target), move |row: &usize| {
                row.checked_add_signed(step).filter(|row| *row <= last)
            })
        };
        // Continue past decoration in the direction of travel, then fall back.
        walk(step)
            .chain(walk(-step))
            .find(|row| self.rows[*row].navigable)
            .unwrap_or(from)
    }

    /// Pointer-only padding; insertion and keyboard hit testing stay exact.
    pub fn pointer_task<'a>(
        &self,
        parsed: &'a Parsed,
        row: usize,
        column: usize,
        viewport_width: usize,
    ) -> Option<&'a Task> {
        if self.revision != parsed.snapshot.revision || column >= viewport_width {
            return None;
        }
        let row = self.rows.get(row)?;
        let occupied = row
            .glyphs
            .iter()
            .find(|glyph| glyph.column <= column && column < glyph.column + glyph.width);
        if let Some(glyph) = occupied {
            if let Some(index) = glyph.task {
                return parsed.snapshot.tasks.get(index);
            }
            // Padding can claim whitespace, never label text or another glyph.
            if !glyph.text.chars().all(char::is_whitespace) {
                return None;
            }
        }
        let mut neighbors = row.glyphs.iter().filter(|glyph| {
            glyph.task.is_some()
                && glyph.column < viewport_width
                && ((glyph.column > 0 && column == glyph.column - 1)
                    || column == glyph.column + glyph.width)
        });
        let target = neighbors.next()?;
        // Keep an ambiguous shared padding cell available for text placement.
        if neighbors.next().is_some() {
            return None;
        }
        parsed.snapshot.tasks.get(target.task?)
    }
}

pub fn safe_text(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{00}'..='\u{1f}' => char::from_u32(0x2400 + c as u32).unwrap(),
            '\u{7f}' => '␡',
            '\u{80}'..='\u{9f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => '�',
            _ => c,
        })
        .collect()
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}
