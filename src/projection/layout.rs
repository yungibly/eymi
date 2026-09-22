//! Source-anchored layout. Graphemes are placed in source order with
//! concealment, word or column wrapping, hanging indents, and row surfaces.
//! Rendered tables become grids whose borders and padding carry no source.
use super::{
    Fill, FillKind, Glyph, Projection, VisualRow, overlaps,
    parse::{Decoration, Parsed, RAIL, Table, line_end, next_line},
    safe_text,
};
use crate::theme::{self, Palette};
use pulldown_cmark::Alignment;
use ratatui::style::Style;
use std::{collections::BTreeMap, ops::Range};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(super) fn build(
    source: &str,
    parsed: &Parsed,
    disclosed: &[Range<usize>],
    width: usize,
    live: bool,
) -> Projection {
    let mut layout = Layout {
        source,
        parsed,
        disclosed,
        live,
        // One spare cell leaves an insertion caret visible after a full row.
        limit: width.saturating_sub(1).max(1),
        colors: theme::palette(),
        decorations: parsed
            .decorations
            .iter()
            .filter(|d| live && !disclosed.iter().any(|r| overlaps(r, &d.range)))
            .collect(),
        next_decoration: 0,
        sweep: Sweep {
            spans: &parsed.styles,
            next: 0,
            active: Vec::new(),
        },
        rows: vec![VisualRow::new(0, Some(1))],
        positions: BTreeMap::new(),
        column: 0,
        line: 1,
        word_start: true,
        context: Context::default(),
        cursors: [0; 4],
    };
    layout.run();
    layout
        .positions
        .entry(source.len())
        .or_insert((layout.rows.len() - 1, layout.column));
    Projection {
        rows: layout.rows,
        revision: parsed.snapshot.revision,
        positions: layout.positions,
    }
}

/// Per-source-line decisions made when a line begins.
#[derive(Default)]
struct Context {
    /// Column wrapping for code, raw tables, and source view.
    literal: bool,
    /// A row surface starting where this line's construct begins.
    fill: Option<PendingFill>,
    /// Where hanging indentation is measured to on the first row.
    content: usize,
    content_column: Option<usize>,
    first_row: usize,
    hang: Option<(Vec<Glyph>, usize)>,
}

#[derive(Clone)]
struct PendingFill {
    anchor: usize,
    hang: isize,
    kind: FillKind,
    color: ratatui::style::Color,
    label: Option<(String, Style)>,
}

struct Sweep<'a> {
    spans: &'a [(Range<usize>, Style)],
    next: usize,
    active: Vec<usize>,
}

impl Sweep<'_> {
    /// Styles are requested in source order, so spans enter and leave once.
    fn style(&mut self, base: Style, range: &Range<usize>) -> Style {
        while self
            .spans
            .get(self.next)
            .is_some_and(|(span, _)| span.start < range.end)
        {
            self.active.push(self.next);
            self.next += 1;
        }
        let spans = self.spans;
        self.active
            .retain(|index| spans[*index].0.end > range.start);
        self.active
            .iter()
            .filter(|index| overlaps(&spans[**index].0, range))
            .fold(base, |style, index| style.patch(spans[*index].1))
    }
}

struct Atom {
    text: String,
    source: Range<usize>,
    style: Option<Style>,
    task: Option<usize>,
}

struct Layout<'a> {
    source: &'a str,
    parsed: &'a Parsed,
    disclosed: &'a [Range<usize>],
    live: bool,
    limit: usize,
    colors: Palette,
    decorations: Vec<&'a Decoration>,
    next_decoration: usize,
    sweep: Sweep<'a>,
    rows: Vec<VisualRow>,
    positions: BTreeMap<usize, (usize, usize)>,
    column: usize,
    line: usize,
    word_start: bool,
    context: Context,
    /// Next unexamined surface, heading, rule, and table.
    cursors: [usize; 4],
}

