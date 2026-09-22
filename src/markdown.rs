use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

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

pub fn analyze(text: &str, revision: u64) -> MarkdownSnapshot {
    let mut snapshot = MarkdownSnapshot {
        revision,
        ..Default::default()
    };
    let mut items = Vec::<Range<usize>>::new();
    let bom_len = bom_len(text);
    for (event, raw_range) in Parser::new_ext(&text[bom_len..], options()).into_offset_iter() {
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
