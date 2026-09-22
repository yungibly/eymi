//! One-row tab strip. Keep the active document visible when the row overflows.
use super::clipped;
use crate::theme::chrome_palette;
use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    widgets::{Block, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

pub(super) struct TabLabel {
    pub text: String,
    pub dirty: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Activate(usize),
    NewDocument,
}

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    rail_width: u16,
    labels: &[TabLabel],
    active: usize,
) -> Vec<(Rect, Target)> {
    let mut hits = vec![];
    if area.width == 0 || area.height == 0 || labels.is_empty() {
        return hits;
    }
    let colors = chrome_palette();
    let strip = Rect::new(area.x, area.y, area.width, 1);
    frame.render_widget(Clear, strip);
    frame.render_widget(Block::default().style(colors.tab_inactive.style()), strip);
    let brand = if rail_width > 0 {
        rail_width
    } else if area.width >= 72 {
        8
    } else {
        0
    };
    if brand > 0 {
        let brand_rect = Rect::new(area.x, area.y, brand, 1);
        frame.render_widget(
            Paragraph::new("  eymi").style(colors.sidebar.style().add_modifier(Modifier::BOLD)),
            brand_rect,
        );
        if rail_width > 0 {
            frame.buffer_mut().set_string(
                brand_rect.right() - 1,
                area.y,
                "│",
                colors.sidebar.style().fg(colors.separator),
            );
        }
    }
    let area = Rect::new(area.x + brand, area.y, area.width - brand, 1);
    let mut width = usize::from(area.width);
    if width == 0 {
        return hits;
    }
    let desired: Vec<_> = labels
        .iter()
        .map(|tab| (UnicodeWidthStr::width(tab.text.as_str()) + 2).min(34))
        .collect();
    // A new-document control must not take space needed by the active label
    // or either overflow arrow. Hide it before truncating that label further.
    let arrows = usize::from(active > 0) * 2 + usize::from(active + 1 < labels.len()) * 2;
    let minimum_tabs = (desired[active] + arrows).max(if labels.len() > 1 { 7 } else { 0 });
    let new_width = if width >= minimum_tabs + 3 { 3 } else { 0 };
    width -= new_width;
    let area = Rect::new(area.x, area.y, width as u16, 1);
    let mut start = active;
    let mut used = desired[active].min(width);
    while start > 0 {
        let extra = usize::from(start > 1) * 2 + usize::from(active + 1 < labels.len()) * 2;
        if used + desired[start - 1] + extra > width {
            break;
        }
        start -= 1;
        used += desired[start];
    }
    let left = if start > 0 && width >= 7 { 2 } else { 0 };
    let remaining: usize = desired[start..].iter().sum();
    let right = if remaining + left > width && active + 1 < labels.len() && width >= 7 {
        2
    } else {
        0
    };
    let mut x = area.x;
    if left > 0 {
        frame
            .buffer_mut()
            .set_string(x, area.y, "‹ ", colors.tab_inactive.style());
        hits.push((Rect::new(x, area.y, 2, 1), Target::Activate(start - 1)));
        x += 2;
    }
    let end = area.right() - right as u16;
    let mut last = start;
    for (index, tab) in labels.iter().enumerate().skip(start) {
        let available = usize::from(end.saturating_sub(x));
        if available == 0 {
            break;
        }
        let cells = desired[index].min(available) as u16;
        let selected = index == active;
        let style = if selected {
            colors.tab_active.style().add_modifier(Modifier::BOLD)
        } else {
            colors.tab_inactive.style()
        };
        let rect = Rect::new(x, area.y, cells, 1);
        frame.render_widget(Block::default().style(style), rect);
        let small = cells < 4;
        let prefix = if small {
            ""
        } else if selected {
            "▎"
        } else {
            " "
        };
        let text = if cells == 1 && tab.dirty {
            "*".to_owned()
        } else {
            clipped(&format!("{prefix}{} ", tab.text), cells as usize)
        };
        frame.render_widget(Paragraph::new(text).style(style), rect);
        if selected && !small {
            frame
                .buffer_mut()
                .set_string(x, area.y, "▎", style.fg(colors.tab_indicator));
        }
        hits.push((rect, Target::Activate(index)));
        last = index;
        x += cells;
    }
    if right > 0 {
        let target = (last + 1).min(labels.len() - 1);
        let rect = Rect::new(end, area.y, 2, 1);
        frame.render_widget(
            Paragraph::new(" ›").style(colors.tab_inactive.style()),
            rect,
        );
        hits.push((rect, Target::Activate(target)));
    }
    if new_width > 0 {
        let x = if right > 0 { area.right() } else { x };
        let rect = Rect::new(x, area.y, new_width as u16, 1);
        frame.render_widget(
            Paragraph::new(" + ").style(colors.tab_inactive.style().add_modifier(Modifier::BOLD)),
            rect,
        );
        hits.push((rect, Target::NewDocument));
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn new_document_button_yields_to_the_active_label() {
        let labels = [TabLabel {
            text: "note.md".into(),
            dirty: false,
        }];
        for width in 1..=18 {
            let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
            let mut hits = vec![];
            terminal
                .draw(|frame| {
                    hits = draw(frame, frame.area(), 0, &labels, 0);
                })
                .unwrap();
            let button = hits
                .iter()
                .find(|(_, target)| *target == Target::NewDocument);
            assert_eq!(button.is_some(), width >= 12, "width {width}");
            if let Some((rect, _)) = button {
                assert_eq!(rect.x, 9);
                assert_eq!(terminal.backend().buffer()[(rect.x + 1, 0)].symbol(), "+");
                let label: String = (0..rect.x)
                    .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
                    .collect();
                assert_eq!(label, "▎note.md ");
            }
        }
    }

    #[test]
    fn new_document_button_keeps_active_and_overflow_targets_reachable() {
        let labels: Vec<_> = [
            "a.md",
            "* a-long-name-that-exceeds-the-label-cap.md",
            "界面 👩🏽‍💻.md",
            "\u{e73e} notes.md",
            "* tail.md",
            "\u{301}",
        ]
        .into_iter()
        .map(|text| TabLabel {
            text: text.into(),
            dirty: text.starts_with("* "),
        })
        .collect();
        for rail in [0, 23, 27] {
            for width in (rail + 1)..=120 {
                for active in 0..labels.len() {
                    let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
                    let mut hits = vec![];
                    terminal
                        .draw(|frame| {
                            hits = draw(frame, frame.area(), rail, &labels, active);
                        })
                        .unwrap();
                    let active_rect = hits
                        .iter()
                        .find_map(|(rect, target)| {
                            (*target == Target::Activate(active)).then_some(*rect)
                        })
                        .unwrap();
                    assert!(active_rect.width > 0);
                    for pair in hits.windows(2) {
                        assert!(pair[0].0.right() <= pair[1].0.x);
                    }
                    assert!(hits.iter().all(|(rect, _)| rect.right() <= width));
                    let button = hits
                        .iter()
                        .find(|(_, target)| *target == Target::NewDocument);
                    if let Some((rect, _)) = button {
                        let desired =
                            (UnicodeWidthStr::width(labels[active].text.as_str()) + 2).min(34);
                        assert_eq!(usize::from(active_rect.width), desired);
                        assert_eq!(terminal.backend().buffer()[(rect.x + 1, 0)].symbol(), "+");
                    }
                    if labels[active].dirty {
                        assert!(
                            (active_rect.x..active_rect.right()).any(|x| terminal
                                .backend()
                                .buffer()[(x, 0)]
                                .symbol()
                                == "*")
                        );
                    }
                    let brand = if rail > 0 {
                        rail
                    } else if width >= 72 {
                        8
                    } else {
                        0
                    };
                    if width - brand >= 7 {
                        assert!(
                            active == 0
                                || hits.iter().any(|(_, target)| {
                                    matches!(target, Target::Activate(index) if *index < active)
                                })
                        );
                        assert!(
                            active + 1 == labels.len()
                                || hits.iter().any(|(_, target)| {
                                    matches!(target, Target::Activate(index) if *index > active)
                                })
                        );
                    }
                }
            }
        }
    }
}
