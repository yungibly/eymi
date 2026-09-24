//! Document workspace. Each tab owns its editor; clipboard and quit intent are session-wide.
mod matching;
mod navigation;
mod palette;
mod persistence;
mod picker;
mod sidebar;
#[cfg(test)]
mod state_tests;
mod tabs;
use crate::theme::{Theme, current_theme};
pub(crate) use crate::ui::clipped;
use crate::{
    app::App,
    browser::{Action as BrowserAction, Browser},
    clipboard::Clipboard,
    projection::safe_text,
};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use palette::{Action as PaletteAction, Command, Palette};
use persistence::Persistence;
use picker::{Action as ChoiceAction, Picker};
use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
};
use sidebar::{Sidebar, Target};
use std::time::Instant;
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
    recovered: Option<(String, Option<PathBuf>)>,
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

enum ChoiceMode {
    Theme {
        original: Theme,
        themes: Vec<Theme>,
    },
    Recovery,
    Documents {
        numbers: Vec<usize>,
    },
    Headings {
        number: usize,
        revision: u64,
        offsets: Vec<usize>,
    },
}
struct Choice {
    picker: Picker,
    mode: ChoiceMode,
}

pub struct Workspace {
    tabs: Vec<Tab>,
    active: usize,
    next_number: usize,
    clipboard: Clipboard,
    pending: Option<Pending>,
    browser: Option<Browser>,
    tab_hits: Vec<(Rect, tabs::Target)>,
    sidebar: Sidebar,
    palette: Option<Palette>,
    choice: Option<Choice>,
    persistence: Option<Persistence>,
    last_tick: Instant,
    area: Rect,
    layout_valid: bool,
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
                recovered: None,
            }],
            active: 0,
            next_number: 2,
            clipboard: Clipboard::internal(),
            pending: None,
            browser: None,
            tab_hits: vec![],
            sidebar: Sidebar::default(),
            palette: None,
            choice: None,
            persistence: None,
            last_tick: Instant::now(),
            area: Rect::new(0, 0, 80, 24),
            layout_valid: false,
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
        self.sidebar.selected = 0;
        self.tab_hits.clear();
        self.sidebar.invalidate();
    }

    fn new_tab(&mut self) {
        self.editor_mut().deactivate();
        self.tabs.push(Tab {
            editor: App::open(None).expect("empty document"),
            number: self.next_number,
            identity: None,
            recovered: None,
        });
        self.next_number += 1;
        self.active = self.tabs.len() - 1;
        self.sidebar.selected = 0;
        self.tab_hits.clear();
        self.sidebar.invalidate();
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
        self.sidebar.invalidate();
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
            recovered: None,
        });
        self.next_number += 1;
        self.active = self.tabs.len() - 1;
        self.sidebar.selected = 0;
        self.tab_hits.clear();
        self.sidebar.invalidate();
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
        if accepted && !self.editor().document.is_dirty() {
            self.clear_recovery(self.tabs[self.active].number);
        }
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
        self.clear_recovery(self.tabs[self.active].number);
        self.tabs.remove(self.active);
        if self.tabs.is_empty() {
            self.tabs.push(Tab {
                editor: App::open(None).expect("empty document"),
                number: self.next_number,
                identity: None,
                recovered: None,
            });
            self.next_number += 1;
        }
        self.active = self.active.min(self.tabs.len() - 1);
        self.tab_hits.clear();
        self.sidebar.invalidate();
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

    fn refresh_outline(&mut self) {
        let tab = &self.tabs[self.active];
        let markdown = tab.editor.is_markdown();
        self.sidebar.refresh(
            tab.number,
            tab.editor.document.revision(),
            markdown,
            tab.editor.document.text(),
        );
    }

    fn open_palette(&mut self, line_mode: bool) {
        self.editor_mut().deactivate();
        self.sidebar.focused = false;
        self.palette = Some(Palette::new(line_mode));
        self.tab_hits.clear();
        self.sidebar.invalidate();
    }

    fn activate_sidebar(&mut self, target: Target) {
        match target {
            Target::Heading(index) => {
                if let Some(heading) = self.sidebar.headings.get(index) {
                    let offset = heading.offset;
                    self.editor_mut().jump_to_source(offset);
                }
            }
        }
        self.sidebar.focused = false;
        self.sidebar.invalidate();
    }

    fn toggle_sidebar(&mut self, focus: bool) {
        let visible = self.sidebar.visible(self.area, self.editor().is_markdown());
        if focus && visible && !self.sidebar.focused {
            self.editor_mut().deactivate();
            self.sidebar.focused = true;
            self.sidebar.selected = 0;
        } else {
            self.sidebar.preference = Some(!visible);
            self.persist_sidebar();
            self.sidebar.focused = focus && !visible;
            self.layout_valid = false;
        }
        if !self.sidebar.visible(self.area, self.editor().is_markdown()) {
            self.sidebar.focused = false;
        }
        if self.sidebar.focused {
            self.editor_mut().deactivate();
            self.refresh_outline();
            let caret = self.editor().document.selection().head;
            self.sidebar.select_current(caret);
        }
        self.sidebar.invalidate();
    }

    fn run_command(&mut self, command: Command) {
        self.palette = None;
        self.sidebar.focused = false;
        let (code, modifiers) = match command {
            Command::New => (KeyCode::Char('n'), KeyModifiers::CONTROL),
            Command::Open => (KeyCode::Char('o'), KeyModifiers::CONTROL),
            Command::Save => (KeyCode::Char('s'), KeyModifiers::CONTROL),
            Command::SaveAs => (KeyCode::F(4), KeyModifiers::NONE),
            Command::Close => (KeyCode::Char('w'), KeyModifiers::CONTROL),
            Command::Find => (KeyCode::Char('f'), KeyModifiers::CONTROL),
            Command::Replace => (KeyCode::Char('r'), KeyModifiers::CONTROL),
            Command::View => (KeyCode::F(6), KeyModifiers::NONE),
            Command::Sidebar => {
                self.toggle_sidebar(false);
                return;
            }
            Command::NextTab => (KeyCode::F(8), KeyModifiers::NONE),
            Command::PreviousTab => (KeyCode::F(7), KeyModifiers::NONE),
            Command::GoToLine => {
                self.open_palette(true);
                return;
            }
            Command::Documents => {
                self.open_documents();
                return;
            }
            Command::Headings => {
                self.open_headings();
                return;
            }
            Command::Help => (KeyCode::F(1), KeyModifiers::NONE),
            Command::Quit => (KeyCode::Char('q'), KeyModifiers::CONTROL),
            Command::Undo => (KeyCode::Char('z'), KeyModifiers::CONTROL),
            Command::Redo => (KeyCode::Char('y'), KeyModifiers::CONTROL),
            Command::Bold => (KeyCode::Char('b'), KeyModifiers::CONTROL),
            Command::Italic => (KeyCode::Char('i'), KeyModifiers::ALT),
            Command::InlineCode => (KeyCode::Char('`'), KeyModifiers::ALT),
            Command::Indent => (KeyCode::Char(']'), KeyModifiers::CONTROL),
            Command::Outdent => (KeyCode::Char('['), KeyModifiers::CONTROL),
            Command::MoveUp => (KeyCode::Up, KeyModifiers::ALT),
            Command::MoveDown => (KeyCode::Down, KeyModifiers::ALT),
            Command::DuplicateUp => (KeyCode::Up, KeyModifiers::ALT | KeyModifiers::SHIFT),
            Command::DuplicateDown => (KeyCode::Down, KeyModifiers::ALT | KeyModifiers::SHIFT),
            Command::Theme => {
                self.open_themes();
                return;
            }
            Command::Icons => {
                let next = match crate::icons::current() {
                    crate::icons::IconSet::Plain => crate::icons::IconSet::Nerd,
                    crate::icons::IconSet::Nerd => crate::icons::IconSet::Plain,
                };
                crate::icons::set(next);
                self.persist_icons();
                self.layout_valid = false;
                return;
            }
            Command::Reload => (KeyCode::F(5), KeyModifiers::NONE),
            Command::Recover => {
                self.open_recovery();
                return;
            }
        };
        self.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
    }

    fn open_themes(&mut self) {
        self.editor_mut().deactivate();
        let themes: Vec<_> = Theme::all().collect();
        let original = current_theme();
        let selected = themes
            .iter()
            .position(|theme| *theme == original)
            .unwrap_or(0);
        let entries = themes
            .iter()
            .map(|theme| {
                (
                    theme.name().to_owned(),
                    if theme.is_dark() { "Dark" } else { "Light" }.to_owned(),
                )
            })
            .collect();
        self.choice = Some(Choice {
            picker: Picker::new(
                "Themes",
                "↑↓ preview · ⏎ apply · esc cancel",
                entries,
                selected,
            ),
            mode: ChoiceMode::Theme { original, themes },
        });
        self.tab_hits.clear();
        self.sidebar.invalidate();
    }

    fn handle_choice(&mut self, event: Event) {
        let mut choice = self.choice.take().expect("open picker");
        match choice.picker.handle(event) {
            ChoiceAction::Cancel => {
                if let ChoiceMode::Theme { original, .. } = choice.mode {
                    self.editor_mut().set_theme(original);
                }
            }
            ChoiceAction::Quit => {
                if let ChoiceMode::Theme { original, .. } = choice.mode {
                    self.editor_mut().set_theme(original);
                }
                self.request(Intent::Quit);
            }
            ChoiceAction::Accept(index) => match choice.mode {
                ChoiceMode::Theme { themes, .. } => {
                    if let Some(theme) = themes.get(index) {
                        self.editor_mut().set_theme(*theme);
                        self.persist_theme(*theme);
                    }
                }
                ChoiceMode::Recovery => self.recover_document(index),
                ChoiceMode::Documents { numbers } => {
                    if let Some(number) = numbers.get(index)
                        && let Some(target) = self.tabs.iter().position(|tab| tab.number == *number)
                    {
                        self.switch(target);
                    }
                }
                ChoiceMode::Headings {
                    number,
                    revision,
                    offsets,
                } => {
                    if self.tabs[self.active].number == number
                        && self.editor().document.revision() == revision
                    {
                        if let Some(&offset) = offsets.get(index) {
                            self.editor_mut().jump_to_source(offset);
                        }
                    } else {
                        self.editor_mut()
                            .set_message("Document changed; reopen headings to choose a section.");
                    }
                }
            },
            ChoiceAction::Preview(index) => {
                if let ChoiceMode::Theme { themes, .. } = &choice.mode
                    && let Some(theme) = themes.get(index)
                {
                    self.editor_mut().set_theme(*theme);
                }
                self.choice = Some(choice);
            }
            ChoiceAction::None => self.choice = Some(choice),
        }
    }

    pub fn handle_event(&mut self, event: Event) {
        if matches!(event, Event::Key(key) if key.kind == KeyEventKind::Release) {
            return;
        }
        // Workspace commands may return without reaching App, including a tab
        // switch to the already active tab. They still end a typing transaction.
        let typing = matches!(&event, Event::Key(key)
            if matches!(key.code, KeyCode::Char(_))
                && !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT));
        if !typing {
            self.editor_mut().document.break_undo_group();
        }
        if let Event::Resize(width, height) = &event {
            self.area = Rect::new(0, 0, *width, *height);
            self.layout_valid = false;
            self.tab_hits.clear();
            self.sidebar.invalidate();
            if !self.sidebar.visible(self.area, self.editor().is_markdown()) {
                self.sidebar.focused = false;
            }
        }
        if matches!(event, Event::Mouse(_)) && !self.layout_valid {
            return;
        }
        if self.choice.is_some() {
            self.handle_choice(event);
            return;
        }
        if let Some(palette) = &mut self.palette {
            match palette.handle(event) {
                PaletteAction::Close => self.palette = None,
                PaletteAction::Command(command) => self.run_command(command),
                PaletteAction::Line(line) => {
                    let source = self.editor().document.text();
                    let mut current = 1;
                    let mut offset = source.len();
                    if line == 1 {
                        offset = 0;
                    } else {
                        for (index, grapheme) in source.grapheme_indices(true) {
                            if matches!(grapheme, "\r" | "\n" | "\r\n") {
                                current += 1;
                                if current == line {
                                    offset = index + grapheme.len();
                                    break;
                                }
                            }
                        }
                    }
                    self.palette = None;
                    self.editor_mut().jump_to_source(offset);
                }
                PaletteAction::None => {}
            }
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
            self.sidebar.invalidate();
            return;
        }
        if self.pending.as_ref().is_some_and(|pending| !pending.saving) {
            if !crate::ui::dialog_fits(self.area)
                && matches!(&event, Event::Key(key) if key.code != KeyCode::Esc)
            {
                return;
            }
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
                self.sidebar.invalidate();
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
        if self.editor().has_modal() && matches!(&event, Event::Mouse(_)) {
            self.forward(event);
            return;
        }
        if self.editor().workspace_commands_allowed() {
            if let Event::Key(key) = &event {
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                match key.code {
                    KeyCode::F(2) if key.modifiers.is_empty() => {
                        self.open_palette(false);
                        return;
                    }
                    KeyCode::Char('p') if ctrl => {
                        self.open_palette(false);
                        return;
                    }
                    KeyCode::Char('g') if ctrl => {
                        self.open_palette(true);
                        return;
                    }
                    KeyCode::F(9) if key.modifiers.is_empty() => {
                        self.toggle_sidebar(true);
                        return;
                    }
                    KeyCode::F(10) if key.modifiers.is_empty() => {
                        self.open_documents();
                        return;
                    }
                    KeyCode::F(11) if key.modifiers.is_empty() => {
                        self.open_headings();
                        return;
                    }
                    _ => {}
                }
                if self.sidebar.focused {
                    self.refresh_outline();
                    match key.code {
                        KeyCode::Esc | KeyCode::Tab => {
                            self.sidebar.focused = false;
                            return;
                        }
                        KeyCode::Up => {
                            self.sidebar.move_selection(-1);
                            return;
                        }
                        KeyCode::Down => {
                            self.sidebar.move_selection(1);
                            return;
                        }
                        KeyCode::Home => {
                            self.sidebar.selected = 0;
                            return;
                        }
                        KeyCode::End => {
                            self.sidebar.selected = self.sidebar.headings.len().saturating_sub(1);
                            return;
                        }
                        KeyCode::Enter => {
                            if let Some(target) = self.sidebar.target() {
                                self.activate_sidebar(target);
                            }
                            return;
                        }
                        _ if !ctrl && !matches!(key.code, KeyCode::F(_)) => return,
                        _ => self.sidebar.focused = false,
                    }
                }
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
            if self.sidebar.focused && matches!(event, Event::Paste(_)) {
                return;
            }
            if let Event::Mouse(mouse) = &event {
                self.refresh_outline();
                if self.sidebar.area.contains((mouse.column, mouse.row).into()) {
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            let target = self.sidebar.hits.iter().find_map(|(rect, target)| {
                                rect.contains((mouse.column, mouse.row).into())
                                    .then_some(*target)
                            });
                            if let Some(target) = target {
                                self.activate_sidebar(target);
                            }
                        }
                        MouseEventKind::ScrollDown => {
                            if self.sidebar.focused {
                                self.sidebar.move_selection(3);
                            } else {
                                self.sidebar.scroll_rows(3);
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            if self.sidebar.focused {
                                self.sidebar.move_selection(-3);
                            } else {
                                self.sidebar.scroll_rows(-3);
                            }
                        }
                        _ => {}
                    }
                    return;
                }
                if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                    self.sidebar.focused = false;
                }
            }
            if let Event::Mouse(mouse) = &event
                && mouse.row == 0
            {
                if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                    && let Some(target) = self.tab_hits.iter().find_map(|(rect, target)| {
                        rect.contains((mouse.column, mouse.row).into())
                            .then_some(*target)
                    })
                {
                    match target {
                        tabs::Target::Activate(index) => self.switch(index),
                        tabs::Target::NewDocument => self.new_tab(),
                    }
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
        self.sidebar.invalidate();
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
        if !self.editor().document.is_dirty() {
            self.clear_recovery(self.tabs[self.active].number);
        }
    }

    fn label(&self, index: usize) -> String {
        let dirty = self.tabs[index].editor.document.is_dirty();
        format!(
            "{}{} ",
            if dirty {
                format!("{} ", tabs::DIRTY)
            } else {
                " ".into()
            },
            self.name(index)
        )
    }

    /// The tab's display name, disambiguated by parent directory when needed.
    fn name(&self, index: usize) -> String {
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
            .unwrap_or_else(|| {
                tab.recovered
                    .as_ref()
                    .map(|(label, _)| {
                        format!(
                            "Recovered {}",
                            label.rsplit(['/', '\\']).next().unwrap_or(label)
                        )
                    })
                    .unwrap_or_else(|| format!("Untitled {}.md", tab.number))
            });
        safe_text(&filename)
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        self.area = area;
        self.layout_valid = true;
        self.refresh_outline();
        let sidebar_visible = self.sidebar.visible(area, self.editor().is_markdown());
        if !sidebar_visible {
            self.sidebar.focused = false;
        }
        let show_cursor = self.browser.is_none()
            && self.palette.is_none()
            && self.choice.is_none()
            && !self.sidebar.focused
            && self.pending.as_ref().is_none_or(|p| p.saving);
        let sidebar_width = if sidebar_visible {
            if area.width >= 90 { 27 } else { 23 }
        } else {
            0
        };
        let editor_area = Rect::new(
            area.x + sidebar_width,
            area.y,
            area.width.saturating_sub(sidebar_width),
            area.height,
        );
        self.editor_mut().draw_in(frame, editor_area, show_cursor);
        self.tab_hits.clear();
        self.sidebar.invalidate();
        let labels: Vec<_> = (0..self.tabs.len())
            .map(|index| tabs::TabLabel {
                name: self.name(index),
                icon: match self.tabs[index].editor.language() {
                    Some(language) if crate::icons::current() == crate::icons::IconSet::Nerd => {
                        language.icon()
                    }
                    _ => crate::icons::current().file(self.tabs[index].editor.is_markdown()),
                },
                dirty: self.tabs[index].editor.document.is_dirty(),
            })
            .collect();
        self.tab_hits = tabs::draw(frame, area, sidebar_width, &labels, self.active);
        if sidebar_visible {
            // The sidebar's header shares the tab row.
            let sidebar_area =
                Rect::new(area.x, area.y, sidebar_width, area.height.saturating_sub(1));
            let caret = self.editor().document.selection().head;
            self.sidebar.draw(frame, sidebar_area, caret);
        }
        self.editor().draw_footer(frame, area);
        if self.editor().has_modal() {
            self.editor().draw_overlay(frame);
            self.tab_hits.clear();
            self.sidebar.hits.clear();
        }
        if let Some(pending) = &self.pending
            && !pending.saving
        {
            let quitting = pending.intent == Intent::Quit;
            let muted = crate::ui::muted();
            let mut body = vec![
                Line::from(Span::styled(
                    self.name(self.active),
                    crate::ui::surface().add_modifier(Modifier::BOLD),
                )),
                Line::from(if quitting {
                    "Save changes before quitting?"
                } else {
                    "Save changes before closing?"
                }),
            ];
            if quitting {
                body.push(Line::from(Span::styled(
                    "Other unsaved tabs are checked next; cancelling keeps every tab.",
                    muted,
                )));
            } else {
                body.push(Line::from(Span::styled(
                    "Only this document will close.",
                    muted,
                )));
            }
            crate::ui::dialog(
                frame,
                "Unsaved document",
                &body,
                &[("Y", "Save"), ("N", "Discard"), ("Esc", "Cancel")],
                72,
            );
        }
        if let Some(browser) = &mut self.browser {
            browser.draw(frame);
        }
        if let Some(palette) = &mut self.palette {
            palette.draw(frame);
        }
        if let Some(choice) = &mut self.choice {
            choice.picker.draw(frame);
        }
    }
}

/// Keep the basename visible; use spare dialog space for its nearest parent.
fn compact_path(path: &Path, width: usize) -> String {
    let name = safe_text(
        &path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy(),
    );
    let name_width = UnicodeWidthStr::width(name.as_str());
    let parent = path.parent().and_then(Path::file_name);
    if name_width + 2 <= width
        && let Some(parent) = parent
    {
        let parent = clipped(
            &safe_text(&parent.to_string_lossy()),
            width - name_width - 1,
        );
        format!("{parent}/{name}")
    } else {
        clipped(&name, width)
    }
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
    use ratatui::{
        Terminal,
        backend::TestBackend,
        style::{Color, Modifier},
    };

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
    fn no_op_workspace_navigation_and_current_tab_click_end_typing_groups() {
        for event in [
            Event::Key(KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::CONTROL)),
            Event::Key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::CONTROL)),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 1,
                row: 0,
                modifiers: KeyModifiers::NONE,
            }),
        ] {
            let mut app = Workspace::open(None).unwrap();
            for c in "first".chars() {
                plain(&mut app, c);
            }
            draw(&mut app, 80, 24);
            app.handle_event(event);
            for c in "second".chars() {
                plain(&mut app, c);
            }
            assert_eq!(app.editor().document.text(), "firstsecond");
            ctrl(&mut app, 'z');
            assert_eq!(app.editor().document.text(), "first");
            ctrl(&mut app, 'z');
            assert_eq!(app.editor().document.text(), "");
        }
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
            .find(|(_, target)| *target == tabs::Target::Activate(0))
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
            .find(|(_, target)| *target == tabs::Target::Activate(1))
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

    fn new_document_button(app: &Workspace) -> Option<Rect> {
        app.tab_hits
            .iter()
            .find_map(|(rect, target)| (*target == tabs::Target::NewDocument).then_some(*rect))
    }

    #[test]
    fn new_document_button_creates_markdown_once_and_keeps_undo_history() {
        let old_icons = crate::icons::current();
        for icons in [crate::icons::IconSet::Plain, crate::icons::IconSet::Nerd] {
            crate::icons::set(icons);
            let mut app = Workspace::open(None).unwrap();
            plain(&mut app, 'a');
            plain(&mut app, 'b');
            draw(&mut app, 120, 24);
            key(&mut app, KeyCode::F(9), KeyModifiers::NONE);
            let terminal = draw(&mut app, 120, 24);
            assert!(app.sidebar.focused);
            let rect = new_document_button(&app).unwrap();
            assert_eq!(
                terminal.backend().buffer()[(rect.x + 1, rect.y)].symbol(),
                "+"
            );
            let mouse = MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: rect.x + 1,
                row: rect.y,
                modifiers: KeyModifiers::NONE,
            };
            app.handle_event(Event::Mouse(mouse));
            assert_eq!(app.tabs.len(), 2);
            assert_eq!(app.active, 1);
            assert!(!app.sidebar.focused);
            assert_eq!(app.editor().document.text(), "");
            assert!(!app.editor().document.is_dirty());
            assert!(app.editor().is_markdown());
            assert!(app.editor().path().is_none());
            assert_eq!(app.label(1).trim(), "Untitled 2.md");
            assert_eq!(app.tabs[0].editor.document.text(), "ab");
            assert!(app.tab_hits.is_empty());
            draw(&mut app, 120, 24);
            let rect = new_document_button(&app).unwrap();
            // Release over the newly laid-out button must not create another tab.
            app.handle_event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: rect.x + 1,
                row: rect.y,
                ..mouse
            }));
            assert_eq!(app.tabs.len(), 2);
            key(&mut app, KeyCode::F(7), KeyModifiers::NONE);
            plain(&mut app, 'c');
            ctrl(&mut app, 'z');
            assert_eq!(app.editor().document.text(), "ab");
            ctrl(&mut app, 'z');
            assert_eq!(app.editor().document.text(), "");
        }
        crate::icons::set(old_icons);
    }

    #[test]
    fn new_document_button_respects_modal_and_resize_guards() {
        for (code, modifiers) in [
            (KeyCode::F(1), KeyModifiers::NONE),
            (KeyCode::F(4), KeyModifiers::NONE),
            (KeyCode::F(2), KeyModifiers::NONE),
            (KeyCode::Char('q'), KeyModifiers::CONTROL),
        ] {
            let mut app = Workspace::open(None).unwrap();
            plain(&mut app, 'a');
            draw(&mut app, 80, 24);
            let rect = new_document_button(&app).unwrap();
            key(&mut app, code, modifiers);
            draw(&mut app, 80, 24);
            click_rect(&mut app, rect);
            assert_eq!(app.tabs.len(), 1, "{code:?}");
            assert_eq!(app.editor().document.text(), "a");
        }
        let mut app = Workspace::open(None).unwrap();
        draw(&mut app, 80, 24);
        let stale = new_document_button(&app).unwrap();
        app.handle_event(Event::Resize(8, 3));
        click_rect(&mut app, stale);
        assert_eq!(app.tabs.len(), 1);
        draw(&mut app, 8, 3);
        assert!(new_document_button(&app).is_none());
        click_rect(&mut app, stale);
        assert_eq!(app.tabs.len(), 1);
        draw(&mut app, 80, 24);
        let rect = new_document_button(&app).unwrap();
        click_rect(&mut app, rect);
        assert_eq!(app.tabs.len(), 2);
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
    fn single_header_keeps_identity_in_tabs_and_metadata_in_footer() {
        let mut app = Workspace::open(None).unwrap();
        app.handle_event(Event::Paste("draft".into()));
        ctrl(&mut app, 'n');
        key(&mut app, KeyCode::F(7), KeyModifiers::NONE);
        let terminal = draw(&mut app, 80, 24);
        let active = app
            .tab_hits
            .iter()
            .find(|(_, target)| *target == tabs::Target::Activate(app.active))
            .unwrap()
            .0;
        let first = &terminal.backend().buffer()[(active.x, active.y)];
        assert_eq!(first.symbol(), "▎");
        assert!(first.modifier.contains(Modifier::BOLD));
        assert!(!first.modifier.contains(Modifier::REVERSED));
        assert_ne!(first.bg, Color::Reset);
        assert_ne!(first.fg, Color::Reset);
        let inactive = app
            .tab_hits
            .iter()
            .find(|(_, target)| matches!(target, tabs::Target::Activate(index) if *index != app.active))
            .unwrap()
            .0;
        assert_ne!(
            first.bg,
            terminal.backend().buffer()[(inactive.x, inactive.y)].bg
        );
        for x in 0..80 {
            assert_ne!(terminal.backend().buffer()[(x, 0)].bg, Color::Reset);
            assert_eq!(
                terminal.backend().buffer()[(x, 1)].bg,
                crate::theme::palette().background
            );
        }
        let snapshot = crate::simulation::snapshot(&mut app, 80, 24).unwrap();
        assert!(snapshot.lines().next().unwrap().contains("Untitled 1.md ●"));
        assert_eq!(snapshot.matches("Untitled 1.md").count(), 1);
        assert!(snapshot.lines().nth(1).unwrap().contains("draft"));
        assert!(snapshot.contains("MARKDOWN"));
        assert!(snapshot.contains("Ln 1, Col 6"));
        for redundant in ["Live", "F1 Help", "F2 Commands", "F9", "DOCUMENTS"] {
            assert!(!snapshot.contains(redundant), "{redundant}");
        }
        assert!(!snapshot.contains("clipboard"));
        ctrl(&mut app, 'e');
        let snapshot = crate::simulation::snapshot(&mut app, 80, 24).unwrap();
        assert!(snapshot.contains("SOURCE"));
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
        let old_icons = crate::icons::current();
        for icons in [crate::icons::IconSet::Plain, crate::icons::IconSet::Nerd] {
            crate::icons::set(icons);
            for width in [1, 2, 8, 30, 80] {
                let terminal = draw(&mut app, width, 24);
                let active = app
                    .tab_hits
                    .iter()
                    .find(|(_, target)| *target == tabs::Target::Activate(app.active))
                    .unwrap()
                    .0;
                assert!(
                    (active.x..active.right())
                        .any(|x| terminal.backend().buffer()[(x, 0)].symbol() == tabs::DIRTY),
                    "width {width}, {icons:?}"
                );
            }
        }
        crate::icons::set(old_icons);
    }

    #[test]
    fn workspace_bars_replace_editor_chrome_without_leaking_paths_or_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a-unique-note.md");
        std::fs::write(&path, "# A title\n\nA short note.\n").unwrap();
        let mut app = Workspace::open(Some(path)).unwrap();
        for width in [80, 120, 160] {
            let snapshot = crate::simulation::snapshot(&mut app, width, 36).unwrap();
            assert_eq!(
                snapshot.matches("a-unique-note.md").count(),
                1,
                "{snapshot}"
            );
            assert!(!snapshot.contains(&dir.path().display().to_string()));
            let footer = snapshot.lines().nth(35).unwrap();
            for segment in ["MARKDOWN", " words", "Ln 1, Col 1"] {
                assert_eq!(footer.matches(segment).count(), 1, "{footer}");
            }
            assert!(snapshot.lines().nth(1).unwrap().contains("A title"));
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
    fn palette_query(app: &mut Workspace, query: &str, width: u16, height: u16) {
        key(app, KeyCode::F(2), KeyModifiers::NONE);
        app.handle_event(Event::Paste(query.into()));
        draw(app, width, height);
    }

    fn click_rect(app: &mut Workspace, rect: Rect) {
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        }));
    }

    #[test]
    fn sidebar_defaults_are_responsive_without_stealing_editor_input() {
        let mut app = Workspace::open(None).unwrap();
        app.editor_mut().document = eymi::Document::new("# First\n\n## Second\n");
        for (width, height) in [(42, 16), (80, 24), (120, 36), (160, 45), (60, 7), (1, 1)] {
            draw(&mut app, width, height);
            assert_eq!(app.sidebar.area.width > 0, width >= 110 && height >= 8);
            assert!(!app.sidebar.focused);
            assert!(
                app.sidebar
                    .hits
                    .iter()
                    .all(|(rect, _)| rect.right() <= width && rect.bottom() <= height)
            );
        }
        draw(&mut app, 120, 36);
        plain(&mut app, 'x');
        assert!(app.editor().document.text().starts_with("x#"));
        key(&mut app, KeyCode::F(9), KeyModifiers::NONE);
        let before = app.editor().document.text().to_string();
        plain(&mut app, 'y');
        app.handle_event(Event::Paste("must stay out".into()));
        assert_eq!(app.editor().document.text(), before);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        plain(&mut app, 'z');
        assert!(app.editor().document.text().starts_with("xz#"));
        key(&mut app, KeyCode::F(9), KeyModifiers::NONE);
        app.handle_event(Event::Resize(42, 16));
        assert!(!app.sidebar.focused);
        plain(&mut app, 'a');
        assert!(app.editor().document.text().starts_with("xza#"));
    }

    #[test]
    fn sidebar_keyboard_and_mouse_jump_to_unicode_headings_without_edits() {
        let mut app = Workspace::open(None).unwrap();
        let source = format!(
            "# Café\n\n{}\n最後 **section**\n--------------\n",
            "a paragraph\n\n".repeat(80)
        );
        let target = source.find("最後").unwrap();
        app.editor_mut().document = eymi::Document::new(source.clone());
        draw(&mut app, 120, 36);
        key(&mut app, KeyCode::F(9), KeyModifiers::NONE);
        key(&mut app, KeyCode::End, KeyModifiers::NONE);
        draw(&mut app, 120, 36);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        draw(&mut app, 120, 36);
        assert_eq!(app.editor().document.selection().head, target);
        assert!(app.editor().scroll > 0);
        assert!(!app.sidebar.focused);
        assert_eq!(app.editor().document.text(), source);
        assert!(!app.editor().document.is_dirty());
        assert!(!app.editor().document.can_undo());
        let first = app
            .sidebar
            .hits
            .iter()
            .find(|(_, target)| *target == Target::Heading(0))
            .unwrap()
            .0;
        click_rect(&mut app, first);
        draw(&mut app, 120, 36);
        assert_eq!(app.editor().document.selection().head, 0);
        assert_eq!(app.editor().scroll, 0);
    }

    #[test]
    fn outline_clicks_bring_a_heading_a_third_of_the_way_down() {
        let mut app = Workspace::open(None).unwrap();
        let filler = "a paragraph\n\n".repeat(40);
        let source = format!("# One\n\n{filler}## Two\n\n{filler}");
        app.editor_mut().document = eymi::Document::new(source.clone());
        draw(&mut app, 120, 36);
        let two = app
            .sidebar
            .hits
            .iter()
            .find(|(_, target)| *target == Target::Heading(1))
            .unwrap()
            .0;
        click_rect(&mut app, two);
        draw(&mut app, 120, 36);
        let editor = app.editor();
        assert_eq!(
            editor.document.selection().head,
            source.find("## Two").unwrap()
        );
        assert_eq!(
            editor.caret_position().0 - editor.scroll,
            usize::from(editor.viewport.height) / 3
        );
        assert!(!editor.document.is_dirty());
    }

    #[test]
    fn sidebar_rebuilds_outline_on_edits_tabs_and_plain_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("text.txt");
        std::fs::write(&path, "# not markdown").unwrap();
        let mut app = Workspace::open(None).unwrap();
        app.handle_event(Event::Paste("# one\n\n```\n# hidden\n```\n".into()));
        draw(&mut app, 120, 36);
        assert_eq!(app.sidebar.headings.len(), 1);
        app.handle_event(Event::Paste("\n## two\n".into()));
        draw(&mut app, 120, 36);
        assert_eq!(app.sidebar.headings.len(), 2);
        app.open_path(path).unwrap();
        draw(&mut app, 120, 36);
        assert!(app.sidebar.headings.is_empty());
        let first = app
            .tab_hits
            .iter()
            .find(|(_, target)| *target == tabs::Target::Activate(0))
            .unwrap()
            .0;
        click_rect(&mut app, first);
        draw(&mut app, 120, 36);
        assert_eq!(app.active, 0);
        assert_eq!(app.sidebar.headings.len(), 2);
    }

    #[test]
    fn sidebar_explicit_toggle_and_resize_discard_stale_hits() {
        let mut app = Workspace::open(None).unwrap();
        app.editor_mut().document = eymi::Document::new("# One\n\n## Two\n");
        draw(&mut app, 80, 24);
        key(&mut app, KeyCode::F(9), KeyModifiers::NONE);
        draw(&mut app, 80, 24);
        assert!(app.sidebar.focused);
        assert!(app.sidebar.area.width > 0);
        let heading = app
            .sidebar
            .hits
            .iter()
            .find(|(_, target)| *target == Target::Heading(1))
            .unwrap()
            .0;
        app.handle_event(Event::Resize(42, 16));
        click_rect(&mut app, heading);
        assert_eq!(app.editor().document.selection().head, 0);
        draw(&mut app, 42, 16);
        assert!(app.sidebar.hits.is_empty());
        draw(&mut app, 80, 24);
        key(&mut app, KeyCode::F(9), KeyModifiers::NONE);
        key(&mut app, KeyCode::F(9), KeyModifiers::NONE);
        draw(&mut app, 120, 36);
        assert_eq!(app.sidebar.area.width, 0);
    }

    #[test]
    fn palette_filters_and_escape_preserve_document_selection_and_history() {
        let mut app = Workspace::open(None).unwrap();
        app.handle_event(Event::Paste("Café source\n".into()));
        app.editor_mut().document.select_all();
        let selection = app.editor().document.selection();
        let revision = app.editor().document.revision();
        palette_query(&mut app, "SAVE", 80, 24);
        assert_eq!(
            app.palette.as_ref().unwrap().matches(),
            vec![Command::Save, Command::SaveAs]
        );
        ctrl(&mut app, 'a');
        app.handle_event(Event::Paste("不存在\n\r\0".into()));
        draw(&mut app, 42, 16);
        assert_eq!(app.palette.as_ref().unwrap().query.text(), "不存在");
        assert!(app.palette.as_ref().unwrap().matches().is_empty());
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.palette.is_some());
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.editor().document.selection(), selection);
        assert_eq!(app.editor().document.revision(), revision);
        assert_eq!(app.editor().document.text(), "Café source\n");
        ctrl(&mut app, 'z');
        assert_eq!(app.editor().document.text(), "");
    }

    #[test]
    fn palette_mouse_actions_and_keyboard_actions_reuse_workspace_semantics() {
        let mut app = Workspace::open(None).unwrap();
        app.handle_event(Event::Paste("first".into()));
        palette_query(&mut app, "new", 80, 24);
        let hit = app.palette.as_ref().unwrap().hits[0].0;
        click_rect(&mut app, hit);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active, 1);
        app.handle_event(Event::Paste("second".into()));
        palette_query(&mut app, "previous", 80, 24);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.editor().document.text(), "first");
        palette_query(&mut app, "close", 80, 24);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.pending.is_some());
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.tabs.len(), 2);
        palette_query(&mut app, "quit", 80, 24);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        plain(&mut app, 'n');
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!app.should_exit);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.tabs[0].editor.document.text(), "first");
    }

    #[test]
    fn palette_save_as_keeps_duplicate_tab_protection() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("owned.md");
        std::fs::write(&existing, "first").unwrap();
        let mut app = Workspace::open(Some(existing.clone())).unwrap();
        ctrl(&mut app, 'n');
        app.handle_event(Event::Paste("second".into()));
        palette_query(&mut app, "save as", 80, 24);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.editor().saving_as());
        app.handle_event(Event::Paste(existing.display().to_string()));
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.editor().saving_as());
        assert_eq!(std::fs::read_to_string(existing).unwrap(), "first");
        assert_eq!(app.editor().document.text(), "second");
        key(&mut app, KeyCode::F(2), KeyModifiers::NONE);
        assert!(app.palette.is_none());
    }

    #[test]
    fn palette_short_resize_and_hidden_targets_never_run_actions() {
        let mut app = Workspace::open(None).unwrap();
        for (width, height) in [
            (42, 16),
            (80, 24),
            (120, 36),
            (160, 45),
            (12, 5),
            (8, 4),
            (1, 1),
        ] {
            palette_query(&mut app, "new", width, height);
            let palette = app.palette.as_ref().unwrap();
            assert!(
                palette
                    .hits
                    .iter()
                    .all(|(rect, _)| rect.right() <= width && rect.bottom() <= height)
            );
            if height < 5 || width < 12 {
                assert!(palette.hits.is_empty());
                key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
                assert_eq!(app.tabs.len(), 1);
            }
            key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        }
        palette_query(&mut app, "new", 80, 24);
        let hit = app.palette.as_ref().unwrap().hits[0].0;
        app.handle_event(Event::Resize(42, 16));
        click_rect(&mut app, hit);
        assert_eq!(app.tabs.len(), 1);
        draw(&mut app, 42, 16);
        let hit = app.palette.as_ref().unwrap().hits[0].0;
        ctrl(&mut app, 'a');
        app.handle_event(Event::Paste("no such command".into()));
        click_rect(&mut app, hit);
        assert_eq!(app.tabs.len(), 1);
    }

    #[test]
    fn go_to_line_handles_all_newlines_clamps_eof_and_preserves_source() {
        let mut app = Workspace::open(None).unwrap();
        let source = "one\r\n界 two\rthree\nfour";
        app.editor_mut().document = eymi::Document::new(source);
        for (line, offset) in [(1, 0), (2, 5), (3, 13), (4, 19), (999, source.len())] {
            ctrl(&mut app, 'g');
            app.handle_event(Event::Paste(line.to_string()));
            draw(&mut app, 80, 24);
            key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
            assert_eq!(app.editor().document.selection().head, offset);
            assert_eq!(app.editor().document.text(), source);
            assert!(!app.editor().document.can_undo());
        }
        ctrl(&mut app, 'g');
        app.handle_event(Event::Paste("0".into()));
        draw(&mut app, 80, 24);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.palette.as_ref().unwrap().line_mode);
    }

    #[test]
    fn palette_theme_changes_both_chrome_and_body_without_editing_source() {
        let mut app = Workspace::open(None).unwrap();
        app.editor_mut().document = eymi::Document::new("# Theme\n");
        let old_theme = crate::theme::current_theme();
        let before = draw(&mut app, 120, 36).backend().buffer()[(0, 0)].bg;
        palette_query(&mut app, "theme", 120, 36);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        app.handle_event(Event::Paste(
            if old_theme.is_dark() {
                "Sage Light"
            } else {
                "Sage Dark"
            }
            .into(),
        ));
        draw(&mut app, 120, 36);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let rendered = draw(&mut app, 120, 36);
        assert_ne!(rendered.backend().buffer()[(0, 0)].bg, before);
        assert_eq!(
            rendered.backend().buffer()[(0, 2)].bg,
            crate::theme::chrome_palette().sidebar.background
        );
        assert_eq!(app.editor().document.text(), "# Theme\n");
        assert!(!app.editor().document.can_undo());
        crate::theme::set_theme(old_theme);
    }
    #[test]
    fn palette_selected_query_is_visible_and_empty_paste_keeps_it() {
        let mut app = Workspace::open(None).unwrap();
        palette_query(&mut app, "é界", 80, 24);
        ctrl(&mut app, 'a');
        let selected = app.palette.as_ref().unwrap().query.selection();
        let revision = app.palette.as_ref().unwrap().query.revision();
        for paste in ["", "\r\n\0\t\u{1b}"] {
            app.handle_event(Event::Paste(paste.into()));
            assert_eq!(app.palette.as_ref().unwrap().query.text(), "é界");
            assert_eq!(app.palette.as_ref().unwrap().query.selection(), selected);
            assert_eq!(app.palette.as_ref().unwrap().query.revision(), revision);
        }
        let rendered = draw(&mut app, 80, 24);
        let cell = rendered
            .backend()
            .buffer()
            .content
            .iter()
            .find(|cell| cell.symbol() == "é")
            .unwrap();
        assert_eq!(cell.bg, crate::theme::palette().selection);
        assert_eq!(cell.fg, crate::theme::palette().selection_text);
        app.handle_event(Event::Paste("save".into()));
        assert_eq!(app.palette.as_ref().unwrap().query.text(), "save");
        assert!(app.editor().document.text().is_empty());
    }

    #[test]
    fn modal_draws_over_sidebar_and_blocks_hidden_targets() {
        let mut app = Workspace::open(None).unwrap();
        app.editor_mut().document = eymi::Document::new("# One\n\n## Two\n");
        draw(&mut app, 120, 36);
        let target = app
            .sidebar
            .hits
            .iter()
            .find(|(_, target)| *target == Target::Heading(1))
            .unwrap()
            .0;
        key(&mut app, KeyCode::F(1), KeyModifiers::NONE);
        let text = crate::simulation::snapshot(&mut app, 120, 36).unwrap();
        assert!(text.contains("Eymi · Help"));
        assert!(app.sidebar.hits.is_empty());
        assert!(app.tab_hits.is_empty());
        click_rect(&mut app, target);
        assert_eq!(app.editor().document.selection().head, 0);
        assert!(app.editor().has_modal());
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut app, KeyCode::F(4), KeyModifiers::NONE);
        let text = crate::simulation::snapshot(&mut app, 120, 36).unwrap();
        assert!(text.contains("Save As · new filename"));
        assert!(app.sidebar.hits.is_empty());
    }

    #[test]
    fn tiny_confirmation_requires_visible_prompt_and_cancel_stays_available() {
        let mut app = Workspace::open(None).unwrap();
        app.handle_event(Event::Paste("valuable".into()));
        draw(&mut app, 8, 3);
        ctrl(&mut app, 'q');
        plain(&mut app, 'n');
        assert!(!app.should_exit);
        assert_eq!(app.editor().document.text(), "valuable");
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.pending.is_none());
    }

    #[test]
    fn scrolling_sidebar_does_not_capture_keyboard_focus() {
        let mut app = Workspace::open(None).unwrap();
        app.editor_mut().document = eymi::Document::new("# Heading\n\n".repeat(30));
        draw(&mut app, 120, 16);
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }));
        draw(&mut app, 120, 16);
        assert!(!app.sidebar.focused);
        plain(&mut app, 'x');
        assert!(app.editor().document.text().starts_with("x#"));
    }
    #[test]
    fn filtered_palette_shrinks_below_a_stable_query_and_only_visible_rows_activate() {
        for (width, height) in [(42, 16), (80, 24), (120, 36), (160, 45)] {
            let mut app = Workspace::open(None).unwrap();
            key(&mut app, KeyCode::F(2), KeyModifiers::NONE);
            let mut initial = draw(&mut app, width, height);
            let query_y = initial.get_cursor_position().unwrap().y;
            let old_second = app.palette.as_ref().unwrap().hits[1].0;
            let old_bottom = initial
                .backend()
                .buffer()
                .content
                .iter()
                .filter(|cell| cell.symbol() == "│")
                .count();
            app.handle_event(Event::Paste("new".into()));
            let mut filtered = draw(&mut app, width, height);
            assert_eq!(filtered.get_cursor_position().unwrap().y, query_y);
            let hits = &app.palette.as_ref().unwrap().hits;
            assert_eq!(hits.len(), 1);
            let hit = hits[0].0;
            assert_eq!(
                filtered.backend().buffer()[(hit.x - 1, hit.bottom())].symbol(),
                "╰"
            );
            assert!(
                filtered
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .filter(|cell| cell.symbol() == "│")
                    .count()
                    < old_bottom
            );
            click_rect(&mut app, old_second);
            assert_eq!(app.tabs.len(), 1);
            for (x, y) in [
                (hit.x - 1, hit.y),
                (hit.right(), hit.y),
                (hit.x, hit.bottom()),
            ] {
                click_rect(&mut app, Rect::new(x, y, 1, 1));
                assert_eq!(app.tabs.len(), 1);
            }
            ctrl(&mut app, 'a');
            app.handle_event(Event::Paste("no matching command".into()));
            let mut empty = draw(&mut app, width, height);
            assert_eq!(empty.get_cursor_position().unwrap().y, query_y);
            assert!(app.palette.as_ref().unwrap().hits.is_empty());
            ctrl(&mut app, 'a');
            key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
            let mut expanded = draw(&mut app, width, height);
            assert_eq!(expanded.get_cursor_position().unwrap().y, query_y);
            assert!(app.palette.as_ref().unwrap().hits.len() > 1);
            app.handle_event(Event::Paste("new".into()));
            draw(&mut app, width, height);
            let hit = app.palette.as_ref().unwrap().hits[0].0;
            click_rect(&mut app, hit);
            assert_eq!(app.tabs.len(), 2);
            assert!(app.palette.is_none());
        }
    }

    #[test]
    fn go_to_line_uses_a_compact_prompt_at_the_same_query_anchor() {
        let mut app = Workspace::open(None).unwrap();
        key(&mut app, KeyCode::F(2), KeyModifiers::NONE);
        let mut initial = draw(&mut app, 80, 24);
        let query_y = initial.get_cursor_position().unwrap().y;
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        ctrl(&mut app, 'g');
        let mut line = draw(&mut app, 80, 24);
        let cursor = line.get_cursor_position().unwrap();
        assert_eq!(cursor.y, query_y);
        assert_eq!(
            line.backend().buffer()[(cursor.x - 4, query_y - 1)].symbol(),
            "╭"
        );
        assert_eq!(
            line.backend().buffer()[(cursor.x - 4, query_y + 4)].symbol(),
            "╰"
        );
        app.handle_event(Event::Paste("1".into()));
        draw(&mut app, 80, 24);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.palette.is_none());
    }
}
