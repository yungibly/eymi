//! A small command picker. Query edits always stay in its own document.
use super::clipped;
use crate::ui;
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use eymi::Document;
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Clear, Paragraph},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Command {
    New,
    Open,
    Save,
    SaveAs,
    Close,
    Find,
    Replace,
    View,
    Sidebar,
    NextTab,
    PreviousTab,
    GoToLine,
    Documents,
    Headings,
    Help,
    Quit,
    Undo,
    Redo,
    Bold,
    Italic,
    InlineCode,
    Indent,
    Outdent,
    MoveUp,
    MoveDown,
    DuplicateUp,
    DuplicateDown,
    Theme,
    Icons,
    Reload,
    Recover,
}
impl Command {
    pub const ALL: [Self; 31] = [
        Self::New,
        Self::Open,
        Self::Save,
        Self::SaveAs,
        Self::Close,
        Self::Find,
        Self::Replace,
        Self::View,
        Self::Sidebar,
        Self::NextTab,
        Self::PreviousTab,
        Self::GoToLine,
        Self::Documents,
        Self::Headings,
        Self::Help,
        Self::Quit,
        Self::Undo,
        Self::Redo,
        Self::Bold,
        Self::Italic,
        Self::InlineCode,
        Self::Indent,
        Self::Outdent,
        Self::MoveUp,
        Self::MoveDown,
        Self::DuplicateUp,
        Self::DuplicateDown,
        Self::Theme,
        Self::Icons,
        Self::Reload,
        Self::Recover,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::New => "New document",
            Self::Open => "Open file…",
            Self::Save => "Save document",
            Self::SaveAs => "Save as…",
            Self::Close => "Close document",
            Self::Find => "Find in document",
            Self::Replace => "Find and replace",
            Self::View => "Toggle live / source view",
            Self::Sidebar => "Toggle sidebar",
            Self::NextTab => "Next document tab",
            Self::PreviousTab => "Previous document tab",
            Self::GoToLine => "Go to line…",
            Self::Documents => "Switch document…",
            Self::Headings => "Go to heading…",
            Self::Help => "Help / keyboard shortcuts",
            Self::Quit => "Quit Eymi",
            Self::Undo => "Undo edit",
            Self::Redo => "Redo edit",
            Self::Bold => "Format bold",
            Self::Italic => "Format italic",
            Self::InlineCode => "Format inline code",
            Self::Indent => "Indent selected lines",
            Self::Outdent => "Outdent selected lines",
            Self::MoveUp => "Move lines up",
            Self::MoveDown => "Move lines down",
            Self::DuplicateUp => "Duplicate lines above",
            Self::DuplicateDown => "Duplicate lines below",
            Self::Theme => "Choose theme…",
            Self::Icons => "Toggle Nerd Font icons",
            Self::Reload => "Reload file from disk",
            Self::Recover => "Recover documents…",
        }
    }
    pub fn shortcut(self) -> &'static str {
        match self {
            Self::New => "Ctrl+N",
            Self::Open => "Ctrl+O",
            Self::Save => "Ctrl+S",
            Self::SaveAs => "F4",
            Self::Close => "Ctrl+W",
            Self::Find => "Ctrl+F",
            Self::Replace => "Ctrl+R",
            Self::View => "F6",
            Self::Sidebar => "F9 focus",
            Self::NextTab => "F8",
            Self::PreviousTab => "F7",
            Self::GoToLine => "Ctrl+G",
            Self::Documents => "F10",
            Self::Headings => "F11",
            Self::Help => "F1",
            Self::Quit => "Ctrl+Q",
            Self::Undo => "Ctrl+Z",
            Self::Redo => "Ctrl+Y",
            Self::Bold => "Ctrl+B",
            Self::Italic => "Alt+I",
            Self::InlineCode => "Alt+`",
            Self::Indent => "Ctrl+]",
            Self::Outdent => "Ctrl+[",
            Self::MoveUp => "Alt+Up",
            Self::MoveDown => "Alt+Down",
            Self::DuplicateUp => "Alt+Shift+Up",
            Self::DuplicateDown => "Alt+Shift+Down",
            Self::Theme => "",
            Self::Icons => "",
            Self::Reload => "F5",
            Self::Recover => "",
        }
    }
}

pub(super) enum Action {
    None,
    Close,
    Command(Command),
    Line(usize),
}

