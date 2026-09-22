# Marklane

A terminal editor for writing, reading, and reviewing Markdown, with familiar shortcuts and support for other UTF-8 text files.

**Status: under active development.** The product and command name is **Marklane** / `marklane`; the repository directory remains `md-term-editor`. Build locally to try the current usability checkpoint. CI, release packaging, and Homebrew distribution are deferred. The plans distinguish implemented behavior from future work.

The central idea: make a Markdown document pleasant to work in directly, with dependable cursor movement and selection, while preserving the underlying file exactly outside intentional edits.

- [Product and interaction plan](docs/product-plan.md): live editing, rendering, controls, tabs, themes, and ideas for working alongside agents.
- [Technical plan and milestones](docs/technical-plan.md): proposed Rust stack, source mapping, file safety, compatibility, and the first prototype.
- [Implementation coordination](docs/coordination.md): current agent ownership, shared interfaces, and prototype acceptance criteria.
- [Prototype checks](docs/prototype-checks.md): headless verification and a short manual terminal checklist.
- [Visual testing and UI polish](docs/visual-testing-plan.md): screenshot feedback, the Ghostty keyboard fix, and the reusable test loop with its known limits.

## Try it

With a recent Rust toolchain, build and print a headless screen without opening an interactive terminal:

```sh
cargo build --locked
cargo run --locked -- --snapshot tests/ui/demo.md
cargo run --locked -- --snapshot --snapshot-size 160x45 --theme light tests/ui/visual/writing.md
```

