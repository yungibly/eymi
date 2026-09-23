//! Session-local document and heading pickers. Queries never edit source.
use super::{Choice, ChoiceMode, Picker, Workspace, safe_text};
use std::path::{Path, PathBuf};
use unicode_segmentation::UnicodeSegmentation;

impl Workspace {
    pub(super) fn open_documents(&mut self) {
        self.editor_mut().deactivate();
        self.sidebar.focused = false;
        let directory = std::env::current_dir().ok();
        let paths: Vec<_> = self.tabs.iter().map(|tab| tab.editor.path()).collect();
        let search_names = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                tab.editor.path().map_or_else(
                    || self.name(index),
                    |path| {
                        safe_text(
                            &path
                                .file_name()
                                .unwrap_or(path.as_os_str())
                                .to_string_lossy(),
                        )
                    },
                )
            })
            .collect();
        let entries = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let path = tab.editor.path().map_or_else(
                    || "Unsaved document".to_owned(),
                    |path| {
                        let path = directory
                            .as_ref()
                            .and_then(|directory| path.strip_prefix(directory).ok())
                            .unwrap_or(path);
                        safe_text(&path.to_string_lossy())
                    },
                );
                let label = if tab.editor.path().is_some() {
                    format!(
                        "{}{}",
                        if tab.editor.document.is_dirty() {
                            "● "
                        } else {
                            ""
                        },
                        unique_suffix(&paths, index)
                    )
                } else {
                    self.label(index).trim().to_owned()
                };
                (
                    format!("{} {label}", index + 1),
                    format!(
                        "{}{path}",
                        if index == self.active {
                            "Current · "
                        } else {
                            ""
                        }
                    ),
                )
            })
            .collect();
        self.choice = Some(Choice {
            picker: Picker::new(
                "Open documents",
                "↑↓ choose · ⏎ switch · esc cancel",
                entries,
                self.active,
            )
            .navigation()
            .search_names(search_names),
            mode: ChoiceMode::Documents {
                numbers: self.tabs.iter().map(|tab| tab.number).collect(),
            },
        });
        self.tab_hits.clear();
        self.sidebar.invalidate();
    }

    pub(super) fn open_headings(&mut self) {
        self.editor_mut().deactivate();
        self.sidebar.focused = false;
        self.refresh_outline();
        let source = self.editor().document.text();
        let mut line_starts = vec![0];
        line_starts.extend(
            source
                .grapheme_indices(true)
                .filter_map(|(index, grapheme)| {
                    matches!(grapheme, "\r" | "\n" | "\r\n").then_some(index + grapheme.len())
                }),
        );
        let entries = self
            .sidebar
            .headings
            .iter()
            .map(|heading| {
                let line = line_starts.partition_point(|&start| start <= heading.offset);
                (
                    heading.title.clone(),
                    format!("H{} · line {line}", heading.level),
                )
            })
            .collect();
        let caret = self.editor().document.selection().head;
        let selected = self
            .sidebar
            .headings
            .iter()
            .rposition(|heading| heading.offset <= caret)
            .unwrap_or(0);
        self.choice = Some(Choice {
            picker: Picker::new(
                "Headings",
                "↑↓ choose · ⏎ jump · esc cancel",
                entries,
                selected,
            )
            .navigation()
            .empty_message("No headings in this document"),
            mode: ChoiceMode::Headings {
                number: self.tabs[self.active].number,
                revision: self.editor().document.revision(),
                offsets: self
                    .sidebar
                    .headings
                    .iter()
                    .map(|heading| heading.offset)
                    .collect(),
            },
        });
        self.tab_hits.clear();
        self.sidebar.invalidate();
    }
}

