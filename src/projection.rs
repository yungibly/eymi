//! A conservative, source-anchored projection. Unrecognized syntax stays visible.
use crate::theme::{self, Theme, palette};
use marklane::{
    Selection,
    markdown::{BlockKind, MarkdownSnapshot, Task},
};
use pulldown_cmark::{Event, Options, Parser, Tag};
use ratatui::style::{Modifier, Style};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone)]
struct Decoration {
    range: Range<usize>,
    replacement: String,
    task: Option<usize>,
}

pub struct Parsed {
    pub snapshot: MarkdownSnapshot,
    pub theme: Theme,
    styles: Vec<(Range<usize>, Style)>,
    decorations: Vec<Decoration>,
}

impl Parsed {
    pub fn new(source: &str, snapshot: MarkdownSnapshot, markdown: bool) -> Self {
        let mut parsed = Self {
            snapshot,
            theme: theme::current_theme(),
            styles: Vec::new(),
            decorations: Vec::new(),
        };
        let colors = palette();
        if !markdown {
            return parsed;
        }
        let bom = if source.starts_with('\u{feff}') { 3 } else { 0 };
        for (event, range) in Parser::new_ext(
            &source[bom..],
            Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH,
        )
        .into_offset_iter()
        {
            let range = range.start + bom..range.end + bom;
            let raw = &source[range.clone()];
            match event {
                Event::Start(Tag::Heading { .. }) => {
                    parsed.styles.push((
                        range.clone(),
                        Style::default()
                            .fg(colors.heading)
                            .add_modifier(Modifier::BOLD),
                    ));
                    let count = raw.bytes().take_while(|b| *b == b'#').count();
                    if (1..=6).contains(&count) && raw.as_bytes().get(count) == Some(&b' ') {
                        parsed.hide(range.start..range.start + count + 1);
                    }
                }
                Event::Start(Tag::Strong) => {
                    parsed
                        .styles
                        .push((range.clone(), Style::default().add_modifier(Modifier::BOLD)));
                    parsed.delimiters(source, range, &["**", "__"]);
                }
                Event::Start(Tag::Emphasis) => {
                    parsed.styles.push((
                        range.clone(),
                        Style::default().add_modifier(Modifier::ITALIC),
                    ));
                    parsed.delimiters(source, range, &["*", "_"]);
                }
                Event::Start(Tag::Strikethrough) => {
                    parsed.styles.push((
                        range.clone(),
                        Style::default().add_modifier(Modifier::CROSSED_OUT),
                    ));
                    parsed.delimiters(source, range, &["~~"]);
                }
                Event::Start(Tag::Link { .. }) => {
                    parsed.styles.push((
                        range.clone(),
                        Style::default()
                            .fg(colors.link)
                            .add_modifier(Modifier::UNDERLINED),
                    ));
                    // Only conceal a simple inline link whose label is an exact source slice.
                    if raw.starts_with('[')
                        && raw.ends_with(')')
                        && let Some(close) = raw.find("](")
                    {
                        let label = &raw[1..close];
                        if !label.contains(['[', ']', '\\']) {
                            parsed.hide(range.start..range.start + 1);
                            parsed.hide(range.start + close..range.end);
                        }
                    }
                }
                Event::Code(_) | Event::Start(Tag::CodeBlock(_)) => {
                    parsed.styles.push((
                        range,
                        Style::default().fg(colors.code).bg(colors.code_background),
                    ));
                }
                Event::Start(Tag::BlockQuote(_)) => {
                    parsed.styles.push((
                        range.clone(),
                        Style::default()
                            .fg(colors.quote)
                            .add_modifier(Modifier::ITALIC),
                    ));
                    let mut offset = range.start;
                    for line in raw.split_inclusive(['\n', '\r']) {
                        let prefix = line.len() - line.trim_start_matches(' ').len();
                        if line.as_bytes().get(prefix) == Some(&b'>') {
                            parsed.decorations.push(Decoration {
                                range: offset + prefix..offset + prefix + 1,
                                replacement: "│".into(),
                                task: None,
                            });
                        }
                        offset += line.len();
                    }
                }
                Event::Start(Tag::Item)
                    if matches!(raw.as_bytes().first(), Some(b'-' | b'+' | b'*'))
                        && raw.as_bytes().get(1).is_some_and(u8::is_ascii_whitespace) =>
                {
                    parsed.decorations.push(Decoration {
                        range: range.start..range.start + 1,
                        replacement: "•".into(),
                        task: None,
                    });
                }
                _ => {}
            }
        }
        for (index, task) in parsed.snapshot.tasks.iter().enumerate() {
            let state = task.marker_range.start;
            if state > 0
                && source.as_bytes().get(state - 1) == Some(&b'[')
                && source.as_bytes().get(state + 1) == Some(&b']')
            {
                parsed.decorations.push(Decoration {
                    range: state - 1..state + 2,
                    replacement: if task.checked { "☑" } else { "☐" }.into(),
                    task: Some(index),
                });
            }
        }
        parsed
            .decorations
            .sort_by_key(|d| (d.range.start, d.range.end));
        parsed.decorations.dedup_by(|a, b| a.range == b.range);
        // Even ASCII syntax can share a grapheme with a following combining mark.
        // Such syntax stays literal rather than creating an invalid insertion point.
        let boundaries: BTreeSet<_> = source
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain([source.len()])
            .collect();
        parsed
            .decorations
            .retain(|d| boundaries.contains(&d.range.start) && boundaries.contains(&d.range.end));
        parsed
    }

