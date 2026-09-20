# Prototype checks

The headless harness exercises the application's real input and rendering paths. Real-terminal behavior is a separate manual check because agents cannot operate terminal apps through Computer Use in this environment.

Status: search polish passed combined verification on September 20, 2026: 102 tests, build, strict Clippy, and formatting. The user reported that the previous build works and requested simpler field navigation, clickable controls, and highlighting all matches. Those refinements are implemented; the short search checklist below covers the new behavior.

## Automated checks

From the repository root:

```sh
cargo build --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run --locked -- --snapshot tests/ui/demo.md
cargo run --locked -- --snapshot --source tests/ui/demo.md
```

`--snapshot` draws the actual application into an 80×24 in-memory terminal and prints its cell text plus caret coordinates. It does not start an interactive terminal. Tests replay synthetic keyboard, mouse, paste, and resize events through the app's input handler.

Plain-text snapshot output omits terminal colors and modifiers. Neither snapshots nor passing tests establish how a particular terminal displays fonts, transmits keys, or restores its screen on exit.

## Manual smoke check once the build is ready

Use a copy of the fixture so experiments do not modify the repository's example:

```sh
cargo build --locked
marklane_demo_dir="$(mktemp -d)"
cp tests/ui/demo.md "$marklane_demo_dir/demo.md"
./target/debug/marklane "$marklane_demo_dir/demo.md"
```

Record the terminal name/version and whether a multiplexer or SSH is involved. A single initial local terminal is enough to start; additional combinations remain unverified until tried.

1. **Screen and disclosure.** The document is readable and the initial heading is editable source. Click another paragraph: that block's syntax appears and the rest remains rendered. Press Ctrl+E or F6 to switch source/live views without changing the document.
2. **Checkbox versus text.** Click an inactive task's box, then the space immediately beside it on each side: completion toggles while the existing text caret stays in place. Ctrl+Z restores the earlier task. Click the task's label: the caret enters editing instead of toggling the task. Raw markers in an active block remain editable text.
3. **Lists.** Move to the end of a task and press Enter: a new unchecked task appears. Enter on that empty item leaves the list, inserting a separating blank line where needed. Press Enter several more times: only ordinary blank lines appear. Type prose and press Enter again: no marker returns. Repeat on a bullet and a numbered item. Nested items exit one level per empty-item Enter. Ctrl+Z should undo one command at a time.
4. **Selection and paste.** Use Shift+arrows and mouse drag across formatted text. Ctrl+C/X/V try the native clipboard locally and report internal fallback on backend errors; SSH uses internal copy/paste. The terminal's own paste shortcut also accepts text from another application. A multiline paste should stay literal and undo in one step, and empty paste should leave the selection intact.
5. **Wrapping and Unicode.** Resize to a narrow pane. Check Home/End and Up/Down on a wrapped paragraph, and place the caret around `界`, combining accents, and emoji. The caret should remain visible and match the source position being edited.
6. **Save and exit.** Ctrl+S saves the scratch copy. Ctrl+Q exits; with unsaved changes, Y saves and exits, N discards and exits, and Escape returns to editing. After exit, the shell should echo input normally and show its cursor, with mouse capture disabled.

For an untitled document, run `./target/debug/marklane`. Ctrl+S opens Save As; F4 also opens Save As. This prototype requires a new filename for Save As and protects existing targets from overwrite.

Alt+Enter inserts a literal newline in Markdown. Ctrl+T toggles the task containing the caret. F1 shows current controls. Native terminal bindings can intercept a shortcut; report the terminal and the exact key that failed rather than assuming the editor received it.

## Follow-up check for find and clipboard

Use the scratch file above after rebuilding. The initial manual pass need not be repeated in full; concentrate on the two changed interactions and these new controls:

1. Click the spaces immediately beside a rendered checkbox, then its label. Repeat Enter after exiting a bullet, number, or task list; type a plain paragraph afterward.
2. Ctrl+F opens Find. Search for text in a link destination: the selected result should reveal its Markdown source. Enter/F3 advances, Shift+Enter/Shift+F3 goes backward, and Escape returns to the document. Matching is literal and case-sensitive.
3. Ctrl+R opens Find/Replace. Tab switches between Find and With and selects that field's text. Enter in Find navigates; Enter in With replaces one result and advances. Click Replace All or press Alt+A to replace all. Press Escape then Ctrl+Z: one undo restores the original document. Ctrl+Z while a text field is focused undoes that field's input.
4. In a local session, copy a harmless selected sentence with Ctrl+C and paste into another app. Copy a different harmless sentence there and use Ctrl+V in Marklane. Check cut/paste/undo as well. Empty or nontext clipboard content must leave selected document text intact. A native backend error switches the session to an explicitly reported internal clipboard; terminal paste still works. Restart to retry native access. SSH sessions use the internal clipboard and terminal paste.

