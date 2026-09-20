//! Headless snapshots use the production renderer and Ratatui's cell buffer.
use crate::app::App;
use crate::workspace::Workspace;
use ratatui::{Frame, Terminal, backend::TestBackend};
use std::io;

pub trait SnapshotTarget {
    fn draw_snapshot(&mut self, frame: &mut Frame);
    fn snapshot_editor(&self) -> &App;
}

impl SnapshotTarget for App {
    fn draw_snapshot(&mut self, frame: &mut Frame) {
        self.draw(frame);
    }
    fn snapshot_editor(&self) -> &App {
        self
    }
}

impl SnapshotTarget for Workspace {
    fn draw_snapshot(&mut self, frame: &mut Frame) {
        self.draw(frame);
    }
    fn snapshot_editor(&self) -> &App {
        self.editor()
    }
}

pub fn snapshot(app: &mut impl SnapshotTarget, width: u16, height: u16) -> io::Result<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|frame| app.draw_snapshot(frame))?;
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
    let editor = app.snapshot_editor();
    let (row, column) = editor.caret_position();
    output.push_str(&format!(
        "\nCaret source byte: {} · visual row: {} · column: {}\n",
        editor.document.selection().head,
        row + 1,
        column + 1
    ));
    Ok(output)
}
