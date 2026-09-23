//! One-row tab strip. Keep the active document visible when the row overflows.
use crate::{theme::chrome_palette, ui::clipped};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Clear},
};
use unicode_width::UnicodeWidthStr;

/// Unsaved documents keep this mark even when their name is clipped.
pub(super) const DIRTY: &str = "●";

pub(super) struct TabLabel {
    pub name: String,
    pub icon: &'static str,
    pub dirty: bool,
}

impl TabLabel {
    /// Indicator, icon, name, unsaved mark, and trailing space.
    fn width(&self) -> usize {
        let icon = if self.icon.is_empty() { 0 } else { 2 };
        1 + icon + UnicodeWidthStr::width(self.name.as_str()) + 2 * usize::from(self.dirty) + 1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Activate(usize),
    NewDocument,
}

/// Draw tabs to the right of `rail_width`, which the sidebar header owns.
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
    let rail_width = rail_width.min(area.width);
    let area = Rect::new(area.x + rail_width, area.y, area.width - rail_width, 1);
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(colors.tab_inactive.style()), area);
    let mut width = usize::from(area.width);
    if width == 0 {
        return hits;
    }
    let desired: Vec<_> = labels.iter().map(|tab| tab.width().min(34)).collect();
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
    let buffer_style = colors.tab_inactive.style();
    let mut x = area.x;
    if left > 0 {
        frame.buffer_mut().set_string(x, area.y, "‹ ", buffer_style);
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
        draw_label(frame, rect, tab, selected, style);
        hits.push((rect, Target::Activate(index)));
        last = index;
        x += cells;
    }
    if right > 0 {
        let target = (last + 1).min(labels.len() - 1);
        let rect = Rect::new(end, area.y, 2, 1);
        frame
            .buffer_mut()
            .set_string(end, area.y, " ›", buffer_style);
        hits.push((rect, Target::Activate(target)));
    }
    if new_width > 0 {
        let x = if right > 0 { area.right() } else { x };
        let rect = Rect::new(x, area.y, new_width as u16, 1);
        frame.buffer_mut().set_string(
            x,
            area.y,
            " + ",
            buffer_style
                .fg(colors.sidebar_muted)
                .add_modifier(Modifier::BOLD),
        );
        hits.push((rect, Target::NewDocument));
    }
    hits
}

/// The name yields space first; the unsaved mark stays visible down to a
/// single cell, and the active indicator down to four.
fn draw_label(frame: &mut Frame, rect: Rect, tab: &TabLabel, selected: bool, style: Style) {
    let colors = chrome_palette();
    let cells = usize::from(rect.width);
    let buffer = frame.buffer_mut();
    if cells < 4 {
        let text = if tab.dirty {
            DIRTY.to_owned()
        } else {
            clipped(&tab.name, cells)
        };
        buffer.set_stringn(rect.x, rect.y, text, cells, style);
        return;
    }
    let mut x = rect.x;
    let indicator = if selected {
        style.fg(colors.tab_indicator)
    } else {
        style
    };
    buffer.set_string(x, rect.y, if selected { "▎" } else { " " }, indicator);
    x += 1;
    let mut room = cells - 2 - 2 * usize::from(tab.dirty);
    if !tab.icon.is_empty() && room >= 4 {
        let icon_style = if selected {
            style.fg(colors.tab_indicator)
        } else {
            style
        };
        buffer.set_string(x, rect.y, tab.icon, icon_style);
        x += 2;
        room -= 2;
    }
    let name = clipped(&tab.name, room);
    buffer.set_string(x, rect.y, &name, style);
    x += UnicodeWidthStr::width(name.as_str()) as u16;
    if tab.dirty {
        let mark = if selected {
            style.fg(colors.tab_indicator)
        } else {
            style.fg(colors.sidebar_muted)
        };
        buffer.set_string(x + 1, rect.y, DIRTY, mark);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn row(terminal: &Terminal<TestBackend>, range: std::ops::Range<u16>) -> String {
        range
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol().to_owned())
            .collect()
    }

    #[test]
    fn new_document_button_yields_to_the_active_label() {
        let labels = [TabLabel {
            name: "note.md".into(),
            icon: "",
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
                assert_eq!(row(&terminal, 0..rect.x), "▎note.md ");
            }
        }
    }

    #[test]
    fn unsaved_mark_and_icon_survive_clipping_in_order() {
        let labels = [TabLabel {
            name: "a-rather-long-document-name.md".into(),
            icon: "\u{e73e}",
            dirty: true,
        }];
        let mut terminal = Terminal::new(TestBackend::new(16, 1)).unwrap();
        terminal
            .draw(|frame| {
                draw(frame, frame.area(), 0, &labels, 0);
            })
            .unwrap();
        let text = row(&terminal, 0..16);
        assert!(text.starts_with("▎\u{e73e} a-rather"), "{text}");
        assert!(text.contains("… ●"), "{text}");
    }

    #[test]
    fn new_document_button_keeps_active_and_overflow_targets_reachable() {
        let labels: Vec<_> = [
            ("a.md", false),
            ("a-long-name-that-exceeds-the-label-cap.md", true),
            ("界面 👩🏽‍💻.md", false),
            ("notes.md", false),
            ("tail.md", true),
            ("\u{301}", false),
        ]
        .into_iter()
        .map(|(name, dirty)| TabLabel {
            name: name.into(),
            icon: "",
            dirty,
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
                    assert!(active_rect.x >= rail);
                    for pair in hits.windows(2) {
                        assert!(pair[0].0.right() <= pair[1].0.x);
                    }
                    assert!(hits.iter().all(|(rect, _)| rect.right() <= width));
                    let button = hits
                        .iter()
                        .find(|(_, target)| *target == Target::NewDocument);
                    if let Some((rect, _)) = button {
                        assert_eq!(active_rect.width as usize, labels[active].width().min(34));
                        assert_eq!(terminal.backend().buffer()[(rect.x + 1, 0)].symbol(), "+");
                    }
                    if labels[active].dirty {
                        assert!(
                            (active_rect.x..active_rect.right()).any(|x| terminal
                                .backend()
                                .buffer()[(x, 0)]
                                .symbol()
                                == DIRTY)
                        );
                    }
                    if width - rail >= 7 {
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