impl Layout<'_> {
    fn run(&mut self) {
        let source = self.source;
        let mut offset = 0;
        self.begin_line(0);
        while offset < source.len() {
            if self.context.first_row == self.rows.len() - 1
                && self.column == 0
                && let Some(end) = self.rendered_table(offset)
            {
                offset = end;
                continue;
            }
            let atom = self.atom(offset);
            let start = offset;
            offset = atom.source.end;
            self.positions.insert(start, self.here());
            if matches!(atom.text.as_str(), "\n" | "\r\n" | "\r") {
                self.anchor_fill(start);
                self.row().end = start;
                self.line += 1;
                self.rows.push(VisualRow::new(offset, Some(self.line)));
                self.column = 0;
                self.word_start = true;
                self.positions.insert(offset, self.here());
                self.begin_line(offset);
                continue;
            }
            if atom.text.is_empty() || (start == 0 && atom.text == "\u{feff}") {
                self.positions.insert(offset, self.here());
                self.row().end = offset;
                continue;
            }
            self.place(atom);
        }
        self.anchor_fill(source.len());
    }

    fn row(&mut self) -> &mut VisualRow {
        self.rows.last_mut().expect("layout always has a row")
    }

    fn here(&self) -> (usize, usize) {
        (self.rows.len() - 1, self.column)
    }

    /// The next grapheme or decoration at `offset`, in display form.
    fn atom(&mut self, offset: usize) -> Atom {
        while self
            .decorations
            .get(self.next_decoration)
            .is_some_and(|d| d.range.start < offset)
        {
            self.next_decoration += 1;
        }
        if let Some(decoration) = self
            .decorations
            .get(self.next_decoration)
            .filter(|d| d.range.start == offset)
        {
            self.next_decoration += 1;
            return Atom {
                text: decoration.replacement.clone(),
                source: decoration.range.clone(),
                style: decoration.style,
                task: decoration.task,
            };
        }
        let grapheme = self.source[offset..]
            .graphemes(true)
            .next()
            .expect("offset is inside the source");
        Atom {
            text: grapheme.to_string(),
            source: offset..offset + grapheme.len(),
            style: None,
            task: None,
        }
    }

    fn style(&mut self, atom: &Atom) -> Style {
        let base = Style::default()
            .fg(self.colors.foreground)
            .bg(self.colors.background);
        match atom.style {
            Some(style) => base.patch(style),
            None => self.sweep.style(base, &atom.source),
        }
    }

    /// Display text and width, with tabs and invisible text made safe.
    fn display(&self, text: &str) -> (String, usize) {
        let mut displayed = if text == "\t" {
            " ".repeat((4 - self.column % 4).min(self.limit))
        } else {
            safe_text(text)
        };
        if UnicodeWidthStr::width(displayed.as_str()) == 0 {
            displayed.insert(0, '◌');
        }
        let width = UnicodeWidthStr::width(displayed.as_str()).max(1);
        if width > self.limit {
            return ("�".into(), 1);
        }
        (displayed, width)
    }

    fn place(&mut self, atom: Atom) {
        let start = atom.source.start;
        let end = atom.source.end;
        let whitespace = atom.text.chars().all(char::is_whitespace);
        let is_tab = atom.text == "\t";
        let (mut displayed, mut width) = self.display(&atom.text);
        let has_text =
            self.row().glyphs.iter().any(|glyph| {
                !glyph.source.is_empty() && !glyph.text.chars().all(char::is_whitespace)
            });
        // Move a complete prose word to the next row when it fits there. Keep
        // every source whitespace glyph, including trailing spaces.
        let wrap_word = !self.context.literal && self.word_start && !whitespace && has_text && {
            let word = width + self.following_word_width(end);
            let room = self.limit - self.hang_width();
            word <= room && self.column + word > self.limit
        };
        let overflow = self.column + width > self.limit && (has_text || self.column > 0);
        if wrap_word || overflow {
            self.wrap(start);
            if is_tab {
                (displayed, width) = self.display(&atom.text);
            }
        }
        self.anchor_fill(start);
        if self.context.first_row == self.rows.len() - 1
            && self.context.content_column.is_none()
            && start >= self.context.content
        {
            self.context.content_column = Some(self.column);
        }
        self.positions.insert(start, self.here());
        let style = self.style(&atom);
        let column = self.column;
        let row = self.row();
        row.glyphs.push(Glyph {
            text: displayed,
            source: start..end,
            column,
            width,
            style,
            task: atom.task,
        });
        row.end = end;
        self.column += width;
        self.word_start = whitespace;
        self.positions.insert(end, self.here());
    }

    /// Bounded lookahead, once per word. Concealed syntax has no width.
    fn following_word_width(&self, mut offset: usize) -> usize {
        let mut index = self.next_decoration;
        let mut width = 0;
        while offset < self.source.len() && width <= self.limit {
            while self
                .decorations
                .get(index)
                .is_some_and(|d| d.range.start < offset)
            {
                index += 1;
            }
            let text = if let Some(decoration) = self
                .decorations
                .get(index)
                .filter(|d| d.range.start == offset)
            {
                offset = decoration.range.end;
                index += 1;
                decoration.replacement.as_str()
            } else {
                let grapheme = self.source[offset..].graphemes(true).next().unwrap();
                offset += grapheme.len();
                grapheme
            };
            if text.is_empty() {
                continue;
            }
            if text.chars().any(char::is_whitespace) {
                break;
            }
            width += UnicodeWidthStr::width(safe_text(text).as_str()).max(1);
        }
        width
    }

    /// Measured once the line's content has been placed on its first row.
    fn hang_width(&mut self) -> usize {
        if self.context.hang.is_none() && self.context.content_column.is_some() {
            self.context.hang = Some(self.hang());
        }
        self.context.hang.as_ref().map_or(0, |(_, width)| *width)
    }

    /// Continuation rows align with the line's content. Quote rails repeat;
    /// markers and indentation become space. Deep indents in narrow views
    /// fall back to the left edge.
    fn hang(&self) -> (Vec<Glyph>, usize) {
        let Some(width) = self.context.content_column else {
            return (Vec::new(), 0);
        };
        if width == 0 || width > self.limit / 2 || self.limit < 8 {
            return (Vec::new(), 0);
        }
        let mut glyphs = Vec::new();
        for glyph in &self.rows[self.context.first_row].glyphs {
            if glyph.column >= width {
                break;
            }
            if glyph.text == RAIL {
                glyphs.push(Glyph {
                    source: 0..0,
                    ..glyph.clone()
                });
            } else {
                for cell in 0..glyph.width.min(width - glyph.column) {
                    glyphs.push(Glyph {
                        text: " ".into(),
                        source: 0..0,
                        column: glyph.column + cell,
                        width: 1,
                        style: Style::default().bg(self.colors.background),
                        task: None,
                    });
                }
            }
        }
        (glyphs, width)
    }

    fn wrap(&mut self, start: usize) {
        let width = self.hang_width();
        let fill = self.row().fill.clone().map(|fill| Fill {
            // A heading's level mark continues; language tabs do not.
            label: fill.label.filter(|_| fill.kind == FillKind::Band),
            ..fill
        });
        let mut row = VisualRow::new(start, None);
        row.fill = fill;
        if let Some((glyphs, _)) = &self.context.hang {
            row.glyphs = glyphs
                .iter()
                .map(|glyph| Glyph {
                    source: start..start,
                    ..glyph.clone()
                })
                .collect();
        }
        self.rows.push(row);
        self.column = width;
    }

    /// Apply this line's pending surface once layout reaches its construct.
    fn anchor_fill(&mut self, offset: usize) {
        let Some(pending) = &self.context.fill else {
            return;
        };
        let row = self.rows.last().expect("layout always has a row");
        if offset < pending.anchor || row.fill.is_some() {
            return;
        }
        let fill = Fill {
            from: self.column as isize + pending.hang,
            kind: pending.kind,
            color: pending.color,
            label: pending.label.clone(),
        };
        self.row().fill = Some(fill);
    }

    fn rendered(&self, range: &Range<usize>) -> bool {
        self.live && !self.disclosed.iter().any(|r| overlaps(r, range))
    }

    /// Choose wrapping, surfaces, and the hanging-indent anchor for the line
    /// starting at `start`.
    fn begin_line(&mut self, start: usize) {
        let parsed = self.parsed;
        let source = self.source;
        let end = line_end(source, start);
        let stop = next_line(source, end).max(start + 1);
        self.context = Context {
            literal: !self.live,
            content: start + leading_markers(&source[start..end]),
            first_row: self.rows.len() - 1,
            ..Context::default()
        };
        if !self.live {
            return;
        }
        let intersecting = |range: &Range<usize>| range.start < stop && range.end > start;
        if let Some(surface) = advance(&parsed.surfaces, &mut self.cursors[0], start, |s| &s.range)
            .filter(|s| intersecting(&s.range))
        {
            let rendered = self.rendered(&surface.range);
            self.context.literal = true;
            let band = |anchor: usize| PendingFill {
                anchor,
                hang: -1,
                kind: FillKind::Band,
                color: self.colors.code_background,
                label: None,
            };
            let on_line = |range: &Range<usize>| range.start >= start && range.start <= end;
            self.context.fill = Some(
                if let Some(open) = surface.open.as_ref().filter(|r| on_line(r)) {
                    PendingFill {
                        kind: if rendered {
                            FillKind::Lower
                        } else {
                            FillKind::Band
                        },
                        label: (rendered && !surface.label.is_empty()).then(|| {
                            (
                                format!(" {} ", surface.label),
                                Style::default()
                                    .fg(self.colors.muted)
                                    .bg(self.colors.code_background),
                            )
                        }),
                        ..band(open.start)
                    }
                } else if let Some(close) = surface.close.as_ref().filter(|r| on_line(r)) {
                    PendingFill {
                        kind: if rendered {
                            FillKind::Upper
                        } else {
                            FillKind::Band
                        },
                        ..band(close.start)
                    }
                } else {
                    let anchor = surface
                        .lines
                        .iter()
                        .find(|segment| segment.end > start && segment.start <= end)
                        .map_or(start, |segment| segment.start.max(start));
                    self.context.content = anchor + source[anchor..end.max(anchor)].len()
                        - source[anchor..end.max(anchor)].trim_start().len();
                    band(anchor)
                },
            );
            return;
        }
        if let Some(heading) = advance(&parsed.headings, &mut self.cursors[1], start, |h| &h.range)
            .filter(|h| intersecting(&h.range))
        {
            let level = heading.level;
            let underline = heading
                .underline
                .as_ref()
                .filter(|u| u.start >= start && u.start <= end);
            let color = self.colors.headings[level - 1];
            self.context.fill = Some(match underline {
                Some(underline) if self.rendered(&heading.range) => PendingFill {
                    anchor: underline.start,
                    hang: 0,
                    kind: FillKind::Rule,
                    color,
                    label: None,
                },
                _ => PendingFill {
                    anchor: heading.range.start.max(start),
                    hang: -2,
                    kind: FillKind::Band,
                    color: self.colors.heading_bands[level - 1],
                    label: Some((
                        self.parsed.icons.heading(level).into(),
                        Style::default()
                            .fg(color)
                            .bg(self.colors.heading_bands[level - 1]),
                    )),
                },
            });
            return;
        }
        if let Some(rule) =
            advance(&parsed.rules, &mut self.cursors[2], start, |r| r).filter(|r| intersecting(r))
            && self.rendered(rule)
        {
            self.context.fill = Some(PendingFill {
                anchor: rule.start,
                hang: 0,
                kind: FillKind::Rule,
                color: self.colors.faint,
                label: None,
            });
            return;
        }
        if advance(&parsed.tables, &mut self.cursors[3], start, |t| &t.range)
            .is_some_and(|t| intersecting(&t.range))
        {
            self.context.literal = true;
        }
    }

    /// Lay out a rendered table beginning on this line, returning where
    /// source layout resumes. Tables in containers, disclosed tables, and
    /// grids that cannot fit stay literal source.
    fn rendered_table(&mut self, offset: usize) -> Option<usize> {
        let table = self
            .parsed
            .tables
            .get(self.cursors[3])
            .filter(|t| {
                t.range.start >= offset && self.source[offset..t.range.start].trim().is_empty()
            })
            .filter(|t| t.range.start < line_end(self.source, offset).max(offset + 1))?;
        if !self.rendered(&table.range) {
            return None;
        }
        let grid = self.grid(table)?;
        self.cursors[3] += 1;
        Some(self.emit_table(table, grid))
    }

    fn grid(&mut self, table: &Table) -> Option<Grid> {
        let columns = table
            .rows
            .iter()
            .map(|row| row.cells.len())
            .max()
            .unwrap_or(0)
            .max(table.alignments.len());
        let chrome = 3 * columns + 1;
        if columns == 0 || self.limit < chrome + 3 * columns {
            return None;
        }
        // Measure each cell's atoms without disturbing layout cursors.
        let saved = (
            self.next_decoration,
            self.sweep.next,
            self.sweep.active.clone(),
        );
        let mut cells = Vec::new();
        for row in &table.rows {
            let mut atoms_by_cell = Vec::new();
            for column in 0..columns {
                let range = row.cells.get(column).cloned().unwrap_or(0..0);
                let mut atoms = Vec::new();
                let mut offset = range.start;
                while offset < range.end {
                    let atom = self.atom(offset);
                    offset = atom.source.end;
                    let (text, width) = if atom.text.is_empty() {
                        (String::new(), 0)
                    } else {
                        let text = if atom.text == "\t" {
                            " ".into()
                        } else {
                            atom.text.clone()
                        };
                        self.display_fixed(&text)
                    };
                    let style = self.style(&atom);
                    atoms.push(CellAtom {
                        text,
                        width,
                        source: atom.source,
                        style,
                    });
                }
                atoms_by_cell.push(atoms);
            }
            cells.push(atoms_by_cell);
        }
        (self.next_decoration, self.sweep.next, self.sweep.active) = saved;
        let natural: Vec<usize> = (0..columns)
            .map(|column| {
                cells
                    .iter()
                    .map(|row| row[column].iter().map(|a| a.width).sum::<usize>())
                    .max()
                    .unwrap_or(0)
                    .max(1)
            })
            .collect();
        let widths = fit(&natural, self.limit - chrome)?;
        Some(Grid { cells, widths })
    }

    fn display_fixed(&self, text: &str) -> (String, usize) {
        let mut displayed = safe_text(text);
        if UnicodeWidthStr::width(displayed.as_str()) == 0 {
            displayed.insert(0, '◌');
        }
        let width = UnicodeWidthStr::width(displayed.as_str()).max(1);
        (displayed, width)
    }

    fn emit_table(&mut self, table: &Table, grid: Grid) -> usize {
        let border = Style::default()
            .fg(self.colors.faint)
            .bg(self.colors.background);
        let widths = grid.widths.clone();
        let rule = |left: &str, join: &str, right: &str| {
            let mut text = String::from(left);
            for (index, width) in widths.iter().enumerate() {
                if index > 0 {
                    text.push_str(join);
                }
                text.push_str(&"─".repeat(width + 2));
            }
            text.push_str(right);
            text
        };
        // The current row is empty and begins at the table's line.
        let first = self.rows.len() - 1;
        self.rows[first].navigable = false;
        self.rows[first].line = None;
        self.rows[first].end = self.rows[first].start;
        self.push_virtual(&rule("╭", "┬", "╮"), border);
        let mut start_line = self.line;
        for (index, (row, cells)) in table.rows.iter().zip(grid.cells).enumerate() {
            let lines: Vec<Vec<Vec<CellAtom>>> = cells
                .into_iter()
                .zip(&grid.widths)
                .map(|(atoms, width)| wrap_cell(atoms, *width))
                .collect();
            let height = lines.iter().map(Vec::len).max().unwrap_or(1).max(1);
            for visual in 0..height {
                let mut next = VisualRow::new(row.line.start, (visual == 0).then_some(start_line));
                next.end = row.line.end;
                self.rows.push(next);
                self.column = 0;
                if visual == 0 {
                    self.positions.insert(row.line.start, self.here());
                }
                self.push_virtual("│", border);
                for (column, width) in grid.widths.iter().enumerate() {
                    let atoms = lines[column].get(visual).map(Vec::as_slice).unwrap_or(&[]);
                    let used: usize = atoms.iter().map(|a| a.width).sum();
                    let alignment = table
                        .alignments
                        .get(column)
                        .copied()
                        .unwrap_or(Alignment::None);
                    let before = match alignment {
                        Alignment::Right => width - used,
                        Alignment::Center => (width - used) / 2,
                        _ => 0,
                    };
                    let anchor = atoms.first().map_or(row.line.start, |a| a.source.start);
                    self.push_padding(1 + before, anchor);
                    for atom in atoms {
                        self.positions.insert(atom.source.start, self.here());
                        if atom.width > 0 {
                            let column = self.column;
                            self.row().glyphs.push(Glyph {
                                text: atom.text.clone(),
                                source: atom.source.clone(),
                                column,
                                width: atom.width,
                                style: atom.style,
                                task: None,
                            });
                            self.column += atom.width;
                        }
                        self.positions.insert(atom.source.end, self.here());
                    }
                    let anchor = atoms.last().map_or(anchor, |a| a.source.end);
                    self.push_padding(width - used - before + 1, anchor);
                    self.push_virtual("│", border);
                }
            }
            start_line += 1;
            if index == 0 {
                let mut separator = VisualRow::new(table.delimiter.start, Some(start_line));
                separator.end = table.delimiter.end;
                separator.navigable = false;
                self.rows.push(separator);
                self.column = 0;
                self.positions.insert(table.delimiter.start, self.here());
                self.push_virtual(&rule("├", "┼", "┤"), border);
                start_line += 1;
            }
        }
        let end = table.range.end;
        let mut bottom = VisualRow::new(end, None);
        bottom.navigable = false;
        self.rows.push(bottom);
        self.column = 0;
        self.push_virtual(&rule("╰", "┴", "╯"), border);
        // Resume ordinary layout on the line after the table.
        self.line = start_line;
        while self
            .decorations
            .get(self.next_decoration)
            .is_some_and(|d| d.range.start < end)
        {
            self.next_decoration += 1;
        }
        if end < self.source.len() || self.source[..end].ends_with(['\n', '\r']) {
            self.rows.push(VisualRow::new(end, Some(self.line)));
            self.column = 0;
            self.word_start = true;
            self.positions.insert(end, self.here());
            self.begin_line(end);
        }
        end
    }

    fn push_virtual(&mut self, text: &str, style: Style) {
        let anchor = self.row().start;
        let width = UnicodeWidthStr::width(text);
        let column = self.column;
        self.row().glyphs.push(Glyph {
            text: text.into(),
            source: anchor..anchor,
            column,
            width,
            style,
            task: None,
        });
        self.column += width;
    }

    fn push_padding(&mut self, cells: usize, anchor: usize) {
        let style = Style::default().bg(self.colors.background);
        for _ in 0..cells {
            let column = self.column;
            self.row().glyphs.push(Glyph {
                text: " ".into(),
                source: anchor..anchor,
                column,
                width: 1,
                style,
                task: None,
            });
            self.column += 1;
        }
    }
}

