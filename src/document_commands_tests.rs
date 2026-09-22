use super::*;
use crate::InlineStyle;

fn assert_valid(document: &Document) {
    assert!(document.is_grapheme_boundary(document.selection().anchor));
    assert!(document.is_grapheme_boundary(document.selection().head));
    assert!(document.undo.len() + document.redo.len() <= document.history_limits.max_entries);
    assert!(document.history_bytes() <= document.history_limits.max_bytes);
}

#[test]
fn word_movement_has_unicode_punctuation_and_whitespace_stops() {
    let source = "cafe\u{301},  can't\r\n👩🏽‍💻 世界";
    let mut doc = Document::new(source);
    let mut previous = 0;
    for expected in ["cafe\u{301}", ",", "  can't", "\r\n👩🏽‍💻", " 世", "界"] {
        doc.move_word_right(false);
        let end = doc.selection().head;
        assert_eq!(&source[previous..end], expected);
        previous = end;
        assert_valid(&doc);
    }
    assert_eq!(previous, source.len());
    doc.move_word_right(false);
    assert_eq!(doc.selection().head, source.len());
    let mut last = source.len();
    while doc.selection().head > 0 {
        doc.move_word_left(false);
        assert!(doc.selection().head < last);
        last = doc.selection().head;
        assert_valid(&doc);
    }
    doc.move_word_left(false);
    assert_eq!(doc.revision(), 0);
    assert!(!doc.is_dirty());
}

#[test]
fn word_extension_and_collapse_preserve_direction() {
    let mut doc = Document::new("one two three");
    doc.set_caret(7).unwrap();
    doc.move_word_left(true);
    assert_eq!(doc.selection(), Selection { anchor: 7, head: 4 });
    doc.move_word_left(true);
    assert_eq!(doc.selection(), Selection { anchor: 7, head: 0 });
    doc.move_word_right(false);
    assert_eq!(doc.selection(), Selection::caret(7));
    doc.move_word_right(true);
    assert_eq!(
        doc.selection(),
        Selection {
            anchor: 7,
            head: 13
        }
    );
    doc.move_word_left(false);
    assert_eq!(doc.selection(), Selection::caret(7));
}

#[test]
fn word_deletion_is_one_transaction_including_whitespace_and_selections() {
    let mut doc = Document::new("e\u{301} 👩🏽‍💻\r\nlast");
    doc.set_caret("e\u{301} 👩🏽‍💻\r\n".len()).unwrap();
    let before = doc.selection();
    assert!(doc.delete_word_backward());
    assert_eq!(doc.text(), "e\u{301} last");
    assert!(doc.undo());
    assert_eq!(doc.selection(), before);
    assert!(!doc.can_undo());
    doc.set_caret(0).unwrap();
    assert!(doc.delete_word_forward());
    assert_eq!(doc.text(), " 👩🏽‍💻\r\nlast");
    doc.undo();
    doc.set_selection(Selection {
        anchor: doc.text().len(),
        head: 0,
    })
    .unwrap();
    assert!(doc.delete_word_backward());
    assert_eq!(doc.text(), "");
    doc.undo();
    assert_eq!(
        doc.selection(),
        Selection {
            anchor: doc.text().len(),
            head: 0
        }
    );
}

#[test]
fn indentation_preserves_mixed_newlines_bom_and_direction() {
    let original = "\u{feff}alpha\r\nbeta\ngamma\rdelta";
    for reversed in [false, true] {
        let mut doc = Document::new(original);
        let end = original.find("delta").unwrap();
        let selection = if reversed {
            Selection {
                anchor: end,
                head: 3,
            }
        } else {
            Selection {
                anchor: 3,
                head: end,
            }
        };
        doc.set_selection(selection).unwrap();
        doc.indent_lines();
        assert_eq!(
            doc.text(),
            "\u{feff}    alpha\r\n    beta\n    gamma\rdelta"
        );
        assert_eq!(doc.selection().range(), 7..end + 12);
        assert_eq!(doc.selection().anchor > doc.selection().head, reversed);
        doc.outdent_lines();
        assert_eq!(doc.text(), original);
        assert_eq!(doc.selection(), selection);
        doc.undo();
        doc.undo();
        assert_eq!(doc.selection(), selection);
        assert_eq!(doc.text(), original);
        assert!(!doc.can_undo());
    }
}

