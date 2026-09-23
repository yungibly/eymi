# Prototype checks

The headless harness exercises the application's real input and rendering paths. Real-terminal behavior is a separate manual check because agents cannot operate terminal apps through Computer Use in this environment.

Status: the September 22 UI polish checkpoint adds a single tab row, an outline-only sidebar, themed status segments, and saved optional Nerd Font icons. It builds on 624 themes, safe external-file reloads, and private crash recovery. The verification record below separates automated evidence from native terminal checks. Earlier user reports cover prior builds; a native Ghostty pass of this checkpoint remains separate. The [visual testing record](visual-testing-plan.md) explains the executable runner and its limits.

## Automated checks

From the repository root:

```sh
cargo build --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run --locked -- --snapshot tests/ui/demo.md
cargo run --locked -- --snapshot --source tests/ui/demo.md
cargo run --locked -- --snapshot --snapshot-size 160x45 --theme light tests/ui/visual/writing.md
```

`--snapshot` draws the actual application into an in-memory terminal and prints its cell text plus caret coordinates. It defaults to 80×24; `--snapshot-size WIDTHxHEIGHT` accepts dimensions up to 400×160. It does not start an interactive terminal. Tests replay synthetic keyboard, mouse, paste, and resize events through the app's input handler.

## Everyday usability check

Build, copy `tests/ui/visual/writing.md` to a scratch location, and open that copy
with `--theme light`. The same sequence works with `--theme dark`.

1. At a wide size, check the centered prose, heading gutter, code background,
   and outline sidebar. Click a heading, then use F9 and arrows/Enter
   to navigate by keyboard. Ctrl+G jumps to a source line. Narrow the window to
   80×24 and 42×16; the sidebar should hide and editing remain usable.
2. Open F2, filter for `theme`, and run the command. Filter `Catppuccin`, use arrows to preview, Escape to cancel, then reopen and Enter to apply. Reopen F2 and filter `bold`.
   Escape should retain the source selection and return to editing. Filtered
   commands should remain clickable; resizing must not activate stale targets.
3. Select a word with Ctrl/Alt+Shift+Right. Try Ctrl+B, Alt+I, and Alt+backtick,
   undoing each operation. Select two lines, Tab to indent and Shift+Tab to
   outdent. Plain Tab at a caret remains a literal tab.
4. Type a word and undo once. Repeat with a navigation, view, or tab command
   between typing runs: undo should only remove the later run. Multiline paste
   should still undo once. Ctrl+Y or Ctrl+Shift+Z redoes the change.
5. F1 shows scrollable help. Check its final entries in a short window using
   arrows/PageDown. Open a second document, switch tabs, save and close. Dirty
   confirmations and external-change failures must retain edits on cancellation.
6. With a Nerd Font configured in the terminal, run F2 → Toggle Nerd Font icons.
   Check the document and outline symbols, then restart to confirm the preference.
   `--icons plain` overrides it for one launch. The tab row should contain the
   filename once, and idle status should contain no repeated commands or Live label.

The automated runner covers corresponding actions with scratch files and raw
keyboard traffic. Native clipboard, terminal key interception, IME, and actual
font/emoji presentation still require environment-specific observation.

Plain-text snapshot output omits terminal colors and modifiers. Neither snapshots nor passing tests establish how a particular terminal displays fonts, transmits keys, or restores its screen on exit.

## Manual smoke check once the build is ready

Use a copy of the fixture so experiments do not modify the repository's example:

```sh
cargo build --locked
eymi_demo_dir="$(mktemp -d)"
cp tests/ui/demo.md "$eymi_demo_dir/demo.md"
./target/debug/eymi "$eymi_demo_dir/demo.md"
```

Record the terminal name/version and whether a multiplexer or SSH is involved. A single initial local terminal is enough to start; additional combinations remain unverified until tried.

