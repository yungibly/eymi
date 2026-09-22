# Marklane implementation coordination

Working product name: **Marklane**. Command and Cargo package/binary name: **`marklane`**.

## Theme library and recovery checkpoint — September 22, 2026

The current work supersedes the historical ownership below. The same three
agents worked in isolated worktrees; the coordinator integrated their commits
and owns final state/UI integration, tests, and documentation.

| Owner | Worktree branch | Integrated scope |
| --- | --- | --- |
| Editor surface | `codex/theme-library` | 624 themes, contrast-safe editor roles, pinned color import and attribution |
| Editing core | `codex/external-file-safety` | Advisory disk monitoring, F5 reload with confirmation and undo, explicitly unsaved recovered documents |
| Workspace/storage | `codex/recovery-storage` | Private locked sessions, atomic checkpoints, bounded validation, safe recovery handoff |
| Coordinator | `main` | Searchable live-preview theme picker, settings, CLI, polling, recovery workflow, cross-feature regressions, terminal verification |

Shared APIs are `Theme::all/from_name/name/id/is_dark`,
`App::check_external_change/request_reload/recovered`, and
`recovery::Store::{open,checkpoint,remove,list_abandoned,consume,finish_clean}`.
Configuration persists stable theme IDs. Snapshot/help/version/listing paths are
state-free. A periodic event poll checkpoints dirty tabs and checks the active
file without interrupting typing groups. Recoveries always create detached,
unsaved copies; old records are consumed only after durable handoff.

