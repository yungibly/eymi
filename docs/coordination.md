# Marklane implementation coordination

Working product name: **Marklane**. Command and Cargo package/binary name: **`marklane`**.

This note records the two-agent implementation assignments, starting with the first working editing surface and followed by the find/clipboard checkpoint below. The [product plan](product-plan.md) and [technical plan](technical-plan.md) describe the broader direction.

## Confirmed product decisions

- Familiar, non-modal editing controls.
- Live Markdown reveals syntax in the active semantic block and renders the surroundings.
- Clicking the checkbox itself toggles completion; clicking the label places the caret for editing.
- Enter continues bullet, numbered, and task lists. New tasks are unchecked; Enter on an empty item exits the current list level.
- Toggles, continuation, paste, and ordinary edits are undoable source edits.
- Original Markdown remains authoritative. Presentation changes never rewrite the document.
- Rust with Ratatui/Crossterm is the starting stack. A String-backed core is acceptable for the prototype; adopting a rope is not a gate for validating the interaction.

## Ownership

All participants are working in the same checkout. Changes are immediately visible; no copying or cherry-picking between agents is required.

| Owner | Task ID | Exclusive write ownership |
| --- | --- | --- |
| Coordinator | `01a0bcd1-fd02-7120-8c0c-8097af85e9a8` | `README.md`, `docs/`, scope decisions, integration review |
| Agent A: document/editing core | `01a0bce9-1888-7cd3-8520-70a91d278a10` | `Cargo.toml`, `Cargo.lock`, `.gitignore`, `src/lib.rs`, `src/document.rs`, `src/editing.rs`, `src/markdown.rs`, `tests/core/`, core module-local tests |
| Agent B: terminal/projection | `01a0bcea-4928-7273-af72-a047900f90d0` | `src/projection.rs`, `src/file_io.rs`, `src/clipboard.rs`, `src/search.rs`, `src/search_highlight.rs`; app/main/UI integration temporarily transferred to the forward implementer below |
| Forward implementer | `01a0bd31-94a3-7e11-a3b5-43c15370db44` | Next workspace slice: `src/app.rs`, `src/main.rs`, new `src/workspace.rs`, new `src/browser.rs`, associated UI tests; `src/simulation.rs` / `src/terminal.rs` only if essential integration |

Agent B declares its UI modules from the binary initially, leaving A's library exports under A's ownership. Request manifest/dependency changes from A. Extra paths or changes to the ownership boundary require coordination before editing. Cargo may update its lockfile during checks; only A deliberately manages dependency/lockfile changes.

Use module-local tests initially, or add explicit integration-test entrypoints under the agreed ownership. Nested `tests/core/` and `tests/ui/` directories alone are not automatically discovered as Cargo integration tests.

Completed review-fix handoff: B explicitly released `src/file_io.rs` to A to fix reproduced read-only overwrite and FIFO-baseline-read hangs. A preserved the public `FileState` API, added focused regressions, and returned ownership to B. B then ran final combined checks. The normal ownership table above applies again.

The user has authorized commits at coherent milestones. The coordinator owns integration commits across reviewed participant files; implementing agents leave their work unstaged until handoff. Keep implementation in the working tree until a coherent checkpoint is ready. Do not spawn additional agents for this assignment without coordination.

## Shared interface handoff

Agent A publishes a small compileable scaffold and sends its public API to B and the coordinator before doing the bulk of implementation. B can independently prepare terminal lifecycle and projection structure in the meantime.

The shared API must expose:

1. Current source text and document revision.
2. Selection anchor/head and safe setters, with explicit UTF-8 byte-offset semantics.
3. Insertion/selection replacement, grapheme-safe horizontal movement and deletion, Markdown Enter, literal newline, undo/redo, and task toggle commands.
4. Dirty state and saved-content baseline operations.
5. Semantic block source ranges, block kinds, and task marker source ranges tied to the parsed document revision.

The core owns edits and undo transactions. The projection owns visual rows, wrapping, display-cell positions, hit testing, and vertical movement geometry. Vertical movement computes a source target and asks the core to update selection. Input translates keys/mouse/paste into commands; it must not independently mutate source or implement a second version of list continuation.

