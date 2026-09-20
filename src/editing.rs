//! Markdown-aware edit commands. Literal insertion never passes through helpers.
use std::ops::Range;

use pulldown_cmark::{Event, Parser};

use crate::{
    Document,
    markdown::{self, Block, BlockKind, MarkdownSnapshot},
};

/// Prefer the current line's ending, then the nearest preceding ending, then LF.
pub fn preferred_newline(text: &str, offset: usize) -> &'static str {
    let bytes = text.as_bytes();
    let terminator = bytes
        .iter()
        .enumerate()
        .skip(offset.min(bytes.len()))
        .find(|(_, b)| matches!(b, b'\r' | b'\n'))
        .map(|(i, _)| i)
        .or_else(|| {
            bytes[..offset.min(bytes.len())]
                .iter()
                .rposition(|b| matches!(b, b'\r' | b'\n'))
        });
    match terminator {
        Some(i) if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') => "\r\n",
        Some(i) if bytes[i] == b'\n' && i > 0 && bytes[i - 1] == b'\r' => "\r\n",
        Some(i) if bytes[i] == b'\r' => "\r",
        _ => "\n",
    }
}

/// Enter continues the parser-confirmed current item, or inserts a literal newline.
/// All helper edits, including removal of a selection, are one undo transaction.
pub fn enter(document: &mut Document) -> bool {
    let range = document.selection().range();
    let source = document.text();
    let line = line_range(source, range.start);
    let snapshot = document.markdown();

    // A selection across lines/constructs is simply replaced by a newline.
    // Inline code also remains literal when the caret is inside its delimiters.
    if range.end > line.end
        || is_blank_container_line(&source[line.clone()])
        || snapshot.blocks.iter().any(|block| {
            matches!(block.kind, BlockKind::Code | BlockKind::Html)
                && block.range.start <= line.end
                && block.range.end > line.start
        })
        || Parser::new_ext(&source[markdown::bom_len(source)..], markdown::options())
            .into_offset_iter()
            .any(|(event, literal)| {
                matches!(event, Event::Code(_))
                    && literal.start + markdown::bom_len(source) < range.start
                    && range.start < literal.end + markdown::bom_len(source)
            })
    {
        return document.literal_newline();
    }

    let empty_dash = empty_nested_dash(source, &line, &snapshot);
    let item = snapshot
        .blocks
        .iter()
        .chain(empty_dash.iter())
        .filter(|block| {
            block.kind == BlockKind::ListItem
                && (block.range.contains(&range.start)
                    || (block.range.end <= range.start
                        // A previous item ending at this line's start does
                        // not own the new line, even if the suffix is empty.
                        && block.range.end > line.start
                        && source[block.range.end..range.start]
                            .bytes()
                            .all(|b| matches!(b, b' ' | b'\t'))))
        })
        .min_by_key(|block| block.range.len());
    let Some(item) = item else {
        return document.literal_newline();
    };
    let Some(prefix) = item_prefix(source, &item.range, &snapshot) else {
        return document.literal_newline();
    };
    // Typing Enter in a marker edits literal source, not a guessed list context.
    if range.start < prefix.content_start {
        return document.literal_newline();
    }

    if line.start == prefix.line_start
        && source[prefix.content_start..range.start].trim().is_empty()
        && source[range.end..line.end].trim().is_empty()
    {
        // Leave exactly one nesting level. Retain a surrounding blockquote.
        let parent = snapshot
            .blocks
            .iter()
            .filter(|block| {
                block.kind == BlockKind::ListItem
                    && block.range.start < item.range.start
                    && block.range.end >= item.range.end
            })
            .min_by_key(|block| block.range.len());
        let replacement = if let Some(parent) =
            parent.and_then(|parent| item_prefix(source, &parent.range, &snapshot))
        {
            parent.continuation
        } else if needs_exit_separator(source, &line, &snapshot) {
            // Without a blank separator, immediately typed prose would be a
            // CommonMark lazy continuation of the preceding list. Make the
            // exit explicit in the source so it also survives undo/reopening.
            let newline = preferred_newline(source, range.start);
            format!("{}{newline}{}", prefix.quote_context, prefix.quote_context)
        } else {
            prefix.quote_context
        };
        return document
            .replace_range(line, &replacement)
            .expect("line has grapheme boundaries");
    }

    let newline = preferred_newline(source, range.start);
    let replacement = format!("{newline}{}", prefix.continuation);
    document
        .replace_range(range, &replacement)
        .expect("valid selection")
}

