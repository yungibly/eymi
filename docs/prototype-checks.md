# Prototype checks

The headless harness exercises the application's real input and rendering paths. Real-terminal behavior is a separate manual check because agents cannot operate terminal apps through Computer Use in this environment.

Status: compact UI, shaded control bars, and enhanced keyboard input passed combined verification on September 20, 2026: 148 tests, build, strict Clippy, and formatting. The user previously reported that tabs/browser worked in Ghostty directly, with UI clutter and a nonworking Shift+Enter shortcut. These changes address those reports; a native Ghostty check of this new build remains separate. The [visual testing record](visual-testing-plan.md) explains the executable runner and its limits.

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

At 80 columns, Find uses one row and Find/Replace uses two, with one shared footer. Narrow windows use a separate action row. Readiness is calculated from the visible fields and remaining document space; unusably small windows show a resize prompt and disable hidden fields/actions. Narrow windows shorten button labels and omit controls that cannot fit, retaining Close/Escape.

## Search polish check

Use a scratch document with several instances of the same word, including one in bold and one inside a link destination.

1. Open Find and type that word. Every visible result has a quiet highlight; the current result is brighter and bold/underlined. Click Next and Previous and check that the bright highlight moves. Scroll over the document to inspect other matches; Next/Previous brings the active result into view again. Selecting the hidden destination reveals its source. Closing search clears extra highlights and retains ordinary text selection.
2. Open Find/Replace and use Tab, Shift+Tab, Up, and Down between the two fields. The destination field's text is selected, ready to replace by typing. Click within a field, Shift-click, and drag across Unicode text; the caret and selection should match the displayed characters, including when a long query has scrolled sideways.
3. Click Replace once: only one result changes and the next is selected. Releasing the mouse must not replace another result. Click Replace All: all remaining matches change in one undo step. With no matches, navigation/replacement controls are visibly disabled. Enter in With replaces one; Alt+A explicitly replaces all.
4. Narrow and shorten the window with search open. Visible controls retain their targets; omitted controls cannot be clicked through their old positions. A short-window prompt leaves Close/Escape available. Restore the window and continue editing without losing either field.

Automated checks assert cell colors/modifiers alongside actual input events. They verify the renderer's intended distinction; appearance still depends on the terminal palette.

## Tabs and browser check

Use scratch documents for this checkpoint. Existing editor and search checks above remain applicable.

1. **Independent tabs.** Edit a document, select text, switch to source view, and scroll. Ctrl+N opens an untitled Markdown tab. Add different text, then switch back with F7/F8, Ctrl+PageUp/PageDown, and the tab strip. Each tab retains its state and undo history. Copy in one tab and paste in another, including after closing the tab where the copy originated. Switching tabs closes Find.
2. **Unsaved work.** Ctrl+W prompts before closing a dirty tab. Escape retains it, Y saves it, and N discards it. Ctrl+N or Ctrl+Y while the confirmation is shown must not count as a discard or save answer. A long filename still displays its `*` unsaved marker. Closing the last tab leaves a new untitled document.
3. **Quit cancellation.** Make two tabs dirty and press Ctrl+Q. Choose N for the first, then Escape for the second: both documents and their edits must remain. Repeat and choose Y for an untitled document, then cancel Save As: quitting must stop. A failed save must also retain unsaved text.
4. **Browse and open.** Ctrl+O opens near the current file. Use arrows, PageUp/PageDown, Enter, and clicks to navigate; Backspace or the Up button goes to the parent. Tab edits the path. F2 toggles hidden entries; F5 shows all file types. Open a text file, then open the same path again: its existing tab is selected. Escape or Close returns to editing.
5. **Rejected opens and collisions.** Try an invalid UTF-8 or NUL-containing file and a text file larger than 8 MiB. An error must leave the original document intact. Save As must reject a target already owned by another tab, even if that target does not yet exist. Empty paste in the browser path must preserve any selected path text.
6. **Small windows.** Narrow the tab strip and browser; visible controls must keep their actual targets. In a browser too short for an entry, the resize state must not open an unseen selection. Restore the window and continue without losing edits.

These checks exercise new terminal interactions. Automated event and cell-buffer tests cover the corresponding state transitions; a real-terminal manual pass remains separate.

## Compact UI and keyboard follow-up

Rebuild and use a scratch file with at least three instances of a word. Two matches cannot distinguish next from previous when navigation wraps.

1. Open Find, type the word, and press Enter to select result 2. Shift+Enter should return to result 1 in Ghostty directly. Check Shift+F3 and Previous as well.
2. Expand with Ctrl+R: the query and result stay selected. Tab reaches With; Enter replaces one, and Replace All remains explicit. Escape then Ctrl+Z undoes the document edit.
3. At roughly 80×24, check the compact fields, one count, visible focus, and distinct control bars. Narrow to roughly 42 columns and verify that the fields and visible buttons still work. The tab name should not repeat `LIVE`; the footer reports the current view when space permits.
4. Quit and verify normal shell input, cursor, and mouse behavior. Native Ghostty font/emoji rendering and OS clipboard delivery remain manual checks; headless captures do not establish them.

The [runner guide](../tools/tui-test/README.md) gives the separate automated visual/protocol command, artifact contents, and Unicode renderer limits. It uses a pinned project-local binary; no Homebrew installation is required.

