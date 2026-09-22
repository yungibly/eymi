use super::*;
use crate::theme::{Theme, current_theme, set_theme};
use ratatui::{Terminal, backend::TestBackend};
use std::{fs, time::Duration};

fn key(app: &mut Workspace, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
}
fn draw(app: &mut Workspace, width: u16, height: u16) {
    Terminal::new(TestBackend::new(width, height))
        .unwrap()
        .draw(|frame| app.draw(frame))
        .unwrap();
}
fn checkpoint(app: &mut Workspace) {
    app.last_tick = Instant::now() - Duration::from_secs(2);
    app.tick();
}
fn configure(app: &mut Workspace, dir: &Path) {
    app.enable_state(&dir.join("config"), &dir.join("state"), false);
}

#[test]
fn theme_preview_cancel_accept_and_restart_preserve_document() {
    let original = current_theme();
    set_theme(Theme::Dark);
    let dir = tempfile::tempdir().unwrap();
    let mut app = Workspace::open(None).unwrap();
    configure(&mut app, dir.path());
    app.editor_mut().document.insert("# source\n");
    let revision = app.editor().document.revision();
    app.open_themes();
    app.handle_event(Event::Paste("Sage Light".into()));
    assert_eq!(current_theme(), Theme::Light);
    assert_eq!(app.editor().document.revision(), revision);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(current_theme(), Theme::Dark);
    assert!(!dir.path().join("config/settings.conf").exists());
    app.open_themes();
    app.handle_event(Event::Paste("Catppuccin Mocha".into()));
    let selected = current_theme();
    draw(&mut app, 80, 24);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.choice.is_none());
    assert_ne!(selected, Theme::Dark);
    assert_eq!(app.editor().document.text(), "# source\n");
    let mut restarted = Workspace::open(None).unwrap();
    set_theme(Theme::Dark);
    configure(&mut restarted, dir.path());
    assert_eq!(current_theme(), selected);
    set_theme(Theme::Light);
    let mut explicit = Workspace::open(None).unwrap();
    explicit.enable_state(&dir.path().join("config"), &dir.path().join("state"), true);
    assert_eq!(current_theme(), Theme::Light);
    app.finish_state().unwrap();
    restarted.finish_state().unwrap();
    explicit.finish_state().unwrap();
    set_theme(original);
}

#[test]
fn icon_preference_survives_restart_without_changing_source_or_selection() {
    use crate::icons::{self, IconSet};
    let original = icons::current();
    icons::set(IconSet::Plain);
    let dir = tempfile::tempdir().unwrap();
    let mut app = Workspace::open(None).unwrap();
    configure(&mut app, dir.path());
    app.editor_mut().document.insert("# A note\n");
    app.editor_mut().document.select_all();
    let selection = app.editor().document.selection();
    let revision = app.editor().document.revision();
    app.run_command(Command::Icons);
    assert_eq!(icons::current(), IconSet::Nerd);
    assert_eq!(app.editor().document.selection(), selection);
    assert_eq!(app.editor().document.revision(), revision);
    let preference = fs::read_to_string(dir.path().join("config/settings.conf")).unwrap();
    assert!(preference.contains("icons=nerd"));
    icons::set(IconSet::Plain);
    let mut restarted = Workspace::open(None).unwrap();
    configure(&mut restarted, dir.path());
    assert_eq!(icons::current(), IconSet::Nerd);
    restarted.run_command(Command::Icons);
    assert_eq!(icons::current(), IconSet::Plain);
    assert_eq!(app.editor().document.text(), "# A note\n");
    key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.editor().document.text(), "");
    app.finish_state().unwrap();
    restarted.finish_state().unwrap();
    icons::set(original);
}

#[test]
fn invisible_or_stale_theme_picker_cannot_accept_and_query_never_edits_source() {
    let original = current_theme();
    let mut app = Workspace::open(None).unwrap();
    app.open_themes();
    app.handle_event(Event::Paste("Nord".into()));
    draw(&mut app, 80, 24);
    app.handle_event(Event::Resize(20, 4));
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.choice.is_some());
    draw(&mut app, 20, 4);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.choice.is_some());
    key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
    app.handle_event(Event::Paste("\0\r\n".into()));
    app.handle_event(Event::Resize(80, 24));
    draw(&mut app, 80, 24);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(current_theme(), original);
    assert!(!app.editor().document.can_undo());
    assert_eq!(app.editor().document.text(), "");
}

