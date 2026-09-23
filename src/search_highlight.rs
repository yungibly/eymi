//! Search decoration for visible, source-mapped glyphs; no layout or edits.
use std::ops::Range;

use crate::theme::palette;
use ratatui::style::{Modifier, Style};

/// Apply search/selection styling to one visible glyph's source span.
///
/// `matches` must contain sorted, nonoverlapping, nonempty ranges from the
/// current source revision. The caller passes an empty slice and no active
/// range while the search panel is closed; ordinary source selection remains.
/// Concealed syntax has no visible glyph to pass here and stays concealed.
///
/// Precedence: a current active match gets a bright background plus bold and
/// underline; any other selected text takes the theme's selection surface;
/// inactive matches get a quiet background; everything else keeps base styling.
/// Match colors override foreground/background to keep contrast independent of
/// Markdown token colors. Other Markdown modifiers survive, except DIM and
/// selection reversal are removed from search highlights for readability.
/// Empty source spans (inserted decorations) never acquire match highlighting.
/// Each glyph uses binary lookups, including validation of the active hint.
pub fn style_match(
    base: Style,
    source: &Range<usize>,
    selected: bool,
    matches: &[Range<usize>],
    active: Option<&Range<usize>>,
) -> Style {
    // Projection may have cached selection reversal from an earlier frame.
    let base = base.remove_modifier(Modifier::REVERSED);
    let first = matches.partition_point(|range| range.end <= source.start);
    let matched = matches
        .get(first)
        .is_some_and(|range| overlaps(source, range));
    let active_match = matched
        && active.is_some_and(|active| {
            overlaps(source, active)
                && matches
                    .binary_search_by_key(&active.start, |range| range.start)
                    .ok()
                    .is_some_and(|index| matches[index] == *active)
        });
    if active_match {
        return base
            .fg(palette().search_active_text)
            .bg(palette().search_active)
            .remove_modifier(Modifier::REVERSED | Modifier::DIM)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    }
    if selected {
        return crate::theme::selected(base);
    }
    if matched {
        return base
            .fg(palette().search_text)
            .bg(palette().search)
            .remove_modifier(Modifier::REVERSED | Modifier::DIM);
    }
    base.remove_modifier(Modifier::REVERSED)
}

