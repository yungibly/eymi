# Eymi

A terminal editor for writing, reading, and reviewing Markdown, with familiar shortcuts and support for other UTF-8 text files.

The central idea: make a Markdown document pleasant to work in directly, with dependable cursor movement and selection, while preserving the underlying file exactly outside intentional edits.

## Install

```sh
brew install yungibly/tap/eymi
eymi note.md
```

[Release archives](https://github.com/yungibly/eymi/releases) include prebuilt binaries for macOS on Apple Silicon and Intel, and Linux x86_64 with glibc 2.35 or newer. Extract an archive and put `eymi` on your PATH. All themes are included; a Nerd Font is optional. Linux ARM and Windows release binaries are not provided.

Press **F1** for help or **F2** for searchable commands. Use **Ctrl+S** to save and **Ctrl+Q** to quit. With no filename, `eymi` starts an untitled Markdown document.

## Development

Building from source requires **Rust 1.89 or newer**. The [release guide](docs/releases.md) describes CI, package verification, and the Homebrew update process. Eymi's code is [MIT licensed](LICENSE); bundled themes and Rust dependencies retain their own notices in release archives.

- [Product and interaction plan](docs/product-plan.md): live editing, rendering, controls, tabs, themes, and ideas for working alongside agents.
- [Technical plan and milestones](docs/technical-plan.md): proposed Rust stack, source mapping, file safety, compatibility, and the first prototype.
- [Implementation coordination](docs/coordination.md): current agent ownership, shared interfaces, and prototype acceptance criteria.
- [Prototype checks](docs/prototype-checks.md): headless verification and a short manual terminal checklist.
- [Visual testing and UI polish](docs/visual-testing-plan.md): screenshot feedback, the Ghostty keyboard fix, and the reusable test loop with its known limits.

Build and print a headless screen without opening an interactive terminal:

```sh
cargo build --locked
cargo run --locked -- --snapshot tests/ui/demo.md
cargo run --locked -- --snapshot --snapshot-size 160x45 --theme "Catppuccin Mocha" tests/ui/visual/writing.md
cargo run --locked -- --list-themes
```

To edit a file in your terminal, run `cargo run --locked -- --theme "Catppuccin Mocha" path/to/note.md`. A saved theme is used by default, falling back to Sage Dark. `--theme` overrides the saved preference for that launch. With no filename, Eymi opens an untitled Markdown document. Use a [scratch copy of a fixture](docs/prototype-checks.md#manual-smoke-check-once-the-build-is-ready) for experimenting.

Eymi has document tabs, live/source views, clickable rendered task boxes, automatic list continuation, selection, undo/redo, and save. Checkbox clicks include the space immediately on either side. Leaving a list keeps subsequent Enter presses in ordinary text. Live view reveals the active block's source. Ctrl+E or F6 changes view, Ctrl+S saves, F4 opens Save As, and F1 shows controls.

The writing surface uses explicit paired colors, a centered live column up to 88 cells wide, and word-aware prose wrapping. Source view uses the available width; code and tables retain source-oriented wrapping. A single tab row keeps the filename visible, with a distinct active tab and clickable overflow arrows. The themed status bar shows document type, source-word count, position, and encoding as space permits. It displays SOURCE when Markdown source view is active. Commands are discoverable through F1 and F2; contextual search guidance and errors appear when needed. Find uses one row at 80 columns; Replace adds one more. Narrow windows place actions on a separate row. Routine feedback clears on the next input, while actionable errors remain visible until dismissed or resolved.

F2 or Ctrl+P opens a filterable command palette with shortcut hints. It includes file, search, formatting, history, view, sidebar, theme, and icon actions. Ctrl+G jumps to a source line. An outline sidebar appears automatically at 110 columns and above, highlighting the current section. Click a heading to jump, or press F9 to focus the sidebar and use arrows/Enter. Escape returns to the editor; F9 while focused hides it. The sidebar hides in small windows and can be shown explicitly when at least 60 columns are available. Headings come from the Markdown parser, so fenced-code examples do not appear as headings.

## Themes and preferences

**624 built-in themes** include Catppuccin, Dracula, Nord, Gruvbox, Solarized, Tokyo Night, One Dark, and many more. Press F2, choose **Choose theme**, and type to filter. Arrow keys preview the highlighted theme throughout the editor; Enter or a visible row click applies and remembers it. Escape cancels the preview. Theme changes preserve source, selection, and undo history.

`eymi --list-themes` lists stable IDs and display names. `--theme nord`, `--theme catppuccin-mocha`, and quoted display names work; `dark` and `light` retain the original Sage palettes. The 622 imported palettes are pinned color data from iTerm2-Color-Schemes; builds and runtime need no theme downloads. [Provenance, licenses, exclusions, and the reproducible importer](third_party/iterm2-themes/README.md) accompany the catalog.

Optional **Nerd Font icons** add small document and outline symbols. Configure your terminal to use a Nerd Font, then choose **Toggle Nerd Font icons** in F2, or launch with `--icons nerd`. The default is plain text; `--icons plain` selects it explicitly. The CLI flag overrides your saved icon preference for that launch. No font is bundled or downloaded.

Explicit theme, sidebar, and icon choices are saved to `$XDG_CONFIG_HOME/eymi/settings.conf`, or `~/.config/eymi/settings.conf`. If the `eymi` config directory is absent and an existing `marklane` directory is present, Eymi uses those preferences in place. No files are migrated or deleted. Concurrent edits, read-only settings, and invalid files are reported and preserved. `--no-state` disables preferences and recovery. Help, version, theme listing, and headless snapshots never read or write user settings or recovery files.

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
| Alt+Up / Alt+Down | Move the current or selected source lines |
| Alt+Shift+Up / Alt+Shift+Down | Duplicate source lines above / below |
| Ctrl+Z / Ctrl+Y or Ctrl+Shift+Z | Undo / redo |

Formatting wraps selected content or inserts a marker pair with the caret inside. Indent adds four spaces; outdent removes up to four spaces or one tab. A plain Tab at a caret inserts a literal tab. Enhanced terminals can also deliver Ctrl+I for italic; Alt+I and the palette avoid its legacy Tab ambiguity.

Move and duplicate act on whole source lines, including every line touched by a selection except a final endpoint at the next line's start. The selection follows its text, and duplication selects the new copy. Each change is one undo step. Moving keeps the BOM, newline sequence, and final-newline presence; duplication uses a local line ending for the new separator. Edge moves are no-ops. These commands are also in F2 when a terminal intercepts modified arrows.

Contiguous non-whitespace typing forms one undo group. Whitespace, navigation, saves, and other commands start a new group; paste, formatting, list continuation, and Replace All remain individual transactions. History retains at most 256 snapshots and 32 MiB across undo/redo, excluding the current document and saved baseline. Older history is evicted first; very large edited snapshots can exceed this budget and cannot retain an undo inverse. The editor preserves source bytes outside intentional edits, including BOM, line endings, and trailing whitespace.

## Workspace, search, and files

Ctrl+N or the tab row's **+** button creates a Markdown tab. Click a tab or use F7/F8 or Ctrl+PageUp/PageDown to switch. Each tab retains its document, undo history, selection, view, and scroll position; clipboard contents are shared. Switching tabs closes Find. Ctrl+W closes the current tab and prompts for unsaved edits. Ctrl+Q checks every dirty tab before quitting; canceling keeps all tabs, including any earlier choices to discard. An unsaved tab has a `*` beside its name. The plus button yields space to the active label in very small windows.

**F10** opens a searchable list of open documents, including unsaved tabs. Filter by filename or path; abbreviated letters match in order, and separate words can match the name and directory. **F11** searches Markdown headings, with level and source line to distinguish repeated names. Both are available from F2 as **Switch document** and **Go to heading**. Arrows choose a result, Enter accepts, and Escape keeps the original document and selection. Heading navigation uses the existing parser, so fenced code is excluded and no language server is required. These additions follow a [focused review of Neovim workflows](docs/lazyvim-research.md).

Ctrl+O opens a directory browser with folders first and text-file candidates visible by default. Enter or click opens an entry; Backspace or the Up button goes to its parent directory. Tab switches between the list and an editable path, F2 toggles hidden files, and F5 toggles all file types. Opening an already open path selects its tab. Errors keep the current document available. Browser opens require UTF-8 text without NUL bytes and are limited to 8 MiB; a listing examines at most 2,048 directory entries, with direct paths available for omitted names.

Ctrl+F opens a compact Find row; Ctrl+R expands it with a replacement field while retaining the query and current result. Search is literal and case-sensitive, including hidden Markdown source. All visible matches receive a subtle highlight, with the current match brighter. F3 / Shift+F3 navigates matches, and the panel's navigation, replacement, and close buttons are clickable. Click or drag within a search field to position its caret or select text.

Tab / Shift+Tab switches directly between Find and With, selecting the destination field's existing text. Enter in Find advances; Enter in With replaces the current result and advances. Alt+R replaces one result; Alt+A replaces all. Replace All is one document undo step. Press Escape before Ctrl+Z to undo document changes; within a search field, Ctrl+Z undoes field input.

On supporting Unix terminals, Eymi requests disambiguated keyboard input so Shift+Enter can navigate backward. The protocol is restored on exit; Shift+F3 and the Previous button remain available. The [executable test runner](tools/tui-test/README.md) verifies the negotiated input bytes and captures the actual application without opening a native terminal window.

Ctrl+C/X/V uses the native clipboard locally, with a reported internal fallback on backend errors or SSH. The terminal's own paste shortcut also accepts external text. Empty paste preserves the selection. Automated clipboard tests use fake backends; the user reports the follow-up build works, without a per-platform compatibility record. Native Wayland and OSC 52 are not implemented.

The active file is checked for external changes about once a second. F5 or **Reload file from disk** loads fresh disk text. Dirty buffers require a visible confirmation; reload is undoable and is refused if the old text cannot fit the history budget. Undo restores local text as dirty against the latest disk baseline. Missing, unreadable, invalid, or oversized files leave the buffer intact.

Save As requires a new path that is not owned by another tab, and a changed disk baseline blocks overwrite. Changed saves to read-only files are rejected. Both command-line and browser opens reject invalid UTF-8, binary NUL bytes, and files over 8 MiB. Disk-baseline checks stream at most the accepted file size plus one byte, so an externally enlarged file does not cause an unbounded allocation. Atomic replacement still cannot exclude a separate writer after the last check.

## Crash recovery

Interactive sessions checkpoint dirty named and untitled documents about once a second. After an interrupted session, a startup notice offers **F2 → Recover documents**. Each chosen copy opens in a new, explicitly unsaved tab—even when its recovered text is empty. Save As chooses a new filename; recovery never overwrites a newer disk version. A recovered tab is durably checkpointed before its old recovery record is consumed.

Private snapshots live in `$XDG_STATE_HOME/eymi/recovery`, or `~/.local/state/eymi/recovery`. The state directory independently falls back to an existing `marklane` directory when `eymi` is absent, preserving access to older snapshots without migration. Session locks isolate simultaneous editors. Save and confirmed discard remove the corresponding checkpoint; a canceled quit retains the remaining dirty tabs. Corrupt or unreadable records stay on disk with a diagnostic. Ordinary clean shutdown removes only that session's owned snapshots. Recovery is implemented for Unix and tested on macOS; unsupported platforms report that recovery is unavailable while editing remains usable.

Recovery is a safety net with a roughly one-second checkpoint interval, so edits since the last successful checkpoint can be lost. Each snapshot accepts up to 16 MiB; discovery is bounded to 64 records / 64 MiB and 128 state entries, reporting skipped records. Original documents are never written by recovery. `--no-state` disables all preference and recovery storage.

Unsupported Markdown constructs remain source, including tables. The core still uses bounded whole-document history and is intended for small documents. Automatic merging and restoring the complete previous workspace remain future work. Native Wayland/OSC 52, IME, and cross-platform terminal compatibility have not been established by automated tests.

The [verification record](docs/prototype-checks.md#verification-record) records automated tests, build/lint checks, and actual executable captures separately from terminal-specific manual checks. The [local terminal runner](tools/tui-test/README.md) covers compact and wide layouts in light/dark Sage and seven imported palettes, command/outline navigation, source edits, and keyboard protocol behavior. Its known combining-grapheme and emoji rendering limitations remain documented.

The product should be useful with ordinary terminal capabilities. Larger headings and images are optional experiments. Its files remain ordinary Markdown; no account, service, or proprietary document format is required.
