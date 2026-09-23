//! Shared drawing for floating panels: rounded frames, query prompts,
//! selectable rows with match highlights, and key hints set into borders.
use crate::{
    projection::safe_text,
    theme::{chrome_palette, palette},
};
use eymi::Document;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn surface() -> Style {
    let colors = palette();
    Style::default().fg(colors.chrome_text).bg(colors.chrome)
}

pub fn muted() -> Style {
    surface().fg(palette().chrome_muted)
}

/// Clear `area` and frame it; returns the inner area.
pub fn panel(frame: &mut Frame, area: Rect, title: &str, count: Option<&str>) -> Rect {
    let colors = palette();
    frame.render_widget(Clear, area);
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(surface().fg(colors.border))
        .style(surface())
        .title(Line::from(Span::styled(
            format!(" {title} "),
            surface().fg(colors.accent).add_modifier(Modifier::BOLD),
        )));
    if let Some(count) = count.filter(|_| area.width > 24) {
        block =
            block.title(Line::from(Span::styled(format!(" {count} "), muted())).right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

/// Key hints in the bottom border, right-aligned, as far as they fit.
pub fn hints(frame: &mut Frame, area: Rect, text: &str) {
    if area.width < 8 || area.height < 2 {
        return;
    }
    let text = clipped(&format!(" {text} "), usize::from(area.width - 4));
    let width = UnicodeWidthStr::width(text.as_str()) as u16;
    frame
        .buffer_mut()
        .set_string(area.right() - 2 - width, area.bottom() - 1, text, muted());
}

/// A rule across the panel at `y`, joined to its side borders.
pub fn divider(frame: &mut Frame, area: Rect, y: u16) {
    if area.width < 2 {
        return;
    }
    let style = surface().fg(palette().border);
    let line = format!("├{}┤", "─".repeat(usize::from(area.width - 2)));
    frame.buffer_mut().set_string(area.x, y, line, style);
}

/// A chevron, the query with its selection, or a placeholder. Returns the
/// caret's cell, scrolling horizontally to keep it visible.
pub fn prompt(frame: &mut Frame, row: Rect, query: &Document, placeholder: &str) -> (u16, u16) {
    let colors = palette();
    let buffer = frame.buffer_mut();
    buffer.set_style(row, surface());
    if row.width < 4 {
        return (row.x, row.y);
    }
    buffer.set_string(
        row.x,
        row.y,
        "❯",
        surface().fg(colors.accent).add_modifier(Modifier::BOLD),
    );
    let field = Rect::new(row.x + 2, row.y, row.width - 2, 1);
    let text = query.text();
    let caret = query.selection().head;
    let mut start = 0;
    while UnicodeWidthStr::width(&text[start..caret]) >= usize::from(field.width).max(1) {
        let Some(grapheme) = text[start..caret].graphemes(true).next() else {
            break;
        };
        start += grapheme.len();
    }
    if text.is_empty() {
        buffer.set_stringn(
            field.x,
            field.y,
            placeholder,
            usize::from(field.width),
            muted().add_modifier(Modifier::ITALIC),
        );
    } else {
        let selection = query.selection().range();
        let mut column = field.x;
        for (index, grapheme) in text[start..].grapheme_indices(true) {
            let shown = safe_text(grapheme);
            let width = UnicodeWidthStr::width(shown.as_str()).max(1) as u16;
            if column + width > field.right() {
                break;
            }
            let offset = start + index;
            let style = if offset < selection.end && selection.start < offset + grapheme.len() {
                surface().bg(colors.selection).fg(colors.selection_text)
            } else {
                surface()
            };
            buffer.set_string(column, field.y, shown, style);
            column += width;
        }
    }
    let caret_column = UnicodeWidthStr::width(&text[start..caret]) as u16;
    (
        field.x + caret_column.min(field.width.saturating_sub(1)),
        field.y,
    )
}

/// A selectable row: accent bar and surface when selected, query matches
/// emphasized, and a right-aligned hint when it fits beside the label.
pub fn item(frame: &mut Frame, row: Rect, label: &str, hint: &str, selected: bool, query: &str) {
    let colors = palette();
    let chrome = chrome_palette();
    let base = if selected {
        chrome.tab_active.style()
    } else {
        surface()
    };
    let buffer = frame.buffer_mut();
    buffer.set_style(row, base);
    if row.width < 3 {
        return;
    }
    if selected {
        buffer.set_string(row.x, row.y, "▌", base.fg(chrome.tab_indicator));
    }
    let hint_width = UnicodeWidthStr::width(hint);
    let label_width = UnicodeWidthStr::width(label);
    let show_hint = hint_width > 0 && usize::from(row.width) >= label_width + hint_width + 5;
    let room = usize::from(row.width) - 2 - if show_hint { hint_width + 2 } else { 0 };
    let label = clipped(label, room);
    let marks = highlights(&label, query);
    let text = if selected {
        base.add_modifier(Modifier::BOLD)
    } else {
        base
    };
    let emphasis = base.fg(colors.accent).add_modifier(Modifier::BOLD);
    let mut x = row.x + 2;
    for (grapheme, marked) in label.graphemes(true).zip(marks) {
        buffer.set_string(x, row.y, grapheme, if marked { emphasis } else { text });
        x += UnicodeWidthStr::width(grapheme) as u16;
    }
    if show_hint {
        let style = if selected {
            base
        } else {
            base.fg(colors.chrome_muted)
        };
        buffer.set_string(row.right() - 1 - hint_width as u16, row.y, hint, style);
    }
}

/// Which graphemes of `label` match the query: each word as a substring
/// where possible, otherwise as an ordered subsequence.
pub fn highlights(label: &str, query: &str) -> Vec<bool> {
    let graphemes: Vec<&str> = label.graphemes(true).collect();
    let lower: Vec<String> = graphemes.iter().map(|g| g.to_lowercase()).collect();
    let mut marks = vec![false; graphemes.len()];
    for word in query.split_whitespace() {
        let word: Vec<String> = word.graphemes(true).map(str::to_lowercase).collect();
        if word.is_empty() || word.len() > lower.len() {
            continue;
        }
        if let Some(start) = (0..=lower.len() - word.len())
            .find(|start| lower[*start..*start + word.len()] == word[..])
        {
            marks[start..start + word.len()].fill(true);
            continue;
        }
        let mut next = 0;
        let mut found = Vec::new();
        for part in &word {
            let Some(at) = (next..lower.len()).find(|at| &lower[*at] == part) else {
                found.clear();
                break;
            };
            found.push(at);
            next = at + 1;
        }
        for at in found {
            marks[at] = true;
        }
    }
    marks
}

/// Whether a confirmation can show its whole prompt and every key. Input
/// handlers use the same rule, so an unseen choice is never accepted.
pub fn dialog_fits(area: Rect) -> bool {
    area.width >= 44 && area.height >= 9
}

/// A centered confirmation sized to its body, with keys along the bottom.
/// Returns whether the whole prompt is visible; otherwise a resize notice
/// that keeps cancelling available takes its place.
pub fn dialog(
    frame: &mut Frame,
    title: &str,
    body: &[Line],
    keys_row: &[(&str, &str)],
    max_width: u16,
) -> bool {
    let area = frame.area();
    if !dialog_fits(area) {
        frame.render_widget(Clear, area);
        frame.render_widget(
            ratatui::widgets::Paragraph::new("Resize to confirm · Esc cancels")
                .wrap(ratatui::widgets::Wrap { trim: true })
                .style(surface()),
            area,
        );
        return false;
    }
    let width = area.width.saturating_sub(4).min(max_width);
    let inner_width = usize::from(width.saturating_sub(4)).max(1);
    let lines: u16 = body
        .iter()
        .map(|line| line.width().max(1).div_ceil(inner_width) as u16)
        .sum();
    let height = (lines + 5).min(area.height.saturating_sub(2));
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 3,
        width,
        height,
    );
    let inner = panel(frame, rect, title, None);
    let text = Rect::new(
        inner.x + 1,
        inner.y + 1,
        inner.width - 2,
        inner.height.saturating_sub(3),
    );
    frame.render_widget(
        ratatui::widgets::Paragraph::new(body.to_vec())
            .wrap(ratatui::widgets::Wrap { trim: false })
            .style(surface()),
        text,
    );
    keys(
        frame,
        Rect::new(inner.x + 1, inner.bottom() - 1, inner.width - 2, 1),
        keys_row,
    );
    true
}

/// Keys drawn as quiet chips followed by their action, for dialogs.
pub fn keys(frame: &mut Frame, row: Rect, pairs: &[(&str, &str)]) {
    let colors = palette();
    let chip = surface()
        .bg(chrome_palette().tab_active.background)
        .fg(colors.chrome_text)
        .add_modifier(Modifier::BOLD);
    let mut x = row.x;
    for (key, action) in pairs {
        let needed = UnicodeWidthStr::width(*key) + UnicodeWidthStr::width(*action) + 4;
        if usize::from(row.right().saturating_sub(x)) < needed {
            break;
        }
        let key = format!(" {key} ");
        frame.buffer_mut().set_string(x, row.y, &key, chip);
        x += UnicodeWidthStr::width(key.as_str()) as u16 + 1;
        frame.buffer_mut().set_string(x, row.y, *action, muted());
        x += UnicodeWidthStr::width(*action) as u16 + 2;
    }
}

pub fn clipped(text: &str, max: usize) -> String {
    if UnicodeWidthStr::width(text) <= max {
        return text.into();
    }
    if max == 0 {
        return String::new();
    }
    let mut output = String::new();
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        let width = UnicodeWidthStr::width(grapheme);
        if used + width > max - 1 {
            break;
        }
        output.push_str(grapheme);
        used += width;
    }
    output.push('…');
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_prefer_substrings_then_ordered_letters() {
        let marks = |label, query| -> String {
            highlights(label, query)
                .into_iter()
                .map(|m| if m { '^' } else { ' ' })
                .collect()
        };
        assert_eq!(marks("Find and replace", "rep"), "         ^^^    ");
        assert_eq!(marks("Go to heading…", "gth"), "^  ^  ^       ");
        assert_eq!(marks("Save as…", "SAVE"), "^^^^    ");
        assert_eq!(marks("Save", "zz"), "    ");
        assert_eq!(marks("界面", "面"), " ^");
    }
}