For checkbox hits, B identifies the exact marker range plus document revision, and A verifies/toggles the marker. Text hits identify insertion boundaries. A stale parse or hit map must be refreshed or rejected rather than applied at a guessed offset.

Ranged parser events are input to mapping, not a complete caret map. Unsupported or uncertain constructs may remain styled source. Active-block disclosure must preserve source anchors; source view remains available for inspection.

### Accepted initial Rust contract

Agent A proposed, and the coordinator approved, this interface for the first integration. The actual definitions in `src/lib.rs` and its modules are authoritative as the scaffold lands.

- `Document::new(impl Into<String>)`, `text() -> &str`, `revision() -> u64`, `is_dirty() -> bool`, and `mark_saved()`.
- `Selection { anchor: usize, head: usize }`, using UTF-8 byte offsets at extended grapheme boundaries, with `caret()`, `range()`, and `is_empty()` helpers.
- `Document::selection()`, `set_selection(Selection) -> Result<(), EditError>`, and `set_caret(usize) -> Result<(), EditError>`.
- `insert(&str)`, `backspace()`, `delete_forward()`, `enter()`, `literal_newline()`, `undo()`, and `redo()` return whether they changed the document. Literal insertion is one transaction, suitable for paste.
- `move_left(extend: bool)`, `move_right(extend: bool)`, and `select_all()` update selection without changing the document revision.
- `markdown() -> MarkdownSnapshot`, containing the document revision, semantic `blocks`, and `tasks`; `markdown::analyze(text, revision)` is also public.
- `Block { range: Range<usize>, kind: BlockKind }`, with paragraph, heading, list-item, code, quote, table, rule, and HTML kinds. Ranges may nest; code/table disclosure remains atomic.
- `Task` contains a single-character `marker_range`, `item_range`, `checked`, and `revision`.
- `toggle_task(&Task) -> Result<bool, EditError>` rejects stale revisions; `toggle_task_at_caret()` supports keyboard use.

A documents prefix/newline inclusion for ranges. B caches parse snapshots by source revision, independently of cursor movement, and handles blank source gaps. For non-Markdown files, B dispatches `literal_newline()` instead of Markdown `enter()`. Edits must retain grapheme-valid selection even when new text combines with adjacent characters.

A and B should resolve small naming changes directly and notify the coordinator of decisions that alter ownership or behavior. Format owned files only while implementation is concurrent.

## Agent A deliverable

A headless editing core and Markdown structure index with focused tests for source fidelity, valid positions, editing/undo, task toggles, and contextual list continuation. Preserve existing newline sequences, BOM, whitespace, and final-newline state. Treat paste as literal inserted text, not simulated key events.

Use parser/container context to exclude fenced and indented code from list/task helpers. Preserve list markers and indentation. Handle source and live presentations through the same editing commands.

## Agent B deliverable

A runnable terminal editor for one named UTF-8 file or an untitled Markdown buffer, with source/live views, normal text navigation and selection, internal copy/cut/paste plus bracketed paste, mouse placement, checkbox interaction, undo/redo, save, a small help/status surface, and unsaved-work handling on quit.

Start with conservative rendering of headings, emphasis, lists, quotes, and code. Clickable task glyphs have distinct hit targets. Save failures retain dirty content; unsupported encodings are not decoded lossily; a changed disk baseline prevents silent overwrite. Terminal state is restored on normal/error exit paths.

The initial save implementation belongs to B because it is part of opening/running the application; A provides baseline and dirty-state support. Rich external-change review is deferred, but detecting a conflict and retaining the user's buffer is required.

## Integration acceptance

- `cargo build` produces the `marklane` executable, and the focused core/UI tests pass.
- Opening and saving an untouched fixture preserves its bytes.
- Source/live switching retains the same buffer, selection, and undo history.
- Click a visible task checkbox: only its marker changes; the text caret stays anchored; undo restores the earlier content.
- Clicking task text enters editing rather than toggling completion.
- Enter continues bullets, increments numbered items, and inserts unchecked tasks; an empty item exits appropriately.
- Paste inserts literal content as one undoable operation.
- Caret placement and selection are correct at wrapping and wide/combining-character boundaries in supported constructs.
- Failed saves and disk conflicts retain unsaved work; quit does not silently discard it.
- The terminal is usable again after exit.

