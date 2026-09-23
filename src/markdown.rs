use std::{borrow::Cow, cmp::Reverse, ops::Range};

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// Emphasis matching one parse may spend, counted as underscores times
/// delimiters per paragraph: pulldown-cmark's worst case.
const EMPHASIS_BUDGET: u64 = 1 << 26;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading(u8),
    ListItem,
    Code,
    Quote,
    Table,
    Rule,
    Html,
    /// Leading YAML front matter, delimited by `---` lines.
    Metadata,
}

/// Original UTF-8 source range, as reported by the parser, rebased past a BOM.
/// Blocks nest. Ranges include syntax and usually the trailing newline; item
/// ranges may include leading indentation but omit an enclosing quote prefix
/// on the first line. Later lines retain all original container prefixes.
/// Paragraphs omit list markers. Blank source gaps are not synthetic blocks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub range: Range<usize>,
    pub kind: BlockKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    /// The single ASCII state character inside `[ ]`, `[x]`, or `[X]`.
    pub marker_range: Range<usize>,
    pub item_range: Range<usize>,
    pub checked: bool,
    pub revision: u64,
}

#[derive(Clone, Debug, Default)]
pub struct MarkdownSnapshot {
    pub revision: u64,
    pub blocks: Vec<Block>,
    pub tasks: Vec<Task>,
}

/// GFM adds only alert kinds (`> [!NOTE]`) to block quotes; front matter
/// keeps leading metadata from reading as a rule and a setext heading.
pub fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_GFM
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
}

/// Text to hand the parser. pulldown-cmark rescans every open emphasis
/// delimiter for each `_` that can only close one, so a paragraph can cost
/// its underscores times its delimiters. Past a budget, the costliest
/// paragraphs are parsed with `_` read as `,`. Offsets stay exact, and the
/// document and its display never change.
pub fn parser_input(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    let mut paragraphs = Vec::new();
    let (mut start, mut underscores, mut delimiters) = (0, 0u64, 0u64);
    let mut line = 0;
    while line <= bytes.len() {
        let end = bytes[line..]
            .iter()
            .position(|b| matches!(b, b'\n' | b'\r'))
            .map_or(bytes.len(), |at| line + at);
        let content = &bytes[line..end];
        // Blank lines end paragraphs; other block boundaries only make
        // this estimate more cautious.
        if content.iter().all(|b| matches!(b, b' ' | b'\t')) {
            if underscores > 0 {
                paragraphs.push((start..line, underscores.saturating_mul(delimiters)));
            }
            (start, underscores, delimiters) = (end, 0, 0);
        }
        for byte in content {
            match byte {
                b'_' => (underscores, delimiters) = (underscores + 1, delimiters + 1),
                b'*' | b'~' => delimiters += 1,
                _ => {}
            }
        }
        line = end
            + if bytes[end..].starts_with(b"\r\n") {
                2
            } else {
                1
            };
    }
    if underscores > 0 {
        paragraphs.push((start..bytes.len(), underscores.saturating_mul(delimiters)));
    }
    let mut total = paragraphs
        .iter()
        .fold(0u64, |total, (_, cost)| total.saturating_add(*cost));
    if total <= EMPHASIS_BUDGET {
        return Cow::Borrowed(text);
    }
    paragraphs.sort_unstable_by_key(|(_, cost)| Reverse(*cost));
    let mut input = bytes.to_vec();
    for (range, cost) in paragraphs {
        if total <= EMPHASIS_BUDGET {
            break;
        }
        for byte in &mut input[range] {
            if *byte == b'_' {
                *byte = b',';
            }
        }
        total = total.saturating_sub(cost);
    }
    Cow::Owned(String::from_utf8(input).expect("replacing ASCII keeps UTF-8 valid"))
}

pub fn analyze(text: &str, revision: u64) -> MarkdownSnapshot {
    let mut snapshot = MarkdownSnapshot {
        revision,
        ..Default::default()
    };
    let mut items = Vec::<Range<usize>>::new();
    let bom_len = bom_len(text);
    let input = parser_input(&text[bom_len..]);
    for (event, raw_range) in Parser::new_ext(&input, options()).into_offset_iter() {
        let range = raw_range.start + bom_len..raw_range.end + bom_len;
        let kind = match event {
            Event::Start(Tag::Paragraph) => Some(BlockKind::Paragraph),
            Event::Start(Tag::Heading { level, .. }) => Some(BlockKind::Heading(level as u8)),
            Event::Start(Tag::Item) => {
                items.push(range.clone());
                Some(BlockKind::ListItem)
            }
            Event::End(TagEnd::Item) => {
                items.pop();
                None
            }
            Event::Start(Tag::CodeBlock(_)) => Some(BlockKind::Code),
            Event::Start(Tag::BlockQuote(_)) => Some(BlockKind::Quote),
            Event::Start(Tag::Table(_)) => Some(BlockKind::Table),
            Event::Start(Tag::HtmlBlock) => Some(BlockKind::Html),
            Event::Start(Tag::MetadataBlock(_)) => Some(BlockKind::Metadata),
            Event::Rule => Some(BlockKind::Rule),
            Event::TaskListMarker(checked) => {
                if let Some(item_range) = items.last() {
                    let raw = &text[range.clone()];
                    if let Some(index) = raw
                        .as_bytes()
                        .windows(3)
                        .position(|w| matches!(w, b"[ ]" | b"[x]" | b"[X]"))
                    {
                        let marker = range.start + index + 1;
                        snapshot.tasks.push(Task {
                            marker_range: marker..marker + 1,
                            item_range: item_range.clone(),
                            checked,
                            revision,
                        });
                    }
                }
                None
            }
            _ => None,
        };
        if let Some(kind) = kind {
            snapshot.blocks.push(Block { range, kind });
        }
    }
    snapshot
}

