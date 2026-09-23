use super::*;
use eymi::Document;
use ratatui::style::Modifier;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
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
    assert!(display(&p).contains("  ◦ child text"));
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

#[test]
fn prose_wraps_whole_words_and_retains_source_whitespace() {
    let text = "alpha beta gamma delta";
    let projected = project(text, 0, 13, true);
    assert_eq!(display(&projected), "alpha beta \ngamma delta");
    assert_eq!(
        projected
            .rows
            .iter()
            .flat_map(|row| &row.glyphs)
            .map(|glyph| glyph.text.as_str())
            .collect::<String>(),
        text
    );
    let boundary = text.find("gamma").unwrap();
    assert_eq!(projected.cursor(boundary), (1, 0));
    assert_eq!(
        projected.cursor_with_affinity(boundary, Affinity::Upstream),
        (0, 11)
    );
    assert_eq!(projected.hit(0, 11).offset, boundary);
    assert_eq!(projected.hit(1, 0).offset, boundary);
    // Markdown source view still wraps prose at words; code and plain-text
    // files preserve their column-based layout.
    assert_eq!(
        display(&project(text, 0, 13, false)),
        "alpha beta \ngamma delta"
    );
    let plain = Parsed::new(text, Document::new(text).markdown(), false);
    let plain = Projection::build(text, &plain, Selection::caret(0), 13, false);
    assert_eq!(display(&plain), "alpha beta g\namma delta");
    let code = format!("```\n{text}\n```\n");
    assert!(display(&project(&code, 0, 13, true)).contains("alpha beta g\namma delta"));
}

#[test]
fn word_wrap_handles_hidden_markers_wide_text_tabs_and_unbroken_words() {
    let text = "intro\n\nalpha **beta** gamma [delta](somewhere) e\u{301}clair 界面\tlast abcdefghijklmnop";
    let boundaries: Vec<_> = text
        .grapheme_indices(true)
        .map(|(offset, _)| offset)
        .chain([text.len()])
        .collect();
    for width in 2..30 {
        for caret in [0, text.find("beta").unwrap(), text.len()] {
            let projected = project(text, caret, width, true);
            for (row_number, row) in projected.rows.iter().enumerate() {
                for col in 0..width + 2 {
                    assert!(boundaries.contains(&projected.hit(row_number, col).offset));
                }
                for glyph in row.glyphs.iter().filter(|g| !g.source.is_empty()) {
                    assert!(boundaries.contains(&glyph.source.start));
                    assert!(boundaries.contains(&glyph.source.end));
                    assert!(glyph.column + glyph.width < width);
                    assert_eq!(
                        projected.cursor(glyph.source.start),
                        (row_number, glyph.column)
                    );
                    // A space collapsed at a soft wrap occupies no cell.
                    if glyph.width > 0 {
                        assert_eq!(
                            projected.hit(row_number, glyph.column).offset,
                            glyph.source.start
                        );
                    }
                }
            }
        }
    }
    let rendered = display(&project("intro\n\nalpha **beta** gamma", 0, 13, true));
    assert!(rendered.contains("alpha beta \ngamma"));
}

fn parsed(text: &str) -> Parsed {
    Parsed::new(text, Document::new(text).markdown(), true)
}

fn row_text(row: &VisualRow) -> String {
    row.glyphs.iter().map(|g| g.text.as_str()).collect()
}

const RICH: &str = "---\ntitle: Notes\n---\n\n# Title\n\nSetext\n======\n\nA **bold** `code` and [link](u) with ~~old~~ text.\n\n- [x] done item\n- [ ] open item that is long enough to wrap in narrow views\n  - nested\n\n1. first\n2. second\n\n> quoted text that also wraps when the window is narrow\n>> nested quote\n\n> [!WARNING]\n> Careful.\n\n```rust\nfn main() {}\n```\n\n    indented code\n\n| a | bb |\n| :- | -: |\n| xyz | 1 |\n| wide cell with several words | 22 |\n\n---\n\nend\n";