fn overlaps(left: &Range<usize>, right: &Range<usize>) -> bool {
    !left.is_empty() && !right.is_empty() && left.start < right.end && right.start < left.end
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn every_visible_occurrence_gets_a_quiet_highlight_without_boundary_bleed() {
        let base = Style::default();
        let matches = [2..5, 9..12, 20..23];
        let highlighted = style_match(base, &(2..3), false, &matches, None);
        assert!(highlighted.bg.is_some());
        assert!(!highlighted.add_modifier.contains(Modifier::DIM));
        for span in [3..5, 9..10, 11..12, 20..23] {
            assert_eq!(style_match(base, &span, false, &matches, None), highlighted);
        }
        for span in [0..2, 5..9, 12..20, 23..25] {
            assert_eq!(
                style_match(base, &span, false, &matches, None),
                base.remove_modifier(Modifier::REVERSED)
            );
        }
    }

    #[test]
    fn active_result_is_distinct_in_both_color_and_text_attributes() {
        let matches = [2..5, 9..12];
        let base = Style::default().add_modifier(Modifier::DIM);
        let inactive = style_match(base, &(2..5), false, &matches, Some(&matches[1]));
        let active = style_match(base, &(9..12), true, &matches, Some(&matches[1]));
        assert_ne!(inactive.bg, active.bg);
        assert_ne!(inactive.fg, active.fg);
        assert!(
            active
                .add_modifier
                .contains(Modifier::BOLD | Modifier::UNDERLINED)
        );
        assert!(
            !inactive
                .add_modifier
                .intersects(Modifier::BOLD | Modifier::UNDERLINED)
        );
        assert!(
            !active
                .add_modifier
                .intersects(Modifier::DIM | Modifier::REVERSED)
        );
        assert!(!inactive.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn search_keeps_markdown_modifiers_and_unrelated_base_styles() {
        let markdown =
            Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED | Modifier::CROSSED_OUT;
        let base = Style::default()
            .fg(Color::Cyan)
            .bg(Color::Blue)
            .add_modifier(markdown);
        let matches = [4..8, 12..16];
        for (source, active) in [(&(4..8), None), (&(12..16), Some(&matches[1]))] {
            let style = style_match(base, source, false, &matches, active);
            assert!(style.add_modifier.contains(markdown));
        }
        assert_eq!(
            style_match(base, &(0..3), false, &matches, Some(&matches[1])),
            base.remove_modifier(Modifier::REVERSED)
        );
    }

    #[test]
    fn ordinary_selection_takes_precedence_over_inactive_matches() {
        let base = Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::ITALIC);
        let matches = [2..5, 9..12];
        let selection = crate::theme::selected(base);
        assert_eq!(
            style_match(base, &(0..2), true, &matches, Some(&matches[1])),
            selection
        );
        assert_eq!(
            style_match(base, &(2..5), true, &matches, Some(&matches[1])),
            selection
        );
        assert_ne!(
            style_match(base, &(9..12), true, &matches, Some(&matches[1])),
            selection
        );
    }

    #[test]
    fn closing_search_removes_highlights_but_keeps_source_selection() {
        let base = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let active = 2..5;
        assert_eq!(
            style_match(base, &active, true, &[], None),
            crate::theme::selected(base)
        );
        assert_eq!(
            style_match(base, &active, false, &[], None),
            base.remove_modifier(Modifier::REVERSED)
        );
        // An old active hint cannot keep a highlight when matches were cleared.
        assert_eq!(
            style_match(base, &active, true, &[], Some(&active)),
            crate::theme::selected(base)
        );
    }

    #[test]
    fn active_hint_must_be_an_exact_current_match() {
        let base = Style::default();
        let matches = [3..7, 10..14];
        let inactive = style_match(base, &(3..7), false, &matches, None);
        assert_eq!(
            style_match(base, &(3..7), false, &matches, Some(&(3..6))),
            inactive
        );
        assert_eq!(
            style_match(base, &(3..7), false, &matches, Some(&(2..7))),
            inactive
        );
        // A broad mapped glyph can overlap more than one match, including an
        // active match beyond the first binary-search result.
        let broad = style_match(base, &(0..20), false, &matches, Some(&matches[1]));
        assert_eq!(
            broad,
            style_match(base, &(10..14), false, &matches, Some(&matches[1]))
        );
    }

    #[test]
    fn unicode_and_wrapped_pieces_use_source_spans_without_splitting_glyphs() {
        let doc = eymi::Document::new("e\u{301}界 👩‍💻 e\u{301}界");
        let matches = doc.find_matches("e\u{301}界");
        assert_eq!(matches.len(), 2);
        let base = Style::default();
        let inactive = style_match(base, &matches[0], false, &matches, Some(&matches[1]));
        let active = style_match(base, &matches[1], false, &matches, Some(&matches[1]));
        // Combining e and wide CJK glyph may land on different visual rows.
        for (range, expected) in [(&matches[0], inactive), (&matches[1], active)] {
            for piece in [range.start..range.start + 3, range.start + 3..range.end] {
                assert_eq!(
                    style_match(base, &piece, false, &matches, Some(&matches[1])),
                    expected
                );
            }
        }
        let emoji = doc.find_matches("👩‍💻");
        assert_eq!(
            style_match(base, &emoji[0], false, &matches, Some(&matches[1])),
            base.remove_modifier(Modifier::REVERSED)
        );
    }

    #[test]
    fn inserted_decorations_and_empty_spans_are_not_search_hits() {
        let base = Style::default();
        let matched_range = 2..8;
        let matches = [matched_range];
        for span in [2..2, 4..4, 8..8] {
            assert_eq!(
                style_match(base, &span, false, &matches, Some(&matches[0])),
                base.remove_modifier(Modifier::REVERSED)
            );
        }
    }

    #[test]
    fn cached_selection_reversal_is_recomputed_for_each_visible_span() {
        let base = Style::default().add_modifier(Modifier::REVERSED | Modifier::ITALIC);
        let matches = [2..5, 9..12];
        for (source, active) in [
            (&(0..2), None),
            (&(2..5), None),
            (&(9..12), Some(&matches[1])),
        ] {
            let style = style_match(base, source, false, &matches, active);
            assert!(!style.add_modifier.contains(Modifier::REVERSED));
            assert!(style.add_modifier.contains(Modifier::ITALIC));
        }
        let selection = style_match(base, &(0..2), true, &matches, None);
        assert!(!selection.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(selection.bg, Some(crate::theme::palette().selection));
    }
}