Native clipboard delivery is a manual check. Automated fake-backend tests exercise failures, timeouts, source fidelity, and no-op empty paste without accessing the user's actual clipboard. Native Wayland clipboard support and OSC 52 are deferred; the selected Linux backend uses X11, where availability also depends on the desktop session.

Find needs at least 11 terminal rows and Find/Replace needs 12. Shorter windows show a resize prompt and disable hidden fields/actions. Narrow windows shorten button labels and omit controls that cannot fit; keyboard shortcuts remain available when the panel is tall enough.

## Search polish check

Use a scratch document with several instances of the same word, including one in bold and one inside a link destination.

1. Open Find and type that word. Every visible result has a quiet highlight; the current result is brighter and bold/underlined. Click Next and Previous and check that the bright highlight moves. Scroll over the document to inspect other matches; Next/Previous brings the active result into view again. Selecting the hidden destination reveals its source. Closing search clears extra highlights and retains ordinary text selection.
2. Open Find/Replace and use Tab, Shift+Tab, Up, and Down between the two fields. The destination field's text is selected, ready to replace by typing. Click within a field, Shift-click, and drag across Unicode text; the caret and selection should match the displayed characters, including when a long query has scrolled sideways.
3. Click Replace once: only one result changes and the next is selected. Releasing the mouse must not replace another result. Click Replace All: all remaining matches change in one undo step. With no matches, navigation/replacement controls are visibly disabled. Enter in With replaces one; Alt+A explicitly replaces all.
4. Narrow and shorten the window with search open. Visible controls retain their targets; omitted controls cannot be clicked through their old positions. A short-window prompt leaves Close/Escape available. Restore the window and continue editing without losing either field.

Automated checks assert cell colors/modifiers alongside actual input events. They verify the renderer's intended distinction; appearance still depends on the terminal palette.

## Report a problem

Include the starting fixture or a minimal text sample, the exact input sequence, the terminal and window dimensions if relevant, and what appeared instead of the expected behavior. A screenshot can help with visual defects, but preserve the Markdown text too so the interaction can become a headless regression test.

## Verification record

- Search polish checkpoint: `cargo test --locked --offline` passes 102 tests: 37 core, 32 app, 9 projection, 9 file I/O, 6 clipboard, and 9 search-highlight tests. A test-fixture-only Clippy adjustment was then verified by all 9 focused highlight tests and strict all-target Clippy.
- `cargo build --locked --offline`, `cargo clippy --locked --offline --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check` pass.
- Regression checks cover checkbox padding, repeated Enter and reopened prose, source find/replace, stale match invalidation, one-step Replace All undo, Unicode search fields, short-window action suppression, clipboard fallback/timeout/teardown, empty native and terminal paste, EOF disclosure, wrap-boundary cursor affinity, BOM/line endings, read-only saves, and nonblocking rejection of FIFO targets.
- Headless demos: both live and source views successfully rendered through the production draw path during final coordinator verification. CLI help also checked.
- Verification environment: local macOS with `rustc 1.98.0 (88d9e12ae 2026-08-18)`. Cross-platform terminal behavior is not established by this run.
- User manual verification: reported passing overall on September 20, 2026, with checkbox click-padding and repeated-Enter list-exit follow-ups. The terminal name/version was not specified; this does not establish support for other environments.
- The user subsequently reported that the find/clipboard follow-up works and requested search interaction polish. No terminal name/version or per-platform test matrix was supplied. All clipboard automation used fake or internal backends.
- Search polish regressions exercise actual field/button mouse events, two-field keyboard navigation, narrow/short/resize behavior, wheel scrolling with search open, single-click replacement and undo, and final Ratatui cell colors/modifiers for multiple matches, active-result movement, query changes, replacement, undo, and closing the panel. New search appearance/interaction changes have not yet received a user manual pass.