## Report a problem

Include the starting fixture or a minimal text sample, the exact input sequence, the terminal and window dimensions if relevant, and what appeared instead of the expected behavior. A screenshot can help with visual defects, but preserve the Markdown text too so the interaction can become a headless regression test.

## Verification record

- Compact UI/control-bar checkpoint: `cargo test --locked --offline` passes 148 tests: 37 core, 109 binary tests (including 7 terminal lifecycle tests), and 2 real PTY integration tests. Coordinator independently ran the combined suite, build, strict all-target Clippy, formatting, and whitespace checks after integrating the bar backgrounds and clipboard-feedback fixes.
- New coverage includes compact/narrow geometry, pointer replacement and undo, field focus, persistent file and clipboard warnings, successful clipboard retry, full-width bar backgrounds without document bleed, paired field colors, partial terminal setup, idempotent panic/error cleanup, negotiated keyboard bytes, and restored termios. Clipboard tests use fakes; the PTY tests use temporary files.
- Final executable runner: 52 assertions pass across the dark capture/protocol/Unicode-source suites, and 21 pass in the light capture suite. All 26 layout PNGs export successfully. Coordinator inspected the integrated 80×24 dark replacement and 42×16 light fallback frames, alongside the agent's wider and empty-field captures. Artifacts are under ignored `target/visual-final-dark/` and `target/visual-final-light/`; each manifest's binary hash matches the final built executable. The runner observed activation followed by backend-generated `ESC[13;2u`, moving result 2 to 1, and one cleanup pop before alternate-screen exit.
- Full Unicode visual acceptance remains false: three original-fixture PNGs fail on combining graphemes, with SVG/cells/raw traces retained; the backend splits the ZWJ emoji despite intact emitted bytes. Exact saved-source, checkbox undo, multiline paste/undo, and pointer-positioned wheel assertions pass. These documented renderer/profile limits are not silently treated as passing image checks.
- Tabs/browser checkpoint: `cargo test --locked --offline` passes 131 tests: 37 core, 32 app, 14 workspace, 8 browser, 9 projection, 16 file I/O, 6 clipboard, and 9 search-highlight tests. Coordinator independently ran the full suite, build, strict all-target Clippy, formatting, and whitespace checks after the final path-identity fix.
- New regressions cover independent tab state, shared clipboard, close/quit cancellation and save failure, modified/repeated confirmation keys, symlink path collisions, dirty markers under clipping, rendered browser controls, empty path paste, small windows, bounded directory scans and reads, file growth, invalid UTF-8/NUL data, and preservation of existing buffers on open failure. An inaccessible old tab's directory no longer blocks unrelated opens or Save As; cached identities still prevent duplicate aliases. Both permission regressions exercised actual denial on this host, with permissions restored afterward. Browser opens are capped at 8 MiB; initial command-line opens retain the existing unbounded behavior.
- Production CLI help and 80×24 live/source headless renders pass through the new Workspace entry point. These checks do not establish native terminal appearance or shortcut delivery. The subsequent user manual report says tabs/browser work in Ghostty directly; UI clutter and Shift+Enter remain follow-ups.
- Search polish checkpoint: `cargo test --locked --offline` passes 102 tests: 37 core, 32 app, 9 projection, 9 file I/O, 6 clipboard, and 9 search-highlight tests. A test-fixture-only Clippy adjustment was then verified by all 9 focused highlight tests and strict all-target Clippy.
- `cargo build --locked --offline`, `cargo clippy --locked --offline --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check` pass.
- Regression checks cover checkbox padding, repeated Enter and reopened prose, source find/replace, stale match invalidation, one-step Replace All undo, Unicode search fields, short-window action suppression, clipboard fallback/timeout/teardown, empty native and terminal paste, EOF disclosure, wrap-boundary cursor affinity, BOM/line endings, read-only saves, and nonblocking rejection of FIFO targets.
- Headless demos: both live and source views successfully rendered through the production draw path during final coordinator verification. CLI help also checked.
- Verification environment: local macOS with `rustc 1.98.0 (88d9e12ae 2026-08-18)`. Cross-platform terminal behavior is not established by this run.
- User manual verification: reported passing overall on September 20, 2026, with checkbox click-padding and repeated-Enter list-exit follow-ups. The terminal name/version was not specified; this does not establish support for other environments.
- The user subsequently reported that the find/clipboard follow-up works and requested search interaction polish. No terminal name/version or per-platform test matrix was supplied. All clipboard automation used fake or internal backends.
- Search polish regressions exercise actual field/button mouse events, two-field keyboard navigation, narrow/short/resize behavior, wheel scrolling with search open, single-click replacement and undo, and final Ratatui cell colors/modifiers for multiple matches, active-result movement, query changes, replacement, undo, and closing the panel. The later user screenshots show that passing these checks did not ensure a clear interface or correct native delivery of Shift+Enter.
- Read-only keyboard diagnosis: the compiled binary in a headless PTY ignores Ghostty 1.3.1's default `ESC[27;2;13~`, accepts enhanced `ESC[13;2u` as previous-result navigation, and accepts legacy Shift+F3. No enhancement activation is emitted on startup. This is a byte-transport regression reproduction, not a user-configured Ghostty event capture; no file was saved or runtime code changed. See the visual testing plan for the proposed fix and protocol-aware runner acceptance.