#[test]
fn tables_render_as_aligned_grids_that_map_cells_to_source() {
    let text = "intro\n\n| a | bb |\n| :- | -: |\n| xyz | 1 |\n\nafter\n";
    let p = project(text, 0, 80, true);
    let rows: Vec<_> = p.rows.iter().map(row_text).collect();
    let top = rows.iter().position(|r| r == "╭─────┬────╮").unwrap();
    assert_eq!(rows[top + 1], "│ a   │ bb │");
    assert_eq!(rows[top + 2], "├─────┼────┤");
    assert_eq!(rows[top + 3], "│ xyz │  1 │");
    assert_eq!(rows[top + 4], "╰─────┴────╯");
    assert!(!p.rows[top].navigable && !p.rows[top + 4].navigable);
    assert_eq!(p.rows[top + 1].line, Some(3));
    assert_eq!(p.rows[top + 2].line, Some(4));
    assert_eq!(p.rows[top + 3].line, Some(5));
    assert_eq!(p.rows[top + 6].line, Some(7));
    let xyz = text.find("xyz").unwrap();
    let (row, col) = p.cursor(xyz);
    assert_eq!((row, col), (top + 3, 2));
    assert_eq!(p.hit(row, col).offset, xyz);
    // Borders and padding answer with neighbouring source, never a new offset.
    assert_eq!(p.hit(top + 3, 0).offset, text.find("| xyz").unwrap());
    assert_eq!(p.navigable_row(top - 1, 1), top + 1);
    assert_eq!(p.navigable_row(top + 3, 1), top + 5);
    // With the caret inside, the table is ordinary editable source.
    let open = project(text, xyz, 80, true);
    assert!(display(&open).contains("| xyz | 1 |"));
}

#[test]
fn narrow_tables_wrap_cells_inside_fitted_columns_or_stay_source() {
    let text =
        "x\n\n| name | description |\n| --- | --- |\n| a | a long description that cannot fit |\n";
    let p = project(text, 0, 30, true);
    let rows: Vec<_> = p.rows.iter().map(row_text).collect();
    let first = rows.iter().position(|r| r.starts_with("│ a")).unwrap();
    assert!(rows[first + 1].starts_with("│      │"), "{rows:#?}");
    for row in &p.rows {
        for glyph in &row.glyphs {
            assert!(glyph.column + glyph.width < 30, "{rows:#?}");
        }
    }
    let words: String = p
        .rows
        .iter()
        .flat_map(|r| &r.glyphs)
        .filter(|g| !g.source.is_empty())
        .map(|g| g.text.as_str())
        .collect();
    assert!(words.contains("fit"));
    // Too narrow for a grid: source stays visible and editable.
    assert!(display(&project(text, 0, 12, true)).contains("| a |"));
}

#[test]
fn code_surfaces_hide_fences_and_carry_a_language_tab() {
    let text = "intro\n\n```rust\nfn x() {}\n```\n";
    let p = project(text, 0, 40, true);
    let fence = &p.rows[2];
    let fill = fence.fill.as_ref().unwrap();
    assert_eq!(fill.kind, FillKind::Lower);
    assert_eq!(fill.from, -1);
    assert_eq!(fill.label.as_ref().unwrap().0, " rust ");
    assert!(fence.glyphs.is_empty());
    assert_eq!(row_text(&p.rows[3]), "fn x() {}");
    assert_eq!(p.rows[3].fill.as_ref().unwrap().kind, FillKind::Band);
    assert_eq!(p.rows[4].fill.as_ref().unwrap().kind, FillKind::Upper);
    let inside = project(text, text.find("fn").unwrap(), 40, true);
    assert_eq!(row_text(&inside.rows[2]), "```rust");
    assert!(
        inside.rows[2..=4]
            .iter()
            .all(|row| row.fill.as_ref().unwrap().kind == FillKind::Band)
    );
    // Source view shows plain source without surfaces.
    assert!(
        project(text, 0, 40, false)
            .rows
            .iter()
            .all(|r| r.fill.is_none())
    );
}