struct ItemPrefix {
    line_start: usize,
    content_start: usize,
    continuation: String,
    quote_context: String,
}

fn is_blank_container_line(line: &str) -> bool {
    line.strip_prefix('\u{feff}')
        .unwrap_or(line)
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'>'))
}

/// Outer-list exit needs one separating blank line unless one is already
/// present. Keep quoted separators inside their original quote container.
fn needs_exit_separator(source: &str, line: &Range<usize>, snapshot: &MarkdownSnapshot) -> bool {
    if line.start == 0 {
        return false;
    }
    let bytes = source.as_bytes();
    let previous_end = if line.start >= 2 && &bytes[line.start - 2..line.start] == b"\r\n" {
        line.start - 2
    } else {
        line.start - 1
    };
    let previous_line = line_range(source, previous_end);
    !is_blank_container_line(&source[previous_line.clone()])
        && snapshot.blocks.iter().any(|block| {
            block.kind == BlockKind::ListItem
                && block.range.start < line.start
                && block.range.end > previous_line.start
        })
}

/// A single indented dash after an item's text is a setext underline in
/// CommonMark. While editing a list, treat this one ambiguous empty marker as
/// a nested item so repeated Enter can leave one level. Literal Enter bypasses
/// this convenience; a multi-dash heading underline is never treated as empty.
fn empty_nested_dash(
    source: &str,
    line: &Range<usize>,
    snapshot: &MarkdownSnapshot,
) -> Option<Block> {
    let raw = source[line.clone()].trim_end_matches([' ', '\t']);
    let before_dash = raw.strip_suffix('-')?;
    if !before_dash
        .bytes()
        .all(|b| matches!(b, b' ' | b'\t' | b'>'))
    {
        return None;
    }
    let marker = line.start + before_dash.len();
    let parent = snapshot
        .blocks
        .iter()
        .filter(|block| {
            block.kind == BlockKind::ListItem
                && block.range.start < line.start
                && block.range.end >= marker
        })
        .min_by_key(|block| block.range.len())?;
    let parent_line = line_range(source, parent.range.start);
    let parent_marker = source[parent.range.start..parent_line.end]
        .bytes()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .count()
        + parent.range.start;
    if marker - line.start <= parent_marker - parent_line.start {
        return None;
    }
    Some(Block {
        range: marker..line.end,
        kind: BlockKind::ListItem,
    })
}

/// Inspect marker spelling only after the Markdown parser establishes an item.
fn item_prefix(
    source: &str,
    item: &Range<usize>,
    snapshot: &MarkdownSnapshot,
) -> Option<ItemPrefix> {
    let line = line_range(source, item.start);
    let bytes = source.as_bytes();
    let mut marker_start = item.start;
    while marker_start < line.end && matches!(bytes[marker_start], b' ' | b'\t') {
        marker_start += 1;
    }
    let mut end = marker_start;
    let marker = match *bytes.get(end)? {
        b'-' | b'*' | b'+' => {
            end += 1;
            source[marker_start..end].to_owned()
        }
        b'0'..=b'9' => {
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            let delimiter = *bytes.get(end)?;
            if !matches!(delimiter, b'.' | b')') {
                return None;
            }
            let number = source[marker_start..end]
                .parse::<u64>()
                .ok()?
                .checked_add(1)?;
            // CommonMark only recognizes up to nine digits in a list marker.
            if number > 999_999_999 {
                return None;
            }
            end += 1;
            format!("{number}{}", delimiter as char)
        }
        _ => return None,
    };
    let whitespace_start = end;
    while end < line.end && matches!(bytes[end], b' ' | b'\t') {
        end += 1;
    }
    if end < line.end && whitespace_start == end {
        return None;
    }
    let spacing = if end == whitespace_start {
        " "
    } else {
        &source[whitespace_start..end]
    };
    let context = &source[line.start..marker_start];
    // Compact nesting such as `- - inner` has a parent marker on this same line.
    // Its continuation is indentation, while quote delimiters retain spelling.
    let base: String = context
        .chars()
        .filter(|&c| c != '\u{feff}')
        .map(|c| {
            if matches!(c, ' ' | '\t' | '>' | '\u{feff}') {
                c
            } else {
                ' '
            }
        })
        .collect();
    let quote_context = context
        .rfind('>')
        .map(|index| {
            let end = index + 1 + usize::from(context.as_bytes().get(index + 1) == Some(&b' '));
            context[..end].to_owned()
        })
        .unwrap_or_else(|| {
            if context.starts_with('\u{feff}') {
                "\u{feff}".to_owned()
            } else {
                String::new()
            }
        });
    let mut continuation = format!("{base}{marker}{spacing}");
    if let Some(task) = snapshot.tasks.iter().find(|task| task.item_range == *item) {
        // A task marker always begins directly after the list prefix.
        if task.marker_range.start == end + 1 {
            end = task.marker_range.end + 1;
            let task_space = end;
            while end < line.end && matches!(bytes[end], b' ' | b'\t') {
                end += 1;
            }
            continuation.push_str("[ ]");
            continuation.push_str(if end == task_space {
                " "
            } else {
                &source[task_space..end]
            });
        }
    }
    Some(ItemPrefix {
        line_start: line.start,
        content_start: end,
        continuation,
        quote_context,
    })
}