To edit a file in your terminal, run `cargo run --locked -- --theme light path/to/note.md`. Dark is the default theme; both use explicit foreground/background pairs. With no filename, Marklane opens an untitled Markdown document. Use a [scratch copy of a fixture](docs/prototype-checks.md#manual-smoke-check-once-the-build-is-ready) for experimenting.

Marklane has document tabs, live/source views, clickable rendered task boxes, automatic list continuation, selection, undo/redo, and save. Checkbox clicks include the space immediately on either side. Leaving a list keeps subsequent Enter presses in ordinary text. Live view reveals the active block's source. Ctrl+E or F6 changes view, Ctrl+S saves, F4 opens Save As, and F1 shows controls.

The writing surface uses a sage palette, a centered live column up to 88 cells wide, and word-aware prose wrapping. Source view uses the available width; code and tables retain source-oriented wrapping. Quiet tab, search, and footer bars separate controls from the document. Find uses one row at 80 columns; Replace adds one more. Narrow windows place actions on a separate row. Routine feedback clears on the next input, while actionable errors remain visible until dismissed or resolved.

F2 or Ctrl+P opens a filterable command palette with shortcut hints. It includes file, search, formatting, history, view, sidebar, and theme actions. Ctrl+G jumps to a source line. A documents/heading sidebar appears automatically at 110 columns and above. Click a heading to jump, or press F9 to focus the sidebar and use arrows/Enter. Escape returns to the editor; F9 while focused hides it. The sidebar hides in small windows and can be shown explicitly when at least 60 columns are available. Headings come from the Markdown parser, so fenced-code examples do not appear as headings.

## Editing controls

| Keys | Action |
| --- | --- |
| Ctrl/Alt + Left/Right | Move by word; add Shift to select |
| Ctrl/Alt + Backspace/Delete | Delete a word or the selection |
| Ctrl+B | Toggle bold |
| Alt+I | Toggle italic |
| Alt+backtick | Toggle inline code |
| Ctrl+] / Ctrl+[ | Indent / outdent source lines |
| Tab with a selection / Shift+Tab | Indent / outdent source lines |
| Ctrl+Z / Ctrl+Y or Ctrl+Shift+Z | Undo / redo |

Formatting wraps selected content or inserts a marker pair with the caret inside. Indent adds four spaces; outdent removes up to four spaces or one tab. A plain Tab at a caret inserts a literal tab. Enhanced terminals can also deliver Ctrl+I for italic; Alt+I and the palette avoid its legacy Tab ambiguity.

Contiguous non-whitespace typing forms one undo group. Whitespace, navigation, saves, and other commands start a new group; paste, formatting, list continuation, and Replace All remain individual transactions. History retains at most 256 snapshots and 32 MiB across undo/redo, excluding the current document and saved baseline. Older history is evicted first; very large edited snapshots can exceed this budget and cannot retain an undo inverse. The editor preserves source bytes outside intentional edits, including BOM, line endings, and trailing whitespace.

## Workspace, search, and files

Ctrl+N creates a Markdown tab. Click a tab or use F7/F8 or Ctrl+PageUp/PageDown to switch. Each tab retains its document, undo history, selection, view, and scroll position; clipboard contents are shared. Switching tabs closes Find. Ctrl+W closes the current tab and prompts for unsaved edits. Ctrl+Q checks every dirty tab before quitting; canceling keeps all tabs, including any earlier choices to discard. An unsaved tab has a `*` beside its name.

Ctrl+O opens a directory browser with folders first and text-file candidates visible by default. Enter or click opens an entry; Backspace or the Up button goes to its parent directory. Tab switches between the list and an editable path, F2 toggles hidden files, and F5 toggles all file types. Opening an already open path selects its tab. Errors keep the current document available. Browser opens require UTF-8 text without NUL bytes and are limited to 8 MiB; a listing examines at most 2,048 directory entries, with direct paths available for omitted names.

Ctrl+F opens a compact Find row; Ctrl+R expands it with a replacement field while retaining the query and current result. Search is literal and case-sensitive, including hidden Markdown source. All visible matches receive a subtle highlight, with the current match brighter. F3 / Shift+F3 navigates matches, and the panel's navigation, replacement, and close buttons are clickable. Click or drag within a search field to position its caret or select text.

Tab / Shift+Tab switches directly between Find and With, selecting the destination field's existing text. Enter in Find advances; Enter in With replaces the current result and advances. Alt+R replaces one result; Alt+A replaces all. Replace All is one document undo step. Press Escape before Ctrl+Z to undo document changes; within a search field, Ctrl+Z undoes field input.

On supporting Unix terminals, Marklane requests disambiguated keyboard input so Shift+Enter can navigate backward. The protocol is restored on exit; Shift+F3 and the Previous button remain available. The [executable test runner](tools/tui-test/README.md) verifies the negotiated input bytes and captures the actual application without opening a native terminal window.

Ctrl+C/X/V uses the native clipboard locally, with a reported internal fallback on backend errors or SSH. The terminal's own paste shortcut also accepts external text. Empty paste preserves the selection. Automated clipboard tests use fake backends; the user reports the follow-up build works, without a per-platform compatibility record. Native Wayland and OSC 52 are not implemented.

Save As requires a new path that is not owned by another tab, and a changed disk baseline blocks overwrite. Changed saves to read-only files are rejected. Both command-line and browser opens reject invalid UTF-8, binary NUL bytes, and files over 8 MiB. Disk-baseline checks stream at most the accepted file size plus one byte, so an externally enlarged file does not cause an unbounded allocation. Atomic replacement still cannot exclude a separate writer after the last check.

Unsupported Markdown constructs remain source, including tables. The core still uses bounded whole-document snapshots and is intended for small documents. Crash recovery, file watching/merge, persistent settings, and session restore remain future work. Theme selection applies to the current process and is not saved. Native Wayland/OSC 52, IME, and cross-platform terminal compatibility have not been established by automated tests.

The [verification record](docs/prototype-checks.md#verification-record) records automated tests, build/lint checks, and actual executable captures separately from terminal-specific manual checks. The [local terminal runner](tools/tui-test/README.md) covers compact and wide layouts in both themes, command/outline navigation, source edits, and keyboard protocol behavior. Its known combining-grapheme and emoji rendering limitations remain documented.

The product should be useful with ordinary terminal capabilities. Larger headings and images are optional experiments. Its files remain ordinary Markdown; no account, service, or proprietary document format is required.