#[test]
fn headings_carry_level_bands_and_margin_marks() {
    let text = "# One\n\n## Two\n\nTitle\n---\n";
    let p = project(text, text.len(), 40, true);
    let colors = crate::theme::palette();
    for (row, level) in [(0, 1), (2, 2), (4, 2)] {
        let fill = p.rows[row].fill.as_ref().unwrap();
        assert_eq!(fill.kind, FillKind::Band);
        assert_eq!(fill.from, -2);
        assert_eq!(fill.color, colors.heading_bands[level - 1]);
        let (mark, style) = fill.label.as_ref().unwrap();
        assert_eq!(mark, crate::icons::current().heading(level));
        assert_eq!(style.fg, Some(colors.headings[level - 1]));
    }
    assert_eq!(row_text(&p.rows[0]), "One");
    assert_eq!(p.rows[5].fill.as_ref().unwrap().kind, FillKind::Rule);
    assert!(p.rows[5].glyphs.is_empty());
}

#[test]
fn wrapped_items_and_quotes_hang_under_their_content() {
    let text = "- [ ] one two three four five six seven\n\n> alpha beta gamma delta epsilon zeta\n";
    let p = project(text, text.len(), 24, true);
    let item = p.rows.iter().position(|r| r.line == Some(1)).unwrap();
    let continuation = &p.rows[item + 1];
    assert_eq!(continuation.line, None);
    assert!(continuation.glyphs[..2].iter().all(|g| g.source.is_empty()));
    assert_eq!(continuation.glyphs[2].column, 2);
    assert!(!continuation.glyphs[2].source.is_empty());
    let quote = p.rows.iter().position(|r| r.line == Some(3)).unwrap();
    let wrapped = &p.rows[quote + 1];
    assert_eq!(wrapped.glyphs[0].text, parse::RAIL);
    assert!(wrapped.glyphs[0].source.is_empty());
    // Clicking the hanging indent places the caret at the row's first letter.
    assert_eq!(p.hit(item + 1, 0).offset, continuation.start);
    assert_eq!(p.cursor(continuation.start), (item + 1, 2));
}

#[test]
fn alerts_become_titled_callouts_with_colored_rails() {
    let text = "> [!TIP]\n> Try this.\n";
    let p = project(text, text.len(), 40, true);
    assert_eq!(row_text(&p.rows[0]), format!("{} Tip", parse::RAIL));
    assert_eq!(row_text(&p.rows[1]), format!("{} Try this.", parse::RAIL));
    let tip = crate::theme::palette().callouts[1];
    assert_eq!(p.rows[0].glyphs[0].style.fg, Some(tip));
    assert_eq!(p.rows[0].glyphs[2].style.fg, Some(tip));
}

#[test]
fn tasks_replace_their_bullet_and_checked_text_is_struck() {
    let text = "- [x] done\n- [ ] open\n* plain\n";
    let p = project(text, text.len(), 40, true);
    let icons = crate::icons::current();
    assert_eq!(row_text(&p.rows[0]), format!("{} done", icons.task(true)));
    assert_eq!(row_text(&p.rows[1]), format!("{} open", icons.task(false)));
    assert_eq!(row_text(&p.rows[2]), "• plain");
    let done = p.rows[0].glyphs.iter().find(|g| g.text == "d").unwrap();
    assert!(done.style.add_modifier.contains(Modifier::CROSSED_OUT));
    let open = p.rows[1].glyphs.iter().find(|g| g.text == "o").unwrap();
    assert!(!open.style.add_modifier.contains(Modifier::CROSSED_OUT));
}