pub(super) struct Palette {
    pub query: Document,
    pub selected: usize,
    pub line_mode: bool,
    pub hits: Vec<(Rect, Command)>,
    ready: bool,
    error: Option<&'static str>,
}
impl Palette {
    pub fn new(line_mode: bool) -> Self {
        Self {
            query: Document::new(""),
            selected: 0,
            line_mode,
            hits: vec![],
            ready: false,
            error: None,
        }
    }
    pub fn matches(&self) -> Vec<Command> {
        let query = self.query.text().to_lowercase();
        Command::ALL
            .into_iter()
            .filter(|command| {
                let label = format!("{} {}", command.label(), command.shortcut()).to_lowercase();
                query.split_whitespace().all(|part| label.contains(part))
            })
            .collect()
    }
    pub fn invalidate(&mut self) {
        self.hits.clear();
        self.ready = false;
    }
    pub fn handle(&mut self, event: Event) -> Action {
        let revision = self.query.revision();
        match event {
            Event::Resize(..) => self.invalidate(),
            Event::Paste(text) => {
                let clean: String = text.chars().filter(|c| !c.is_control()).collect();
                if !clean.is_empty() {
                    self.query.insert(&clean);
                }
            }
            Event::Key(key) => {
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                match key.code {
                    KeyCode::Esc => return Action::Close,
                    KeyCode::Char('q') if ctrl => return Action::Command(Command::Quit),
                    KeyCode::Char('a') if ctrl => self.query.select_all(),
                    KeyCode::Char('u') if ctrl => {
                        self.query.select_all();
                        self.query.insert("");
                    }
                    KeyCode::Up if !self.line_mode => {
                        self.selected = self.selected.saturating_sub(1);
                        self.hits.clear();
                    }
                    KeyCode::Down if !self.line_mode => {
                        self.selected =
                            (self.selected + 1).min(self.matches().len().saturating_sub(1));
                        self.hits.clear();
                    }
                    KeyCode::Enter if self.ready => {
                        if self.line_mode {
                            match self.query.text().trim().parse::<usize>() {
                                Ok(line) if line > 0 => return Action::Line(line),
                                _ => self.error = Some("Enter a line number starting at 1"),
                            }
                        } else if let Some(command) = self.matches().get(self.selected) {
                            return Action::Command(*command);
                        }
                    }
                    KeyCode::Left => self
                        .query
                        .move_left(key.modifiers.contains(KeyModifiers::SHIFT)),
                    KeyCode::Right => self
                        .query
                        .move_right(key.modifiers.contains(KeyModifiers::SHIFT)),
                    KeyCode::Home => {
                        let _ = self.query.set_caret(0);
                    }
                    KeyCode::End => {
                        let _ = self.query.set_caret(self.query.text().len());
                    }
                    KeyCode::Backspace => {
                        self.query.backspace();
                    }
                    KeyCode::Delete => {
                        self.query.delete_forward();
                    }
                    KeyCode::Char(c)
                        if !ctrl
                            && !key.modifiers.contains(KeyModifiers::ALT)
                            && !c.is_control() =>
                    {
                        self.query.insert(&c.to_string());
                    }
                    _ => {}
                }
            }
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                if let Some((_, command)) = self
                    .hits
                    .iter()
                    .find(|(rect, _)| rect.contains((mouse.column, mouse.row).into()))
                {
                    return Action::Command(*command);
                }
            }
            _ => {}
        }
        if revision != self.query.revision() {
            self.selected = 0;
            self.error = None;
            self.invalidate();
        }
        Action::None
    }
    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        self.invalidate();
        if area.width == 0 || area.height == 0 {
            return;
        }
        if area.width < 12 || area.height < 5 {
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new("Resize for commands\nEsc: close").style(ui::surface()),
                area,
            );
            return;
        }
        let width = area.width.saturating_sub(4).clamp(12, 76).min(area.width);
        let max_height = area.height.saturating_sub(2).clamp(5, 19).min(area.height);
        let commands = self.matches();
        let height = if self.line_mode {
            6.min(max_height)
        } else {
            (commands.len() as u16 + 4).clamp(5, max_height)
        };
        let rect = Rect::new(
            area.x + (area.width - width) / 2,
            // Keep the query anchored while the list grows or shrinks below it.
            area.y + (area.height - max_height) / 3,
            width,
            height,
        );
        let count = format!("{}/{}", commands.len(), Command::ALL.len());
        let inner = ui::panel(
            frame,
            rect,
            if self.line_mode {
                "Go to line"
            } else {
                "Commands"
            },
            (!self.line_mode).then_some(count.as_str()),
        );
        let field = Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(2), 1);
        let caret = ui::prompt(
            frame,
            field,
            &self.query,
            if self.line_mode {
                "Line number…"
            } else {
                "Type to filter commands…"
            },
        );
        if field.width > 0 {
            frame.set_cursor_position(caret);
        }
        ui::divider(frame, rect, rect.y + 2);
        let available = usize::from(height.saturating_sub(4));
        self.ready = available > 0;
        if self.line_mode {
            frame.render_widget(
                Paragraph::new(clipped(
                    self.error.unwrap_or("Enter jumps to the line"),
                    field.width as usize,
                ))
                .style(if self.error.is_some() {
                    ui::surface().fg(crate::theme::palette().warning)
                } else {
                    ui::muted()
                }),
                Rect::new(field.x, rect.y + 3, field.width, 1),
            );
        } else {
            self.selected = self.selected.min(commands.len().saturating_sub(1));
            let start = self.selected.saturating_sub(available.saturating_sub(1));
            let query = self.query.text().to_owned();
            for (row, command) in commands.iter().skip(start).take(available).enumerate() {
                let row_rect = Rect::new(inner.x, rect.y + 3 + row as u16, inner.width, 1);
                ui::item(
                    frame,
                    row_rect,
                    command.label(),
                    command.shortcut(),
                    start + row == self.selected,
                    &query,
                );
                self.hits.push((row_rect, *command));
            }
            if commands.is_empty() {
                frame.render_widget(
                    Paragraph::new("  No matching commands").style(ui::muted()),
                    Rect::new(inner.x, rect.y + 3, inner.width, 1),
                );
            }
        }
        ui::hints(
            frame,
            rect,
            if self.line_mode {
                "⏎ jump · esc close"
            } else {
                "↑↓ select · ⏎ run · esc close"
            },
        );
    }
}
