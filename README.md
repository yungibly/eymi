# Marklane

A terminal editor prototype for writing, reading, and reviewing Markdown, with familiar shortcuts and basic support for other UTF-8 text files.

**Status: runnable prototype, under active development.** The working product and command name is **Marklane** / `marklane`; the repository directory remains `md-term-editor`. The user has reported passing the initial and follow-up manual checks. The plans distinguish intended behavior from implemented features.

The central idea: make a Markdown document pleasant to work in directly, with dependable cursor movement and selection, while preserving the underlying file exactly outside intentional edits.

- [Product and interaction plan](docs/product-plan.md): live editing, rendering, controls, tabs, themes, and ideas for working alongside agents.
- [Technical plan and milestones](docs/technical-plan.md): proposed Rust stack, source mapping, file safety, compatibility, and the first prototype.
- [Implementation coordination](docs/coordination.md): current agent ownership, shared interfaces, and prototype acceptance criteria.
- [Prototype checks](docs/prototype-checks.md): headless verification and a short manual terminal checklist.
- [Visual testing and UI polish](docs/visual-testing-plan.md): screenshot feedback, the Ghostty keyboard fix, and the reusable test loop with its known limits.

## Try the prototype

With a recent Rust toolchain, build and print a headless screen without opening an interactive terminal:

```sh
cargo build --locked
cargo run --locked -- --snapshot tests/ui/demo.md
```

To edit a file in your terminal, run `cargo run -- path/to/note.md`. With no filename, Marklane opens an untitled Markdown document. Use a [scratch copy of a fixture](docs/prototype-checks.md#manual-smoke-check-once-the-build-is-ready) for experimenting.

The prototype has document tabs, live/source views, clickable rendered task boxes, automatic list continuation, selection, undo/redo, and save. Checkbox clicks include the space immediately on either side. Leaving a list keeps subsequent Enter presses in ordinary text. Live view reveals the active block's source. Ctrl+E or F6 changes view, Ctrl+S saves, F4 opens Save As, and F1 shows controls.

Shaded tab, search, and footer bars separate controls from the document. Find uses one row at 80 columns; Replace adds one more. Narrow windows place actions on a separate row. Routine feedback clears on the next input, while actionable errors remain visible until dismissed or resolved.

Ctrl+N creates a Markdown tab. Click a tab or use F7/F8 or Ctrl+PageUp/PageDown to switch. Each tab retains its document, undo history, selection, view, and scroll position; clipboard contents are shared. Switching tabs closes Find. Ctrl+W closes the current tab and prompts for unsaved edits. Ctrl+Q checks every dirty tab before quitting; canceling keeps all tabs, including any earlier choices to discard. An unsaved tab has a `*` beside its name.

Ctrl+O opens a directory browser with folders first and text-file candidates visible by default. Enter or click opens an entry; Backspace or the Up button goes to its parent directory. Tab switches between the list and an editable path, F2 toggles hidden files, and F5 toggles all file types. Opening an already open path selects its tab. Errors keep the current document available. Browser opens require UTF-8 text without NUL bytes and are limited to 8 MiB; a listing examines at most 2,048 directory entries, with direct paths available for omitted names.

Ctrl+F opens a compact Find row; Ctrl+R expands it with a replacement field while retaining the query and current result. Search is literal and case-sensitive, including hidden Markdown source. All visible matches receive a subtle highlight, with the current match brighter. F3 / Shift+F3 navigates matches, and the panel's navigation, replacement, and close buttons are clickable. Click or drag within a search field to position its caret or select text.

Tab / Shift+Tab switches directly between Find and With, selecting the destination field's existing text. Enter in Find advances; Enter in With replaces the current result and advances. Alt+R replaces one result; Alt+A replaces all. Replace All is one document undo step. Press Escape before Ctrl+Z to undo document changes; within a search field, Ctrl+Z undoes field input.

On supporting Unix terminals, Marklane requests disambiguated keyboard input so Shift+Enter can navigate backward. The protocol is restored on exit; Shift+F3 and the Previous button remain available. The [executable test runner](tools/tui-test/README.md) verifies the negotiated input bytes and captures the actual application without opening a native terminal window.

Ctrl+C/X/V uses the native clipboard locally, with a reported internal fallback on backend errors or SSH. The terminal's own paste shortcut also accepts external text. Empty paste preserves the selection. Automated clipboard tests use fake backends; the user reports the follow-up build works, without a per-platform compatibility record. Native Wayland and OSC 52 are not implemented.

Save As currently requires a new path that is not owned by another tab, and a changed disk baseline blocks overwrite. Changed saves to read-only files are rejected. Unsupported Markdown constructs remain source. The core keeps full history snapshots and is intended for small files. The browser's size limit does not apply to the initial command-line file; recovery, file watching, and themes remain planned work.

The current implementation passes 148 automated tests, a build, strict Clippy, and formatting checks on the development machine. The [verification record](docs/prototype-checks.md#verification-record) separates those results from terminal-specific manual checks; the [compact UI checklist](docs/prototype-checks.md#compact-ui-and-keyboard-follow-up) covers the latest changes.

The product should be useful with ordinary terminal capabilities. Larger headings and images are optional experiments. Its files remain ordinary Markdown; no account, service, or proprietary document format is required.
