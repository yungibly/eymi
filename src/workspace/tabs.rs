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

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    rail_width: u16,
    labels: &[TabLabel],
    active: usize,
) -> Vec<(Rect, usize)> {
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
        12
    } else {
        0
    };
    if brand > 0 {
        let brand_rect = Rect::new(area.x, area.y, brand, 1);
        frame.render_widget(
            Paragraph::new("  marklane").style(colors.sidebar.style().add_modifier(Modifier::BOLD)),
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
    let width = usize::from(area.width);
    if width == 0 {
        return hits;
    }
    let desired: Vec<_> = labels
        .iter()
        .map(|tab| (UnicodeWidthStr::width(tab.text.as_str()) + 2).min(34))
        .collect();
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
        hits.push((Rect::new(x, area.y, 2, 1), start - 1));
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
        hits.push((rect, index));
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
        hits.push((rect, target));
    }
    hits
}
