# Visual testing and a quieter interface

## September 22 Eymi release checkpoint

The renamed Eymi executable adds a clickable new-document button, fuzzy
document/heading navigation, and source-preserving line move/duplicate commands.
The new workflows suite passes 77 assertions with 16 PNGs; the final chrome
suite adds 168 assertions and 16 PNGs. Compact and wide captures were inspected
with no export failures in these simplified fixtures. Paths and executable
hashes are in the [verification record](prototype-checks.md#verification-record).

Earlier checkpoints below retain the name Marklane where it identifies the
executable that was actually tested. The known combining/ZWJ, native clipboard,
and IME limits remain separate from release CI and Homebrew package checks.

## September 22 UI polish checkpoint

The header is one tab row; filenames and commands are no longer repeated in
the rail or idle status bar. The outline highlights the current section and
the status bar uses distinct theme roles for format, word count and position.
Optional Nerd Font icons have a saved preference and a plain-font default.

The new chrome suite verifies wide/compact/narrow layouts, active and dirty tabs
through overflow, unique filename/footer text, outline pointer/keyboard jumps,
five themes, source selection, and exact source restored by one undo. Plain
and Nerd runs pass 168 assertions each and export all 32 PNGs. Actual captures
exposed stale editor header/footer text underneath the workspace replacement;
explicit clearing and both Rust and terminal regressions verify the fix.

The six broader dark suites and light captures add 129 passing assertions.
Final evidence totals 465 assertions and 99 frames, with 96 successful PNGs;
the three known combining-grapheme failures remain separate. See the current
[verification record](prototype-checks.md#verification-record) for paths and
executable hashes. The runner now accepts `--icons` and a recorded `--font`
override. Wide/compact writing previews use the locally installed JetBrainsMono
Nerd Font Mono, with glyph shape and spacing inspected in the actual PNGs.

## September 22 theme and recovery checkpoint

The editor now embeds 624 palettes and has a searchable preview picker, saved
preferences, and a recovery workflow. The actual executable passes 182 terminal
assertions: 75 each in Sage dark/light, 25 across seven imported themes, and
7 release-binary reload assertions.
Current artifacts are `target/visual-theme-recovery-{dark,light}/` and
`target/visual-imported-themes-final/`, plus release reload frames in
`target/visual-disk-reload-verified/`. They retain 88 frames and 82 successful PNGs;
the six original-Unicode-fixture PNG failures remain reported separately.

Theme checks verify original background RGB values in real terminal cells,
preview/cancel/accept behavior, unchanged source, and compact/tiny geometry.
Preferences/recovery use isolated XDG directories and an internal clipboard.
The real PTY integration suite separately kills/restarts an editor and verifies
recovery to a new unsaved tab followed by Save As without touching a newer disk
file. The existing native-terminal and Unicode-renderer limits still apply.

## September 22 usability checkpoint

The final integrated build at source commit `9ff0c52` uses paired sage light/dark
palettes, a centered live writing column, word-aware prose wrapping, margin
heading labels where space permits, a responsive documents/outline sidebar, and
a compact filterable command palette. Source view, active-block disclosure,
selection geometry, and literal code/table fallbacks remain source anchored.

Both final theme runs pass 75 executable assertions each. The four suites cover
normal/Find/Replace layouts, negotiated keyboard bytes, Unicode source fidelity,
pointer/keyboard outline navigation, line jumps, theme commands, formatting,
indentation, word-group undo, no-op tab command boundaries, tabs, paste and tiny
palette fallback. Each theme retains 33 frames, of which 30 export as PNG. The
three original combining-grapheme PNG failures and the backend's ZWJ-width limit
remain explicit; this checkpoint does not claim full Unicode renderer acceptance.

Final artifacts: `target/visual-usability-final-dark/` and
`target/visual-usability-final-light/`. Every suite's binary hash matches the
tested debug executable. The coordinator visually inspected the light 160×45
writing surface and dark filtered command palette, plus compact 80×24 workspace
and 42×16 replacement frames during integration. Visual review led to a shorter
filtered palette, stable query placement, compact path context, and fewer repeated
sidebar hints. See the [runner guide](../tools/tui-test/README.md) for replay.

The production code also passes 206 tests, strict all-target Clippy, formatting,
debug/release builds, and a release snapshot. Native terminal fonts, OS shortcut
interception, clipboard delivery, IME, and other platforms remain separate from
these local macOS checks. CI and Homebrew packaging were not part of this work.

## Earlier investigation

Decision and implementation record · September 20, 2026 · UI baseline: `27ef0f5`

The user reports that the tabs/browser build works, but the interface is verbose and cumbersome. Screenshots were taken in Ghostty directly, without tmux or SSH. The early interaction sketch remains the visual direction. The sections below record the original findings and acceptance plan; the implementation record at the end distinguishes verified results from remaining limits.

## What the baseline screenshots expose

- The active tab already has a strong selected appearance; repeating `LIVE` in its title adds weight. Live/source is a view setting, not a second active-tab indicator. Keep the tab label to its name and dirty state; expose view switching separately.
- Find currently consumes five rows and Replace six, in addition to two footer rows. A title, field label, result count, instructions, and status repeat information. Fields have weak visual boundaries while instructions dominate.
- Clipboard capability messages occupy persistent status space during ordinary editing. Normal capabilities belong in help; failures and action results should appear when relevant.
- Prose stretches across the full window and wraps inside ordinary words. The sketch's bounded reading column, margins, and hierarchy matter more than reproducing its exact colors.
- Shift+Enter is handled correctly when a test supplies an already-decoded Shift modifier, but the production terminal setup does not request enhanced keyboard reporting. The missing input layer is a likely explanation for the reported failure.

## Recommendation: evaluate an existing runner first

Start with [Microsoft's tui-test](https://github.com/microsoft/tui-test). Its documented surface includes terminal backends, keyboard/mouse/resize operations, cell/style inspection, image capture, and traces. The research pass found an embedded Ghostty backend and protocol-aware input encoding, but screenshot rendering is its own fixed-cell renderer. Pin a tested revision/release; current source and cached documentation differ in beta version. Verify the actual backend and input behavior in a small spike before adopting it; a supported backend name is not proof of native Ghostty equivalence.

The inspected candidate is `0.1.0-beta.5`, commit `6a991eea96d499689a875b7ee5781aa0c18b88d2`. It was not installed during this investigation. Its [Ghostty adapter](https://github.com/microsoft/tui-test/blob/6a991eea96d499689a875b7ee5781aa0c18b88d2/crates/tui-test/src/terminal/ghostty/core.rs) supplies backend keyboard encoding and terminal replies; its [engine](https://github.com/microsoft/tui-test/blob/6a991eea96d499689a875b7ee5781aa0c18b88d2/crates/tui-test/src/engine.rs) prefers that encoder over the generic fallback. Verify bytes from the selected backend, not just a successful high-level key action.

Three small gaps need explicit handling in the spike: text typing is not bracketed paste; mouse-wheel helpers use cell `(0,0)` while Marklane routes scrolling by pointer location; cast recordings preserve output/resize playback rather than re-executing scenario actions. Retain the action script separately, add a mode-aware paste action and coordinate-aware wheel action where needed, and capture raw input/output alongside images. The [logger](https://github.com/microsoft/tui-test/blob/6a991eea96d499689a875b7ee5781aa0c18b88d2/crates/tui-test/src/logger.rs) and [input diagnostics](https://github.com/microsoft/tui-test/blob/6a991eea96d499689a875b7ee5781aa0c18b88d2/crates/tui-test/src/diagnostics/input.rs) provide byte evidence. Images use the tool's [own renderer](https://github.com/microsoft/tui-test/blob/6a991eea96d499689a875b7ee5781aa0c18b88d2/crates/tui-test/src/render/svg.rs), not Ghostty desktop pixels.

[tuibot](https://github.com/tui-testing/tuibot) and [xterm.js](https://github.com/xtermjs/xterm.js) are alternatives to evaluate if that spike fails. Do not start a custom terminal parser, rendering engine, or general terminal application simply to obtain screenshots.

The reusable part should be a framework-independent runner around the compiled program. Marklane contributes fixtures and scenarios. Its existing Ratatui tests remain the fast way to check source edits and semantic invariants.

## Two automated layers, plus native checks

| Layer | What it exercises | What it cannot establish |
| --- | --- | --- |
| Existing app/cell tests | Production edit and draw code, source mappings, cell styles, hit targets, undo and save rules | Actual terminal byte encoding or presentation |
| Executable under a pseudo-terminal and emulator | Production terminal initialization, encoded keys/mouse/paste, resize, terminal replies, emitted control sequences, visible frames | Exact macOS/Ghostty fonts, OS key interception, native clipboard and IME behavior |
| User's Ghostty session | Native rendering and key delivery in the intended environment | A reproducible automated regression by itself |

The second layer runs through ordinary execution/test tooling and emits inspectable images and traces. Native terminal GUI automation remains unavailable. A passive image or recording viewer is sufficient for visual review; a live browser shell is unnecessary.

Keyboard input must honor the protocol the application actually requested. A runner that always turns `Shift+Enter` into a modified event would reproduce the blind spot in the current unit tests. Test both legacy input and negotiated enhanced input. [Kitty's keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/) defines the enhancement; [Ghostty documents support](https://ghostty.org/docs/features), and the pinned [Crossterm API](https://docs.rs/crossterm/0.28.1/crossterm/event/struct.PushKeyboardEnhancementFlags.html) exposes activation and restoration commands.

The input investigation found a specific mismatch in the installed Ghostty 1.3.1: its [default Shift+Enter encoding](https://github.com/ghostty-org/ghostty/blob/v1.3.1/src/input/function_keys.zig#L179) is `ESC[27;2;13~`, which the pinned Crossterm parser does not accept. Ghostty's [enhanced encoding test](https://github.com/ghostty-org/ghostty/blob/v1.3.1/src/input/key_encode.zig#L1236) expects `ESC[13;2u` after disambiguation is enabled; Crossterm understands that form. This is strong source-level evidence for the reported no-op, although the user's actual event stream has not been captured. Do not incorrectly model every pre-enhancement terminal as sending plain Enter.

A verified the existing compiled Marklane through a headless pseudo-terminal: startup emitted no enhancement activation; plain Enter advanced Find from result 1 to 2; Ghostty's default sequence caused no movement; the enhanced sequence moved 2 back to 1; Shift+F3 also navigated backward. No document was saved. This exercises the real byte/parser/event path, although the enhanced bytes were deliberately supplied for diagnosis and do not demonstrate negotiation by the application.

The minimum fix must pair activation after entering the alternate screen with restoration before leaving it. Make cleanup idempotent: the current panic hook and guard both call restoration. Do not add a second keyboard-stack pop during panic cleanup. Also avoid blindly using the pinned capability-query helper: its nominal timed poll is followed by an unbounded read of another reply. Keep a legacy previous-result binding and clickable action regardless of enhanced support.

## Smallest useful spike

Run the existing compiled Marklane against a temporary fixture with at least three occurrences of a query word, Markdown tasks, a wrapped paragraph, and Unicode. Two matches cannot distinguish next from previous when navigation wraps. Keep system clipboard access out of the test's effects.

1. Start at a fixed terminal size and capture the actual initial frame as an image and structured cells.
2. Open Find, type a query, advance to result 2, and try Shift+Enter. Record input bytes, negotiated modes, visible result position, and cursor. Cover Ghostty 1.3.1's default `ESC[27;2;13~` (current build stays at 2), enhanced `ESC[13;2u` (moves to 1), and legacy `CR` (moves to 3) as distinct cases. If the embedded backend version emits different default bytes, report that profile discrepancy and keep an explicit raw-byte regression for Ghostty 1.3.1. A passing negotiation check must use the backend's encoder after observing the app's activation; manually forcing enhanced bytes is only a parser regression, not proof the app negotiated correctly.
3. Click a rendered search control and a checkbox, paste multiline text, resize, and capture each state. Assert resulting source changes and undo separately from appearance.
4. Compare ordinary editing, Find, and Replace frames at 80×24, 120×36, and 160×48, with pinned font metrics and explicit light/dark palettes.
5. Save a replayable action trace and named images so a reviewer can inspect a failed step without rerunning it. Clean up only the runner-owned process and temporary files.

Acceptance: the runner drives the real executable without a native terminal window; exported images preserve backgrounds, emphasis, selection, cursor, and wide glyph placement; pointer coordinates map to cells; input encoding reacts to protocol negotiation; failures retain enough evidence to reproduce. Record the tool version, backend, dimensions, palette, font, and limitations with each run. An idle screen alone is not proof an action succeeded.

If the candidate cannot honor keyboard modes, it may still be useful for visual review, but must not be presented as a keyboard compatibility test. Keep this gate explicit before expanding the infrastructure.

## First UI pass using that loop

- Quiet tabs: filename, small dirty marker, restrained active styling. Remove `LIVE` from tab titles; keep view state discoverable outside the filename.
- Compact Find with a clearly bounded input, one result count, previous/next actions, and close. Prefer one row where labelled controls and a useful input width fit; a second action row is acceptable when clearer. Expand for replacement text and explicit Replace/All actions. At narrow widths, reduce optional labels before sacrificing field usability or close access. Remove the current separate title, count, and instruction rows.
- Keep query state when expanding/collapsing Replace. Escape returns to the document. Preserve all-match highlighting and a stronger active match. Keep replacement actions explicit and one-step undoable.
- One quiet status row. Show short, contextual guidance; place full shortcut lists and normal clipboard details in help. Failures must remain visible long enough to act on.
- Introduce a bounded reading column and word-aware prose wrapping as a separately verified geometry change. Preserve source fallback for code/tables and validate mouse placement, wide characters, selection, and scrolling at the new origin.

An 80-column target can retain labelled controls and roughly 40 cells of input:

```text
 Find: remain                                       6/21 [Prev] [Next] [Close]
 With: stay                                      [Replace 1] [Replace all]
```

These are layout constraints for the production renderer, not another HTML mockup. At 80×24, one tab row, one breathing row, and one footer would leave 21 body rows normally, 20 with Find, and 19 with Replace. Use a calculated narrow fallback with a separate action row when the input and labels cannot fit; draw and hit-test from the same geometry. Full-field focus, caret, no-match state, and long counts must remain clear.

Do not add a sidebar, outline, new themes, or more workspace features to this first polish pass. The sketch guides spacing and hierarchy; production screenshots determine whether the implemented design succeeds.

## Coordination

Following the user's agreement, the coordinator installed the official macOS arm64 `0.1.0-beta.5` binary under ignored `target/tools/`, verified the release SHA-256, and checked its version/help. [The runner guide](../tools/tui-test/README.md) reproduces this project-local setup without Homebrew or a Cargo dependency.

The completed three-agent research pass was read-only: A investigated terminal input, B reviewed the UI against the screenshots, and the forward implementer evaluated existing harnesses. The coordinator consolidated the decision and owns this document. No installs or runtime edits occurred during that research assignment; the local installation above followed it.

Implementation uses three separate worktrees from `54e9dde`, assigned to keyboard input, test infrastructure, and compact UI. The coordinator reviews and integrates focused commits. Reading width and word-aware wrapping remain a separate geometry change.

## Runner trial findings

The pinned runner drives the real executable through its PTY and embedded Ghostty backend. Its encoder reproduced the baseline failure: `Shift+Enter` generated `ESC[27;2;13~` and left result 2 selected. After the application requested disambiguated input with `ESC[>1u`, the same backend key action generated `ESC[13;2u` and moved from result 2 to result 1. Three matches make direction observable. Separate raw-byte cases retain parser coverage for default Ghostty bytes, enhanced bytes, and legacy Enter; they are not substitutes for that negotiation test.

The terminal fix pushes enhancement after entering the alternate screen and pops it before leaving. Shared, idempotent cleanup prevents the panic hook and guard from popping twice. Focused fault-injection tests cover setup failures and cleanup; real PTY tests verify normal exit and termios restoration. The potentially blocking capability query is avoided.

The reusable Python adapter is independent of Ratatui. Marklane-specific scenarios supply fixtures and assertions. Artifacts retain source/tool hashes, environment, backend, dimensions, palette, actions, cells, cursor/modes, native PNG/SVG captures, byte logs, and casts. Saved scenarios can reexecute actions; casts replay output and resize events only. Each session has a private runtime directory and edits scratch copies without accessing the native clipboard.

The first run inherited `NO_COLOR=1` and `TERM=dumb`, suppressing application colors. Corrected runs remove `NO_COLOR`, set `TERM=xterm-256color` and `COLORTERM=truecolor`, and assert real nondefault cells plus active/passive highlight styles. Palette substitution alone would not have corrected the missing application styles.

Adopt the runner for bounded layout, protocol, and source-interaction checks, with these explicit limits:

- Native PNG export rejects cells containing multi-scalar graphemes, including `e` plus a combining acute accent. The cell data preserves the sequence; SVG and failure traces are retained. Installing a font does not fix the renderer's scalar-count restriction.
- The original ZWJ emoji bytes reach the backend intact, but its cells split the sequence and overlap the application's two-column cursor placement. No DEC 2027 grapheme-mode activation is emitted. This is an unresolved width/profile mismatch upstream of image export; SVG conversion cannot repair the cell state.
- A separate fixture with precomposed accents and CJK supports layout-image review. It does not replace the original Unicode fixture or establish full Unicode visual acceptance.
- Font fallback is not fully pinned. Captures use tui-test's own fixed-cell renderer, not the user's Ghostty desktop pixels. Native font shaping, OS key interception, IME, clipboard delivery, and actual Ghostty appearance remain manual checks.

These limits keep the runner useful now without claiming a general terminal compatibility certification.

## Compact interface implementation

Tabs now show the filename and dirty marker, with active styling rather than a repeated `LIVE` suffix. A single contextual footer replaces permanent capability instructions. At 80×24 the document has 21 rows normally, 20 with Find, and 19 with Replace. The search dock has bounded fields, one count, clickable labelled actions, and a calculated narrow fallback. Ctrl+R retains the query/current result; Tab moves between fields; replacement remains explicit and undoable.

Following the user's additional feedback, neutral backgrounds separate the tab strip, search dock, and footer from document text. Each control background has a paired foreground; input fields have a darker fill. This is a small shared chrome palette, not the planned theme system. The document continues using the terminal's default background.

File failures and clipboard fallback/no-text warnings persist through ordinary input. Explicit dismissal clears them; a successful native clipboard retry replaces a clipboard warning without hiding an unrelated file error. Native clipboard automation remains fake-only.

The background change exposed an assertion that counted the query field as another passive result. The runner now identifies the three document occurrences by their fixture context before checking their styles, retaining the strict one-active/two-passive requirement. Production cell styling did not need to be weakened to satisfy the test.

Bounded reading width, word-aware wrapping, a sidebar, and theme selection remain outside this checkpoint.

## Final integration evidence

The integrated build passes 148 Rust tests plus build, strict all-target Clippy, formatting, and whitespace checks. The coordinator ran all three dark suites and light captures against the same final binary: 52 and 21 assertions respectively. The 26 layout PNGs export successfully; original Unicode failures remain recorded separately rather than counted as image passes.

Local artifacts are retained under ignored `target/visual-final-dark/` and `target/visual-final-light/`, including `captures/05-replace-with-focused-80x24.png` and `captures/06-replace-42x16.png`. Their metadata identifies the exact executable and environment. The coordinator reviewed those integrated frames; both the UI agent and independent reviewer also inspected light/dark and wide/narrow frames. The [prototype guide](prototype-checks.md#compact-ui-and-keyboard-follow-up) lists the short remaining native-terminal check.