pub fn bom_len(text: &str) -> usize {
    if text.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runaway_underscore_paragraphs_are_parsed_as_commas_at_the_same_offsets() {
        let ordinary = "Some _emphasis_, snake_case, and ~~more~~.\n\n*a* __b__\n";
        assert!(matches!(parser_input(ordinary), Cow::Borrowed(_)));
        // Each line alone stays within budget; a CRLF paragraph joins them.
        let line = "*a_".repeat(3000);
        let text = format!("Keep _this_.\n\n{line}\r\n{line}\r\n\r\nAnd _this_.\n");
        let input = parser_input(&text);
        assert_eq!(input.len(), text.len());
        let runaway = 14..text.len() - 13;
        assert!(!input[runaway.clone()].contains('_'));
        assert_eq!(
            input[runaway.clone()].replace(',', "_"),
            text[runaway.clone()]
        );
        assert_eq!(input[..runaway.start], text[..runaway.start]);
        assert_eq!(input[runaway.end..], text[runaway.end..]);
        let separate = format!("{line}\r\n\r\n{line}\n");
        assert!(matches!(parser_input(&separate), Cow::Borrowed(_)));
    }

    #[test]
    fn task_ranges_point_only_to_state_and_keep_source_revision() {
        let source =
            "> * [X] first\r\n> * [ ] second\n\n```md\n- [ ] code\n```\n\n    - [x] indented\n";
        let snapshot = analyze(source, 42);
        assert_eq!(snapshot.revision, 42);
        assert_eq!(snapshot.tasks.len(), 2, "{snapshot:?}");
        for (task, expected) in snapshot.tasks.iter().zip(["X", " "]) {
            assert_eq!(&source[task.marker_range.clone()], expected);
            assert_eq!(task.marker_range.len(), 1);
            assert_eq!(task.revision, 42);
            assert!(source[task.item_range.clone()].starts_with("* ["));
            assert!(task.item_range.contains(&task.marker_range.start));
        }
        assert!(snapshot.tasks[0].checked);
        assert!(!snapshot.tasks[1].checked);
    }

    #[test]
    fn blocks_preserve_raw_parser_ranges_with_nesting() {
        let source =
            "# Heading\r\n\n> - first\n>   continuation\n\n| a | b |\n| - | - |\n| x | y |\n";
        let snapshot = analyze(source, 0);
        let heading = snapshot
            .blocks
            .iter()
            .find(|b| b.kind == BlockKind::Heading(1))
            .unwrap();
        assert_eq!(&source[heading.range.clone()], "# Heading\r\n");
        let item = snapshot
            .blocks
            .iter()
            .find(|b| b.kind == BlockKind::ListItem)
            .unwrap();
        assert_eq!(&source[item.range.clone()], "- first\n>   continuation\n");
        let quote = snapshot
            .blocks
            .iter()
            .find(|b| b.kind == BlockKind::Quote)
            .unwrap();
        assert!(quote.range.start < item.range.start);
        assert!(quote.range.end >= item.range.end);
        assert!(snapshot.blocks.iter().any(|b| b.kind == BlockKind::Table));
        for block in snapshot.blocks {
            assert!(source.is_char_boundary(block.range.start));
            assert!(source.is_char_boundary(block.range.end));
            assert!(block.range.end <= source.len());
        }
    }

    #[test]
    fn front_matter_is_one_atomic_block_rather_than_a_rule_and_heading() {
        let source = "---\ntitle: Plan\ntags: [a]\n---\n\n# Real heading\n";
        let blocks = analyze(source, 0).blocks;
        assert_eq!(blocks[0].kind, BlockKind::Metadata);
        assert_eq!(
            &source[blocks[0].range.clone()],
            "---\ntitle: Plan\ntags: [a]\n---"
        );
        assert!(!blocks.iter().any(|b| b.kind == BlockKind::Rule));
        let headings: Vec<_> = blocks
            .iter()
            .filter(|b| matches!(b.kind, BlockKind::Heading(_)))
            .collect();
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].kind, BlockKind::Heading(1));
    }

    #[test]
    fn nested_tasks_have_distinct_marker_and_item_ranges() {
        let source = "- [ ] parent\n  - [x] child\n";
        let tasks = analyze(source, 7).tasks;
        assert_eq!(tasks.len(), 2);
        assert!(tasks[0].item_range.contains(&tasks[1].marker_range.start));
        assert!(!tasks[1].item_range.contains(&tasks[0].marker_range.start));
        assert_eq!(tasks[0].marker_range, 3..4);
        assert_eq!(tasks[1].marker_range, 18..19);
    }

    #[test]
    fn bom_and_indentation_keep_original_source_offsets() {
        let source = "\u{feff}  - [x] task\r\n";
        let snapshot = analyze(source, 1);
        assert_eq!(snapshot.tasks.len(), 1);
        assert_eq!(snapshot.tasks[0].marker_range, 8..9);
        assert_eq!(&source[snapshot.tasks[0].marker_range.clone()], "x");
        assert_eq!(snapshot.tasks[0].item_range, 3..source.len());
    }
}