A runnable subset must clearly disclose limitations. Do not claim full Markdown support, complete Unicode terminal agreement, or system clipboard integration based solely on headless tests. Coordinator reviews combined behavior after the two implementations are ready; tests and demos are evidence, not a substitute for the interface contract.

## Verification without terminal-app automation

The user reports that Computer Use blocks agents from operating terminal apps, including Apple Terminal and Ghostty. Do not attempt terminal-app GUI automation or work around that restriction. Ordinary authorized shell commands for building and running automated tests remain available. The user will perform essential visual checks in a real terminal once a concrete build and short checklist are ready.

Agent B owns a small deterministic screen/event harness alongside the production app:

- Render through the actual app/projection code into a Ratatui `TestBackend` or equivalent in-memory cell buffer.
- Inject synthetic key, mouse, resize, and paste events through the same input/command path as the live app. Do not implement a second editing engine for the harness.
- Capture cell text/styles, cursor, selection, viewport dimensions, source revision, and hit targets where useful for an assertion.
- Exercise checkbox versus label clicks, live/source switching, block disclosure, list continuation, selection, and Unicode/wrapping boundaries at a few fixed viewport sizes.
- Export a plain-text screen snapshot initially. A styled HTML rendering of the same cell grid is optional if inexpensive; a full terminal emulator or browser shell is outside scope.

Keep scenarios reproducible with fixed fixtures and explicit events. Use targeted snapshots plus semantic assertions, such as “only the checkbox marker changed” and “the click selected this source offset.” A screenshot alone is not sufficient evidence of correct editing.

The harness verifies application logic and its intended cell output. Actual terminal fonts/glyph widths, escape-sequence transport, key interception, clipboard integration, IME, and terminal cleanup still need real-environment checks. Leave these marked unverified until the user runs them; do not substitute simulated results for terminal compatibility claims.

At handoff, B supplies the exact run command, fixture, and a short manual checklist. The coordinator consolidates the checks and findings. Do not interrupt the user for visual testing before a runnable build is ready.

## Deferred work

Tabs, file browser, sophisticated command palette, extensive theme packs, heading folding, direct table-cell editing, automatic external reload/merge, recovery journals, LSP, image rendering, text scaling, and built-in agent/model integrations remain in the broader plans. Native clipboard support moved into the completed follow-up checkpoint below; native Wayland and OSC 52 remain deferred.

## Initial checkpoint

- Agent A completed the core and the two file-save review fixes. Its 22 library tests and 9 focused file I/O tests pass; file ownership is returned to B.
- Agent B completed the terminal/projection prototype and the coordinator-requested empty-clipboard, EOF-disclosure, wrap-affinity, BOM, and standalone-CR corrections.
- Final combined verification on September 20, 2026: 49 tests pass (22 core, 10 app, 8 projection, 9 file I/O), plus build, strict all-target Clippy, formatting check, and a production-renderer headless snapshot.
- User clarified that terminal-app GUI verification is unavailable to agents; headless simulation is the automated verification path, with essential real-terminal checks reserved for the user.
- Coordinator owns name, scope, ownership, and integration decisions.
- Both agents have completed the initial assignment. No implementation commits or staging were performed by the agents or coordinator.

### Current implementation limits

The core uses complete String snapshots per edit, without typing coalescing or a history cap, and is intended for small files. Semantic edit commands parse the full source; UI snapshots are cached by revision. Indent/outdent is limited to empty-item Enter. Ordered-marker overflow uses a literal newline rather than creating an invalid ten-digit Markdown marker. An indented empty hyphen inside an existing list has an intentional note-taking convenience interpretation where CommonMark can otherwise treat it as a Setext underline.

The UI lays out the full document and wraps at grapheme boundaries; refined prose word wrapping and hanging indents are future polish. Tables and code fences retain source; entity/escape/reference-link rendering is conservative. The initial checkpoint used internal clipboard storage; native clipboard work is assigned below. Disk conflict handling requires Save As; there is no watcher, recovery journal, or merge UI yet. Edited symlink/hardlink targets are refused. Atomic replacement still cannot exclude an independent writer after the final baseline check. The user reported passing initial manual checks, with the two interaction follow-ups below; the terminal name/version was not recorded.