#[test]
fn indentation_handles_empty_final_and_tabbed_lines() {
    for (original, offset, expected, after) in [
        ("", 0, "    ", 4),
        ("\r\n", 2, "\r\n    ", 6),
        ("abc", 1, "    abc", 5),
        ("\u{feff}abc", 3, "\u{feff}    abc", 7),
    ] {
        let mut doc = Document::new(original);
        doc.set_caret(offset).unwrap();
        assert!(doc.indent_lines());
        assert_eq!(doc.text(), expected);
        assert_eq!(doc.selection(), Selection::caret(after));
        assert!(doc.outdent_lines());
        assert_eq!(doc.text(), original);
        assert_eq!(doc.selection(), Selection::caret(offset));
    }
    let mut doc = Document::new("\talpha\n  beta\n     gamma\n  \tdelta\n");
    doc.select_all();
    doc.outdent_lines();
    assert_eq!(doc.text(), "alpha\nbeta\n gamma\n\tdelta\n");
    assert_eq!(
        doc.selection(),
        Selection {
            anchor: 0,
            head: doc.text().len()
        }
    );
    let mut doc = Document::new("\u{feff}  a\r\n\r\n");
    doc.select_all();
    doc.indent_lines();
    assert_eq!(doc.text(), "\u{feff}      a\r\n    \r\n");
    assert_eq!(doc.selection().anchor, 0);
}

#[test]
fn outdent_does_not_delete_combining_text_attached_to_whitespace() {
    let mut doc = Document::new("  \u{301}a");
    doc.select_all();
    doc.outdent_lines();
    assert_eq!(doc.text(), " \u{301}a");
    assert_valid(&doc);
    assert!(!doc.outdent_lines());
}

#[test]
fn inline_commands_preserve_selection_direction_and_round_trip() {
    for style in [InlineStyle::Bold, InlineStyle::Italic, InlineStyle::Code] {
        for reversed in [false, true] {
            let mut doc = Document::new("prefix e\u{301}👩🏽‍💻 suffix");
            let selection = Selection {
                anchor: 7,
                head: doc.text().find(" suffix").unwrap(),
            };
            let selection = if reversed {
                Selection {
                    anchor: selection.head,
                    head: selection.anchor,
                }
            } else {
                selection
            };
            let original = doc.text().to_owned();
            doc.set_selection(selection).unwrap();
            assert!(doc.toggle_inline(style));
            assert_eq!(doc.selected_text(), "e\u{301}👩🏽‍💻");
            assert_eq!(doc.selection().anchor > doc.selection().head, reversed);
            assert_valid(&doc);
            let formatted = doc.text().to_owned();
            doc.undo();
            assert_eq!(doc.text(), original);
            assert_eq!(doc.selection(), selection);
            doc.redo();
            assert_eq!(doc.text(), formatted);
            assert!(doc.toggle_inline(style));
            assert_eq!(doc.text(), original);
            assert_eq!(doc.selection(), selection);
        }
    }
}

#[test]
fn collapsed_formatting_inserts_pairs_and_keeps_caret_inside() {
    for (style, expected, offset) in [
        (InlineStyle::Bold, "a****b", 3),
        (InlineStyle::Italic, "a**b", 2),
        (InlineStyle::Code, "a``b", 2),
    ] {
        let mut doc = Document::new("ab");
        doc.set_caret(1).unwrap();
        doc.toggle_inline(style);
        assert_eq!(doc.text(), expected);
        assert_eq!(doc.selection(), Selection::caret(offset));
        doc.toggle_inline(style);
        assert_eq!(doc.text(), "ab");
        assert_eq!(doc.selection(), Selection::caret(1));
    }
}

