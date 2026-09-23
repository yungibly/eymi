//! Rendering analysis for one revision: inline styles, concealed or
//! substituted syntax, and the block structures that layout decorates.
use super::overlaps;
use crate::{
    icons::{self, IconSet},
    syntax::{self, Language, Token},
    theme::{self, Palette, Theme},
};
use eymi::{
    Selection,
    markdown::{self, BlockKind, MarkdownSnapshot},
};
use pulldown_cmark::{Alignment, BlockQuoteKind, CodeBlockKind, Event, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use std::ops::Range;
use unicode_segmentation::GraphemeCursor;

/// Quote markers become a rail, repeated on wrapped rows.
pub(super) const RAIL: &str = "▎";
const BULLETS: [&str; 4] = ["•", "◦", "▪", "▫"];

#[derive(Clone, Debug)]
pub(super) struct Decoration {
    pub range: Range<usize>,
    /// Empty conceals the range.
    pub replacement: String,
    /// Substituted glyphs use this instead of the spans they cover.
    pub style: Option<Style>,
    pub task: Option<usize>,
}

/// A fenced code block or front matter: a surface with optional fences.
#[derive(Clone, Debug)]
pub(super) struct Surface {
    pub range: Range<usize>,
    /// Fence text on its line, without the line ending.
    pub open: Option<Range<usize>>,
    pub close: Option<Range<usize>>,
    /// Content segments as parsed, excluding container prefixes.
    pub lines: Vec<Range<usize>>,
    pub label: String,
    pub language: Option<&'static Language>,
}

#[derive(Clone, Debug)]
pub(super) struct Heading {
    pub range: Range<usize>,
    pub level: usize,
    /// A setext underline, without the line ending.
    pub underline: Option<Range<usize>>,
}

#[derive(Clone, Debug)]
pub(super) struct Table {
    pub range: Range<usize>,
    pub alignments: Vec<Alignment>,
    /// Header first. Lines exclude endings; cells are trimmed content.
    pub rows: Vec<TableRow>,
    pub delimiter: Range<usize>,
}

#[derive(Clone, Debug)]
pub(super) struct TableRow {
    pub line: Range<usize>,
    pub cells: Vec<Range<usize>>,
}

pub struct Parsed {
    pub snapshot: MarkdownSnapshot,
    pub theme: Theme,
    pub icons: IconSet,
    pub(super) markdown: bool,
    /// Sorted by start; later spans refine earlier ones.
    pub(super) styles: Vec<(Range<usize>, Style)>,
    /// Sorted and nonoverlapping, on grapheme boundaries.
    pub(super) decorations: Vec<Decoration>,
    pub(super) surfaces: Vec<Surface>,
    pub(super) headings: Vec<Heading>,
    pub(super) rules: Vec<Range<usize>>,
    pub(super) tables: Vec<Table>,
}

impl Parsed {
    /// Markdown renders its structure and highlights fenced code; other
    /// files highlight as `language` when one was detected.
    pub fn new(
        source: &str,
        snapshot: MarkdownSnapshot,
        markdown: bool,
        language: Option<&'static Language>,
    ) -> Self {
        let mut parsed = Self {
            snapshot,
            theme: theme::current_theme(),
            icons: icons::current(),
            markdown,
            styles: Vec::new(),
            decorations: Vec::new(),
            surfaces: Vec::new(),
            headings: Vec::new(),
            rules: Vec::new(),
            tables: Vec::new(),
        };
        let colors = theme::palette();
        if markdown {
            parsed.analyze(source, &colors);
        } else if let Some(language) = language {
            let whole = markdown::bom_len(source)..source.len();
            parsed.highlight(language, source, std::slice::from_ref(&whole), &colors);
        }
        parsed.styles.sort_by_key(|(range, _)| range.start);
        parsed
    }

    fn analyze(&mut self, source: &str, colors: &Palette) {
        let bom = markdown::bom_len(source);
        let muted = Style::default().fg(colors.muted);
        let mut lists: Vec<bool> = Vec::new();
        let mut bullet: Option<(usize, usize)> = None;
        let mut struck: Option<usize> = None;
        let mut surface: Option<Surface> = None;
        let mut table: Option<Table> = None;
        let mut in_head = false;
        let mut html: Option<Vec<Range<usize>>> = None;
        let mut quotes: Vec<(Range<usize>, Option<BlockQuoteKind>)> = Vec::new();
        let input = markdown::parser_input(&source[bom..]);
        for (event, range) in Parser::new_ext(&input, markdown::options()).into_offset_iter() {
            let range = range.start + bom..range.end + bom;
            // A bullet waits only for a task marker that would replace it.
            if let Some((start, depth)) = bullet
                && !matches!(
                    event,
                    Event::TaskListMarker(_) | Event::Start(Tag::Paragraph)
                )
            {
                self.bullet(start, depth, colors);
                bullet = None;
            }
            if let Some(from) = struck
                && matches!(
                    event,
                    Event::End(TagEnd::Paragraph | TagEnd::Item) | Event::Start(Tag::List(_))
                )
            {
                let to = if matches!(event, Event::Start(_)) {
                    range.start
                } else {
                    range.end
                };
                let text = &source[from..to.max(from)];
                let from = from + (text.len() - text.trim_start().len());
                let to = from + text.trim().len();
                self.styles
                    .push((from..to, muted.add_modifier(Modifier::CROSSED_OUT)));
                struck = None;
            }
            let raw = &source[range.clone()];
            match event {
                Event::Start(Tag::Heading { level, .. }) => {
                    let level = level as usize;
                    self.styles.push((
                        range.clone(),
                        Style::default()
                            .fg(colors.headings[level - 1])
                            .add_modifier(Modifier::BOLD),
                    ));
                    let lead = raw.len() - raw.trim_start_matches(' ').len();
                    let hashes = raw[lead..].bytes().take_while(|b| *b == b'#').count();
                    let underline = if hashes == 0 {
                        setext_underline(source, &range)
                    } else {
                        None
                    };
                    if (1..=6).contains(&hashes)
                        && matches!(raw.as_bytes().get(lead + hashes), Some(b' ' | b'\t'))
                    {
                        let start = range.start + lead;
                        self.syntax(start..start + hashes + 1, muted);
                    }
                    if let Some(line) = &underline {
                        self.syntax(line.clone(), muted);
                    }
                    self.headings.push(Heading {
                        range,
                        level,
                        underline,
                    });
                }
                Event::Start(Tag::Strong) => {
                    self.styles
                        .push((range.clone(), Style::default().add_modifier(Modifier::BOLD)));
                    self.delimiters(source, range, &["**", "__"], muted);
                }
                Event::Start(Tag::Emphasis) => {
                    self.styles.push((
                        range.clone(),
                        Style::default().add_modifier(Modifier::ITALIC),
                    ));
                    self.delimiters(source, range, &["*", "_"], muted);
                }
                Event::Start(Tag::Strikethrough) => {
                    self.styles
                        .push((range.clone(), muted.add_modifier(Modifier::CROSSED_OUT)));
                    self.delimiters(source, range, &["~~", "~"], muted);
                }
                Event::Start(Tag::Link { .. }) => {
                    self.styles.push((
                        range.clone(),
                        Style::default()
                            .fg(colors.link)
                            .add_modifier(Modifier::UNDERLINED),
                    ));
                    if let Some(close) = simple_label(raw, 1) {
                        self.syntax(range.start..range.start + 1, muted);
                        self.syntax(range.start + close..range.end, muted);
                    } else if raw.len() > 2 && raw.starts_with('<') && raw.ends_with('>') {
                        self.syntax(range.start..range.start + 1, muted);
                        self.syntax(range.end - 1..range.end, muted);
                    }
                }
                Event::Start(Tag::Image { .. }) => {
                    self.styles.push((
                        range.clone(),
                        Style::default()
                            .fg(colors.link)
                            .add_modifier(Modifier::ITALIC),
                    ));
                    if raw.starts_with('!')
                        && let Some(close) = simple_label(&raw[1..], 1)
                    {
                        let icon = self.icons.image();
                        if icon.is_empty() {
                            self.syntax(range.start..range.start + 2, muted);
                        } else {
                            self.decorations.push(Decoration {
                                range: range.start..range.start + 1,
                                replacement: format!("{icon} "),
                                style: Some(Style::default().fg(colors.link)),
                                task: None,
                            });
                            self.syntax(range.start + 1..range.start + 2, muted);
                        }
                        self.syntax(range.start + 1 + close..range.end, muted);
                    }
                }
                Event::Code(_) => {
                    self.styles.push((
                        range.clone(),
                        Style::default().fg(colors.code).bg(colors.code_background),
                    ));
                    // Backtick runs become padding inside the code surface.
                    let ticks = raw.bytes().take_while(|b| *b == b'`').count();
                    if ticks > 0 && raw.len() > 2 * ticks && raw.ends_with(&raw[..ticks]) {
                        for run in [
                            range.start..range.start + ticks,
                            range.end - ticks..range.end,
                        ] {
                            self.styles.push((run.clone(), muted));
                            self.decorations.push(Decoration {
                                range: run,
                                replacement: " ".into(),
                                style: None,
                                task: None,
                            });
                        }
                    }
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    let (open, label) = match &kind {
                        CodeBlockKind::Fenced(info) => {
                            (Some(line_text(source, range.start)), fence_label(info))
                        }
                        CodeBlockKind::Indented => (None, String::new()),
                    };
                    let language = Language::for_fence(&label);
                    surface = Some(Surface {
                        range,
                        open,
                        close: None,
                        lines: Vec::new(),
                        label: self.labelled(&label, language),
                        language,
                    });
                }
                Event::Start(Tag::MetadataBlock(_)) => {
                    let language = Language::for_fence("yaml");
                    surface = Some(Surface {
                        open: Some(line_text(source, range.start)),
                        close: last_line(source, &range).filter(|line| line.start > range.start),
                        range,
                        lines: Vec::new(),
                        label: self.labelled("front matter", language),
                        language,
                    });
                }
                Event::Text(_) if surface.is_some() => {
                    if let Some(surface) = &mut surface {
                        surface.lines.push(range);
                    }
                }
                Event::End(TagEnd::CodeBlock | TagEnd::MetadataBlock(_)) => {
                    if let Some(mut done) = surface.take() {
                        if done.open.is_some() && done.close.is_none() {
                            done.close = closing_fence(source, &done.range);
                        }
                        for fence in [&done.open, &done.close].into_iter().flatten() {
                            self.syntax(fence.clone(), muted);
                        }
                        if let Some(language) = done.language {
                            self.highlight(language, source, &done.lines, colors);
                        }
                        self.surfaces.push(done);
                    }
                }
                Event::Start(Tag::BlockQuote(kind)) => {
                    if kind.is_none() {
                        self.styles.push((
                            range.clone(),
                            Style::default()
                                .fg(colors.quote)
                                .add_modifier(Modifier::ITALIC),
                        ));
                    }
                    quotes.push((range, kind));
                }
                Event::Start(Tag::List(first)) => lists.push(first.is_some()),
                Event::End(TagEnd::List(_)) => {
                    lists.pop();
                }
                Event::Start(Tag::Item) => {
                    let depth = lists.len().saturating_sub(1);
                    if lists.last() == Some(&true) {
                        let digits = raw.bytes().take_while(u8::is_ascii_digit).count();
                        if digits > 0 && matches!(raw.as_bytes().get(digits), Some(b'.' | b')')) {
                            self.styles.push((
                                range.start..range.start + digits + 1,
                                Style::default().fg(colors.list_marker),
                            ));
                        }
                    } else if matches!(raw.as_bytes().first(), Some(b'-' | b'+' | b'*')) {
                        bullet = Some((range.start, depth));
                    }
                }
                Event::TaskListMarker(checked) => {
                    // The checkbox takes the place of its bullet.
                    if let Some((start, _)) = bullet.take() {
                        self.syntax(start..range.start, muted);
                    }
                    if checked {
                        struck = Some(range.end);
                    }
                }
                Event::Start(Tag::Table(alignments)) => {
                    self.styles
                        .push((range.clone(), Style::default().fg(colors.faint)));
                    table = Some(Table {
                        range,
                        alignments,
                        rows: Vec::new(),
                        delimiter: 0..0,
                    });
                }
                Event::Start(Tag::TableHead | Tag::TableRow) => {
                    in_head = matches!(event, Event::Start(Tag::TableHead));
                    if let Some(table) = &mut table {
                        table.rows.push(TableRow {
                            line: line_text(source, range.start),
                            cells: Vec::new(),
                        });
                    }
                }
                Event::Start(Tag::TableCell) => {
                    let lead = raw.len() - raw.trim_start().len();
                    let cell = range.start + lead..range.start + lead + raw.trim().len();
                    let style = if in_head {
                        Style::default()
                            .fg(colors.headings[0])
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(colors.foreground)
                    };
                    if !cell.is_empty() {
                        self.styles.push((cell.clone(), style));
                    }
                    if let Some(row) = table.as_mut().and_then(|table| table.rows.last_mut()) {
                        row.cells.push(cell);
                    }
                }
                Event::End(TagEnd::Table) => {
                    if let Some(mut done) = table.take() {
                        if let Some(header) = done.rows.first() {
                            done.delimiter = line_text(source, next_line(source, header.line.end));
                        }
                        self.tables.push(done);
                    }
                }
                Event::Rule => {
                    let line = trim_ending(source, range);
                    self.rules.push(line.clone());
                    self.syntax(line, muted);
                }
                Event::Start(Tag::HtmlBlock) => html = Some(Vec::new()),
                Event::Html(_) if html.is_some() => {
                    if let Some(lines) = &mut html {
                        lines.push(range);
                    }
                }
                Event::End(TagEnd::HtmlBlock) => {
                    if let (Some(lines), Some(language)) =
                        (html.take(), Language::for_fence("html"))
                    {
                        self.highlight(language, source, &lines, colors);
                    }
                }
                Event::InlineHtml(_) | Event::Html(_) => {
                    self.styles
                        .push((range, Style::default().fg(colors.syntax.tag)));
                }
                _ => {}
            }
        }
        if let Some((start, depth)) = bullet {
            self.bullet(start, depth, colors);
        }
        self.rails(source, &quotes, colors);
        self.tasks(source, colors);
        self.decorations
            .sort_by_key(|d| (d.range.start, d.range.end));
        let mut end = 0;
        self.decorations.retain(|d| {
            // Even ASCII syntax can share a grapheme with a following combining
            // mark; such syntax stays literal rather than splitting it.
            let keep = d.range.start >= end
                && boundary(source, d.range.start)
                && boundary(source, d.range.end);
            if keep {
                end = d.range.end;
            }
            keep
        });
    }

    fn highlight(
        &mut self,
        language: &Language,
        source: &str,
        segments: &[Range<usize>],
        colors: &Palette,
    ) {
        let spans = syntax::highlight(language, source, segments);
        self.styles.extend(
            spans
                .into_iter()
                .map(|(range, token)| (range, token_style(token, colors))),
        );
    }

    /// A code surface's tab: its fence word, led by the language's icon.
    fn labelled(&self, label: &str, language: Option<&'static Language>) -> String {
        match language {
            Some(language) if self.icons == IconSet::Nerd && !label.is_empty() => {
                format!("{} {label}", language.icon())
            }
            _ => label.into(),
        }
    }

    /// Conceal syntax in rendered blocks; it reads as muted when disclosed.
    fn syntax(&mut self, range: Range<usize>, muted: Style) {
        if range.is_empty() {
            return;
        }
        self.styles.push((range.clone(), muted));
        self.decorations.push(Decoration {
            range,
            replacement: String::new(),
            style: None,
            task: None,
        });
    }

    fn delimiters(&mut self, source: &str, range: Range<usize>, candidates: &[&str], muted: Style) {
        let raw = &source[range.clone()];
        if let Some(delimiter) = candidates.iter().find(|delimiter| {
            raw.len() >= 2 * delimiter.len()
                && raw.starts_with(**delimiter)
                && raw.ends_with(**delimiter)
        }) {
            self.syntax(range.start..range.start + delimiter.len(), muted);
            self.syntax(range.end - delimiter.len()..range.end, muted);
        }
    }

    fn bullet(&mut self, start: usize, depth: usize, colors: &Palette) {
        self.decorations.push(Decoration {
            range: start..start + 1,
            replacement: BULLETS[depth % BULLETS.len()].into(),
            style: Some(Style::default().fg(colors.list_marker)),
            task: None,
        });
    }

    fn tasks(&mut self, source: &str, colors: &Palette) {
        for (index, task) in self.snapshot.tasks.iter().enumerate() {
            let state = task.marker_range.start;
            if state > 0
                && source.as_bytes().get(state - 1) == Some(&b'[')
                && source.as_bytes().get(state + 1) == Some(&b']')
            {
                let color = if task.checked {
                    colors.callouts[1]
                } else {
                    colors.muted
                };
                self.decorations.push(Decoration {
                    range: state - 1..state + 2,
                    replacement: self.icons.task(task.checked).into(),
                    style: Some(Style::default().fg(color)),
                    task: Some(index),
                });
            }
        }
    }

    /// Each leading `>` on a quoted line becomes a rail colored by the quote
    /// it opens; alert markers become titled headers.
    fn rails(
        &mut self,
        source: &str,
        quotes: &[(Range<usize>, Option<BlockQuoteKind>)],
        colors: &Palette,
    ) {
        let color = |kind: Option<BlockQuoteKind>| {
            kind.map_or(colors.quote, |kind| colors.callouts[callout(kind)])
        };
        let mut index = 0;
        while index < quotes.len() {
            let outer = &quotes[index].0;
            let nested = quotes[index..]
                .iter()
                .take_while(|(range, _)| range.start < outer.end)
                .count();
            let group = &quotes[index..index + nested];
            let mut offset = outer.start;
            for line in source[outer.clone()].split_inclusive(['\n', '\r']) {
                let bytes = line.as_bytes();
                let mut markers = Vec::new();
                let mut at = 0;
                loop {
                    while matches!(bytes.get(at), Some(b' ' | b'\t')) {
                        at += 1;
                    }
                    if bytes.get(at) != Some(&b'>') {
                        break;
                    }
                    markers.push(offset + at);
                    at += 1;
                }
                if let Some(last) = markers.last() {
                    let containing: Vec<_> = group
                        .iter()
                        .filter(|(range, _)| range.start <= *last && range.end > offset)
                        .collect();
                    for (depth, marker) in markers.iter().enumerate() {
                        let kind = containing.get(depth).and_then(|(_, kind)| *kind);
                        self.decorations.push(Decoration {
                            range: *marker..marker + 1,
                            replacement: RAIL.into(),
                            style: Some(Style::default().fg(color(kind))),
                            task: None,
                        });
                    }
                }
                offset += line.len();
            }
            for (range, kind) in group {
                if let Some(kind) = kind {
                    self.alert_title(source, range, *kind, colors);
                }
            }
            index += nested;
        }
    }

    fn alert_title(
        &mut self,
        source: &str,
        range: &Range<usize>,
        kind: BlockQuoteKind,
        colors: &Palette,
    ) {
        let line = line_text(source, range.start);
        let text = &source[line.clone()];
        let Some(open) = text.find("[!") else {
            return;
        };
        let Some(close) = text[open..].find(']') else {
            return;
        };
        let index = callout(kind);
        let icon = self.icons.callout(index);
        let name = ["Note", "Tip", "Important", "Warning", "Caution"][index];
        self.decorations.push(Decoration {
            range: line.start + open..line.start + open + close + 1,
            replacement: if icon.is_empty() {
                name.into()
            } else {
                format!("{icon} {name}")
            },
            style: Some(
                Style::default()
                    .fg(colors.callouts[index])
                    .add_modifier(Modifier::BOLD),
            ),
            task: None,
        });
    }

    /// Source ranges revealed around the selection: the smallest block at the
    /// caret, atomic for code, tables, and front matter; a line within quotes;
    /// and every block a selection touches.
    pub fn disclosed(&self, source: &str, selection: Selection) -> Vec<Range<usize>> {
        let head = selection.head;
        let contains_head = |range: &Range<usize>| {
            (range.start <= head && head < range.end)
                || (head == source.len() && range.end == head && !source.ends_with(['\n', '\r']))
        };
        let atomic = self
            .snapshot
            .blocks
            .iter()
            .filter(|b| {
                matches!(
                    b.kind,
                    BlockKind::Code | BlockKind::Table | BlockKind::Metadata
                ) && contains_head(&b.range)
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

fn token_style(token: Token, colors: &Palette) -> Style {
    let ink = &colors.syntax;
    let (color, modifier) = match token {
        Token::Comment => (ink.comment, Modifier::ITALIC),
        Token::Keyword => (ink.keyword, Modifier::empty()),
        Token::Type => (ink.types, Modifier::empty()),
        Token::Function => (ink.function, Modifier::empty()),
        Token::String => (ink.string, Modifier::empty()),
        Token::Escape => (ink.escape, Modifier::empty()),
        Token::Number | Token::Constant => (ink.constant, Modifier::empty()),
        Token::Operator => (ink.operator, Modifier::empty()),
        Token::Punctuation => (ink.punctuation, Modifier::empty()),
        Token::Macro | Token::Attribute | Token::Label => (ink.special, Modifier::empty()),
        Token::Property => (ink.property, Modifier::empty()),
        Token::Tag => (ink.tag, Modifier::empty()),
        Token::Variable => (ink.variable, Modifier::empty()),
        Token::Heading => (colors.headings[0], Modifier::BOLD),
        Token::Inserted => (ink.inserted, Modifier::empty()),
        Token::Deleted => (ink.deleted, Modifier::empty()),
    };
    Style::default().fg(color).add_modifier(modifier)
}

fn callout(kind: BlockQuoteKind) -> usize {
    match kind {
        BlockQuoteKind::Note => 0,
        BlockQuoteKind::Tip => 1,
        BlockQuoteKind::Important => 2,
        BlockQuoteKind::Warning => 3,
        BlockQuoteKind::Caution => 4,
    }
}

/// `[label](…)`: the offset of `](` when the label is a plain source slice.
fn simple_label(raw: &str, open: usize) -> Option<usize> {
    if !raw.starts_with('[') || !raw.ends_with(')') {
        return None;
    }
    let close = raw.find("](")?;
    (close >= open && !raw[open..close].contains(['[', ']', '\\'])).then_some(close)
}

/// The first word of a fence info string: `rust,ignore` and `{.python}` too.
fn fence_label(info: &str) -> String {
    info.split(|c: char| c.is_whitespace() || matches!(c, ',' | '{' | '}'))
        .map(|word| word.trim_start_matches('.'))
        .find(|word| !word.is_empty())
        .unwrap_or("")
        .chars()
        .filter(|c| !super::unsafe_char(*c))
        .take(24)
        .collect()
}

pub(super) fn line_end(source: &str, offset: usize) -> usize {
    source[offset..]
        .find(['\n', '\r'])
        .map_or(source.len(), |at| offset + at)
}

pub(super) fn next_line(source: &str, end: usize) -> usize {
    let rest = &source[end..];
    end + if rest.starts_with("\r\n") {
        2
    } else if rest.starts_with(['\n', '\r']) {
        1
    } else {
        0
    }
}

fn line_text(source: &str, offset: usize) -> Range<usize> {
    offset..line_end(source, offset)
}

fn trim_ending(source: &str, range: Range<usize>) -> Range<usize> {
    let text = source[range.clone()].trim_end_matches(['\n', '\r']);
    range.start..range.start + text.len()
}

/// The final line inside `range`, without its ending.
fn last_line(source: &str, range: &Range<usize>) -> Option<Range<usize>> {
    let body = trim_ending(source, range.clone());
    let start = source[body.clone()]
        .rfind(['\n', '\r'])
        .map_or(body.start, |at| body.start + at + 1);
    (start < body.end).then_some(start..body.end)
}

fn setext_underline(source: &str, range: &Range<usize>) -> Option<Range<usize>> {
    let line = last_line(source, range).filter(|line| line.start > range.start)?;
    let text = &source[line.clone()];
    let lead = text.len() - text.trim_start().len();
    let marks = text.trim();
    (!marks.is_empty() && (marks.bytes().all(|b| b == b'=') || marks.bytes().all(|b| b == b'-')))
        .then(|| line.start + lead..line.start + lead + marks.len())
}

/// A closing fence is a final line of three or more backticks or tildes.
fn closing_fence(source: &str, range: &Range<usize>) -> Option<Range<usize>> {
    let line = last_line(source, range).filter(|line| line.start > range.start)?;
    let text = &source[line.clone()];
    let at = text.find(['`', '~'])?;
    let fence = text.as_bytes()[at];
    let run = text[at..].bytes().take_while(|b| *b == fence).count();
    (run >= 3 && text[at + run..].trim().is_empty())
        .then(|| line.start + at..line.start + at + run + text[at + run..].trim_end().len())
}

fn boundary(source: &str, offset: usize) -> bool {
    offset == 0
        || offset == source.len()
        || (source.is_char_boundary(offset)
            && GraphemeCursor::new(offset, source.len(), true)
                .is_boundary(source, 0)
                .unwrap_or(false))
}
