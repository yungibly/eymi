# Local terminal test runner

Use the pinned official standalone `tui-test` binary for bounded visual and protocol checks. It exercises the compiled application without adding dependencies to Marklane or requiring a Homebrew installation.

Pinned release: [`0.1.0-beta.5`](https://github.com/microsoft/tui-test/releases/tag/0.1.0-beta.5), reviewed source commit `6a991eea96d499689a875b7ee5781aa0c18b88d2`.

From any working directory:

```sh
sh /path/to/md-term-editor/tools/tui-test/bootstrap.sh
```

The script prints the installed binary's absolute path. Downloads and binaries live under the ignored `target/tools/tui-test/` directory. The macOS arm64 and x86_64 release checksums are pinned in the script; every run verifies its cached or freshly downloaded archive before extracting it. It does not modify PATH or install a daemon/service. Session commands can start a runner-owned daemon later; each test must close its own session.

On the current Apple Silicon development host:

```sh
./target/tools/tui-test/0.1.0-beta.5/aarch64-apple-darwin/tui-test --version
```

Bootstrap, SHA-256 verification, `--version`, and `--help` were checked on macOS arm64. Intel and other operating systems have not been exercised; the bootstrap currently accepts only macOS. Deleting Cargo's target directory also removes this installation; rerun the bootstrap to restore it.

## Bounded executable acceptance runner

The standard-library Python adapter in `session.py` drives the compiled program through a real PTY and the embedded Ghostty backend. Marklane's scenarios and fixtures live in `tests/ui/visual/`; the adapter contains no editor-specific behavior.

```sh
cargo build --locked
marklane_tui_test="$(sh tools/tui-test/bootstrap.sh)"
python3 tests/ui/visual/run.py \
  --tool "$marklane_tui_test" \
  --binary target/debug/marklane \
  --output target/visual-current \
  --keyboard enhanced --theme dark --palette dark
```

The current build defaults to `--keyboard enhanced`; use `--keyboard baseline` only for a pre-fix binary. Explicit tool/binary paths also allow comparing separate worktrees. `--suite captures`, `--suite protocol`, `--suite unicode`, `--suite workspace`, `--suite themes`, and `--suite reliability` isolate checks; the default runs all six. `--theme light --palette light` selects both the editor's light theme and the emulator's light defaults. Omit `--theme` when comparing a binary predating editor themes. Each output directory must be new, keeping runs and source revisions independent.

Layout captures cover 80×24, 42×16, 120×36, and 160×45. The workspace suite starts with `writing.md` at 160×45, checks pointer/keyboard outline navigation, go-to-line, filtered commands, theme switching, formatting and indentation with exact saved source and undo, grouped keyboard typing, literal paste, tabs, and tiny-palette behavior. It also captures 20×10 and 20×4 fallbacks. Native PNGs, cells and traffic all come from the same executable session. The themes suite checks seven popular imported palettes, their actual upstream RGB backgrounds, preview/cancel/apply, source preservation, and 42×16 / 20×4 picker behavior. The reliability suite checks external disk notices, F5 reload confirmation, tiny-dialog safety, and exact local source restored by undo.

The runner copies fixtures before making any edits. It removes inherited `NO_COLOR`, sets `TERM=xterm-256color` and `COLORTERM=truecolor`, and starts a fresh daemon with an isolated `TUI_TEST_HOME` and explicit config. It also isolates `XDG_CONFIG_HOME` and `XDG_STATE_HOME` and forces the internal clipboard with an SSH test marker; it does not change `HOME`. All operations run within one owning Python process and close only that session. On the tested macOS execution sandbox, the local PTY/socket needs ordinary execution escalation; sandbox failure appears as “socket never started accepting connections.” No native terminal window is required.

Each suite retains:

- Named PNGs and SVGs, complete cells/styles, cursor, modes, and text.
- `terminal.log` with reversible raw traffic, plus decoded `read.bin`, `write.bin`, and `reply.bin`; automatic cast output is retained separately.
- `actions.json` with CLI arguments and log offsets, assertions, and protocol byte evidence.
- Tool/app hashes, version, source checkout revision/status when available, dimensions, environment, palette, and font limitations.

The output root also contains the scenario, helper, fixtures, pinned palette data, and original invocation. Replay the saved `scenario.py` with explicit tool/binary paths and a new output directory. Casts replay terminal output; the Python scenario re-executes actions.

The tests inspect actual cells for emphasis, wide-character placement, source selection, and distinct active/passive match backgrounds. Three query matches distinguish previous from next. Protocol checks observe the app's activation before accepting backend-generated modified input. Raw Ghostty 1.3.1 default `ESC[27;2;13~`, enhanced `ESC[13;2u`, and legacy CR are separately labelled parser regressions. Paste honors the observed bracketed-paste mode; wheel events carry a body cell position. Checkbox and multiline-paste operations verify saved scratch source and one-step undo.

For a quick text-only geometry check without the PTY runner:

```sh
./target/debug/marklane --snapshot --snapshot-size 160x45 --theme light tests/ui/visual/writing.md
```

Snapshot dimensions accept 1×1 through 400×160 and default to 80×24. Text snapshots omit colors; use the executable captures for appearance.

## Verified scope and limits

Baseline `54e9dde` and keyboard-fix `6df659f` passed the bounded semantic/protocol checks on macOS arm64. The embedded backend generated `ESC[27;2;13~` before activation, leaving result 2 unchanged. After the app emitted `ESC[>1u`, the same backend Shift+Enter action generated `ESC[13;2u` and selected result 1. Cleanup popped the enhancement once before leaving the alternate screen. The backend's observed default matched the installed Ghostty 1.3.1 profile in this trial.

This is **limited visual adoption**, not complete renderer acceptance:

- Native PNG export rejects any multi-scalar cell grapheme, including `e` plus combining acute. `acceptance.md` retains this original coverage; failed PNGs keep the SVG, cells, and trace. The rasterizer's suggestion to change fonts does not resolve its multi-scalar rejection.
- The original ZWJ emoji bytes survive app output and source undo, but the backend's cells split the cluster. App cursor placement then overlaps that wider backend representation. No DEC 2027 grapheme-mode activation was observed. SVG conversion cannot repair this cell state; ZWJ visual fidelity is not asserted.
- `screenshots.md` is an explicitly simplified fixture using precomposed accent/CJK characters, intended for chrome, search layout, and colors. It does not replace the original Unicode fixture.
- Images use tui-test's own fixed 10×21-cell renderer, 17px JetBrains Mono preference and available system fallbacks. Fonts/fallbacks are not fully pinned, and the images are not Ghostty desktop pixels. Native clipboard, OS shortcuts, and IME are outside this run.

`result.json` reports assertion results and PNG failures separately and leaves full renderer acceptance false. See the [visual testing plan](../../docs/visual-testing-plan.md) for the broader adoption gate. No custom emulator or renderer was added.