The [prototype check guide](prototype-checks.md) separates automated checks from user-run terminal verification.

## User feedback and next work

On September 20 the user reported that manual tests passed overall and authorized continued work. Two immediate follow-ups are assigned under the original ownership boundaries:

- Agent A: fix repeated Enter reentering an exited list, with sequential regression cases for bullets, numbers, tasks, nesting, quotes, line endings, and undo/redo.
- Agent B: expand initiating checkbox click targets by one safe cell on each side; preserve label clicks, drag selection, keyboard geometry, source editing, and undo behavior.

Both fixes now pass focused regressions. Continue with find/replace and system clipboard support as the next bounded group of everyday-editing features, assigned below. Tabs/browser and the remaining recovery/external-change features retain their place in the broader plan.

### Find and clipboard checkpoint

This work is now assigned to the same two agents. The coordinator continues to own documentation and final integration review.

- **A, core:** literal source search and Replace All. Publish `Document::find_matches(&str) -> Vec<Range<usize>>` and `Document::replace_all(&str, &str) -> usize`. Matches are case-sensitive, nonoverlapping, and bounded by extended graphemes. An empty query has no matches. Replacement text is literal; replacement does not search its own output. Replace All is one undo transaction and preserves all untouched source bytes. Identical replacements leave content, dirty state, revision, and undo history unchanged. Map selection through the replacement and retain valid boundaries; undo restores the original directional selection.
- **B, interface:** Find and Find/Replace controls, match navigation/counts, current-match replacement, and a clipboard adapter. Use the core's search and edit methods. Search includes Markdown source that is normally concealed; selecting a result must reveal it and bring it into view. Input into a search field must not edit the document. Recompute matches after edits so replacements never use stale source offsets.
- **B, clipboard:** access the native clipboard only for an explicit copy, cut, or paste action. Keep a clearly identified internal fallback and terminal-delivered paste. Empty or failed paste must retain selected text. Copy Markdown source verbatim; cut must retain the copied text before deleting. Isolate native behavior behind an injectable backend and use fakes in automated checks, without changing the user's actual clipboard. A alone edits dependencies if B needs one.

The entry controls are Ctrl+F for Find, Ctrl+R for Find/Replace, and F3 / Shift+F3 for next/previous match. Full controls are recorded in the completed checkpoint below. Regex and case-folding modes are deferred; literal matching is explicit in the interface.

Acceptance includes Unicode and line-ending fidelity, searches of concealed link destinations, wraparound navigation, no matches, empty queries, source revision changes, one-step Replace All undo, paste as one transaction, clipboard failures, and narrow-screen rendering. Clipboard platform availability and actual terminal shortcut delivery remain distinct from fake-backend test results.

### Completed follow-up checkpoint

Both reported interaction issues and the find/clipboard assignment are complete. A implemented the source changes and search API, then reviewed B's clipboard adapter without findings. B implemented the pointer-only padding, search panel, native clipboard adapter, and production event-path regressions. Coordinator review also corrected hidden clipboard feedback, invisible replacement actions in short windows, and empty terminal paste deleting a selection.

At this checkpoint, controls were Ctrl+F, Ctrl+R, and F3 / Shift+F3, with Tab traversing fields and actions. The later search-polish checkpoint below supersedes that focus flow and Find's height requirement. Escape preserves the source selection and returns to document editing. Input fields have their own selection and undo history.

The native adapter is enabled only in local interactive sessions and is accessed lazily by copy/cut/paste. A 350 ms operation timeout and at most 150 ms shutdown wait keep native failures bounded. Backend failures switch to internal clipboard for the session; empty/nontext reads do not paste stale internal data. SSH and headless sessions use internal storage. Linux native access requires X11/XWayland; native Wayland and OSC 52 are not included. Actual native delivery remains a manual check.

Coordinator's final combined checks passed September 20, 2026: 86 tests (37 core, 25 app, 9 projection, 9 file I/O, 6 clipboard), build, strict all-target Clippy, formatting, live/source headless renders, CLI help, and diff whitespace checks. See the [verification record](prototype-checks.md#verification-record). The agents are finished with this checkpoint; all implementation remains unstaged and uncommitted.