1. **Screen and disclosure.** The document is readable and the initial heading is editable source. Click another paragraph: that block's syntax appears and the rest remains rendered. Press Ctrl+E or F6 to switch source/live views without changing the document.
2. **Checkbox versus text.** Click an inactive task's box, then the space immediately beside it on each side: completion toggles while the existing text caret stays in place. Ctrl+Z restores the earlier task. Click the task's label: the caret enters editing instead of toggling the task. Raw markers in an active block remain editable text.
3. **Lists.** Move to the end of a task and press Enter: a new unchecked task appears. Enter on that empty item leaves the list, inserting a separating blank line where needed. Press Enter several more times: only ordinary blank lines appear. Type prose and press Enter again: no marker returns. Repeat on a bullet and a numbered item. Nested items exit one level per empty-item Enter. Ctrl+Z should undo one command at a time.
4. **Selection and paste.** Use Shift+arrows and mouse drag across formatted text. Ctrl+C/X/V try the native clipboard locally and report internal fallback on backend errors; SSH uses internal copy/paste. The terminal's own paste shortcut also accepts text from another application. A multiline paste should stay literal and undo in one step, and empty paste should leave the selection intact.
5. **Wrapping and Unicode.** Resize to a narrow pane. Check Home/End and Up/Down on a wrapped paragraph, and place the caret around `界`, combining accents, and emoji. The caret should remain visible and match the source position being edited.
6. **Save and exit.** Ctrl+S saves the scratch copy. Ctrl+Q exits; with unsaved changes, Y saves and exits, N discards and exits, and Escape returns to editing. After exit, the shell should echo input normally and show its cursor, with mouse capture disabled.

For an untitled document, run `./target/debug/eymi`. Ctrl+S opens Save As; F4 also opens Save As. This prototype requires a new filename for Save As and protects existing targets from overwrite.

Alt+Enter inserts a literal newline in Markdown. Ctrl+T toggles the task containing the caret. F1 shows current controls. Native terminal bindings can intercept a shortcut; report the terminal and the exact key that failed rather than assuming the editor received it.

## Follow-up check for find and clipboard

Use the scratch file above after rebuilding. The initial manual pass need not be repeated in full; concentrate on the two changed interactions and these new controls:

1. Click the spaces immediately beside a rendered checkbox, then its label. Repeat Enter after exiting a bullet, number, or task list; type a plain paragraph afterward.
2. Ctrl+F opens Find. Search for text in a link destination: the selected result should reveal its Markdown source. Enter/F3 advances, Shift+Enter/Shift+F3 goes backward, and Escape returns to the document. Matching is literal and case-sensitive.
3. Ctrl+R opens Find/Replace. Tab switches between Find and With and selects that field's text. Enter in Find navigates; Enter in With replaces one result and advances. Click Replace All or press Alt+A to replace all. Press Escape then Ctrl+Z: one undo restores the original document. Ctrl+Z while a text field is focused undoes that field's input.
4. In a local session, copy a harmless selected sentence with Ctrl+C and paste into another app. Copy a different harmless sentence there and use Ctrl+V in Eymi. Check cut/paste/undo as well. Empty or nontext clipboard content must leave selected document text intact. A native backend error switches the session to an explicitly reported internal clipboard; terminal paste still works. Restart to retry native access. SSH sessions use the internal clipboard and terminal paste.

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

- **Rendering and chrome checkpoint**, September 22, 2026, unreleased on
  `main`: **362 tests** pass with both Rust 1.98.0 and Rust 1.89.0 (70 core,
  281 app/helpers, 6 CLI, 5 real-process PTY), with formatting and strict
  all-target Clippy. New coverage includes projection invariants over every
  caret and several widths of a document using every construct, table grids
  and fallbacks, code surfaces and fences, heading bands, hanging indents,
  alerts, soft-wrap spaces, 41-language highlighting with segment and
  fuzzing invariants, contrast for every new role in all 624 themes, the bar
  caret's restoration, and dialog fit rules shared with key handling.
- The same debug executable, SHA-256
  `451efe6f2aa1b7d6be976b5c83b71f25d13546c7e6ab74484134da32b7754e76`, passes
  **567 terminal assertions over 119 captures**: 351 across the eight dark
  suites in `target/visual-polish-final-dark/`, 21 light captures in
  `target/visual-polish-final-light/`, 166 Nerd Font chrome assertions in
  `target/visual-polish-final-nerd/`, and 29 in the new rendering suite in
  `target/visual-polish-rendering/`. Only the three known original-Unicode
  PNG exports fail. A 100 KB document lays out in about 10 ms in an
  optimized build. Native Ghostty rendering, IME, and native clipboard
  remain manual checks.