#[test]
fn formatting_recognizes_selected_markers_and_alternative_emphasis() {
    for (source, style, expected) in [
        ("**bold**", InlineStyle::Bold, "bold"),
        ("__bold__", InlineStyle::Bold, "bold"),
        ("*italic*", InlineStyle::Italic, "italic"),
        ("_italic_", InlineStyle::Italic, "italic"),
        ("``a`b``", InlineStyle::Code, "a`b"),
        ("\u{feff}**bold**", InlineStyle::Bold, "\u{feff}bold"),
    ] {
        let mut doc = Document::new(source);
        doc.select_all();
        doc.toggle_inline(style);
        assert_eq!(doc.text(), expected);
        assert_valid(&doc);
        doc.undo();
        assert_eq!(doc.text(), source);
    }
}

#[test]
fn code_formatting_chooses_safe_fences_and_preserves_content_padding() {
    for source in [
        "a`b", "`edge", "edge`", "`", "``", " padded ", " ", "a``b`c",
    ] {
        let mut doc = Document::new(source);
        doc.select_all();
        doc.toggle_inline(InlineStyle::Code);
        assert_eq!(doc.selected_text(), source);
        let code: Vec<_> = pulldown_cmark::Parser::new(doc.text())
            .filter_map(|event| match event {
                pulldown_cmark::Event::Code(s) => Some(s.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(code, vec![source.to_owned()], "{}", doc.text());
        doc.toggle_inline(InlineStyle::Code);
        assert_eq!(doc.text(), source);
    }
}

#[test]
fn typing_groups_are_explicit_and_whitespace_paste_and_formatting_are_separate() {
    let mut doc = Document::new("");
    for value in ["c", "a", "f", "e", "\u{301}"] {
        doc.type_text(value);
    }
    assert_eq!(doc.text(), "cafe\u{301}");
    doc.type_text(" ");
    doc.type_text("👩");
    doc.type_text("🏽");
    doc.type_text("‍💻");
    assert_valid(&doc);
    doc.insert("\r\nliteral paste");
    doc.toggle_inline(InlineStyle::Bold);
    for expected in [
        "cafe\u{301} 👩🏽‍💻\r\nliteral paste",
        "cafe\u{301} 👩🏽‍💻",
        "cafe\u{301} ",
        "cafe\u{301}",
        "",
    ] {
        assert!(doc.undo());
        assert_eq!(doc.text(), expected);
    }
    assert!(!doc.can_undo());
    for _ in 0..5 {
        assert!(doc.redo());
    }
    assert_eq!(doc.text(), "cafe\u{301} 👩🏽‍💻\r\nliteral paste****");
}

#[test]
fn no_op_navigation_commands_and_save_end_typing_groups() {
    let boundaries: [fn(&mut Document); 11] = [
        |d| d.move_right(false),
        |d| d.move_word_right(false),
        |d| {
            d.set_caret(d.text().len()).unwrap();
        },
        |d| d.mark_saved(),
        |d| d.break_undo_group(),
        |d| {
            d.delete_forward();
        },
        |d| {
            d.outdent_lines();
        },
        |d| {
            d.replace_all("missing", "other");
        },
        |d| {
            d.toggle_task_at_caret();
        },
        |d| {
            d.insert("");
        },
        |d| {
            d.redo();
        },
    ];
    for boundary in boundaries {
        let mut doc = Document::new("");
        doc.type_text("a");
        boundary(&mut doc);
        doc.type_text("b");
        assert!(doc.undo());
        assert_eq!(doc.text(), "a");
        assert!(doc.undo());
        assert_eq!(doc.text(), "");
    }
}

#[test]
fn saved_baseline_survives_grouping_trimming_and_redo_branches() {
    let mut doc = Document::new("");
    doc.set_history_limits(HistoryLimits {
        max_entries: 2,
        max_bytes: 1024,
    });
    doc.type_text("a");
    doc.mark_saved();
    doc.type_text("b");
    doc.type_text("c");
    assert!(doc.undo());
    assert!(!doc.is_dirty());
    assert_eq!(doc.text(), "a");
    doc.type_text("x");
    assert!(!doc.can_redo());
    doc.undo();
    assert!(!doc.is_dirty());
    doc.redo();
    assert_eq!(doc.text(), "ax");
    doc.insert("1");
    doc.insert("2");
    assert_eq!(doc.undo.len(), 2);
    doc.undo();
    doc.undo();
    assert_eq!(doc.text(), "ax");
    assert!(!doc.can_undo());
    assert!(doc.is_dirty());
    doc.select_all();
    doc.insert("a");
    assert!(!doc.is_dirty());
}

#[test]
fn history_byte_and_count_limits_apply_to_both_stacks_without_changing_content() {
    let mut doc = Document::new("saved");
    doc.set_history_limits(HistoryLimits {
        max_entries: 3,
        max_bytes: 400,
    });
    for text in ["alpha", "beta", "gamma", "delta"] {
        doc.insert(text);
        assert_valid(&doc);
    }
    let current = doc.text().to_owned();
    let selection = doc.selection();
    while doc.undo() {
        assert_valid(&doc);
    }
    while doc.redo() {
        assert_valid(&doc);
    }
    assert_eq!(doc.text(), current);
    assert_eq!(doc.selection(), selection);
    doc.set_history_limits(HistoryLimits {
        max_entries: 0,
        max_bytes: 0,
    });
    assert_eq!(doc.text(), current);
    assert_eq!(doc.saved_text, "saved");
    assert!(!doc.can_undo());
    assert!(!doc.can_redo());
    doc.set_caret(doc.text().len()).unwrap();
    doc.insert("x");
    assert_valid(&doc);
    assert_eq!(doc.text(), format!("{current}x"));
    assert!(!doc.undo());
}

#[test]
fn oversized_snapshot_cannot_leave_a_skipped_undo_or_redo_step() {
    let mut doc = Document::new("a");
    doc.set_history_limits(HistoryLimits {
        max_entries: 20,
        max_bytes: 200,
    });
    doc.insert("b");
    doc.insert(&"x".repeat(500));
    assert!(doc.undo());
    assert_eq!(doc.text(), "ba");
    assert!(!doc.can_redo());
    assert!(!doc.can_undo());
    assert_eq!(doc.saved_text, "a");
    assert_valid(&doc);
}

#[test]
fn selection_replacement_typing_is_standalone_and_restores_direction() {
    let mut doc = Document::new("original");
    let selection = Selection { anchor: 8, head: 0 };
    doc.set_selection(selection).unwrap();
    doc.type_text("a");
    doc.type_text("b");
    doc.undo();
    assert_eq!(doc.text(), "a");
    doc.undo();
    assert_eq!(doc.text(), "original");
    assert_eq!(doc.selection(), selection);
}

#[test]
fn core_commands_round_trip_unicode_selection_corpus() {
    for source in [
        "\u{feff} a\r\ne\u{301}\r👩🏽‍💻\n",
        "🇺x🇸 *a* `b`",
        " \u{301}a\n\t界",
    ] {
        let boundaries: Vec<_> = source
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain(std::iter::once(source.len()))
            .collect();
        for &anchor in &boundaries {
            for &head in &boundaries {
                for command in 0..7 {
                    let mut doc = Document::new(source);
                    let selection = Selection { anchor, head };
                    doc.set_selection(selection).unwrap();
                    let changed = match command {
                        0 => doc.indent_lines(),
                        1 => doc.outdent_lines(),
                        2 => doc.delete_word_backward(),
                        3 => doc.delete_word_forward(),
                        4 => doc.toggle_inline(InlineStyle::Bold),
                        5 => doc.toggle_inline(InlineStyle::Italic),
                        _ => doc.toggle_inline(InlineStyle::Code),
                    };
                    assert_valid(&doc);
                    if changed {
                        let after = (doc.text().to_owned(), doc.selection());
                        assert!(doc.undo());
                        assert_eq!(doc.text(), source);
                        assert_eq!(doc.selection(), selection);
                        assert!(!doc.is_dirty());
                        assert!(!doc.can_undo());
                        assert!(doc.redo());
                        assert_eq!((doc.text(), doc.selection()), (after.0.as_str(), after.1));
                    } else {
                        assert_eq!(doc.text(), source);
                        assert!(!doc.can_undo());
                    }
                }
            }
        }
    }
}

#[test]
fn emphasis_keeps_boundary_whitespace_outside_real_markup() {
    for (style, expected) in [
        (InlineStyle::Bold, " \r\n**word** \t"),
        (InlineStyle::Italic, " \r\n*word* \t"),
    ] {
        let mut doc = Document::new(" \r\nword \t");
        doc.select_all();
        doc.toggle_inline(style);
        assert_eq!(doc.text(), expected);
        assert_eq!(doc.selected_text(), "word");
        doc.toggle_inline(style);
        assert_eq!(doc.text(), " \r\nword \t");
        doc.select_all();
        doc.insert(" \r\n ");
        doc.select_all();
        assert!(!doc.toggle_inline(style));
        assert_eq!(doc.text(), " \r\n ");
    }
}

#[test]
fn separate_inline_spans_do_not_lose_their_outer_markers() {
    for (source, style, expected) in [
        (
            "`one` and `two`",
            InlineStyle::Code,
            "`` `one` and `two` ``",
        ),
        (
            "**one** and **two**",
            InlineStyle::Bold,
            "****one** and **two****",
        ),
        ("*one* and *two*", InlineStyle::Italic, "**one* and *two**"),
    ] {
        let mut doc = Document::new(source);
        doc.select_all();
        doc.toggle_inline(style);
        assert_eq!(doc.text(), expected);
        doc.undo();
        assert_eq!(doc.text(), source);
    }
}

#[test]
fn line_moves_keep_bom_separator_order_final_newline_and_relative_caret() {
    for (source, offset, up, expected, after) in [
        (
            "one\r\ntwo\nthree\rfour",
            6,
            true,
            "two\r\none\nthree\rfour",
            1,
        ),
        (
            "one\r\ntwo\nthree\rfour",
            6,
            false,
            "one\r\nthree\ntwo\rfour",
            12,
        ),
        ("\u{feff}a\r\nb\nc", 3, false, "\u{feff}b\r\na\nc", 6),
        ("a\r\nlast", 7, true, "last\r\na", 4),
        ("a\nlast\n", 3, true, "last\na\n", 1),
    ] {
        let mut doc = Document::new(source);
        doc.set_caret(offset).unwrap();
        assert!(if up {
            doc.move_lines_up()
        } else {
            doc.move_lines_down()
        });
        assert_eq!(doc.text(), expected);
        assert_eq!(doc.selection(), Selection::caret(after));
        assert_valid(&doc);
        assert!(doc.undo());
        assert_eq!(doc.text(), source);
        assert_eq!(doc.selection(), Selection::caret(offset));
        assert!(!doc.can_undo());
        assert!(!doc.is_dirty());
        assert!(doc.redo());
        assert_eq!(doc.text(), expected);
        assert_eq!(doc.selection(), Selection::caret(after));
    }
}

#[test]
fn line_moves_retain_direction_and_exclude_selection_end_at_next_line_start() {
    let source = "top\r\ne\u{301}\n👩🏽‍💻\rbottom";
    let first = source.find('e').unwrap();
    let end = source.find("bottom").unwrap();
    for up in [true, false] {
        for reversed in [false, true] {
            let before = if reversed {
                Selection {
                    anchor: end,
                    head: first,
                }
            } else {
                Selection {
                    anchor: first,
                    head: end,
                }
            };
            let mut doc = Document::new(source);
            doc.set_selection(before).unwrap();
            assert!(if up {
                doc.move_lines_up()
            } else {
                doc.move_lines_down()
            });
            let expected = if up {
                "e\u{301}\r\n👩🏽‍💻\ntop\rbottom"
            } else {
                "top\r\nbottom\ne\u{301}\r👩🏽‍💻"
            };
            assert_eq!(doc.text(), expected);
            assert_eq!(
                doc.selected_text(),
                if up {
                    "e\u{301}\r\n👩🏽‍💻\n"
                } else {
                    "e\u{301}\r👩🏽‍💻"
                }
            );
            assert_eq!(doc.selection().anchor > doc.selection().head, reversed);
            assert_valid(&doc);
            doc.undo();
            assert_eq!(doc.selection(), before);
            assert_eq!(doc.text(), source);
        }
    }
}

#[test]
fn line_duplicates_follow_new_copy_and_preserve_source_bytes() {
    for (source, offset, up, expected, after) in [
        ("alpha\r\nbeta\nend", 8, true, "alpha\r\nbeta\nbeta\nend", 8),
        (
            "alpha\r\nbeta\nend",
            8,
            false,
            "alpha\r\nbeta\nbeta\nend",
            13,
        ),
        ("\u{feff}é\r\nx", 3, true, "\u{feff}é\r\né\r\nx", 3),
        ("\u{feff}é\r\nx", 3, false, "\u{feff}é\r\né\r\nx", 7),
        ("a\r\nlast", 5, true, "a\r\nlast\r\nlast", 5),
        ("a\r\nlast", 5, false, "a\r\nlast\r\nlast", 11),
        ("a\n", 2, true, "a\n\n", 2),
        ("a\n", 2, false, "a\n\n", 3),
        ("solo", 2, false, "solo\nsolo", 7),
    ] {
        let mut doc = Document::new(source);
        doc.set_caret(offset).unwrap();
        assert!(if up {
            doc.duplicate_lines_up()
        } else {
            doc.duplicate_lines_down()
        });
        assert_eq!(doc.text(), expected);
        assert_eq!(doc.selection(), Selection::caret(after));
        assert_valid(&doc);
        doc.undo();
        assert_eq!(doc.text(), source);
        assert_eq!(doc.selection(), Selection::caret(offset));
        assert!(!doc.can_undo());
        doc.redo();
        assert_eq!(doc.text(), expected);
        assert_eq!(doc.selection(), Selection::caret(after));
    }
}

#[test]
fn line_duplicates_copy_whole_lines_but_keep_partial_reversed_selection() {
    for up in [true, false] {
        let source = "first\r\n  e\u{301}cho\n👩🏽‍💻 last\rend";
        let start = source.find('e').unwrap();
        let end = source.find(" last").unwrap();
        let before = Selection {
            anchor: end,
            head: start,
        };
        let mut doc = Document::new(source);
        doc.set_selection(before).unwrap();
        assert!(if up {
            doc.duplicate_lines_up()
        } else {
            doc.duplicate_lines_down()
        });
        assert_eq!(
            doc.text(),
            "first\r\n  e\u{301}cho\n👩🏽‍💻 last\r  e\u{301}cho\n👩🏽‍💻 last\rend"
        );
        assert_eq!(doc.selected_text(), "e\u{301}cho\n👩🏽‍💻");
        assert!(doc.selection().anchor > doc.selection().head);
        assert_eq!(
            doc.selection().head,
            if up {
                start
            } else {
                start + "  e\u{301}cho\n👩🏽‍💻 last\r".len()
            }
        );
        assert_valid(&doc);
        doc.undo();
        assert_eq!(doc.selection(), before);
        assert_eq!(doc.text(), source);
    }
}

#[test]
fn line_duplicates_exclude_next_row_at_selection_end_and_do_not_copy_bom() {
    for up in [true, false] {
        let source = "\u{feff}one\r\ntwo\nthree";
        let end = source.find("three").unwrap();
        let mut doc = Document::new(source);
        doc.set_selection(Selection {
            anchor: 0,
            head: end,
        })
        .unwrap();
        assert!(if up {
            doc.duplicate_lines_up()
        } else {
            doc.duplicate_lines_down()
        });
        assert_eq!(doc.text(), "\u{feff}one\r\ntwo\none\r\ntwo\nthree");
        assert_eq!(doc.text().matches('\u{feff}').count(), 1);
        assert_eq!(
            doc.selected_text(),
            if up {
                "\u{feff}one\r\ntwo\n"
            } else {
                "one\r\ntwo\n"
            }
        );
        doc.undo();
        assert_eq!(doc.text(), source);
        assert_eq!(
            doc.selection(),
            Selection {
                anchor: 0,
                head: end
            }
        );
    }
}

#[test]
fn line_no_ops_preserve_selection_revision_redo_and_saved_state() {
    for (source, offset, command) in [
        ("", 0, 0),
        ("", 0, 2),
        ("\u{feff}", 0, 3),
        ("first\nlast", 0, 0),
        ("first\nlast", 6, 1),
        ("first\nlast\n", 11, 0),
        ("first\nlast\n", 11, 1),
        ("same\nsame", 1, 1),
        ("a\n\nb", 2, 1),
        ("a\n\u{feff}b", 2, 0),
        // A standalone CR must not merge with the LF from a moved empty row.
        ("a\rb\n\nc", 4, 0),
    ] {
        let mut doc = Document::new(source);
        doc.insert("temporary");
        doc.undo();
        doc.set_caret(offset).unwrap();
        let revision = doc.revision();
        let changed = match command {
            0 => doc.move_lines_up(),
            1 => doc.move_lines_down(),
            2 => doc.duplicate_lines_up(),
            _ => doc.duplicate_lines_down(),
        };
        assert!(!changed, "{source:?}, {offset}, {command}");
        assert_eq!(doc.text(), source);
        assert_eq!(doc.selection(), Selection::caret(offset));
        assert_eq!(doc.revision(), revision);
        assert!(!doc.can_undo());
        assert!(doc.can_redo());
        assert!(!doc.is_dirty());
    }
}

#[test]
fn line_commands_end_typing_groups_and_create_separate_undo_steps() {
    let mut doc = Document::new("first\nlast");
    doc.type_text("one");
    assert!(!doc.move_lines_up());
    doc.type_text("two");
    doc.undo();
    assert_eq!(doc.text(), "onefirst\nlast");
    assert!(doc.move_lines_down());
    doc.type_text("three");
    doc.undo();
    assert_eq!(doc.text(), "last\nonefirst");
    doc.undo();
    assert_eq!(doc.text(), "onefirst\nlast");
    doc.undo();
    assert_eq!(doc.text(), "first\nlast");
    assert!(!doc.can_undo());
}

#[test]
fn line_operations_keep_unicode_boundaries_and_round_trip_every_selection() {
    for source in [
        "",
        "\u{feff}",
        "a\n",
        "\n\n",
        "a\r\nb\rc\n",
        "a\rb\n\nc",
        "\u{feff}e\u{301}\r\n👩🏽‍💻\n🇺🇸",
        "\u{301}\r\n\u{feff}x\ry",
    ] {
        let boundaries: Vec<_> = source
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain(std::iter::once(source.len()))
            .collect();
        for &anchor in &boundaries {
            for &head in &boundaries {
                for command in 0..4 {
                    let mut doc = Document::new(source);
                    let before = Selection { anchor, head };
                    doc.set_selection(before).unwrap();
                    let changed = match command {
                        0 => doc.move_lines_up(),
                        1 => doc.move_lines_down(),
                        2 => doc.duplicate_lines_up(),
                        _ => doc.duplicate_lines_down(),
                    };
                    assert_valid(&doc);
                    assert_eq!(
                        doc.text().starts_with('\u{feff}'),
                        source.starts_with('\u{feff}')
                    );
                    assert_eq!(
                        doc.text().ends_with(['\r', '\n']),
                        source.ends_with(['\r', '\n']),
                        "{source:?}, {before:?}, {command}"
                    );
                    if changed {
                        let after = (doc.text().to_owned(), doc.selection());
                        doc.undo();
                        assert_eq!(doc.text(), source);
                        assert_eq!(doc.selection(), before);
                        assert!(!doc.can_undo());
                        doc.redo();
                        assert_eq!((doc.text(), doc.selection()), (after.0.as_str(), after.1));
                    } else {
                        assert_eq!(doc.text(), source);
                        assert_eq!(doc.selection(), before);
                        assert!(!doc.can_undo());
                    }
                }
            }
        }
    }
}
