//! Workspace policy for preferences and crash checkpoints. No original file is written here.
use super::{App, Choice, ChoiceMode, Tab, Workspace, picker::Picker};
use crate::{
    recovery::{Candidate, Snapshot, Store},
    settings::Settings,
    theme::Theme,
};
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(PartialEq, Eq)]
struct Stamp {
    revision: u64,
    path: Option<PathBuf>,
    live: bool,
}

pub(super) struct Persistence {
    pub settings: Option<Settings>,
    store: Option<Store>,
    candidates: Vec<Candidate>,
    diagnostics: Vec<String>,
    checkpoints: HashMap<usize, Stamp>,
    last_error: Option<String>,
}

impl Workspace {
    pub fn enable_state(&mut self, config: &Path, state: &Path, explicit_theme: bool) {
        let mut warnings = Vec::new();
        let settings = match Settings::load(config) {
            Ok(settings) => {
                self.sidebar.preference = settings.sidebar();
                if !explicit_theme && let Some(theme) = settings.theme() {
                    self.editor_mut().set_theme(theme);
                }
                if let Some(name) = settings.unknown_theme() {
                    warnings.push(format!(
                        "Unknown saved theme {name:?}; choose a theme with F2"
                    ));
                }
                Some(settings)
            }
            Err(error) => {
                warnings.push(format!("Preferences unavailable: {error}"));
                None
            }
        };
        let mut candidates = vec![];
        let mut diagnostics = vec![];
        let store = match Store::open(state.join("recovery")) {
            Ok(store) => {
                match store.list_abandoned() {
                    Ok(listing) => {
                        candidates = listing.candidates;
                        diagnostics = listing.diagnostics;
                    }
                    Err(error) => warnings.push(format!("Recovery discovery failed: {error}")),
                }
                Some(store)
            }
            Err(error) => {
                warnings.push(format!("Crash recovery unavailable: {error}"));
                None
            }
        };
        candidates.sort_by(|a, b| {
            a.snapshot
                .label
                .cmp(&b.snapshot.label)
                .then_with(|| a.id.cmp(&b.id))
        });
        if !diagnostics.is_empty() {
            warnings.push(format!("Recovery: {}", diagnostics.join("; ")));
        }
        if !candidates.is_empty() {
            warnings.insert(
                0,
                format!(
                    "{} recoverable document(s). F2 → Recover documents",
                    candidates.len()
                ),
            );
        }
        self.persistence = Some(Persistence {
            settings,
            store,
            candidates,
            diagnostics,
            checkpoints: HashMap::new(),
            last_error: None,
        });
        if !warnings.is_empty() {
            self.editor_mut().set_message(warnings.join(" · "));
        }
    }

    pub(super) fn persist_theme(&mut self, theme: Theme) {
        let result = self
            .persistence
            .as_mut()
            .and_then(|state| state.settings.as_mut())
            .map(|settings| settings.save_theme(theme));
        if let Some(Err(error)) = result {
            self.editor_mut().set_message(format!(
                "Theme applied for this session; could not save preferences: {error}"
            ));
        }
    }
    pub(super) fn persist_sidebar(&mut self) {
        let preference = self.sidebar.preference;
        let result = self
            .persistence
            .as_mut()
            .and_then(|state| state.settings.as_mut())
            .map(|settings| settings.save_sidebar(preference));
        if let Some(Err(error)) = result {
            self.editor_mut().set_message(format!(
                "Sidebar changed for this session; could not save preferences: {error}"
            ));
        }
    }

    pub(super) fn clear_recovery(&mut self, number: usize) {
        let Some(state) = &mut self.persistence else {
            return;
        };
        if !state.checkpoints.contains_key(&number) {
            return;
        }
        let result = state
            .store
            .as_mut()
            .map(|store| store.remove(number as u64));
        match result {
            Some(Ok(())) => {
                state.checkpoints.remove(&number);
            }
            Some(Err(error)) => self
                .editor_mut()
                .set_message(format!("Could not remove old recovery copy: {error}")),
            None => {}
        }
    }

    pub fn tick(&mut self) -> bool {
        if self.last_tick.elapsed() < Duration::from_secs(1) {
            return false;
        }
        self.last_tick = Instant::now();
        let mut changed = self.editor_mut().check_external_change();
        let Some(mut state) = self.persistence.take() else {
            return changed;
        };
        let mut errors = Vec::new();
        if let Some(store) = &mut state.store {
            for tab in &self.tabs {
                if !tab.editor.document.is_dirty() {
                    continue;
                }
                let stamp = Stamp {
                    revision: tab.editor.document.revision(),
                    path: tab.editor.path().map(Path::to_owned),
                    live: tab.editor.live,
                };
                if state.checkpoints.get(&tab.number) == Some(&stamp) {
                    continue;
                }
                let snapshot = snapshot(tab);
                match store.checkpoint(tab.number as u64, &snapshot) {
                    Ok(()) => {
                        state.checkpoints.insert(tab.number, stamp);
                    }
                    Err(error) => errors.push(format!(
                        "Crash recovery could not checkpoint {}: {error}",
                        snapshot.label
                    )),
                }
            }
            let remove: Vec<_> = state
                .checkpoints
                .keys()
                .copied()
                .filter(|number| {
                    !self
                        .tabs
                        .iter()
                        .any(|tab| tab.number == *number && tab.editor.document.is_dirty())
                })
                .collect();
            for number in remove {
                match store.remove(number as u64) {
                    Ok(()) => {
                        state.checkpoints.remove(&number);
                    }
                    Err(error) => errors.push(format!("Recovery cleanup failed: {error}")),
                }
            }
        }
        let error = (!errors.is_empty()).then(|| errors.join(" · "));
        if error != state.last_error
            && let Some(error) = &error
        {
            self.editor_mut().set_message(error);
            changed = true;
        }
        state.last_error = error;
        self.persistence = Some(state);
        changed
    }