/// The physical source line excluding its ending; CRLF is one grapheme.
fn line_range(text: &str, offset: usize) -> Range<usize> {
    let bytes = text.as_bytes();
    let start = bytes[..offset]
        .iter()
        .rposition(|b| matches!(b, b'\r' | b'\n'))
        .map_or(0, |i| i + 1);
    let end = bytes[offset..]
        .iter()
        .position(|b| matches!(b, b'\r' | b'\n'))
        .map_or(bytes.len(), |i| offset + i);
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Selection;

    fn enter_at_end(source: &str, expected: &str) {
        let mut doc = Document::new(source);
        doc.set_caret(source.len()).unwrap();
        assert!(doc.enter(), "source: {source:?}");
        assert_eq!(
            doc.text(),
            expected,
            "source: {source:?}; parse: {:?}",
            markdown::analyze(source, 0)
        );
        assert_eq!(doc.revision(), 1);
        assert!(doc.is_grapheme_boundary(doc.selection().head));
        assert!(doc.undo());
        assert_eq!(doc.text(), source);
        assert_eq!(doc.selection(), Selection::caret(source.len()));
        assert!(!doc.undo());
        assert!(!doc.is_dirty());
        assert!(doc.redo());
        assert_eq!(doc.text(), expected);
    }

    #[test]
    fn continues_bullets_numbers_and_tasks_preserving_spelling() {
        for (source, expected) in [
            ("- first", "- first\n- "),
            ("* first", "* first\n* "),
            ("+ first", "+ first\n+ "),
            ("3. third", "3. third\n4. "),
            ("9) ninth", "9) ninth\n10) "),
            ("009. ninth", "009. ninth\n10. "),
            ("- [ ] todo", "- [ ] todo\n- [ ] "),
            ("* [X] done", "* [X] done\n* [ ] "),
            ("3) [x] done", "3) [x] done\n4) [ ] "),
            ("  *  spaced", "  *  spaced\n  *  "),
            ("- [x]  spaced", "- [x]  spaced\n- [ ]  "),
            ("> - quoted", "> - quoted\n> - "),
            ("> > + quoted", "> > + quoted\n> > + "),
            ("- parent\n  * child", "- parent\n  * child\n  * "),
            ("- parent\n  continuation", "- parent\n  continuation\n- "),
            ("- - compact", "- - compact\n  - "),
        ] {
            enter_at_end(source, expected);
        }
    }

    #[test]
    fn empty_items_exit_one_level_and_keep_quotes() {
        for (source, expected) in [
            ("- ", ""),
            ("*", ""),
            ("3. ", ""),
            ("- [ ] ", ""),
            ("- [x] ", ""),
            ("\u{feff}- [ ] ", "\u{feff}"),
            ("  -   ", ""),
            ("> - ", "> "),
            ("> > 3) [ ] ", "> > "),
            ("- parent\n  - ", "- parent\n- "),
            ("3. parent\n   - ", "3. parent\n4. "),
            ("> - parent\n>   - ", "> - parent\n> - "),
            ("- parent\n  - child\n    - ", "- parent\n  - child\n  - "),
            ("- - ", "- "),
        ] {
            enter_at_end(source, expected);
        }
    }

    #[test]
    fn splitting_or_replacing_text_is_one_transaction() {
        let mut doc = Document::new("- [x] hello world\n- later");
        doc.set_caret(12).unwrap();
        doc.enter();
        assert_eq!(doc.text(), "- [x] hello \n- [ ] world\n- later");
        assert_eq!(doc.selection(), Selection::caret(19));
        doc.undo();
        assert_eq!(doc.text(), "- [x] hello world\n- later");
        doc.set_selection(Selection {
            anchor: 11,
            head: 6,
        })
        .unwrap();
        doc.enter();
        assert_eq!(doc.text(), "- [x] \n- [ ]  world\n- later");
        doc.undo();
        assert_eq!(
            doc.selection(),
            Selection {
                anchor: 11,
                head: 6
            }
        );
    }

    #[test]
    fn list_like_literal_code_never_continues_or_toggles() {
        for source in [
            "```md\n- [ ] literal",
            "~~~\n3. literal",
            "    - [x] literal",
            "- parent\n\n      - [ ] code",
            "<pre>\n- [ ] literal",
        ] {
            enter_at_end(source, &format!("{source}\n"));
            assert!(
                Document::new(source).markdown().tasks.is_empty(),
                "{source:?}"
            );
        }
        let mut doc = Document::new("- `literal code`");
        doc.set_caret(6).unwrap();
        doc.enter();
        assert_eq!(doc.text(), "- `lit\neral code`");
    }

    #[test]
    fn literal_and_selected_multiline_newlines_bypass_helpers() {
        let mut doc = Document::new("- one\n- two");
        doc.set_caret(5).unwrap();
        doc.literal_newline();
        assert_eq!(doc.text(), "- one\n\n- two");
        doc.undo();
        doc.set_selection(Selection { anchor: 3, head: 9 }).unwrap();
        doc.enter();
        assert_eq!(doc.text(), "- o\nwo");
    }

    #[test]
    fn new_lines_follow_local_endings_without_normalizing_other_lines() {
        let mut doc = Document::new("- one\r\n- two\nlast");
        doc.set_caret(5).unwrap();
        doc.enter();
        assert_eq!(doc.text(), "- one\r\n- \r\n- two\nlast");
        enter_at_end("- one\r\n- two", "- one\r\n- two\r\n- ");
        enter_at_end("- one\r- two", "- one\r- two\r- ");
        enter_at_end("\u{feff}- first", "\u{feff}- first\n- ");
    }

    #[test]
    fn enter_inside_marker_and_outside_list_is_literal() {
        let mut doc = Document::new("- [ ] task");
        doc.set_caret(3).unwrap();
        doc.enter();
        assert_eq!(doc.text(), "- [\n ] task");
        enter_at_end("ordinary", "ordinary\n");
        enter_at_end("999999999. last", "999999999. last\n");
        enter_at_end("not a list - words", "not a list - words\n");
    }

    #[test]
    fn repeated_enter_does_not_resurrect_a_list_on_blank_lines() {
        let mut doc = Document::new("- item");
        doc.set_caret(doc.text().len()).unwrap();
        doc.enter();
        assert_eq!(doc.text(), "- item\n- ");
        doc.enter();
        // Whether exit adds a separator or leaves this empty line, another
        // Enter must add only a literal newline rather than another marker.
        let exited = doc.text().to_owned();
        doc.enter();
        assert_eq!(doc.text(), format!("{exited}\n"));
        doc.enter();
        doc.enter();
        assert_eq!(doc.text(), format!("{exited}\n\n\n"));
    }

    #[test]
    fn prose_typed_immediately_after_exiting_stays_outside_the_list() {
        let mut doc = Document::new("- item");
        doc.set_caret(doc.text().len()).unwrap();
        doc.enter();
        doc.enter();
        doc.insert("ordinary prose");
        let prose = doc.text().find("ordinary prose").unwrap();
        assert!(
            !doc.markdown()
                .blocks
                .iter()
                .any(|block| { block.kind == BlockKind::ListItem && block.range.contains(&prose) })
        );
        let before = doc.text().to_owned();
        doc.enter();
        assert_eq!(doc.text(), format!("{before}\n"));
    }

    /// Exercise the actual command stream, then undo/redo every transition.
    fn assert_enter_sequence(source: &str, states: &[String]) -> Document {
        let mut doc = Document::new(source);
        doc.set_caret(source.len()).unwrap();
        for (step, expected) in states.iter().enumerate() {
            assert!(doc.enter());
            assert_eq!(
                doc.text(),
                expected,
                "source {source:?}, Enter {}",
                step + 1
            );
            assert_eq!(doc.selection(), Selection::caret(expected.len()));
        }
        for expected in std::iter::once(source)
            .chain(states.iter().map(String::as_str))
            .rev()
            .skip(1)
        {
            assert!(doc.undo());
            assert_eq!(doc.text(), expected);
            assert_eq!(doc.selection(), Selection::caret(expected.len()));
        }
        assert!(!doc.can_undo());
        assert!(!doc.is_dirty());
        for expected in states {
            assert!(doc.redo());
            assert_eq!(doc.text(), expected);
            assert_eq!(doc.selection(), Selection::caret(expected.len()));
        }
        assert!(!doc.can_redo());
        doc
    }

    fn assert_later_prose_is_outside_lists(mut doc: Document) {
        let before = doc.text().to_owned();
        doc.insert("ordinary prose");
        let prose = before.len();
        assert!(
            !doc.markdown()
                .blocks
                .iter()
                .any(|block| { block.kind == BlockKind::ListItem && block.range.contains(&prose) }),
            "prose is still in a list: {:?}",
            doc.text()
        );
        let with_prose = doc.text().to_owned();
        // Context must be encoded in source, not in transient edit history.
        let mut reopened = Document::new(&with_prose);
        reopened.set_caret(with_prose.len()).unwrap();
        let newline = preferred_newline(&with_prose, with_prose.len());
        reopened.enter();
        assert_eq!(reopened.text(), format!("{with_prose}{newline}"));
        doc.undo();
        assert_eq!(doc.text(), before);
        doc.redo();
        doc.enter();
        assert_eq!(doc.text(), reopened.text());
    }

    #[test]
    fn five_enters_leave_all_top_level_list_styles_in_prose() {
        for newline in ["\n", "\r\n"] {
            for (marker, continuation) in [
                ("- ", "- "),
                ("* ", "* "),
                ("+ ", "+ "),
                ("3. ", "4. "),
                ("3) ", "4) "),
                ("- [ ] ", "- [ ] "),
                ("- [x] ", "- [ ] "),
                ("* [X] ", "* [ ] "),
                ("3. [x] ", "4. [ ] "),
                ("3) [ ] ", "4) [ ] "),
            ] {
                let source = format!("Intro{newline}{newline}{marker}item");
                let mut states = vec![format!("{source}{newline}{continuation}")];
                states.extend((2..=5).map(|count| format!("{source}{}", newline.repeat(count))));
                assert_later_prose_is_outside_lists(assert_enter_sequence(&source, &states));
                // Also type immediately after exit, before any extra blank Enter.
                assert_later_prose_is_outside_lists(assert_enter_sequence(&source, &states[..2]));
            }
        }
    }

    #[test]
    fn nested_sequences_exit_one_level_at_a_time_then_stay_outside() {
        for newline in ["\n", "\r\n"] {
            for (lines, suffixes) in [
                (
                    ["- outer", "  * middle", "    + leaf"],
                    ["    + ", "  * ", "- "],
                ),
                (
                    ["- outer", "  - middle", "    - leaf"],
                    ["    - ", "  - ", "- "],
                ),
            ] {
                let source = lines.join(newline);
                let mut states = suffixes
                    .map(|suffix| format!("{source}{newline}{suffix}"))
                    .to_vec();
                states.push(format!("{source}{newline}{newline}"));
                states.push(format!("{source}{newline}{newline}{newline}"));
                assert_later_prose_is_outside_lists(assert_enter_sequence(&source, &states));
                assert_later_prose_is_outside_lists(assert_enter_sequence(&source, &states[..4]));
            }
            let source = format!("1. outer{newline}   - [x] child");
            let states = [
                format!("{source}{newline}   - [ ] "),
                format!("{source}{newline}2. "),
                format!("{source}{newline}{newline}"),
                format!("{source}{newline}{newline}{newline}"),
                format!("{source}{newline}{newline}{newline}{newline}"),
            ];
            assert_later_prose_is_outside_lists(assert_enter_sequence(&source, &states));
        }
    }

    #[test]
    fn quoted_sequences_retain_the_quote_when_exiting_the_list() {
        for newline in ["\n", "\r\n"] {
            for quote in ["> ", "> > "] {
                let source = format!("{quote}- outer{newline}{quote}  * [x] child");
                let exited = format!("{source}{newline}{quote}{newline}{quote}");
                let states = [
                    format!("{source}{newline}{quote}  * [ ] "),
                    format!("{source}{newline}{quote}- "),
                    exited.clone(),
                    format!("{exited}{newline}"),
                    format!("{exited}{newline}{newline}"),
                ];
                assert_enter_sequence(&source, &states);
                let mut doc = assert_enter_sequence(&source, &states[..3]);
                let prose = doc.text().len();
                doc.insert("ordinary prose");
                assert!(doc.markdown().blocks.iter().any(|block| {
                    block.kind == BlockKind::Quote && block.range.contains(&prose)
                }));
                doc.undo();
                assert_later_prose_is_outside_lists(doc);
            }
        }
    }

    #[test]
    fn blank_lines_between_items_do_not_inherit_enclosing_or_previous_items() {
        for newline in ["\n", "\r\n"] {
            for (before, blank, after) in [
                ("- first", "", "- second"),
                ("- first", "  ", "- second"),
                ("- parent", "", "  continuation"),
                ("- parent", "  ", "  - child"),
                ("> - first", "> ", "> - second"),
            ] {
                let source = format!("{before}{newline}{blank}{newline}{after}");
                let mut doc = Document::new(&source);
                let caret = before.len() + newline.len() + blank.len();
                doc.set_caret(caret).unwrap();
                for count in 1..=5 {
                    doc.enter();
                    assert_eq!(
                        doc.text(),
                        format!(
                            "{before}{newline}{blank}{}{newline}{after}",
                            newline.repeat(count)
                        )
                    );
                    assert_eq!(
                        doc.selection(),
                        Selection::caret(caret + count * newline.len())
                    );
                }
                for _ in 0..5 {
                    doc.undo();
                }
                assert_eq!(doc.text(), source);
                assert_eq!(doc.selection(), Selection::caret(caret));
            }
        }
    }

    #[test]
    fn exit_reuses_existing_separator_and_preserves_following_items() {
        for newline in ["\n", "\r\n"] {
            let source = format!("- first{newline}{newline}- ");
            let states: Vec<_> = (2..=6)
                .map(|count| format!("- first{}", newline.repeat(count)))
                .collect();
            assert_later_prose_is_outside_lists(assert_enter_sequence(&source, &states));

            let source = format!("- first{newline}- {newline}- next");
            let mut doc = Document::new(source);
            doc.set_caret("- first".len() + newline.len() + 2).unwrap();
            doc.enter();
            doc.insert("prose");
            assert_eq!(
                doc.text(),
                format!("- first{newline}{newline}prose{newline}- next")
            );
            let prose = doc.text().find("prose").unwrap();
            assert!(
                !doc.markdown()
                    .blocks
                    .iter()
                    .any(|block| block.kind == BlockKind::ListItem && block.range.contains(&prose))
            );
        }
    }

    #[test]
    fn nonempty_multiline_items_still_continue_and_code_stays_literal() {
        for newline in ["\n", "\r\n"] {
            for continuation in ["  continued text", "lazy continuation"] {
                let source = format!("- first{newline}{continuation}");
                let states = [
                    format!("{source}{newline}- "),
                    format!("{source}{newline}{newline}"),
                    format!("{source}{newline}{newline}{newline}"),
                    format!("{source}{newline}{newline}{newline}{newline}"),
                    format!("{source}{newline}{newline}{newline}{newline}{newline}"),
                ];
                assert_later_prose_is_outside_lists(assert_enter_sequence(&source, &states));
            }
            for source in [
                format!("```md{newline}- [x] literal"),
                format!("- parent{newline}{newline}      - literal"),
            ] {
                let states: Vec<_> = (1..=5)
                    .map(|count| format!("{source}{}", newline.repeat(count)))
                    .collect();
                assert_enter_sequence(&source, &states);
            }
        }
    }
}