struct Grid {
    cells: Vec<Vec<Vec<CellAtom>>>,
    widths: Vec<usize>,
}

#[derive(Clone)]
struct CellAtom {
    text: String,
    width: usize,
    source: Range<usize>,
    style: Style,
}

/// Advance a sorted cursor past structures ending before `start`.
fn advance<'a, T>(
    items: &'a [T],
    cursor: &mut usize,
    start: usize,
    range: impl Fn(&T) -> &Range<usize>,
) -> Option<&'a T> {
    while items
        .get(*cursor)
        .is_some_and(|item| range(item).end <= start)
    {
        *cursor += 1;
    }
    items.get(*cursor)
}

/// Quote markers, list markers, and a task box before a line's content.
fn leading_markers(line: &str) -> usize {
    let bytes = line.as_bytes();
    let mut at = 0;
    loop {
        while matches!(bytes.get(at), Some(b' ' | b'\t')) {
            at += 1;
        }
        if bytes.get(at) == Some(&b'>') {
            at += 1;
            continue;
        }
        break;
    }
    let digits = bytes[at..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    let marker = if matches!(bytes.get(at), Some(b'-' | b'*' | b'+')) {
        1
    } else if (1..=9).contains(&digits) && matches!(bytes.get(at + digits), Some(b'.' | b')')) {
        digits + 1
    } else {
        0
    };
    if marker > 0 && matches!(bytes.get(at + marker), Some(b' ' | b'\t')) {
        at += marker;
        while matches!(bytes.get(at), Some(b' ' | b'\t')) {
            at += 1;
        }
        if bytes.len() >= at + 3
            && bytes[at] == b'['
            && matches!(bytes[at + 1], b' ' | b'x' | b'X')
            && bytes[at + 2] == b']'
        {
            at += 3;
            while matches!(bytes.get(at), Some(b' ' | b'\t')) {
                at += 1;
            }
        }
    }
    at
}

/// Column widths within `budget`: narrow columns keep their natural width
/// and the widest share the remainder, keeping at least three cells each.
fn fit(natural: &[usize], budget: usize) -> Option<Vec<usize>> {
    if natural.iter().sum::<usize>() <= budget {
        return Some(natural.to_vec());
    }
    let capped = |cap: usize| natural.iter().map(move |w| (*w).min(cap));
    if capped(3).sum::<usize>() > budget {
        return None;
    }
    // The largest cap whose capped total still fits.
    let (mut low, mut high) = (3, *natural.iter().max()?);
    while low < high {
        let middle = (low + high).div_ceil(2);
        if capped(middle).sum::<usize>() <= budget {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    let mut widths: Vec<_> = capped(low).collect();
    let mut spare = budget - widths.iter().sum::<usize>();
    for (width, wanted) in widths.iter_mut().zip(natural) {
        if spare > 0 && *width < *wanted {
            *width += 1;
            spare -= 1;
        }
    }
    Some(widths)
}

/// Word-wrap one cell's atoms into lines of at most `width` cells. Leading
/// whitespace on a continuation line stays in the source map but not on screen.
fn wrap_cell(atoms: Vec<CellAtom>, width: usize) -> Vec<Vec<CellAtom>> {
    let mut lines: Vec<Vec<CellAtom>> = vec![Vec::new()];
    let mut used = 0;
    let mut index = 0;
    while index < atoms.len() {
        let atom = &atoms[index];
        let blank = atom.text.chars().all(char::is_whitespace);
        if blank && atom.width > 0 {
            if used + atom.width > width || used == 0 && lines.len() > 1 {
                lines.last_mut().unwrap().push(CellAtom {
                    width: 0,
                    text: String::new(),
                    ..atom.clone()
                });
            } else {
                lines.last_mut().unwrap().push(atom.clone());
                used += atom.width;
            }
            index += 1;
            continue;
        }
        let word_end = atoms[index..]
            .iter()
            .position(|a| a.width > 0 && a.text.chars().all(char::is_whitespace))
            .map_or(atoms.len(), |at| index + at);
        let word: usize = atoms[index..word_end].iter().map(|a| a.width).sum();
        if used > 0 && used + word > width && word <= width {
            lines.push(Vec::new());
            used = 0;
        }
        for atom in &atoms[index..word_end] {
            if used + atom.width > width && used > 0 {
                lines.push(Vec::new());
                used = 0;
            }
            lines.last_mut().unwrap().push(atom.clone());
            used += atom.width;
        }
        index = word_end;
    }
    // Trailing spaces never widen a cell past its column.
    for line in &mut lines {
        let mut total: usize = line.iter().map(|a| a.width).sum();
        for atom in line.iter_mut().rev() {
            if total <= width {
                break;
            }
            total -= atom.width;
            atom.width = 0;
            atom.text.clear();
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_keeps_narrow_columns_and_shares_the_rest() {
        assert_eq!(fit(&[4, 6], 20), Some(vec![4, 6]));
        assert_eq!(fit(&[4, 30, 40], 44), Some(vec![4, 20, 20]));
        let widths = fit(&[10, 50, 7], 40).unwrap();
        assert_eq!(widths.iter().sum::<usize>(), 40);
        assert_eq!(widths[2], 7);
        assert_eq!(fit(&[9, 9, 9], 5), None);
    }

    #[test]
    fn markers_before_content_are_measured_lexically() {
        assert_eq!(leading_markers("- [ ] task"), 6);
        assert_eq!(leading_markers("> > quoted"), 4);
        assert_eq!(leading_markers("  12. twelve"), 6);
        assert_eq!(leading_markers("plain"), 0);
        assert_eq!(leading_markers("-not a list"), 0);
        assert_eq!(leading_markers("1234567890. no"), 0);
    }
}