    pub fn finish_state(&mut self) -> io::Result<()> {
        if let Some(store) = self
            .persistence
            .as_mut()
            .and_then(|state| state.store.as_mut())
        {
            store.finish_clean()?;
        }
        Ok(())
    }

    pub(super) fn open_recovery(&mut self) {
        let Some(state) = &mut self.persistence else {
            self.editor_mut()
                .set_message("Crash recovery is disabled for this session");
            return;
        };
        if state.candidates.is_empty()
            && let Some(store) = &state.store
        {
            match store.list_abandoned() {
                Ok(listing) => {
                    state.candidates = listing.candidates;
                    state.diagnostics = listing.diagnostics;
                }
                Err(error) => state.diagnostics = vec![error.to_string()],
            }
        }
        if state.candidates.is_empty() {
            let message = if state.diagnostics.is_empty() {
                "No abandoned recovery copies found".to_owned()
            } else {
                format!(
                    "Recovery copies retained for inspection: {}",
                    state.diagnostics.join("; ")
                )
            };
            self.editor_mut().set_message(message);
            return;
        }
        state.candidates.sort_by(|a, b| {
            a.snapshot
                .label
                .cmp(&b.snapshot.label)
                .then_with(|| a.id.cmp(&b.id))
        });
        let entries = state
            .candidates
            .iter()
            .map(|candidate| {
                let snap = &candidate.snapshot;
                let name = snap
                    .original_path
                    .as_deref()
                    .map(|path| super::compact_path(path, 48))
                    .unwrap_or_else(|| crate::projection::safe_text(&snap.label));
                (name, format!("{} B · new tab", snap.source.len()))
            })
            .collect();
        self.editor_mut().deactivate();
        self.choice = Some(Choice {
            picker: Picker::new(
                "Recover documents",
                "Enter: recover copy · Esc: later",
                entries,
                0,
            ),
            mode: ChoiceMode::Recovery,
        });
    }

    pub(super) fn recover_document(&mut self, index: usize) {
        let Some(mut state) = self.persistence.take() else {
            return;
        };
        let Some(candidate) = state.candidates.get(index) else {
            self.persistence = Some(state);
            return;
        };
        let snap = &candidate.snapshot;
        let editor = App::recovered(
            snap.source.clone(),
            snap.selection,
            snap.live,
            snap.markdown,
        );
        let number = self.next_number;
        let recovered = Some((snap.label.clone(), snap.original_path.clone()));
        let tab = Tab {
            editor,
            number,
            identity: None,
            recovered,
        };
        let checkpoint = snapshot(&tab);
        let result = state
            .store
            .as_mut()
            .ok_or_else(|| io::Error::other("Recovery storage unavailable"))
            .and_then(|store| {
                store.checkpoint(number as u64, &checkpoint)?;
                state.checkpoints.insert(
                    number,
                    Stamp {
                        revision: tab.editor.document.revision(),
                        path: None,
                        live: tab.editor.live,
                    },
                );
                store.consume(candidate)
            });
        if result.is_ok() {
            state.candidates.remove(index);
        }
        // Even if a checkpoint fails, the recovered text remains open and the
        // original recovery record is retained. Never lose either side of a handoff.
        self.editor_mut().deactivate();
        self.tabs.push(tab);
        self.next_number += 1;
        self.active = self.tabs.len() - 1;
        self.sidebar.selected = self.active;
        self.sidebar.invalidate();
        self.tab_hits.clear();
        self.layout_valid = false;
        self.editor_mut().set_message(match result {
            Ok(()) => "Recovered as an unsaved copy. Save As chooses a new filename.".to_owned(),
            Err(error) => format!("Recovered in memory; old recovery copy retained: {error}"),
        });
        self.persistence = Some(state);
    }
}

fn snapshot(tab: &Tab) -> Snapshot {
    Snapshot {
        source: tab.editor.document.text().to_owned(),
        original_path: tab
            .editor
            .path()
            .map(Path::to_owned)
            .or_else(|| tab.recovered.as_ref().and_then(|(_, path)| path.clone())),
        selection: tab.editor.document.selection(),
        live: tab.editor.live,
        markdown: tab.editor.is_markdown(),
        label: crate::projection::safe_text(
            &tab.editor
                .path()
                .map(|path| path.to_string_lossy().into_owned())
                .or_else(|| tab.recovered.as_ref().map(|(label, _)| label.clone()))
                .unwrap_or_else(|| format!("Untitled {}.md", tab.number)),
        )
        .chars()
        .take(1024)
        .collect(),
    }
}
