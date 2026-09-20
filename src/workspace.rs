//! Document workspace. Each tab owns its editor; clipboard and quit intent are session-wide.
use crate::{
    app::App,
    browser::{Action as BrowserAction, Browser},
    clipboard::Clipboard,
    projection::safe_text,
};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use std::{
    io,
    path::{Component, Path, PathBuf},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

struct Tab {
    editor: App,
    number: usize,
    // Retain the accepted identity while a directory is temporarily unavailable.
    // Resolving another tab must never depend on access to this tab's path.
    identity: Option<PathBuf>,
}

impl Tab {
    fn refresh_identity(&mut self) {
        if let Some(path) = self.editor.path()
            && let Ok(identity) = path_identity(path)
        {
            self.identity = Some(identity);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Intent {
    Close,
    Quit,
}

struct Pending {
    intent: Intent,
    remaining: Vec<usize>,
    saving: bool,
}

pub struct Workspace {
    tabs: Vec<Tab>,
    active: usize,
    next_number: usize,
    clipboard: Clipboard,
    pending: Option<Pending>,
    browser: Option<Browser>,
    tab_hits: Vec<(Rect, usize)>,
    pub should_exit: bool,
}

impl Workspace {
    pub fn open(path: Option<PathBuf>) -> io::Result<Self> {
        let identity = path.as_deref().map(path_identity).transpose()?;
        Ok(Self {
            tabs: vec![Tab {
                editor: App::open(path)?,
                number: 1,
                identity,
            }],
            active: 0,
            next_number: 2,
            clipboard: Clipboard::internal(),
            pending: None,
            browser: None,
            tab_hits: vec![],
            should_exit: false,
        })
    }

    pub fn editor(&self) -> &App {
        &self.tabs[self.active].editor
    }
    pub fn editor_mut(&mut self) -> &mut App {
        &mut self.tabs[self.active].editor
    }

    pub fn enable_system_clipboard(&mut self) {
        self.tabs[self.active]
            .editor
            .enable_workspace_clipboard(&mut self.clipboard);
    }

    fn switch(&mut self, index: usize) {
        if index == self.active {
            return;
        }
        self.editor_mut().deactivate();
        self.active = index;
        self.tab_hits.clear();
    }

    fn new_tab(&mut self) {
        self.editor_mut().deactivate();
        self.tabs.push(Tab {
            editor: App::open(None).expect("empty document"),
            number: self.next_number,
            identity: None,
        });
        self.next_number += 1;
        self.active = self.tabs.len() - 1;
        self.tab_hits.clear();
    }

    fn open_browser(&mut self) {
        self.editor_mut().deactivate();
        let directory = self
            .editor()
            .path()
            .and_then(Path::parent)
            .filter(|path| !path.as_os_str().is_empty())
            .map(Path::to_owned)
            .unwrap_or_else(|| PathBuf::from("."));
        match Browser::new(directory) {
            Ok(browser) => self.browser = Some(browser),
            Err(error) => self.editor_mut().set_message(error.to_string()),
        }
        self.tab_hits.clear();
    }

    fn open_path(&mut self, path: PathBuf) -> io::Result<()> {
        if let Some(index) = self.same_open_path(&path, None)? {
            self.switch(index);
            return Ok(());
        }
        let editor = App::open_bounded(path, 8 * 1024 * 1024)?;
        let identity = editor.path().map(path_identity).transpose()?;
        self.editor_mut().deactivate();
        self.tabs.push(Tab {
            editor,
            number: self.next_number,
            identity,
        });
        self.next_number += 1;
        self.active = self.tabs.len() - 1;
        self.tab_hits.clear();
        Ok(())
    }

    fn request(&mut self, intent: Intent) {
        self.editor_mut().deactivate();
        let remaining = if intent == Intent::Close {
            vec![self.active]
        } else {
            self.tabs
                .iter()
                .enumerate()
                .filter_map(|(i, tab)| tab.editor.document.is_dirty().then_some(i))
                .collect()
        };
        self.pending = Some(Pending {
            intent,
            remaining,
            saving: false,
        });
        self.advance(false);
    }

    fn advance(&mut self, accepted: bool) {
        let Some(mut pending) = self.pending.take() else {
            return;
        };
        if accepted && !pending.remaining.is_empty() {
            pending.remaining.remove(0);
        }
        if pending.intent == Intent::Close {
            if accepted || !self.editor().document.is_dirty() {
                self.close_active();
                return;
            }
        } else if pending.remaining.is_empty() {
            self.should_exit = true;
            return;
        }
        if let Some(&index) = pending.remaining.first() {
            self.switch(index);
        }
        pending.saving = false;
        self.pending = Some(pending);
    }

    fn close_active(&mut self) {
        self.tabs.remove(self.active);
        if self.tabs.is_empty() {
            self.tabs.push(Tab {
                editor: App::open(None).expect("empty document"),
                number: self.next_number,
                identity: None,
            });
            self.next_number += 1;
        }
        self.active = self.active.min(self.tabs.len() - 1);
        self.tab_hits.clear();
    }

    fn same_open_path(&mut self, path: &Path, except: Option<usize>) -> io::Result<Option<usize>> {
        let target = path_identity(path)?;
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            if Some(index) == except {
                continue;
            }
            tab.refresh_identity();
            if tab.identity.as_ref() == Some(&target) {
                return Ok(Some(index));
            }
        }
        Ok(None)
    }

    fn check_save_target(&mut self, event: &Event) -> bool {
        if !matches!(event, Event::Key(key) if key.kind != KeyEventKind::Release && key.code == KeyCode::Enter)
        {
            return true;
        }
        let Some(path) = self.editor().pending_save_path() else {
            return true;
        };
        match self.same_open_path(&path, Some(self.active)) {
            Ok(None) => true,
            Ok(Some(_)) => {
                self.editor_mut()
                    .set_message("That path is open in another tab. Choose a different filename.");
                false
            }
            Err(error) => {
                self.editor_mut().set_message(error.to_string());
                false
            }
        }
    }

    pub fn handle_event(&mut self, event: Event) {
        if matches!(event, Event::Key(key) if key.kind == KeyEventKind::Release) {
            return;
        }
        if self.browser.is_some()
            && matches!(&event, Event::Key(key) if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q'))
        {
            self.browser = None;
            self.request(Intent::Quit);
            return;
        }
        if let Some(browser) = &mut self.browser {
            let action = browser.handle_event(event, &mut self.clipboard);
            match action {
                BrowserAction::Close => self.browser = None,
                BrowserAction::Open(path) => match self.open_path(path) {
                    Ok(()) => self.browser = None,
                    Err(error) => self.browser.as_mut().unwrap().set_error(error),
                },
                BrowserAction::None => {}
            }
            self.tab_hits.clear();
            return;
        }
        if self.pending.as_ref().is_some_and(|pending| !pending.saving) {
            if let Event::Key(key) = event {
                if key.kind != KeyEventKind::Press
                    || !(key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT)
                {
                    return;
                }
                match key.code {
                    KeyCode::Esc => {
                        self.pending = None;
                    }
                    KeyCode::Char('n' | 'N') => self.advance(true),
                    KeyCode::Char('y' | 'Y') => {
                        self.editor_mut().save_for_workspace();
                        if !self.editor().document.is_dirty() {
                            self.advance(true);
                        } else if self.editor().saving_as() {
                            self.pending.as_mut().unwrap().saving = true;
                        } else {
                            self.pending = None;
                        }
                    }
                    _ => {}
                }
            } else if matches!(event, Event::Resize(..)) {
                self.tab_hits.clear();
                self.forward(event);
            }
            return;
        }
        if self.pending.as_ref().is_some_and(|pending| pending.saving) {
            if !self.check_save_target(&event) {
                return;
            }
            self.forward(event);
            if !self.editor().document.is_dirty() {
                self.advance(true);
            } else if !self.editor().saving_as() {
                self.pending = None;
            }
            return;
        }
        if self.editor().workspace_commands_allowed() {
            if let Event::Key(key) = &event {
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                match key.code {
                    KeyCode::Char('n') if ctrl => {
                        self.new_tab();
                        return;
                    }
                    KeyCode::Char('w') if ctrl => {
                        self.request(Intent::Close);
                        return;
                    }
                    KeyCode::Char('o') if ctrl => {
                        self.open_browser();
                        return;
                    }
                    KeyCode::Char('q') if ctrl => {
                        self.request(Intent::Quit);
                        return;
                    }
                    KeyCode::F(7) | KeyCode::PageUp if key.code == KeyCode::F(7) || ctrl => {
                        self.switch((self.active + self.tabs.len() - 1) % self.tabs.len());
                        return;
                    }
                    KeyCode::F(8) | KeyCode::PageDown if key.code == KeyCode::F(8) || ctrl => {
                        self.switch((self.active + 1) % self.tabs.len());
                        return;
                    }
                    _ => {}
                }
            }
            if let Event::Mouse(mouse) = &event
                && mouse.row == 0
            {
                if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                    && let Some(index) = self.tab_hits.iter().find_map(|(rect, index)| {
                        rect.contains((mouse.column, mouse.row).into())
                            .then_some(*index)
                    })
                {
                    self.switch(index);
                }
                return;
            }
        }
        if !self.check_save_target(&event) {
            return;
        }
        self.forward(event);
    }

    fn forward(&mut self, event: Event) {
        self.tab_hits.clear();
        let save_target = if matches!(&event, Event::Key(key) if key.code == KeyCode::Enter) {
            self.editor()
                .pending_save_path()
                .and_then(|path| path_identity(&path).ok().map(|identity| (path, identity)))
        } else {
            None
        };
        self.tabs[self.active]
            .editor
            .workspace_event(event, &mut self.clipboard);
        if let Some((path, identity)) = save_target
            && !self.editor().saving_as()
            && self.editor().path() == Some(path.as_path())
        {
            // Capture the already-validated target even if access is lost
            // immediately after saving, then refresh it when possible.
            self.tabs[self.active].identity = Some(identity);
            self.tabs[self.active].refresh_identity();
        }
    }

    fn label(&self, index: usize) -> String {
        let tab = &self.tabs[index];
        let filename = tab
            .editor
            .path()
            .map(|path| {
                let name = path.file_name().unwrap_or(path.as_os_str());
                let duplicated = self.tabs.iter().enumerate().any(|(other, tab)| {
                    other != index
                        && tab
                            .editor
                            .path()
                            .is_some_and(|p| p.file_name() == path.file_name())
                });
                if duplicated {
                    path.parent().and_then(Path::file_name).map_or_else(
                        || path.display().to_string(),
                        |parent| format!("{}/{}", parent.to_string_lossy(), name.to_string_lossy()),
                    )
                } else {
                    name.to_string_lossy().into_owned()
                }
            })
            .unwrap_or_else(|| format!("Untitled {}.md", tab.number));
        format!(
            "{}{} ",
            if tab.editor.document.is_dirty() {
                "* "
            } else {
                " "
            },
            safe_text(&filename)
        )
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let show_cursor = self.browser.is_none() && self.pending.as_ref().is_none_or(|p| p.saving);
        self.editor_mut().draw_with_cursor(frame, show_cursor);
        self.tab_hits.clear();
        let area = frame.area();
        if area.height > 0 && area.width > 0 {
            frame.render_widget(Clear, Rect::new(area.x, area.y, area.width, 1));
            let width = usize::from(area.width);
            let labels: Vec<_> = (0..self.tabs.len())
                .map(|i| clipped(&self.label(i), width.min(30)))
                .collect();
            let mut start = self.active;
            let mut used = UnicodeWidthStr::width(labels[start].as_str());
            while start > 0
                && used + UnicodeWidthStr::width(labels[start - 1].as_str()) + 2 <= width
            {
                start -= 1;
                used += UnicodeWidthStr::width(labels[start].as_str());
            }
            let mut x = area.x;
            if start > 0 && width > 3 {
                frame
                    .buffer_mut()
                    .set_string(x, area.y, "‹ ", Style::default());
                x += 2;
            }
            for (index, label) in labels.iter().enumerate().skip(start) {
                let available = usize::from(area.right().saturating_sub(x));
                if available == 0 {
                    break;
                }
                let label = if available == 1 && self.tabs[index].editor.document.is_dirty() {
                    "*".to_owned()
                } else {
                    clipped(label, available)
                };
                let cells = UnicodeWidthStr::width(label.as_str()) as u16;
                let style = if index == self.active {
                    Style::default().add_modifier(Modifier::UNDERLINED | Modifier::BOLD)
                } else {
                    Style::default().add_modifier(Modifier::DIM)
                };
                frame.buffer_mut().set_string(x, area.y, &label, style);
                self.tab_hits.push((Rect::new(x, area.y, cells, 1), index));
                x += cells;
                if x >= area.right() {
                    break;
                }
            }
        }
        if let Some(pending) = &self.pending
            && !pending.saving
        {
            let body = format!(
                "{}\n\nSave changes {}?\n\nY: Save   N: Discard   Esc: Cancel\n\n{}",
                self.label(self.active).trim(),
                if pending.intent == Intent::Quit {
                    "before quitting"
                } else {
                    "before closing"
                },
                if pending.intent == Intent::Quit {
                    "Other dirty tabs will be checked next.\nCancel keeps all tabs, including earlier discard choices."
                } else {
                    "Only this document will close."
                }
            );
            popup(frame, " Unsaved document ", &body);
        }
        if let Some(browser) = &mut self.browser {
            browser.draw(frame);
        }
    }
}

pub(crate) fn popup(frame: &mut Frame, title: &str, body: &str) {
    let area = frame.area();
    let width = area.width.saturating_sub(2).min(76);
    let height = area.height.saturating_sub(2).min(12);
    if width < 4 || height < 3 {
        return;
    }
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(body)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(title)),
        rect,
    );
}

pub(crate) fn clipped(text: &str, max: usize) -> String {
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

/// Resolve existing aliases and normalize new targets through their nearest
/// existing ancestor, so missing filenames still have one workspace identity.
pub(crate) fn path_identity(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    match std::fs::canonicalize(&absolute) {
        Ok(path) => return Ok(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    // Resolve each existing prefix before interpreting `..`: a symlink may
    // point outside its lexical parent. Missing suffixes remain normalized.
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => {
                normalized.push(component.as_os_str());
                match std::fs::canonicalize(&normalized) {
                    Ok(path) => normalized = path,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, MouseEvent};
    use ratatui::{Terminal, backend::TestBackend};

    fn key(app: &mut Workspace, code: KeyCode, modifiers: KeyModifiers) {
        app.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
    }
    fn ctrl(app: &mut Workspace, c: char) {
        key(app, KeyCode::Char(c), KeyModifiers::CONTROL);
    }
    fn plain(app: &mut Workspace, c: char) {
        key(app, KeyCode::Char(c), KeyModifiers::NONE);
    }
    fn draw(app: &mut Workspace, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        terminal
    }

    #[test]
    fn tabs_preserve_source_selection_undo_view_and_scroll() {
        let mut app = Workspace::open(None).unwrap();
        app.handle_event(Event::Paste(
            (0..60).map(|i| format!("line {i}\n")).collect(),
        ));
        ctrl(&mut app, 'e');
        draw(&mut app, 60, 16);
        let selection = app.editor().document.selection();
        let scroll = app.editor().scroll;
        ctrl(&mut app, 'n');
        app.handle_event(Event::Paste("second 👩🏽‍💻".into()));
        key(&mut app, KeyCode::F(7), KeyModifiers::NONE);
        assert_eq!(app.editor().document.selection(), selection);
        assert_eq!(app.editor().scroll, scroll);
        assert!(!app.editor().live);
        ctrl(&mut app, 'z');
        assert_eq!(app.editor().document.text(), "");
        key(&mut app, KeyCode::PageDown, KeyModifiers::CONTROL);
        assert_eq!(app.editor().document.text(), "second 👩🏽‍💻");
        assert!(app.editor().live);
    }

    #[test]
    fn clipboard_is_global_and_survives_closing_origin() {
        let mut app = Workspace::open(None).unwrap();
        app.handle_event(Event::Paste("copied".into()));
        ctrl(&mut app, 'a');
        ctrl(&mut app, 'c');
        ctrl(&mut app, 'w');
        plain(&mut app, 'n');
        assert_eq!(app.tabs.len(), 1);
        ctrl(&mut app, 'v');
        assert_eq!(app.editor().document.text(), "copied");
        ctrl(&mut app, 'n');
        ctrl(&mut app, 'v');
        assert_eq!(app.editor().document.text(), "copied");
    }

    #[test]
    fn quit_cancel_keeps_previously_discarded_tabs() {
        let mut app = Workspace::open(None).unwrap();
        app.handle_event(Event::Paste("first".into()));
        ctrl(&mut app, 'n');
        app.handle_event(Event::Paste("second".into()));
        ctrl(&mut app, 'q');
        assert_eq!(app.active, 0);
        plain(&mut app, 'n');
        assert_eq!(app.active, 1);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!app.should_exit);
        assert_eq!(app.tabs.len(), 2);
        assert!(app.tabs.iter().all(|tab| tab.editor.document.is_dirty()));
        assert_eq!(app.tabs[0].editor.document.text(), "first");
        ctrl(&mut app, 'q');
        plain(&mut app, 'n');
        plain(&mut app, 'n');
        assert!(app.should_exit);
    }

    #[test]
    fn close_save_as_cancel_and_failure_keep_document() {
        let mut app = Workspace::open(None).unwrap();
        app.handle_event(Event::Paste("unsaved".into()));
        ctrl(&mut app, 'w');
        plain(&mut app, 'y');
        assert!(app.editor().saving_as());
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.pending.is_none());
        assert_eq!(app.editor().document.text(), "unsaved");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, "original").unwrap();
        let mut app = Workspace::open(Some(path.clone())).unwrap();
        app.handle_event(Event::Paste("local".into()));
        std::fs::write(&path, "external").unwrap();
        ctrl(&mut app, 'q');
        plain(&mut app, 'y');
        assert!(!app.should_exit);
        assert!(app.pending.is_none());
        assert!(app.editor().document.is_dirty());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "external");
    }

    #[test]
    fn close_save_as_succeeds_and_existing_tab_path_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("new.md");
        let mut app = Workspace::open(Some(target.clone())).unwrap();
        ctrl(&mut app, 'n');
        app.handle_event(Event::Paste("second".into()));
        key(&mut app, KeyCode::F(4), KeyModifiers::NONE);
        app.handle_event(Event::Paste(target.display().to_string()));
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.editor().saving_as());
        assert!(!target.exists());
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        ctrl(&mut app, 'w');
        plain(&mut app, 'y');
        let other = dir.path().join("other.md");
        app.handle_event(Event::Paste(other.display().to_string()));
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(std::fs::read_to_string(other).unwrap(), "second");
    }

    #[test]
    fn real_tab_hit_geometry_changes_active_once_and_resize_invalidates() {
        let mut app = Workspace::open(None).unwrap();
        ctrl(&mut app, 'n');
        draw(&mut app, 80, 24);
        let rect = app
            .tab_hits
            .iter()
            .find(|(_, index)| *index == 0)
            .unwrap()
            .0;
        let mouse = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        };
        app.handle_event(Event::Mouse(mouse));
        assert_eq!(app.active, 0);
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            ..mouse
        }));
        assert_eq!(app.active, 0);
        draw(&mut app, 80, 24);
        let second = app
            .tab_hits
            .iter()
            .find(|(_, index)| *index == 1)
            .unwrap()
            .0;
        app.handle_event(Event::Resize(3, 6));
        app.handle_event(Event::Mouse(MouseEvent {
            column: second.x,
            ..mouse
        }));
        assert_eq!(app.active, 0);
        for size in [(1, 1), (3, 6), (12, 8), (80, 24)] {
            draw(&mut app, size.0, size.1);
        }
    }

    #[test]
    fn tab_switch_closes_search_and_keeps_document_selection() {
        let mut app = Workspace::open(None).unwrap();
        app.handle_event(Event::Paste("match match".into()));
        ctrl(&mut app, 'f');
        app.handle_event(Event::Paste("match".into()));
        draw(&mut app, 80, 24);
        let selection = app.editor().document.selection();
        ctrl(&mut app, 'n');
        key(&mut app, KeyCode::F(7), KeyModifiers::NONE);
        assert_eq!(app.editor().document.selection(), selection);
        plain(&mut app, 'x');
        assert_eq!(app.editor().document.text(), "x match");
    }

    #[test]
    fn quiet_tabs_keep_identity_dirty_state_and_view_in_footer() {
        let mut app = Workspace::open(None).unwrap();
        app.handle_event(Event::Paste("draft".into()));
        ctrl(&mut app, 'n');
        key(&mut app, KeyCode::F(7), KeyModifiers::NONE);
        let terminal = draw(&mut app, 80, 24);
        let active = app
            .tab_hits
            .iter()
            .find(|(_, index)| *index == app.active)
            .unwrap()
            .0;
        let first = &terminal.backend().buffer()[(active.x, active.y)];
        assert_eq!(first.symbol(), "*");
        assert!(
            first
                .modifier
                .contains(Modifier::BOLD | Modifier::UNDERLINED)
        );
        assert!(!first.modifier.contains(Modifier::REVERSED));
        let snapshot = crate::simulation::snapshot(&mut app, 80, 24).unwrap();
        assert!(snapshot.lines().next().unwrap().contains("* Untitled 1.md"));
        assert!(!snapshot.lines().next().unwrap().contains("LIVE"));
        assert!(snapshot.contains("Live · Ln"));
        assert!(!snapshot.contains("clipboard"));
        ctrl(&mut app, 'e');
        let snapshot = crate::simulation::snapshot(&mut app, 80, 24).unwrap();
        assert!(snapshot.contains("Source · Ln"));
        assert!(!snapshot.lines().next().unwrap().contains("SOURCE"));
    }

    #[test]
    fn modified_or_repeated_confirmation_keys_never_discard_or_save() {
        let mut app = Workspace::open(None).unwrap();
        app.handle_event(Event::Paste("valuable".into()));
        ctrl(&mut app, 'w');
        for modifiers in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
            key(&mut app, KeyCode::Char('n'), modifiers);
            key(&mut app, KeyCode::Char('y'), modifiers);
        }
        for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
            app.handle_event(Event::Key(KeyEvent::new_with_kind(
                KeyCode::Char('n'),
                KeyModifiers::NONE,
                kind,
            )));
        }
        assert!(app.pending.is_some());
        assert_eq!(app.editor().document.text(), "valuable");
        assert!(!app.editor().saving_as());
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        ctrl(&mut app, 'n');
        app.handle_event(Event::Paste("second".into()));
        ctrl(&mut app, 'q');
        ctrl(&mut app, 'n');
        assert_eq!(app.active, 0);
        plain(&mut app, 'n');
        assert_eq!(app.active, 1);
        app.handle_event(Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('n'),
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )));
        assert!(!app.should_exit);
        assert_eq!(app.tabs[0].editor.document.text(), "valuable");
    }

    #[test]
    fn clipped_tabs_always_keep_the_dirty_marker() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = Workspace::open(Some(
            dir.path()
                .join("very-long-filename-repeated-repeated-repeated.md"),
        ))
        .unwrap();
        app.handle_event(Event::Paste("dirty".into()));
        for width in [1, 2, 8, 30, 80] {
            let terminal = draw(&mut app, width, 24);
            assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), "*");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_parent_semantics_protect_missing_save_targets_and_reuse_open_tabs() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("real/child")).unwrap();
        symlink(dir.path().join("real/child"), dir.path().join("alias")).unwrap();
        let target = dir.path().join("real/note.md");
        let alias = dir.path().join("alias/../note.md");
        assert_eq!(
            path_identity(&target).unwrap(),
            path_identity(&alias).unwrap()
        );
        let mut app = Workspace::open(Some(target.clone())).unwrap();
        app.handle_event(Event::Paste("first".into()));
        ctrl(&mut app, 'n');
        app.handle_event(Event::Paste("second".into()));
        key(&mut app, KeyCode::F(4), KeyModifiers::NONE);
        app.handle_event(Event::Paste(alias.display().to_string()));
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.editor().saving_as());
        assert!(!target.exists());
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        std::fs::write(&target, "disk").unwrap();
        app.open_path(alias).unwrap();
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active, 0);
        assert_eq!(app.editor().document.text(), "first");
    }

    #[test]
    fn browser_direct_open_reuses_tabs_and_failure_preserves_all_buffers() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("first.md");
        let second = dir.path().join("second.txt");
        std::fs::write(&original, "first").unwrap();
        std::fs::write(&second, "plain\r\ntext").unwrap();
        let mut app = Workspace::open(Some(original.clone())).unwrap();
        app.handle_event(Event::Paste("local ".into()));
        ctrl(&mut app, 'o');
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        app.handle_event(Event::Paste(second.display().to_string()));
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.browser.is_none());
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.editor().document.text(), "plain\r\ntext");
        assert!(!app.editor().live);
        app.open_path(dir.path().join("./first.md")).unwrap();
        assert_eq!(app.active, 0);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.editor().document.text(), "local first");
        let binary = dir.path().join("binary.md");
        std::fs::write(&binary, b"nul\0body").unwrap();
        ctrl(&mut app, 'o');
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        app.handle_event(Event::Paste(binary.display().to_string()));
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.browser.is_some());
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.editor().document.text(), "local first");
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        let oversized = dir.path().join("big.md");
        let file = std::fs::File::create(&oversized).unwrap();
        file.set_len(8 * 1024 * 1024 + 1).unwrap();
        assert!(
            app.open_path(oversized)
                .unwrap_err()
                .to_string()
                .contains("open limit")
        );
        assert_eq!(app.tabs.len(), 2);
    }

    #[test]
    fn quit_from_browser_checks_dirty_tabs() {
        let mut app = Workspace::open(None).unwrap();
        app.handle_event(Event::Paste("draft".into()));
        ctrl(&mut app, 'o');
        ctrl(&mut app, 'q');
        assert!(app.browser.is_none());
        assert!(app.pending.is_some());
        assert!(!app.should_exit);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.editor().document.text(), "draft");
    }
    #[cfg(unix)]
    #[test]
    fn inaccessible_old_tab_does_not_block_browser_open_or_save_as() {
        use std::{fs, os::unix::fs::PermissionsExt};
        struct RestorePermissions(PathBuf, fs::Permissions);
        impl Drop for RestorePermissions {
            fn drop(&mut self) {
                let _ = fs::set_permissions(&self.0, self.1.clone());
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        fs::create_dir(&locked).unwrap();
        let original = locked.join("old.md");
        let other = dir.path().join("other.md");
        let saved = dir.path().join("saved.md");
        fs::write(&original, "original").unwrap();
        fs::write(&other, "other").unwrap();
        let mut app = Workspace::open(Some(original.clone())).unwrap();
        app.handle_event(Event::Paste("draft ".into()));
        let selection = app.editor().document.selection();
        ctrl(&mut app, 'n');
        let restore =
            RestorePermissions(locked.clone(), fs::metadata(&locked).unwrap().permissions());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        match path_identity(&original) {
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {}
            Ok(_) => {
                eprintln!(
                    "permission regression skipped: this user can traverse chmod000 directories"
                );
                return;
            }
            Err(error) => panic!("unexpected filesystem error: {error}"),
        }
        // Requested targets remain strict even when a matching identity is cached.
        assert_eq!(
            app.open_path(original.clone()).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(app.tabs.len(), 2);
        ctrl(&mut app, 'o');
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        app.handle_event(Event::Paste(other.display().to_string()));
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.browser.is_none());
        assert_eq!(app.editor().document.text(), "other");
        assert_eq!(app.tabs.len(), 3);
        app.switch(1);
        app.handle_event(Event::Paste("new draft".into()));
        key(&mut app, KeyCode::F(4), KeyModifiers::NONE);
        app.handle_event(Event::Paste(saved.display().to_string()));
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(!app.editor().saving_as());
        assert!(!app.editor().document.is_dirty());
        assert_eq!(fs::read_to_string(&saved).unwrap(), "new draft");
        assert_eq!(app.tabs[1].identity, Some(path_identity(&saved).unwrap()));
        drop(restore);
        app.open_path(original).unwrap();
        assert_eq!(app.active, 0);
        assert_eq!(app.tabs.len(), 3);
        assert_eq!(app.editor().document.text(), "draft original");
        assert_eq!(app.editor().document.selection(), selection);
        assert!(app.editor().document.is_dirty());
        ctrl(&mut app, 'z');
        assert_eq!(app.editor().document.text(), "original");
        ctrl(&mut app, 'y');
        assert_eq!(app.editor().document.text(), "draft original");
        app.open_path(saved).unwrap();
        assert_eq!(app.active, 1);
        assert_eq!(app.tabs.len(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn cached_identity_keeps_alias_duplicates_blocked_while_old_parent_is_inaccessible() {
        use std::{
            fs,
            os::unix::fs::{PermissionsExt, symlink},
        };
        struct RestorePermissions(PathBuf, fs::Permissions);
        impl Drop for RestorePermissions {
            fn drop(&mut self) {
                let _ = fs::set_permissions(&self.0, self.1.clone());
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        fs::create_dir(&locked).unwrap();
        let original = dir.path().join("real.md");
        let alias = locked.join("alias.md");
        fs::write(&original, "original").unwrap();
        symlink(&original, &alias).unwrap();
        let mut app = Workspace::open(Some(alias.clone())).unwrap();
        app.handle_event(Event::Paste("draft ".into()));
        ctrl(&mut app, 'n');
        let _restore =
            RestorePermissions(locked.clone(), fs::metadata(&locked).unwrap().permissions());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        if path_identity(&alias).is_ok() {
            eprintln!("permission regression skipped: this user can traverse chmod000 directories");
            return;
        }
        app.open_path(original.clone()).unwrap();
        assert_eq!(app.active, 0);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.editor().document.text(), "draft original");
        app.switch(1);
        app.handle_event(Event::Paste("replacement".into()));
        key(&mut app, KeyCode::F(4), KeyModifiers::NONE);
        app.handle_event(Event::Paste(original.display().to_string()));
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.editor().saving_as());
        let snapshot = crate::simulation::snapshot(&mut app, 100, 24).unwrap();
        assert!(snapshot.contains("open in another tab"));
        assert_eq!(fs::read_to_string(original).unwrap(), "original");
    }
}
