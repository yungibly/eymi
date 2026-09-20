# Technical plan and milestones

Architecture and roadmap · September 19–20, 2026. The first Rust prototype is implemented; this document also describes later work. See [coordination](coordination.md) for the current scope and [prototype checks](prototype-checks.md#verification-record) for observed results.

## Recommended approach

Use Rust with Ratatui and Crossterm for the initial prototype, keeping document editing independent of the UI. Rust is a good fit for a distributable native editor and gives useful control over text buffers and rendering. It does not make cursor mapping or Markdown semantics easy; those remain the central risks.

Go with Bubble Tea is a reasonable alternative if implementation speed or team familiarity favors Go. Its official project describes an event/model/update approach suitable for TUIs. Switching languages would still leave the projection and editing problems to solve. [Bubble Tea](https://github.com/charmbracelet/bubbletea)

Avoid a framework bake-off before testing the difficult interaction. Pin actual dependency versions when implementation begins; the candidates below are responsibilities, not a validated dependency lockfile.

| Responsibility | Initial candidate | Decision boundary |
| --- | --- | --- |
| Layout/chrome and cell rendering | [Ratatui](https://ratatui.rs/concepts/rendering/) | Build a custom document widget; evaluate extension rendering separately |
| Input and terminal lifecycle | [Crossterm](https://docs.rs/crossterm/latest/crossterm/event/index.html) | One input owner normalizes keys, paste, mouse, resize, and replies |
| Editable text | [Ropey](https://docs.rs/ropey/latest/ropey/) | Prototype its Unicode indexing and snapshot costs; source remains authoritative |
| Markdown semantics | [pulldown-cmark](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.OffsetIter.html) | Source-ranged parse events are useful; exact delimiter mapping still needs work |
| Incremental syntax, if needed | [tree-sitter-markdown](https://github.com/tree-sitter-grammars/tree-sitter-markdown) | Evaluate only if correctness or performance requires a concrete syntax layer |
| Search/dialog fields | [ratatui-textarea](https://github.com/ratatui/ratatui-textarea) | Good candidate for small text inputs; do not presume it solves live-document layout |
| File watching, clipboard, config | Select small platform-appropriate libraries after the spike | Keep these behind adapters; verify maintenance and supported platforms at selection time |

Start as one Cargo package with modules. Split crates only if the interfaces or testing make it worthwhile.

## Source-preserving architecture

```text
terminal events -> commands -> edit transactions -> document + revision
                                                    |           |
                                               undo history     +-> saved bytes
                                                    |
                          source snapshot -> parse -> projection -> terminal cells
                                                    |
                                        source <-> screen mapping
```

The original text buffer is the document. A parse tree and a rendered view are derived indexes; neither is serialized back over the file during normal save.

An edit transaction records replacements, selection before/after, and a revision. Typing groups, paste, task toggles, list continuation, and formatting commands each have sensible undo boundaries. Cursor navigation and style changes never dirty the buffer. A save point is tied to document content/history, not merely an “edited once” boolean.

Task glyphs are semantic hit targets in the projection, separate from item-text insertion targets. Store the exact source range of the checkbox state character. A click dispatches `ToggleTask` against the current revision, edits that marker, and leaves the caret anchored; it does not move the caret to the marker and simulate typing. Revalidate the target if its parse revision is stale.

`Enter` dispatches a list-continuation command only when the caret is in an editable list item. Obtain list/container context from the parser plus original source slices, preserve the original marker/indentation, increment explicit numbers, and make new tasks unchecked. Handle splitting, empty-item exit, and nested outdent as one transaction each. Literal code, terminal paste events, and the explicit literal-newline command bypass this helper. Test source and live views against the same command layer.

Leaving an outer list must also end that list in the source. Add a separating blank line when immediately typed prose would otherwise become a lazy continuation of the preceding item; retain the surrounding quote prefix for a quoted list. Subsequent Enter presses on a blank line must not inherit the previous item's range. Verify complete input sequences and reopening the resulting text, not just isolated Enter commands.

Find/replace starts with case-sensitive literal source search. A result must have valid grapheme boundaries so selecting or replacing it uses the same position contract as ordinary editing. Search includes concealed Markdown destinations and delimiters; the projection reveals the selected range. Replace All matches the original source once, maps selection through those edits, and records one transaction. Empty queries and identical replacements must not create artificial edits or history entries.

Apply match colors to the final visible glyph styles, using source-range overlap and the current search revision. All matches share a readable background; the active result has stronger contrast and additional emphasis. This decoration must not change projection geometry, source selection, or document revision. Search fields and buttons export their actual rendered cell bounds for pointer handling, with grapheme-aware field maps and stale-geometry invalidation on resize or input changes. Keep field drags anchored to their starting layout so horizontal scrolling cannot move the selection beneath the pointer.

Separate modules for `document`, `editing`, `markdown`, `projection`, `terminal`, `workspace`, `files`, and `theme`. A headless command runner should exercise editing without opening a real terminal.

### Positions and mapping

Use distinct types for UTF-8 byte offsets, character indexes, grapheme boundaries, source lines, and terminal-cell coordinates. Parser ranges and buffer APIs may use different units; conversions must be explicit. A Unicode scalar is not necessarily a complete user-visible character or a terminal cell.

The projection needs spans with:

- Original source range and revision.
- Display text, style, and measured cell occupancy.
- A mapping kind: literal text, substituted glyph, concealed syntax, or inserted decoration.
- Valid insertion boundaries and affinity for ambiguous left/right edges.
- Block identity and layout version for hit testing, selection, and scroll anchoring.

There is no simple bijection between file bytes and screen cells. `&amp;`, escaped punctuation, tabs, combining characters, hidden link destinations, and decorative rules all demonstrate why.

For `**bold**`, retain the opening markers, inner text, and closing markers as distinct source regions even while hiding them. Clicking a rendered letter chooses its source position; entering that block reveals markers before editing. In the initial hybrid, arrows navigate ordinary exposed source within the active block. Crossing into another block performs a defined reveal and preserves the source anchor.

Select and delete exact source ranges. While dragging, use a stable projection snapshot; reveal selected blocks after the gesture, retaining source endpoints. If an external update arrives during a gesture, defer its presentation until the gesture ends and remap against its revision. Clicking the right half of a wide glyph chooses the nearest valid grapheme boundary. Never split a grapheme for display or deletion.

`pulldown-cmark` exposes events with corresponding source ranges through `OffsetIter`. This is useful input, but it is not a complete lossless token stream or ready-made caret map. [API documentation](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.OffsetIter.html)

Preserve source slices and build a small lexical mapping layer for delimiters, escapes, entities, and link structure. Begin conservatively: if the renderer cannot establish a trustworthy map, show that construct as styled source. Do not approximate byte offsets into rendered text or try to parse arbitrary Markdown with a few regular expressions.

If this mapping layer becomes a second Markdown parser, revisit using a concrete syntax tree. Tree-sitter provides concrete syntax trees and incremental parsing, but the selected Markdown grammar still needs corpus validation against the chosen semantics. Avoid maintaining two disagreeing semantic parsers without a demonstrated benefit. [Tree-sitter parsing documentation](https://tree-sitter.github.io/tree-sitter/using-parsers/2-basic-parsing.html)

### Parsing and rendering cadence

Initially parse an entire small document snapshot, cache the result by revision, and lay out only the viewport plus a margin. Do not reparse on cursor movement when the source is unchanged; only disclosure/layout may need to change.

Measure before adding incremental complexity. A changed fence or reference definition can affect distant content, so reparsing only visible lines is not a sound general solution. For larger files, move parsing to a bounded worker with cancellation/coalescing and discard results from stale revisions. Show current source where a stale projection would otherwise misplace edits.

Maintain a block-height index for scrolling once virtualization needs it. Resize, theme metrics, wrap width, and active-block disclosure invalidate different portions of layout. Rendering should be driven by changes, without a permanent busy redraw loop.

## Terminal compatibility

### Portable baseline

The baseline is fixed-size text cells, ordinary cursor control, readable standard colors, and familiar keypresses. True color, italics, mouse reporting, bracketed paste, hyperlinks, enhanced keyboard input, and graphics are independently negotiated enhancements, not a single “modern terminal” flag.

Use raw input to receive copy/undo shortcuts rather than letting line discipline turn them into signals. Crossterm documents raw mode for keyboard events and requires a consistent event-reading strategy. [Event API](https://docs.rs/crossterm/latest/crossterm/event/index.html)

Terminal replies and keyboard input share a stream. Give protocol queries bounded timeouts and parse replies through the same owner; late replies must not become document text. Restore terminal state on normal exit, handled errors, and recoverable panic paths. No application can guarantee cleanup after an uncatchable kill; document a recovery command if needed.

Enhanced input can distinguish otherwise ambiguous keys. Essential commands still need legacy-compatible alternatives because the terminal, OS, or multiplexer may capture shortcuts. [Keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)

Local clipboard access and remote clipboard access are different adapters. Prefer the local system clipboard when actually local. Support terminal-delivered paste and optional OSC 52 writes over remote sessions, with an explicit internal-copy fallback. Do not assume clipboard reads are available or repeatedly query them without a user paste action.

The implemented clipboard adapter uses `arboard` with image features disabled, behind a lazy serialized worker. Native waits and shutdown are bounded. Internal copies survive native failures, which switch the session to a reported fallback. An empty or nontext clipboard is a no-op, distinct from an unavailable backend. The library's Linux clipboard ownership lasts with its handle, so clipboard lifetime and shutdown matter; copying after process exit also depends on the desktop's clipboard manager. [arboard clipboard behavior](https://docs.rs/arboard/3.6.1/arboard/struct.Clipboard.html), [error distinctions](https://docs.rs/arboard/3.6.1/arboard/enum.Error.html)

Test actual capabilities through tmux/SSH rather than trusting `$TERM` alone. Native terminal mouse selection should remain accessible through the terminal's bypass modifier or an editor mouse-capture toggle.

### Optional rendering extensions

Keep capability information in one adapter: color depth, palette queries, keyboard reporting, clipboard write, text scaling, image protocol, and safe synchronized updates where available. User overrides help when probes fail.

OSC 66 occupies multiple cells and can alter row geometry. Prototype its integration with the chosen renderer independently. Any out-of-band output must be coordinated with cell diffing, damage tracking, clipping, scrolling, and clearing; do not assume printing an escape sequence after each frame is sufficient. Scaling remains optional. [Kitty text-sizing specification](https://sw.kovidgoyal.net/kitty/text-sizing-protocol/)

Never emit document-contained terminal escape sequences verbatim. Display control characters safely. Link activation and image loading are explicit UI actions; raw HTML is preserved as source. These rules belong in the renderer boundary, not in ad hoc Markdown handling.

## Files, external changes, and recovery

Support UTF-8 files initially, retaining UTF-8 BOM presence, CRLF/LF sequences, final-newline state, and untouched whitespace. Mixed line endings remain unchanged outside edits; newly inserted lines follow a documented local/default convention. For other encodings, stop before destructive decoding and offer a clear unsupported-encoding result. Do not silently replace invalid bytes.

Keep three versions where needed: the last accepted disk base, current local buffer, and newly observed disk content. Watcher events are hints; compare actual content again before save. An atomic replacement may change the watched inode, so watching the parent directory and reattaching must be considered.

| Situation | Proposed behavior |
| --- | --- |
| Disk changes; local buffer clean | Reload as a recorded external-change transaction; preserve a source/context anchor and show a quiet notice |
| Disk changes; local buffer dirty | Keep both versions; offer review, reload with local recovery retained, or Save As |
| File deleted or replaced while open | Keep buffer and identify the condition; do not recreate/overwrite silently |
| Save detects a newer disk version | Enter the conflict flow before writing |
| User undoes an external reload | Restore the older buffer as a local edit; retain the latest disk baseline so the difference is explicit |

The first release can provide side-by-side/source comparison and explicit resolution. Three-way merge and per-hunk acceptance are later improvements; ambiguous matches must never silently choose a version. A proposed patch must include its base revision/hash and be rejected or rebased through review when stale.

For saving, use a temporary file in the target directory, flush appropriately, and replace atomically where the platform supports it. Preserve relevant permissions and define behavior for symlinks and hardlinks before shipping. Failed writes keep the buffer dirty and recoverable. Atomic replacement prevents partial-file reads; it does **not** prevent another independent writer from winning a race. Check the base immediately before replacement, detect subsequent changes, and document the remaining coordination limit rather than promise universal concurrency safety.

Default Ctrl+S saves to the original file. Periodic recovery snapshots belong in private application state, not unrequested commits or visible backup clutter. Recovery is separate from auto-saving the original file. Recovery restore must compare current disk content before replacing anything. Settings, named tabs, and cursor positions may be restored per workspace; recovery snapshots also cover untitled documents.

## Milestones and decision gates

Do not attach calendar estimates before the projection spike. Each milestone ends in a working demonstration and a decision, not a feature-count target.

### 0. Compare and specify

Try the closest existing editors on a small corpus: a README, long plan, instruction file, nested lists, a table, and a document with unusual Unicode. Record which operations are uncomfortable rather than assuming novelty. Choose an initial terminal pair and preserve these documents as fixtures.

**Exit:** a short competitive gap list and an explicit Markdown subset. The user has already chosen active-block disclosure as the interaction to test.

### 1. Prove the editing surface

One document, basic source editing, live rendering of headings/emphasis/links/lists/code, clickable task checkboxes, Enter continuation for bullets/numbered lists/tasks, mouse placement, selection, wrapping, undo/redo, and save to a test file. Use the chosen active-block disclosure model. Skip workspace management at this stage.

**Exit:** edit the fixtures through live and source views without losing source fidelity or caret correctness. No-edit saves are byte-identical. Demonstrate disclosure at a wrap boundary, selection across blocks, paste/undo, and unfinished syntax. Validate and refine the disclosure geometry from that experience.

Also demonstrate clicking a task without entering its syntax, undoing that toggle, continuing an ordered list, creating an unchecked task after a checked task, exiting an empty item, and preserving a pasted list byte-for-byte. Those are first-milestone acceptance criteria.

**Stop/revisit:** if disclosure is distracting, refine anchoring and block boundaries before expanding scope. Reconsider the selected model with the user only if those experiments fail.

### 2. Make it safe for daily documents

Reliable file open/save/Save As, plain-text files, clipboard adapters, find/replace, keybinding discovery, error handling, external-change detection, basic conflict review, and crash recovery. Add readable tables with source editing fallback; harden task/list interactions from the first milestone.

**Exit:** a normal plan-editing session works end to end. Forced save failure and concurrent disk changes preserve recoverable work. Clipboard success/failure is honest on the tested local and remote setups.

### 3. Add the workspace and visual finish

Tabs, Ctrl+N, Ctrl+O browser, quick open, heading outline, theme tokens, Terminal/light/dark/high-contrast presets, and persistent workspace state. Expand code highlighting and theme selection after the fundamentals pass.

**Exit:** comfortable use across several real documents, both light and dark terminal backgrounds, and a narrow pane. Validate release packaging on the selected support matrix.

### 4. Explore differentiation

Direct table-cell editing, section-based change review, context-copy commands, structured patch handoff, optional enlarged headings, and local images. Prioritize from observed use rather than committing to all of them.

**Exit for each addition:** better real editing without weakening the baseline or changing untouched Markdown.

## Verification plan

This section defines the intended verification coverage. Results for the implemented first prototype are recorded separately in [prototype checks](prototype-checks.md#verification-record); later-stage targets below are not claims about the current build.

**Implementation constraint:** agents cannot use Computer Use to operate terminal apps in this environment. Run builds and headless tests through authorized shell tools. Build a deterministic event/render harness around the real application code, using Ratatui's test backend or an in-memory cell grid; reserve essential real-terminal visual verification for the user. The [coordination note](coordination.md#verification-without-terminal-app-automation) defines the current split.

- **Document fidelity:** no-op load/save and edit/undo/save over BOM, CRLF, trailing spaces, mixed endings, tabs, and no-final-newline fixtures.
- **Mapping invariants:** every hit yields a valid source boundary; visible literal spans map back correctly; concealed syntax has explicit affinity. Property tests cover random edits and undo restoration.
- **Semantic fixtures:** nested emphasis, delimiter changes, reference links, entities, escapes, code fences, nested containers, tables, and incomplete input. Preserve unsupported syntax visibly.
- **Unicode:** combining marks, CJK, emoji sequences, wide characters at wrap edges, and tabs. Test widths against the actual terminals; a width library alone is not proof of agreement. Assess bidi/IME behavior explicitly and document limitations.
- **Interaction traces:** synthetic key/mouse/resize/paste sequences through the production input/command path, rendered to an in-memory terminal backend. Assert source edits, selection, cursor location, and hit maps alongside focused cell/style snapshots. User-run real-terminal sessions then check key interception, paste transport, font rendering, and graceful exit; simulated results do not establish those properties.
- **Interactive constructs:** checkbox glyph versus label hit testing, exact marker-only toggles, one-step undo, stale task targets, list continuation in the middle/end of an item, incremented numbers, checked-to-unchecked continuation, empty nested items, quoted lists, code exclusions, and literal paste/newline.
- **File failure scenarios:** permission failure, disk-change-before-save, replacement/deletion, repeated watcher events, recovery after interruption, and safe resolution of stale patches.
- **Performance:** record input-to-frame CPU time, startup, parsing/layout, memory, and bytes emitted using 10 KB, 100 KB, 1 MB, and larger pathological fixtures. Include an enormous single paragraph and table.

Provisional targets on a documented local reference machine: warm edit-to-frame processing at p95 under 16 ms for a 100 KB document, ordinary small-file startup under 100 ms, and no UI stall over 50 ms from background parsing on 1 MB documents. These are goals to measure, not current results or promises about SSH latency. Large-file source fallback is preferable to a frozen rich view.

Compatibility should be tracked by observed behavior, not logos. Cover a plain baseline profile; a modern macOS terminal; Kitty for extension experiments; a Linux terminal; Windows Terminal when Windows support is in scope; and at least one SSH/tmux path. Record clipboard, keys, Unicode widths, colors, and recovery separately.

Prepare a concise manual checklist only after the build is runnable. Report automated harness results separately from user-observed terminal results, and leave untested combinations explicitly unverified.