- **Public release `v0.1.0`**, September 22, 2026, ships commit `306dd12`.
  [CI](https://github.com/yungibly/eymi/actions/runs/35737489994) passes all four
  Linux/macOS × stable/1.89 jobs. The
  [native release workflow](https://github.com/yungibly/eymi/actions/runs/35737769797)
  passes on Apple Silicon, Intel macOS, and Linux x86_64, including the 307 Rust
  tests, 19 package/Homebrew regressions, attribution checks, optimized builds,
  and extracted-archive version/help/theme/snapshot smoke tests. The three
  uploaded archives and their exact contents/checksums were independently
  verified before publication.
- [Homebrew verification](https://github.com/yungibly/eymi/actions/runs/35738337830)
  downloads the published archives anonymously, passes `brew install` and
  `brew test` on macOS arm64, macOS Intel, and Linux x86_64, then updates the tap.
  The live `Formula/eymi.rb` matches the locally generated, checked formula
  byte-for-byte. Install with `brew install yungibly/tap/eymi`.
- Release dependency notice bundles cover 70 macOS or 68 Linux locked packages.
  An independent audit compared every bundled notice with its crate source or
  checksum-pinned override. macOS binaries report deployment target 11.0;
  the GNU/Linux binary was built and tested on Ubuntu 22.04. Native older-macOS
  and additional Linux distribution coverage remain outside these runs.
  [Release assets and SHA256SUMS](https://github.com/yungibly/eymi/releases/tag/v0.1.0)
  are the authoritative distributed bytes.
- Eymi feature and rename checkpoint, September 22, 2026: **307 tests** pass
  on macOS with both Rust 1.89.0 and Rust 1.98.0 (69 core, 227 app/helpers,
  6 CLI, 5 real-process PTY). Formatting, strict all-target Clippy, whitespace
  checks, and debug/release builds pass. New coverage includes fuzzy document
  and parsed-heading navigation, source-preserving line move/duplicate and
  undo, the clickable new-document button, legacy preference-root selection,
  byte-compatible recovery records, and recovery through an actual restarted
  Eymi process using legacy state.
- The renamed executable passes **245 terminal assertions with 32 PNGs and
  no export failures**: 77 in `target/visual-eymi-workflows/workflows/` and
  168 in `target/visual-eymi-chrome/chrome/`. Compact and wide captures were
  inspected, including the document picker and the single-row tab/status
  layout. Both runs use debug SHA-256
  `25d18a50ec0d9c9f9112af1da28fa7ce1a3b199f929e7400c067c01932c74e23`.
  These simplified fixtures do not expand the documented combining/ZWJ,
  native clipboard, IME, or native terminal coverage.
- UI polish checkpoint, September 22, 2026: **277 tests** pass (61 core,
  207 app/workspace/helpers, 5 CLI, 4 real PTY). Strict all-target Clippy,
  formatting, whitespace checks, and debug/release builds pass. New coverage
  checks the single-row viewport, full-frame footer, narrow message/search
  fallbacks, icon preferences and state-free snapshots, and named-file redraws
  without duplicate path or metadata text. All 624 themes pass the new explicit
  chrome-pair contrast checks; the existing document colors remain unchanged.
- Final terminal evidence totals **465 assertions**: 108 across the six
  capture/protocol/Unicode/workspace/theme/reload suites in
  `target/visual-chrome-final-dark/`, 21 light layout assertions in
  `target/visual-chrome-final-light/`, and 168 each in
  `target/chrome-agent-plain-fixed/chrome/` and
  `target/chrome-agent-nerd-fixed/chrome/`. The latter runs use the final runner
  after correcting idle checks to dismiss transient Save feedback. Together
  these retain **99 frames, 96 successful PNGs**, and the same three documented
  original-Unicode-fixture PNG failures. The obsolete chrome run started with
  the earlier scenario is not counted. All final application manifests use
  debug SHA-256 `f69b3db9b1735fd64ad61bc3d2752942293dd0c94d8a8f54b19e6e84e6cda050`.
- Chrome acceptance verifies filename/metadata uniqueness, no idle shortcuts,
  heading-only keyboard/mouse navigation, nine-tab overflow, dirty markers,
  selection and exact source restored by undo, five theme previews, and
  20/42/80/120/160-column layouts. Both icon modes export all 32 PNGs. The Nerd
  run uses the installed `JetBrainsMono Nerd Font Mono`; inspected glyphs have
  no boxes or overlap. Wide and compact Catppuccin writing previews are also in
  `target/visual-chrome-writing-final/`. Native Ghostty/clipboard/IME and full
  combining/ZWJ renderer acceptance remain separate from this evidence.
- The optimized UI-polish executable reports `marklane 0.1.0` and renders a
  120×36 Nord snapshot with Nerd Font icons. Release SHA-256:
  `0186a75a627e9d96e37ba1b440440f94ae363e81645f5e28549a1c95f272452a`.
- Theme/recovery checkpoint, September 22, 2026: `cargo test --locked --offline`
  passes **262 tests** (61 core, 193 app/workspace/helpers, 4 CLI, 4 real PTY).
  Exact strict all-target Clippy, formatting, whitespace checks, the offline
  theme-importer reproduction/self-test, and debug/release builds pass on macOS.
- All 624 palettes pass contrast/lookup/ID checks. New integration regressions
  cover theme preview/cancel/persistence, read-only/concurrent/oversized settings,
  invisible choices, recovered empty/Unicode buffers and unusual paths, saved
  checkpoint cleanup, canceled multi-tab quit, external reload/undo baselines,
  and headless CLI isolation. A real PTY test kills an unsaved editor, restarts,
  recovers into a detached tab, saves a copy, and checks the newer original file
  remains unchanged. Recovery unit tests also exercise competing processes,
  corrupt/truncated records, bounded scans, locks, and failed atomic updates.
- Executable UI runs in `target/visual-theme-recovery-dark/` and
  `target/visual-theme-recovery-light/` pass **75 assertions each**, retaining
  34 frames per run (31 native PNGs plus the same three known Unicode-fixture
  failures). `target/visual-imported-themes-final/` adds **25 assertions and 16 PNGs**
  across Catppuccin Mocha/Latte, Dracula, Nord, Gruvbox Dark, TokyoNight, and
  iTerm2 Solarized Light, including preview/cancel/apply and tiny-window safety.
  `target/visual-disk-reload-verified/` adds **7 release-binary assertions and
  4 PNGs**, verifying disk notices, dirty confirmation, tiny fallback, and exact
  local text restored by undo. Source byte preservation and actual upstream
  background colors are checked.
  These artifacts record the source revision, worktree diff status, executable
  hash, settings isolation, and exact actions. Total: **182 terminal assertions**.
- Final release theme/reload captures identify clean source commit `6320d2b`
  and release SHA-256 `924e90085ac6ee2b87ae1960df435031cbfbaed2688e65f53ad1d1aa3ff1b32e`.
  Rust 1.98.0 built the optimized binary; `--version`, the 624-entry theme list,
  and a 120×36 Nord snapshot also pass.
- Settings/state directories and clipboard are isolated in automated runs;
  no test accesses the user's settings or native clipboard. Native IME,
  clipboard and renderer differences remain a manual compatibility pass.


- Everyday usability checkpoint, September 22, 2026: `cargo test --locked
  --offline` passes 206 tests (56 core, 144 app/workspace and helpers, 3 CLI,
  3 real PTY integration tests). The combined build, strict all-target Clippy,
  formatting and whitespace checks pass. This is a local macOS result.
- New coverage checks word movement/deletion, source formatting, indentation,
  bounded/coalesced history, offset editor slices, word-wrap mappings, theme
  contrast, outline offsets, command filtering/actions, go-to-line, tiny
  modal behavior, CLI size limits and bounded external-change comparisons.
  Integration review reproduced a one-tab navigation undo-group bug; both
  synthetic workspace events and encoded F8 through a real PTY now verify
  the fix. Current-tab mouse hits also end the typing group.
- Final executable verification of source commit `9ff0c52`: both light and dark
  runs pass all 75 assertions each across captures, keyboard protocol, Unicode
  source edits, and workspace controls. Each retains 33 captures; 30 native PNGs
  export successfully per theme. Three original Unicode-fixture PNG failures
  per theme retain SVG/cells/raw traces and remain the known renderer limitation.
  Source, undo and keyboard assertions still pass; full Unicode visual acceptance
  remains false. All eight manifests match the final debug binary hash.
  Artifacts are under ignored `target/visual-usability-final-dark/` and
  `target/visual-usability-final-light/`. The coordinator inspected final light
  writing and dark filtered-command captures, plus compact replacement and
  workspace frames during review. Optimized `cargo build --release --locked
  --offline`, release version output, and a 120×36 release snapshot also pass.
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