All three changes are integrated. Independent review fixed settings size,
concurrent-writer/read-only behavior, full-query replacement, control-character
recovery labels, and immediate checkpoint cleanup during a canceled multi-tab
quit. CI and Homebrew remain the next separate step; attribution must accompany
future distributions. See the [verification record](prototype-checks.md#verification-record).

## Everyday usability checkpoint — September 22, 2026

This checkpoint supersedes the historical assignments below. The user requested
a substantial step toward everyday use, with the supplied calm writing-surface
mockup as design inspiration. CI and Homebrew publishing remain deferred.

All three implementers use separate managed worktrees. Only the coordinator
integrates reviewed commits into the main checkout; agents do not modify it.

| Owner | Task | Files and deliverable |
| --- | --- | --- |
| Coordinator | `01a0c8cd-e941-7b02-8a7b-64c2be21ea6a` | CLI, documentation, terminal runner, keyboard integration, review and final verification |
| Editing core | `01a0c8ce-3502-7702-aa01-eab19e7899d3` | `document.rs`, `editing.rs`, `lib.rs`: word navigation/deletion, indentation, inline formatting, bounded/coalesced undo |
| Editor surface | `01a0c8d1-84f0-7f83-bd06-3272b73e0804` | `app.rs`, `projection.rs`, `search_highlight.rs`, `theme.rs`: paired light/dark palettes, readable width, wrapping, offset viewport geometry |
| Workspace | `01a0c8d1-97e2-7d52-9bdc-de9c1a414e07` | `workspace.rs`, `browser.rs`, workspace child modules: responsive documents/outline sidebar and filterable command palette |

Shared interfaces: `App::draw_in(frame, area, show_cursor)` accepts a horizontal
slice with absolute coordinates and full frame height, retaining existing top
row reservations. `App::jump_to_source(offset)` resets transient interaction and
follows the requested caret. Existing chrome-style functions use the current
theme. `theme::set_theme(Theme::{Dark,Light})` selects the palette before opening
the workspace. `Document::insert` remains a literal transaction, with a separate
`type_text` and `break_undo_group` API for explicit typing grouping.

Acceptance includes source fidelity, undo/redo and selection geometry, existing
file-conflict and quit protections, tiny/compact/wide layouts, and actual
executable captures with the pinned `tui-test` at 80×24, 120×36, and 160×45.
New commands must be discoverable from help or the command palette. The
coordinator runs the combined suite, build, strict Clippy, formatting and visual
inspection after integrating all workstreams. Known emulator Unicode limits
remain separate from application correctness.

All three assigned workstreams have been integrated into `main` from their
isolated worktrees. The coordinator also added CLI theme/snapshot controls,
consistent command-line file limits, streaming disk-baseline comparisons,
scrollable help, and real PTY tests of the new editing controls. A final review
found that an already-active tab switch could bypass the typing-group boundary;
the workspace event dispatcher now ends groups before consuming any non-typing
event, with keyboard and pointer regressions. The combined 206 tests and exact
strict lint/format/build checks pass. Implementer worktrees remain available as
the historical handoff; final integrated code is in the main checkout.
Final light/dark executable runs each pass 75 assertions, with 30 successful PNG
exports and three known original-Unicode-fixture PNG failures per theme. Raw
traces and SVG/cells remain available. The optimized release binary builds and
renders successfully. The [verification record](prototype-checks.md#verification-record)
contains artifact locations and the remaining native-terminal checks.

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

The initial checkpoints used one shared checkout with exclusive file ownership. The current visual/input/UI checkpoint uses separate worktrees; its assignment table below supersedes this historical ownership table. Only the coordinator integrates agent commits into `main`.

| Owner | Task ID | Exclusive write ownership |
| --- | --- | --- |
| Coordinator | `01a0bcd1-fd02-7120-8c0c-8097af85e9a8` | `README.md`, `docs/`, scope decisions, integration review |
| Agent A: document/editing core | `01a0bce9-1888-7cd3-8520-70a91d278a10` | `Cargo.toml`, `Cargo.lock`, `.gitignore`, `src/lib.rs`, `src/document.rs`, `src/editing.rs`, `src/markdown.rs`, `tests/core/`, core module-local tests |
| Agent B: terminal/projection | `01a0bcea-4928-7273-af72-a047900f90d0` | `src/projection.rs`, `src/file_io.rs`, `src/clipboard.rs`, `src/search.rs`, `src/search_highlight.rs`; app/main/UI integration temporarily transferred to the forward implementer below |
| Forward implementer | `01a0bd31-94a3-7e11-a3b5-43c15370db44` | Next workspace slice: `src/app.rs`, `src/main.rs`, new `src/workspace.rs`, new `src/browser.rs`, associated UI tests; `src/simulation.rs` / `src/terminal.rs` only if essential integration |

Agent B declares its UI modules from the binary initially, leaving A's library exports under A's ownership. Request manifest/dependency changes from A. Extra paths or changes to the ownership boundary require coordination before editing. Cargo may update its lockfile during checks; only A deliberately manages dependency/lockfile changes.

Use module-local tests initially, or add explicit integration-test entrypoints under the agreed ownership. Nested `tests/core/` and `tests/ui/` directories alone are not automatically discovered as Cargo integration tests.

Completed review-fix handoff: B explicitly released `src/file_io.rs` to A to fix reproduced read-only overwrite and FIFO-baseline-read hangs. A preserved the public `FileState` API, added focused regressions, and returned ownership to B. B then ran final combined checks. The normal ownership table above applies again.

The user has authorized commits at coherent milestones. The coordinator owns integration commits across reviewed participant files; agents in the shared checkout leave their work unstaged until handoff. Separate worktree assignments may authorize focused commits as described below. Keep shared implementation in the working tree until a coherent checkpoint is ready. Do not spawn additional agents for this assignment without coordination.

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

The validated baseline is committed as `de16f81`. The forward implementer is released to edit its assigned paths. A also has a temporary, exclusive `src/file_io.rs` assignment: add backward-compatible `FileState::open_bounded(path, max_bytes)` with a regular-handle read capped at limit plus one, so a growing file cannot bypass a metadata-only size check. Existing open/save behavior remains unchanged. B and the forward implementer keep this file read-only until A's handoff. In-app/browser opening will use an explicit 8 MiB limit and retain the current document on rejection.

A's bounded-open dependency is complete, reviewed by the coordinator, and passes all 16 file I/O tests. File ownership returns to B. The forward implementer has handed off the runnable tabs slice for B's read-only review while retaining write ownership during browser integration.

### Worktree coordination

The user authorizes separate worktrees when they help coordination. Finish the current tabs/browser checkpoint in its existing checkout with the file ownership above; do not move or copy another task's uncommitted work. After a tested integration commit, use separate worktrees for independent implementation and review fixes that would otherwise share files. Each assignment records its base commit, worktree path, branch, and scope. Reviewers can stay read-only in a shared checkout when no independent edits are needed.

Worktree owners may make focused, tested commits within their assignment and report commit IDs and validation. The coordinator integrates those commits, resolves any overlap, and runs combined checks before updating the integration branch. The current shared-checkout slice still leaves staging and commits to the coordinator.

### Tabs/browser review gate

B's isolated read-only review reproduced three blockers in the initial tabs slice: modified key shortcuts could answer dirty-document prompts, lexical normalization before resolving symlinks could bypass path collisions, and clipping a long filename could hide its dirty marker. The forward implementer owns fixes and regression coverage. The coordinator also requested preservation of path selection on empty/sanitized-empty paste and suppression of browser activation when the window cannot show an entry. A reviews the browser after its handoff; B verifies the tab fixes. Final combined checks and the integration commit follow those reviews.

B verified all three tab fixes with independent regressions. The initial frozen handoff passed the coordinator's 129-test suite, build, strict all-target Clippy, formatting, live/source snapshots, and CLI help. A's browser review then reproduced one further issue: resolving an inaccessible old tab path blocks unrelated opens and Save As. The forward implementer has a narrow assignment to fix that lookup while preserving strict target validation and duplicate-path protection; A verifies the correction before the final integration commit.

### Completed tabs/browser checkpoint

The forward implementer delivered and froze the workspace and browser. A independently verified the final identity-cache correction under actual permission denial: unrelated opens and Save As proceed, old dirty text/selection/history survive, and aliases still reuse or protect the existing tab. B independently verified confirmation modifiers/event kinds, cancellation, symlink/parent traversal, and dirty markers down to one cell. Both reviewers report no remaining actionable findings in their assigned scopes.

Final coordinator validation passes 131 tests (37 core, 32 app, 14 workspace, 8 browser, 9 projection, 16 file I/O, 6 clipboard, 9 highlights), build, strict all-target Clippy, formatting, and whitespace checks. Live/source headless render and CLI-help checks pass through Workspace. The [tabs/browser manual checklist](prototype-checks.md#tabs-and-browser-check) covers the remaining native-terminal verification. Source is frozen for the coordinator's integration commit; subsequent independent implementation or fixes can branch from this checkpoint in separate worktrees.

## Visual feedback and testing direction

The tabs/browser checkpoint is committed as `27ef0f5`. The user reports that it works in Ghostty directly (no tmux/SSH), supplies three application screenshots and the original sketch, and requests a less verbose interface. Shift+Enter does not navigate search backward in that environment. The user proposes a reusable terminal environment for visual testing and authorizes all three agents to help.

The follow-up research was read-only. A reproduced the keyboard-byte mismatch through the real binary in a headless PTY and identified protocol activation/cleanup requirements. B reviewed all four screenshots and proposed compact production layouts, followed by a separate reading-width/word-wrap change. The forward implementer evaluated existing tools and recommends a bounded spike using a pinned `tui-test` release before building custom infrastructure. No tool was installed and no runtime source was changed.

The coordinator records the combined recommendation in [visual-testing-plan.md](visual-testing-plan.md). That proposal expands the original in-memory harness scope to evaluate a headless PTY/emulator runner through ordinary execution tools. It does not authorize native terminal GUI automation or a browser shell workaround. Subsequent implementation assignments should use worktrees for input handling, test tooling, and UI changes, with explicit integration gates. All three research assignments are complete; the current document is a proposal, not an implementation handoff.

## Worktree checkpoint: input, visual tests, compact UI

The user approved proceeding with coordinated work. All three assignments start from `54e9dde61cdf4aff80e82448e7b8a43d587dd5e9`, with the standalone test runner pinned locally. The coordinator stays in `/Users/finn/Code/md-term-editor` on `main` and owns documentation, visual review, and integration. The task handoff created new destination task IDs; use the IDs in this table for messages.

| Assignment | Current task ID | Worktree | Branch | Write scope |
| --- | --- | --- | --- | --- |
| A: keyboard protocol | `01a0bebe-6591-7252-9ccf-9683cea4cbd2` | `/Users/finn/.codex/worktrees/fca4/md-term-editor` | `codex/keyboard-protocol` | `src/terminal.rs`, new focused protocol tests |
| Forward implementer: visual runner | `01a0bebe-75a8-7b61-b5f5-f3dedf0aca39` | `/Users/finn/.codex/worktrees/ca81/md-term-editor` | `codex/coordinate-markdown-editor-help` | `tools/tui-test/`, new visual fixtures/scenarios under `tests/ui/` |
| B: compact interface | `01a0bebf-894b-7911-b89e-97ec625d949e` | `/Users/finn/.codex/worktrees/e488/md-term-editor` | `codex/polish-marklane-interface` | `src/app.rs`, `src/workspace.rs`, related search geometry/tests; minimal `src/clipboard.rs` fallback accessor authorized during review |

Agents may make focused tested commits in their worktrees and report commit IDs. They do not edit the shared checkout or merge each other's branches. The runner accepts explicit application-binary and artifact-output paths so it can compare independent builds without merging their source. No additional agents are needed for this checkpoint.

The keyboard slice enables disambiguated input and makes terminal restoration idempotent, without the pinned blocking capability query. The runner first captures the unchanged baseline, then checks actual encoded input, images, cells, paste and pointer behavior against the candidate/fixed builds. The compact interface removes redundant chrome and compresses Find/Replace while retaining safe edits and real clickable geometry. Each agent stops at its bounded handoff.

Integration order is keyboard fix, runner adoption/validation, then compact UI and production-image review. Reading width and word wrapping are explicitly deferred to a separate geometry change. Final combined checks and actual before/after images are required before declaring the UI checkpoint complete.

The user subsequently requested backgrounds on control bars to separate them from document text. B owns that bounded follow-up for tabs, the search dock, and the footer, with light/dark captures. A remains a read-only functional reviewer. Review also identified transient clipboard fallback warnings and a stale no-text warning after a successful retry; B owns typed feedback classification and fake-backend regressions. No native clipboard is accessed by those tests.

### Completed integration

All three agents completed their bounded assignments. Main contains keyboard cleanup/input (`d5cbb61`), the reusable runner (`98dffa8`), compact UI (`27f1e01`), clipboard feedback/retry fixes (`74cda3a`, `91a661a`), and shaded control bars (`9c8aea7`). A reviewed the final UI, warning fixes, and bar styles read-only with no remaining findings. The coordinator reviewed code and production captures and corrected the runner's match assertion to distinguish document occurrences from the search field.

Final combined verification passes 148 Rust tests, build, strict all-target Clippy, formatting, and whitespace checks. The exact integrated executable passes 52 dark runner assertions and 21 light capture assertions; all layout PNGs export. Original Unicode source assertions pass while combining-grapheme PNG failures and a backend ZWJ width mismatch remain explicitly unaccepted visual cases. See the [verification record](prototype-checks.md#verification-record) and [runner guide](../tools/tui-test/README.md).

The worktrees remain available, but all agents are read-only at handoff. No automatic next slice is assigned. Reading width/word wrapping and a native Ghostty check remain separate follow-ups.