#[test]
fn front_matter_is_a_surface_not_a_rule_and_heading() {
    let text = "---\ntitle: x\n---\n\nbody\n";
    let p = project(text, text.len(), 40, true);
    assert_eq!(p.rows[0].fill.as_ref().unwrap().kind, FillKind::Lower);
    assert_eq!(p.rows[1].fill.as_ref().unwrap().kind, FillKind::Band);
    assert_eq!(row_text(&p.rows[1]), "title: x");
    assert_eq!(p.rows[2].fill.as_ref().unwrap().kind, FillKind::Upper);
}

#[test]
fn inline_code_delimiters_become_padding_of_the_same_width() {
    let text = "a `b` c\n\nx";
    let rendered = project(text, text.len(), 40, true);
    assert_eq!(display(&rendered), "a  b  c\n\nx");
    let background = crate::theme::palette().code_background;
    assert!(
        rendered.rows[0].glyphs[2..5]
            .iter()
            .all(|g| g.style.bg == Some(background))
    );
}

#[test]
fn every_layout_of_a_rich_document_keeps_its_source_contracts() {
    let text = RICH;
    let boundaries: Vec<_> = text
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .chain([text.len()])
        .collect();
    let parsed = parsed(text);
    for caret in boundaries.iter().step_by(3) {
        for width in [6, 13, 40, 90] {
            for live in [true, false] {
                let p = Projection::build(text, &parsed, Selection::caret(*caret), width, live);
                assert!(p.rows.iter().any(|r| r.navigable));
                for (index, row) in p.rows.iter().enumerate() {
                    let mut column = 0;
                    for glyph in &row.glyphs {
                        assert!(glyph.column >= column, "overlap at row {index}");
                        column = glyph.column + glyph.width;
                        assert_eq!(UnicodeWidthStr::width(glyph.text.as_str()), glyph.width);
                        assert!(boundaries.contains(&glyph.source.start));
                        assert!(boundaries.contains(&glyph.source.end));
                    }
                    assert!(column < width.max(2) + 1, "row {index} at width {width}");
                    for col in [0, 1, width / 2, width] {
                        assert!(boundaries.contains(&p.hit(index, col).offset));
                    }
                }
                for offset in &boundaries {
                    let (row, _) = p.cursor(*offset);
                    assert!(row < p.rows.len());
                }
                let lines = p.rows.iter().filter_map(|r| r.line).collect::<Vec<_>>();
                assert!(lines.windows(2).all(|pair| pair[1] == pair[0] + 1));
                assert_eq!(lines.first(), Some(&1));
            }
        }
    }
}

#[test]
fn spaces_at_a_soft_wrap_hang_invisibly_instead_of_indenting_the_next_row() {
    // "alpha beta" fills a row exactly; the following space must not lead.
    let text = "alpha beta gamma";
    let p = project(text, text.len(), 11, true);
    assert_eq!(display(&p), "alpha beta\ngamma");
    let space = &p.rows[0].glyphs[10];
    assert_eq!((space.text.as_str(), space.width), ("", 0));
    assert_eq!(p.rows[1].glyphs[0].text, "g");
    let boundary = text.find("gamma").unwrap();
    assert_eq!(p.cursor(boundary), (1, 0));
    assert_eq!(
        p.cursor_with_affinity(boundary, Affinity::Upstream),
        (0, 10)
    );
    // Code keeps every space visible and column-wrapped.
    let code = format!("```\n{text}\n```\n");
    assert!(display(&project(&code, 0, 11, true)).contains("alpha beta\n gamma"));
}

#[test]
fn nested_items_hang_exactly_under_their_text() {
    let text = "x\n\n2. step\n   - with a nested bullet that wraps around\n";
    let p = project(text, 0, 30, true);
    let rows: Vec<_> = p.rows.iter().map(row_text).collect();
    let first = rows.iter().position(|r| r.contains("with")).unwrap();
    let text_column = p.rows[first]
        .glyphs
        .iter()
        .find(|g| g.text == "w")
        .unwrap()
        .column;
    let next = &p.rows[first + 1];
    let continued = next.glyphs.iter().find(|g| !g.source.is_empty()).unwrap();
    assert_eq!(continued.column, text_column, "{rows:#?}");
}