Report interface decisions and blockers through task messages. On completion, include exact files changed, commands/checks run, demonstrated behavior, and remaining limitations.

## Search polish checkpoint

The user reports that the follow-up build works and requests better search navigation, working mouse controls, and subdued highlighting of all matches with a stronger current-match highlight. This checkpoint is limited to those improvements.

- A temporarily owns the new `src/search_highlight.rs` and its local tests. Its pure `style_match` helper decorates projected source spans without changing text, layout, or hit geometry. A then reviews B's pointer/focus changes read-only. Core, manifest, and clipboard behavior need no changes.
- B owns `src/app.rs`, `src/search.rs`, `src/main.rs`, UI tests, and any extracted panel-geometry module. B registers A's module and integrates it into the production draw path. The coordinator owns docs and final validation.
- Tab / Shift+Tab cycles between Find and With; buttons no longer interrupt field switching. Up/Down also switches fields. Find Enter / Shift+Enter navigates results. With Enter replaces the current match and advances, with a visible hint. Alt+R and Alt+A explicitly invoke Replace and Replace All. F3, Escape, and global save/quit/help retain their meanings.
- Rendered Previous, Next, Replace, Replace All, and Close controls accept pointer clicks. Field click, Shift-click, and drag use the same grapheme/cell geometry as drawing, including horizontal scrolling. An action fires once on mouse down; unrendered or disabled controls have no active target. Resizing or changing the panel must invalidate stale pointer geometry.
- While the panel is open, every visible mapped match has a subtle readable highlight; the current result uses stronger contrast and bold/underline. Ordinary selection remains distinct. Closing the panel clears search decoration and retains the normal source selection. Concealed syntax remains concealed until that result is selected. Query/source revisions refresh matches before styling or replacement.

Acceptance uses actual input events and Ratatui cell styles: multiple highlighted results, current-result movement, query edits and replacement clearing stale styles, source/live/Unicode/wrap cases, field and button pointer hits, Tab/reverse-Tab, drag selection, one-click replacement/undo, disabled states, and narrow/short/resize behavior. Terminal-app GUI automation remains unavailable; the user's manual pass is recorded separately from these automated checks.

### Completed search polish

A completed the style helper and read-only pointer/focus review. B integrated it and completed the mouse and keyboard interactions, including document wheel scrolling while Find remains open. Find requires 11 terminal rows; Find/Replace requires 12. Short panels retain Close/Escape; narrow panels omit controls that cannot fit. Field drags keep the starting horizontal layout; off-edge auto-scroll is not implemented. Body clicks do not edit the document while the panel is open.

Coordinator verification: 102 tests pass (37 core, 32 app, 9 projection, 9 file I/O, 6 clipboard, 9 highlight), along with build, strict all-target Clippy, formatting, headless render, and CLI help. One test-only Clippy range-array fixture warning was fixed without behavior changes and rechecked with the 9 highlight tests and all-target lint. No implementation was staged or committed. A's temporary write assignment is complete; `src/search_highlight.rs` returns to B's UI ownership for future changes.

## Forward implementation and review

The user opened “Coordinate markdown editor help” to advance implementation while the original pair reviews and hardens completed slices. Search polish is complete. The coordinator will commit that validated baseline before releasing the new task to edit shared files; the next bounded assignment is document tabs plus a basic text-file browser.

The forward implementer owns the app/workspace/browser paths in the table while implementing. A and B remain read-only reviewers of those paths until an explicit file handoff. Small fixes must go through the current writer or follow an explicit ownership transfer; no concurrent edits to the same files. The coordinator retains documentation and commit ownership. Dependencies remain A's responsibility.

The next slice must provide Ctrl+N, Ctrl+O, per-tab state, Ctrl+W with dirty-document handling, Ctrl+Q protecting every dirty tab, keyboard switching and clickable tabs. Browser opening must reuse safe regular-file/UTF-8 validation, retain the existing document on failure, avoid recursive scans, and prevent duplicate same-path buffers or Save As conflicts with another open tab. Clipboard is app-wide; search behavior on tab switching must be explicit. File watching, recovery, themes, shell sessions, and splits are not part of this slice. Deliver a runnable slice with actual input/render-path tests, then hand it back for review before moving on.