    fn hide(&mut self, range: Range<usize>) {
        self.decorations.push(Decoration {
            range,
            replacement: String::new(),
            task: None,
        });
    }
    fn delimiters(&mut self, source: &str, range: Range<usize>, candidates: &[&str]) {
        let raw = &source[range.clone()];
        for delimiter in candidates {
            if raw.len() >= 2 * delimiter.len()
                && raw.starts_with(delimiter)
                && raw.ends_with(delimiter)
            {
                self.hide(range.start..range.start + delimiter.len());
                self.hide(range.end - delimiter.len()..range.end);
                break;
            }
        }
    }

    fn disclosed(&self, source: &str, selection: Selection) -> Vec<Range<usize>> {
        let head = selection.head;
        let contains_head = |range: &Range<usize>| {
            contains(range, head)
                || (head == source.len() && range.end == head && !source.ends_with(['\n', '\r']))
        };
        let atomic = self
            .snapshot
            .blocks
            .iter()
            .filter(|b| {
                matches!(b.kind, BlockKind::Code | BlockKind::Table) && contains_head(&b.range)
            })
            .min_by_key(|b| b.range.len());
        let active = atomic.or_else(|| {
            self.snapshot
                .blocks
                .iter()
                .filter(|b| contains_head(&b.range))
                .min_by_key(|b| b.range.len())
        });
        let line_start = source[..head].rfind(['\n', '\r']).map_or(0, |p| p + 1);
        let line_end = source[head..]
            .find(['\n', '\r'])
            .map_or(source.len(), |p| head + p);
        let mut ranges = vec![active.map_or(line_start..line_end, |b| {
            if b.kind == BlockKind::Quote {
                return line_start..line_end;
            }
            let start = source[..b.range.start]
                .rfind(['\n', '\r'])
                .map_or(0, |p| p + 1);
            let end = if b.kind == BlockKind::ListItem {
                self.snapshot
                    .blocks
                    .iter()
                    .filter(|child| child.range.start > head && child.range.end <= b.range.end)
                    .map(|child| {
                        source[..child.range.start]
                            .rfind(['\n', '\r'])
                            .map_or(0, |p| p + 1)
                    })
                    .filter(|end| *end > head)
                    .min()
                    .unwrap_or(b.range.end)
            } else {
                b.range.end
            };
            start..end
        })];
        if !selection.is_empty() {
            let selection_range = selection.range();
            ranges.push(selection_range.clone());
            for block in &self.snapshot.blocks {
                if overlaps(&block.range, &selection_range)
                    && !matches!(block.kind, BlockKind::Quote | BlockKind::ListItem)
                {
                    ranges.push(block.range.clone());
                }
            }
        }
        ranges
    }
}