#[test]
fn abandoned_edits_recover_as_detached_unsaved_copies_without_overwriting_disk() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("draft.md");
    fs::write(&file, "disk\n").unwrap();
    {
        let mut crashed = Workspace::open(Some(file.clone())).unwrap();
        configure(&mut crashed, dir.path());
        crashed.editor_mut().document.insert("unsaved 界\n");
        checkpoint(&mut crashed);
        fs::write(&file, "newer external source\n").unwrap();
        // Drop intentionally does not perform clean shutdown.
    }
    let mut app = Workspace::open(Some(file.clone())).unwrap();
    configure(&mut app, dir.path());
    app.open_recovery();
    assert!(app.choice.is_some());
    draw(&mut app, 80, 24);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.editor().path(), None);
    assert!(app.editor().document.is_dirty());
    assert_eq!(app.editor().document.text(), "unsaved 界\ndisk\n");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "newer external source\n"
    );
    assert!(app.label(app.active).contains("Recovered draft.md"));
    drop(app);
    let mut restarted = Workspace::open(None).unwrap();
    configure(&mut restarted, dir.path());
    restarted.open_recovery();
    assert!(
        restarted.choice.is_some(),
        "recovered buffer was checkpointed before consuming the old copy"
    );
    restarted.choice = None;
    restarted.recover_document(0);
    restarted.request(Intent::Close);
    draw(&mut restarted, 80, 24);
    key(&mut restarted, KeyCode::Char('n'), KeyModifiers::NONE);
    drop(restarted);
    let mut final_session = Workspace::open(None).unwrap();
    configure(&mut final_session, dir.path());
    final_session.open_recovery();
    assert!(
        final_session.choice.is_none(),
        "explicitly discarded recovered copy must not reappear"
    );
    final_session.finish_state().unwrap();
}

#[test]
fn save_clears_recovery_and_cancelled_quit_retains_all_dirty_tabs() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("draft.md");
    fs::write(&file, "one\n").unwrap();
    {
        let mut app = Workspace::open(Some(file.clone())).unwrap();
        configure(&mut app, dir.path());
        app.editor_mut().document.insert("saved ");
        checkpoint(&mut app);
        key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
    }
    let mut app = Workspace::open(None).unwrap();
    configure(&mut app, dir.path());
    app.open_recovery();
    assert!(app.choice.is_none());
    app.editor_mut().document.insert("first");
    app.new_tab();
    app.editor_mut().document.insert("second");
    checkpoint(&mut app);
    app.request(Intent::Quit);
    draw(&mut app, 80, 24);
    key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    draw(&mut app, 80, 24);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.should_exit);
    drop(app);
    let mut recovered = Workspace::open(None).unwrap();
    configure(&mut recovered, dir.path());
    recovered.recover_document(0);
    let first = recovered.editor().document.text().to_owned();
    recovered.recover_document(0);
    let second = recovered.editor().document.text().to_owned();
    assert!(first == "first" && second == "second" || first == "second" && second == "first");
    recovered.finish_state().unwrap();
}

#[test]
fn external_change_checks_run_even_without_persistent_state() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("draft.md");
    fs::write(&file, "before").unwrap();
    let mut app = Workspace::open(Some(file.clone())).unwrap();
    fs::write(&file, "after").unwrap();
    app.last_tick = Instant::now() - Duration::from_secs(2);
    assert!(app.tick());
    app.run_command(Command::Reload);
    assert_eq!(app.editor().document.text(), "after");
    key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.editor().document.text(), "before");
    assert!(app.editor().document.is_dirty());
}

#[test]
fn empty_unsaved_recovery_stays_dirty_and_can_be_saved() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("removed.md");
    fs::write(&file, "delete me").unwrap();
    {
        let mut app = Workspace::open(Some(file.clone())).unwrap();
        configure(&mut app, dir.path());
        app.editor_mut().document.select_all();
        app.editor_mut().document.insert("");
        checkpoint(&mut app);
    }
    let mut app = Workspace::open(None).unwrap();
    configure(&mut app, dir.path());
    app.recover_document(0);
    assert_eq!(app.editor().document.text(), "");
    assert!(app.editor().document.is_dirty());
    assert_eq!(app.editor().path(), None);
    assert_eq!(fs::read_to_string(file).unwrap(), "delete me");
    app.finish_state().unwrap();
}

#[cfg(unix)]
#[test]
fn control_characters_in_filenames_do_not_prevent_crash_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("draft\nnotes\t.md");
    fs::write(&file, "disk").unwrap();
    {
        let mut app = Workspace::open(Some(file.clone())).unwrap();
        configure(&mut app, dir.path());
        app.editor_mut().document.insert("local ");
        checkpoint(&mut app);
    }
    let mut app = Workspace::open(None).unwrap();
    configure(&mut app, dir.path());
    app.recover_document(0);
    assert_eq!(app.editor().document.text(), "local disk");
    assert_eq!(
        app.tabs[app.active].recovered.as_ref().unwrap().1,
        Some(file)
    );
    app.finish_state().unwrap();
}

#[test]
fn save_via_cancelled_quit_removes_its_recovery_copy_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.md");
    let second = dir.path().join("second.md");
    fs::write(&first, "first").unwrap();
    fs::write(&second, "second").unwrap();
    {
        let mut app = Workspace::open(Some(first.clone())).unwrap();
        configure(&mut app, dir.path());
        app.editor_mut().document.insert("saved ");
        app.open_path(second).unwrap();
        app.editor_mut().document.insert("unsaved ");
        checkpoint(&mut app);
        app.request(Intent::Quit);
        draw(&mut app, 80, 24);
        key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
        draw(&mut app, 80, 24);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(fs::read_to_string(first).unwrap(), "saved first");
    }
    let mut app = Workspace::open(None).unwrap();
    configure(&mut app, dir.path());
    app.recover_document(0);
    assert_eq!(app.editor().document.text(), "unsaved second");
    app.open_recovery();
    assert!(app.choice.is_none());
    app.finish_state().unwrap();
}
