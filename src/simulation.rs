//! Headless snapshots use the production renderer and Ratatui's cell buffer.
use crate::app::App;
use ratatui::{Terminal, backend::TestBackend};
use std::io;

pub fn snapshot(app: &mut App, width: u16, height: u16) -> io::Result<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|frame| app.draw(frame))?;
    let buffer = terminal.backend().buffer();
    let mut output = String::new();
    for y in 0..height {
        let mut line = String::new();
        let mut x = 0;
        while x < width {
            let cell = &buffer[(x, y)];
            line.push_str(cell.symbol());
            let occupancy = unicode_width::UnicodeWidthStr::width(cell.symbol()).max(1) as u16;
            x = x.saturating_add(occupancy);
        }
        output.push_str(line.trim_end());
        output.push('\n');
    }
    let (row, column) = app.caret_position();
    output.push_str(&format!(
        "\nCaret source byte: {} · visual row: {} · column: {}\n",
        app.document.selection().head,
        row + 1,
        column + 1
    ));
    Ok(output)
}