fn contains(range: &Range<usize>, point: usize) -> bool {
    range.start <= point && point < range.end
}
fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

#[derive(Clone, Debug)]
pub struct Glyph {
    pub text: String,
    pub source: Range<usize>,
    pub column: usize,
    pub width: usize,
    pub style: Style,
    pub task: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct VisualRow {
    pub glyphs: Vec<Glyph>,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug)]
pub struct Projection {
    pub rows: Vec<VisualRow>,
    pub revision: u64,
    positions: BTreeMap<usize, (usize, usize)>,
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
        // One spare cell leaves an insertion caret visible at the end of a full row.
        let limit = width.saturating_sub(1).max(1);
        let disclosed = parsed.disclosed(source, selection);
        let decorations: Vec<_> = parsed
            .decorations
            .iter()
            .filter(|d| live && !disclosed.iter().any(|r| overlaps(r, &d.range)))
            .collect();
        let mut result = Self {
            rows: vec![VisualRow {
                glyphs: Vec::new(),
                start: 0,
                end: 0,
            }],
            revision: parsed.snapshot.revision,
            positions: BTreeMap::new(),
        };
        let mut offset = 0;
        let mut decoration_index = 0;
        let mut column = 0;
        while offset < source.len() {
            while decorations
                .get(decoration_index)
                .is_some_and(|d| d.range.start < offset)
            {
                decoration_index += 1;
            }
            let start = offset;
            let (mut displayed, end, task) = if let Some(d) = decorations
                .get(decoration_index)
                .filter(|d| d.range.start == offset)
            {
                decoration_index += 1;
                (d.replacement.clone(), d.range.end, d.task)
            } else {
                let grapheme = source[offset..].graphemes(true).next().unwrap();
                (grapheme.to_string(), offset + grapheme.len(), None)
            };
            result
                .positions
                .insert(start, (result.rows.len() - 1, column));
            offset = end;
            if matches!(displayed.as_str(), "\n" | "\r\n" | "\r") {
                result.rows.last_mut().unwrap().end = start;
                result.rows.push(VisualRow {
                    glyphs: Vec::new(),
                    start: end,
                    end,
                });
                column = 0;
                result.positions.insert(end, (result.rows.len() - 1, 0));
                continue;
            }
            if start == 0 && displayed == "\u{feff}" {
                displayed.clear();
            }
            if displayed.is_empty() {
                result
                    .positions
                    .insert(end, (result.rows.len() - 1, column));
                result.rows.last_mut().unwrap().end = end;
                continue;
            }
            let is_tab = displayed == "\t";
            if is_tab {
                displayed = " ".repeat((4 - column % 4).min(limit));
            } else {
                displayed = safe_text(&displayed);
            }
            if UnicodeWidthStr::width(displayed.as_str()) == 0 {
                displayed.insert(0, '◌');
            }
            let mut glyph_width = UnicodeWidthStr::width(displayed.as_str()).max(1);
            if glyph_width > limit {
                displayed = "�".into();
                glyph_width = 1;
            }
            if column + glyph_width > limit {
                result.rows.push(VisualRow {
                    glyphs: Vec::new(),
                    start,
                    end: start,
                });
                column = 0;
                if is_tab {
                    displayed = " ".repeat(4.min(limit));
                    glyph_width = displayed.len();
                }
            }
            result
                .positions
                .insert(start, (result.rows.len() - 1, column));
            let mut style = theme::document_style();
            for (range, value) in &parsed.styles {
                if overlaps(range, &(start..end)) {
                    style = style.patch(*value);
                }
            }
            if !selection.is_empty() && overlaps(&selection.range(), &(start..end)) {
                style = style.add_modifier(Modifier::REVERSED);
            }
            result.rows.last_mut().unwrap().glyphs.push(Glyph {
                text: displayed,
                source: start..end,
                column,
                width: glyph_width,
                style,
                task,
            });
            result.rows.last_mut().unwrap().end = end;
            column += glyph_width;
            result
                .positions
                .insert(end, (result.rows.len() - 1, column));
        }
        result
            .positions
            .entry(source.len())
            .or_insert((result.rows.len() - 1, column));
        result
    }

    pub fn cursor(&self, offset: usize) -> (usize, usize) {
        self.positions
            .get(&offset)
            .copied()
            .or_else(|| self.positions.range(..=offset).next_back().map(|(_, p)| *p))
            .unwrap_or((0, 0))
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

    pub fn cursor_with_affinity(&self, offset: usize, affinity: Affinity) -> (usize, usize) {
        let (row, col) = self.cursor(offset);
        if affinity == Affinity::Upstream && col == 0 && row > 0 && self.rows[row - 1].end == offset
        {
            let previous = &self.rows[row - 1];
            return (
                row - 1,
                previous.glyphs.last().map_or(0, |g| g.column + g.width),
            );
        }
        (row, col)
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

#[cfg(test)]
mod tests {
    use super::*;
    use marklane::Document;
    fn project(text: &str, caret: usize, width: usize, live: bool) -> Projection {
        let doc = Document::new(text);
        Projection::build(
            text,
            &Parsed::new(text, doc.markdown(), true),
            Selection {
                anchor: caret,
                head: caret,
            },
            width,
            live,
        )
    }
    fn display(p: &Projection) -> String {
        p.rows
            .iter()
            .map(|r| r.glyphs.iter().map(|g| g.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn live_discloses_active_paragraph_and_preserves_letter_mapping() {
        let text = "active\n\n# Heading\n\nA **bold** word.";
        let p = project(text, 1, 80, true);
        assert!(display(&p).contains("Heading\n\nA bold word."));
        let source = text.find("bold").unwrap();
        let (row, col) = p.cursor(source);
        assert_eq!(p.hit(row, col).offset, source);
        let active = project(text, source, 80, true);
        assert!(display(&active).contains("A **bold** word."));
    }

    #[test]
    fn every_hit_and_wrap_boundary_is_a_grapheme_boundary() {
        let text = "a界e\u{301}👩‍💻\tlast\r\nnext";
        let boundaries: Vec<_> = text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain([text.len()])
            .collect();
        for width in 1..12 {
            let p = project(text, 0, width, false);
            for row in 0..p.rows.len() {
                for col in 0..width + 3 {
                    assert!(boundaries.contains(&p.hit(row, col).offset));
                }
            }
        }
    }

    #[test]
    fn task_and_label_have_distinct_hit_targets() {
        let text = "intro\n\n- [ ] task";
        let p = project(text, 0, 80, true);
        let glyph = p.rows[2].glyphs.iter().find(|g| g.task.is_some()).unwrap();
        assert!(p.hit(2, glyph.column).task.is_some());
        let label = text.find("task").unwrap();
        let (row, col) = p.cursor(label);
        assert_eq!(
            p.hit(row, col),
            Hit {
                offset: label,
                task: None,
                affinity: Affinity::Downstream,
            }
        );
    }

    #[test]
    fn document_control_sequences_never_reach_terminal() {
        let p = project("hello\x1b[31m\u{009b}text", 0, 80, false);
        assert!(!display(&p).contains(['\x1b', '\u{009b}']));
        assert!(display(&p).contains('␛'));
    }

    #[test]
    fn hidden_markers_never_split_combining_sequences() {
        for text in [
            "intro\n\n**bold**\u{301}",
            "intro\n\n[link](url)\u{301}",
            "intro\n\n*\u{301}hi*",
            "intro\n\n\u{200d}tail",
            "\u{feff}# title\r\n\n- [ ] task",
        ] {
            let boundaries: Vec<_> = text
                .grapheme_indices(true)
                .map(|(i, _)| i)
                .chain([text.len()])
                .collect();
            for caret in &boundaries {
                for width in [2, 8, 80] {
                    let p = project(text, *caret, width, true);
                    for row in 0..p.rows.len() {
                        for col in 0..width + 1 {
                            assert!(
                                boundaries.contains(&p.hit(row, col).offset),
                                "{text:?} caret {caret} at {row}/{col}"
                            );
                        }
                    }
                    for glyph in p.rows.iter().flat_map(|r| &r.glyphs) {
                        assert_eq!(UnicodeWidthStr::width(glyph.text.as_str()), glyph.width);
                    }
                }
            }
        }
    }

    #[test]
    fn active_parent_item_does_not_disclose_its_nested_items() {
        let text = "- parent **text**\n  - child **text**\n";
        let p = project(text, 3, 80, true);
        assert!(display(&p).contains("- parent **text**"));
        assert!(display(&p).contains("  • child text"));
    }

    #[test]
    fn eof_reveals_the_whole_multiline_paragraph() {
        let text = "intro\n\nfirst **bold\ncontinued**";
        assert_eq!(display(&project(text, text.len(), 80, true)), text);
        let trailing = format!("{text}\n");
        assert!(!display(&project(&trailing, trailing.len(), 80, true)).contains("**"));
    }

    #[test]
    fn bom_is_preserved_but_invisible_and_cr_is_a_line_boundary() {
        let text = "\u{feff}# title\r\rbody\r\nlast\n";
        let p = project(text, text.find("body").unwrap(), 80, true);
        assert_eq!(display(&p), "title\n\nbody\nlast\n");
        assert_eq!(p.cursor(text.find("body").unwrap()), (2, 0));
        assert_eq!(p.cursor(0), (0, 0));
    }

    #[test]
    fn pointer_padding_preserves_adjacent_text_controls_and_clipping() {
        let text = "intro\n\n- [ ] one\n- [x] two";
        let doc = Document::new(text);
        let parsed = Parsed::new(text, doc.markdown(), true);
        let mut p = Projection::build(text, &parsed, Selection::caret(0), 80, true);
        let mut first = p.rows[2]
            .glyphs
            .iter()
            .find(|g| g.task == Some(0))
            .unwrap()
            .clone();
        let mut second = p.rows[3]
            .glyphs
            .iter()
            .find(|g| g.task == Some(1))
            .unwrap()
            .clone();
        let mut label = p.rows[2]
            .glyphs
            .iter()
            .find(|g| g.text == "o")
            .unwrap()
            .clone();
        first.column = 2;
        second.column = 3;
        label.column = 1;
        p.rows[2].glyphs = vec![label, first.clone(), second.clone()];
        assert!(p.pointer_task(&parsed, 2, 1, 80).is_none());
        assert_eq!(
            p.pointer_task(&parsed, 2, 2, 80),
            Some(&parsed.snapshot.tasks[0])
        );
        assert_eq!(
            p.pointer_task(&parsed, 2, 3, 80),
            Some(&parsed.snapshot.tasks[1])
        );
        p.rows[2].glyphs = vec![first.clone()];
        assert!(
            p.pointer_task(&parsed, 2, 1, 2).is_none(),
            "clipped checkbox must not claim visible padding"
        );
        second.column = 4;
        p.rows[2].glyphs = vec![first, second];
        assert!(
            p.pointer_task(&parsed, 2, 3, 80).is_none(),
            "shared padding stays a text hit"
        );
    }
}