fn unique_suffix(paths: &[Option<&Path>], index: usize) -> String {
    let path = paths[index].expect("named document");
    let parts: Vec<_> = path.components().collect();
    for depth in 1..=parts.len() {
        let suffix: PathBuf = parts[parts.len() - depth..].iter().collect();
        if !paths
            .iter()
            .enumerate()
            .any(|(other, path)| other != index && path.is_some_and(|path| path.ends_with(&suffix)))
        {
            return safe_text(&suffix.to_string_lossy());
        }
    }
    safe_text(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use eymi::Selection;

    fn key(app: &mut Workspace, code: KeyCode) {
        app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }
    fn draw(app: &mut Workspace, width: u16, height: u16) -> String {
        crate::simulation::snapshot(app, width, height).unwrap()
    }

    #[test]
    fn document_picker_filters_paths_and_switches_only_on_acceptance() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        for project in ["alpha", "beta"] {
            let parent = dir.path().join(project).join("docs");
            std::fs::create_dir_all(&parent).unwrap();
            let path = parent.join("note.md");
            std::fs::write(&path, format!("# {project}\r\n")).unwrap();
            paths.push(path);
        }
        let mut app = Workspace::open(Some(paths[0].clone())).unwrap();
        app.editor_mut().document.insert("local ");
        app.editor_mut().document.select_all();
        let selection = app.editor().document.selection();
        let text = app.editor().document.text().to_owned();
        app.open_path(paths[1].clone()).unwrap();
        draw(&mut app, 80, 24);
        key(&mut app, KeyCode::F(10));
        app.handle_event(Event::Paste("alpha nmd".into()));
        let frame = draw(&mut app, 80, 24);
        assert!(frame.contains("Open documents"));
        assert!(frame.contains("● alpha/docs/note.md"));
        assert_eq!(app.active, 1, "filtering must not preview-switch documents");
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.active, 1);
        key(&mut app, KeyCode::F(10));
        app.handle_event(Event::Paste("alpha nmd".into()));
        draw(&mut app, 80, 24);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.active, 0);
        assert_eq!(app.editor().document.selection(), selection);
        assert_eq!(app.editor().document.text(), text);
        assert!(app.editor_mut().document.undo());
        assert_eq!(app.editor().document.text(), "# alpha\r\n");
        assert_eq!(std::fs::read_to_string(&paths[0]).unwrap(), "# alpha\r\n");
    }

    #[test]
    fn exact_filename_ranks_above_a_backup_despite_tab_numbers_and_dirty_markers() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = Workspace::open(Some(dir.path().join("report.md.bak"))).unwrap();
        app.open_path(dir.path().join("report.md")).unwrap();
        app.editor_mut().document.insert("unsaved report");
        app.switch(0);
        draw(&mut app, 80, 24);
        key(&mut app, KeyCode::F(10));
        app.handle_event(Event::Paste("report.md".into()));
        draw(&mut app, 80, 24);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.active, 1);
        assert_eq!(app.editor().document.text(), "unsaved report");
    }

    #[test]
    fn duplicate_names_show_the_shortest_distinguishing_path_suffix() {
        let shared = "/Volumes/work/archive/very-long-shared-project-directory/very-long-shared-project-directory";
        let first = PathBuf::from(format!("{shared}/alpha/docs/note.md"));
        let second = PathBuf::from(format!("{shared}/beta/docs/note.md"));
        let third = PathBuf::from(format!("{shared}/notes.md"));
        let paths = [
            Some(first.as_path()),
            Some(second.as_path()),
            Some(third.as_path()),
            None,
        ];
        assert_eq!(unique_suffix(&paths, 0), "alpha/docs/note.md");
        assert_eq!(unique_suffix(&paths, 1), "beta/docs/note.md");
        assert_eq!(unique_suffix(&paths, 2), "notes.md");
    }

    #[test]
    fn heading_picker_finds_semantic_unicode_headings_and_preserves_cancelled_selection() {
        let source = "\u{feff}# Start\r\n\r\n```\r\n# Hidden\r\n```\r\n\r\n## Résumé\r\n\r\nRésumé\r\n------\r\n";
        let mut app = Workspace::open(None).unwrap();
        app.editor_mut().document = eymi::Document::new(source);
        let selection = Selection {
            anchor: source.len(),
            head: 3,
        };
        app.editor_mut().document.set_selection(selection).unwrap();
        draw(&mut app, 80, 24);
        key(&mut app, KeyCode::F(11));
        app.handle_event(Event::Paste("rém".into()));
        let frame = draw(&mut app, 80, 24);
        assert!(frame.contains("Headings"));
        assert!(frame.contains(" 2/"));
        assert_eq!(app.editor().document.selection(), selection);
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.editor().document.selection(), selection);
        key(&mut app, KeyCode::F(11));
        app.handle_event(Event::Paste("rém 9".into()));
        let frame = draw(&mut app, 80, 24);
        assert!(frame.contains(" 1/"));
        assert!(frame.contains("H2 · line 9"));
        key(&mut app, KeyCode::Enter);
        assert_eq!(
            app.editor().document.selection().head,
            source.rfind("Résumé").unwrap()
        );
        assert_eq!(app.editor().document.text(), source);
        assert!(!app.editor().document.can_undo());
    }

    #[test]
    fn navigation_pickers_reject_invisible_stale_and_empty_targets() {
        let mut app = Workspace::open(None).unwrap();
        app.editor_mut().document = eymi::Document::new("# One\n\n## Two\n");
        draw(&mut app, 80, 24);
        key(&mut app, KeyCode::F(11));
        app.handle_event(Event::Paste("Two".into()));
        let frame = draw(&mut app, 80, 24);
        let (row, column) = frame
            .lines()
            .enumerate()
            .find_map(|(row, line)| {
                line.find("Two")
                    .filter(|_| line.starts_with(' '))
                    .map(|column| (row, column))
            })
            .unwrap();
        app.handle_event(Event::Resize(20, 4));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: column as u16,
            row: row as u16,
            modifiers: KeyModifiers::NONE,
        }));
        key(&mut app, KeyCode::Enter);
        assert!(app.choice.is_some());
        assert_eq!(app.editor().document.selection().head, 0);
        draw(&mut app, 20, 4);
        key(&mut app, KeyCode::Enter);
        assert!(app.choice.is_some());
        draw(&mut app, 80, 24);
        app.editor_mut().document.insert("changed ");
        let caret = app.editor().document.selection().head;
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.editor().document.selection().head, caret);
        assert!(draw(&mut app, 80, 24).contains("Document changed"));
        app.editor_mut().document = eymi::Document::new("No sections here.");
        key(&mut app, KeyCode::F(11));
        assert!(draw(&mut app, 80, 24).contains("No headings in this document"));
        key(&mut app, KeyCode::Enter);
        assert!(app.choice.is_some());
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.editor().document.text(), "No sections here.");
    }
}
