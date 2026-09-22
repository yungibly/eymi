//! Physical-line rearrangement. Separators belong to source rows, not contents.
use std::ops::Range;

use crate::{Document, Selection, markdown};

struct Line<'a> {
    content: Range<usize>,
    ending: &'a str,
}

fn lines(source: &str) -> Vec<Line<'_>> {
    let mut result = Vec::new();
    let mut start = markdown::bom_len(source);
    let mut cursor = start;
    let bytes = source.as_bytes();
    while cursor < bytes.len() {
        if matches!(bytes[cursor], b'\r' | b'\n') {
            let end = cursor;
            cursor += 1;
            if bytes[end] == b'\r' && bytes.get(cursor) == Some(&b'\n') {
                cursor += 1;
            }
            result.push(Line {
                content: start..end,
                ending: &source[end..cursor],
            });
            start = cursor;
        } else {
            cursor += 1;
        }
    }
    // The last entry is the editable empty row when the file ends in a newline.
    result.push(Line {
        content: start..source.len(),
        ending: "",
    });
    result
}

fn line_breaks(bytes: impl Iterator<Item = u8>) -> usize {
    let mut previous = 0;
    let mut count = 0;
    for byte in bytes {
        if byte == b'\r' || (byte == b'\n' && previous != b'\r') {
            count += 1;
        }
        previous = byte;
    }
    count
}

/// Move or duplicate the full lines touched by the source selection. A selection
/// ending at the next line's start excludes that line. Moves retain each row's
/// existing separator, including an absent final newline. Duplication retains
/// copied separators and supplies a missing separator using the local style.
/// The BOM stays at byte zero; selection/caret follows the moved or copied text.
/// Edge moves, identical-content moves, and transformations that would coalesce
/// a standalone CR and LF into one separator leave text and selection unchanged.
/// The virtual row after a final newline can be duplicated but cannot be moved.
pub(crate) fn rearrange(document: &mut Document, up: bool, duplicate: bool) -> bool {
    document.break_undo_group();
    let source = document.text();
    let selection = document.selection();
    let range = selection.range();
    if !selection.is_empty() && range.end <= markdown::bom_len(source) {
        return false;
    }
    let rows = lines(source);
    let first = rows
        .partition_point(|line| line.content.start <= range.start)
        .saturating_sub(1);
    let last = if selection.is_empty() {
        first
    } else {
        rows.partition_point(|line| line.content.start < range.end)
            .saturating_sub(1)
    };
    let final_empty = rows.len() > 1 && rows.last().unwrap().content.is_empty();
    let movable = rows.len() - usize::from(final_empty);
    let (affected, order, prefix) = if duplicate {
        // A blank, unterminated document has no line to copy without changing
        // whether the document has a final newline.
        if source.len() == markdown::bom_len(source) {
            return false;
        }
        let after = rows[last].content.end + rows[last].ending.len();
        let position = if up { rows[first].content.start } else { after };
        let prefix = if !up && rows[last].ending.is_empty() {
            super::preferred_newline(source, rows[last].content.start)
        } else {
            ""
        };
        (
            position..position,
            (first..=last).collect::<Vec<_>>(),
            prefix,
        )
    } else {
        if last >= movable || (up && first == 0) || (!up && last + 1 >= movable) {
            return false;
        }
        let (begin, end) = if up {
            (first - 1, last)
        } else {
            (first, last + 1)
        };
        let mut order: Vec<_> = (begin..=end).collect();
        if up {
            order.rotate_left(1);
        } else {
            order.rotate_right(1);
        }
        (
            rows[begin].content.start..rows[end].content.end + rows[end].ending.len(),
            order,
            "",
        )
    };
    let mut replacement = String::from(prefix);
    let mut destinations = Vec::with_capacity(last - first + 1);
    for (slot, &index) in order.iter().enumerate() {
        let line = &rows[index];
        let start = affected.start + replacement.len();
        replacement.push_str(&source[line.content.clone()]);
        let ending = if duplicate {
            if index == last && up && line.ending.is_empty() {
                super::preferred_newline(source, line.content.start)
            } else {
                line.ending
            }
        } else {
            let begin = if up { first - 1 } else { first };
            rows[begin + slot].ending
        };
        replacement.push_str(ending);
        if (first..=last).contains(&index) {
            destinations.push((start, affected.start + replacement.len()));
        }
    }
    if source[affected.clone()] == replacement {
        return false;
    }
    let result_bytes = || {
        source[..affected.start]
            .bytes()
            .chain(replacement.bytes())
            .chain(source[affected.end..].bytes())
    };
    // Empty content moved into the last unterminated row would manufacture a
    // final newline. An interior U+FEFF moved to byte zero would become a BOM.
    // Preserve both source properties rather than changing their meaning.
    if source.ends_with(['\r', '\n'])
        != result_bytes()
            .last()
            .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
        || (markdown::bom_len(source) == 0 && result_bytes().take(3).eq([0xef, 0xbb, 0xbf]))
    {
        return false;
    }
    let expected_breaks = rows.len() - 1 + if duplicate { last - first + 1 } else { 0 };
    if line_breaks(result_bytes()) != expected_breaks {
        return false;
    }
    let map = |offset: usize| {
        // This endpoint belongs to the selected block, even though its byte
        // offset is also the beginning of the following, excluded row.
        if !selection.is_empty() && last + 1 < rows.len() && offset == rows[last + 1].content.start
        {
            return destinations.last().unwrap().1;
        }
        let index = rows
            .partition_point(|line| line.content.start <= offset)
            .saturating_sub(1)
            .clamp(first, last);
        let start = destinations[index - first].0;
        if offset == 0 && start == markdown::bom_len(source) {
            0
        } else {
            start + offset.saturating_sub(rows[index].content.start)
        }
    };
    let after = Selection {
        anchor: map(selection.anchor),
        head: map(selection.head),
    };
    document
        .replace_with_selection(affected, &replacement, after)
        .expect("complete source lines have grapheme boundaries")
}
