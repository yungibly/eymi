# Product and interaction plan

Planning draft · September 19, 2026 · All behaviors below are proposals.

## Product thesis

A comfortable place to edit the documents that steer work: plans, READMEs, specifications, notes, checklists, and instructions for agents. Open a file in the terminal, immediately understand its structure, make a few changes with familiar controls, and get back to work.

The strongest product promise is **beautiful Markdown that remains dependable to edit**. A plain-text fallback is useful for configuration and code, but Markdown determines the design priorities.

Treat agent-oriented work as a promising audience hypothesis. Validate it with actual sessions involving planning documents and another process editing the same repository; avoid assuming all users now primarily edit Markdown.

## The live-editing decision

Three approaches are worth distinguishing:

| Approach | Experience | Main tradeoff |
| --- | --- | --- |
| Styled source | All punctuation stays visible; syntax receives styling | Predictable, but only partly delivers the visual promise |
| Live Markdown | Inactive content is rendered; the active block reveals editable syntax | Strong initial candidate, with layout changes to solve |
| Fully formatted editing | Syntax remains hidden while the user edits rendered text | Most direct experience, but requires rules for every formatting boundary and structural edit |

**Chosen direction: live Markdown, with source view always available.** The user selected active-block syntax disclosure during this planning session. This is a hybrid editing model, not a claim of full rich-text WYSIWYG. The other approaches above explain the tradeoffs; they are not unresolved competing requirements.

Use a paragraph or other small semantic block as the unit of disclosure. Revealing only the physical source line can split a wrapped paragraph or multiline emphasis into inconsistent presentations. Revealing an entire long list would be too disruptive: use the current item paragraph and its needed container prefixes. Reveal a whole table initially; try editing cells directly later.

### Concrete interaction

Given `Write a **clear plan** before coding.`:

1. Outside the paragraph, display “clear plan” in bold and conceal the asterisks.
2. Clicking a visible letter maps to its original source position.
3. Reveal the paragraph's punctuation, keeping that source position as the caret anchor.
4. Typing, arrows, Backspace, and selection operate on that exposed text normally.
5. Leaving the paragraph restores its rendered appearance. Preserve the caret's screen row where possible and minimize horizontal movement.

Do not promise zero movement: revealing characters can change wrapping. Test whether a reserved prefix gutter, stable paragraph indentation, and scroll anchoring make the change comfortable. Compare a second design where delimiters remain visible but subdued. Reject any design that makes the user chase the caret.

### Editing contract

- Normal typing always inserts text. There are no insert/normal command modes.
- Live and source are presentation choices over the same buffer, selection, and undo history.
- Left/Right and Backspace respect grapheme boundaries. Up/Down move through visual rows and retain the preferred visual column.
- Home/End use visual-row boundaries; document navigation is separately available. Source line navigation is explicit.
- Mouse hit testing uses the geometry actually on screen. Decoration padding maps to a documented nearby insertion point, never a made-up character.
- During a selection drag, keep its layout stable. Afterward, reveal syntax in touched blocks so deletion and replacement are inspectable.
- Ordinary copy copies the selected Markdown source. Offer a separately named **Copy plain text** command. Terminal-native selection may copy what is displayed instead; the editor cannot redefine that behavior.
- Unfinished Markdown stays editable. A missing closing delimiter does not stop typing or silently “repair” the document.
- Switching views, opening a file, or saving without edits must not reformat the file.

If fully formatted editing is ever explored, it would additionally need rules for whether a caret at the end of bold text types inside or outside the bold span, what Backspace does at a link boundary, and how to expose a URL. That work is outside the chosen first prototype.

## Interactive Markdown: core requirements

The user specifically prioritized clickable checkboxes and automatic list continuation. These belong in the first editing prototype, not a later polish release. Markdown constructs should support useful direct actions while remaining ordinary source text.

### Checkboxes have two distinct targets

Click the checkbox glyph to toggle completion immediately. Click the item text to enter source editing at that position. Do not make the whole row a toggle: that would interfere with selecting and editing its words.

A checkbox click changes only the existing task marker, for example `- [ ] Ship the plan` to `- [x] Ship the plan`. Preserve indentation, bullet style, spacing, and the task text. Leave the text caret and scroll anchor where they were; update only the task's presentation. Unchecking accepts existing uppercase `X` as checked input. Every toggle is one undoable transaction, including restoring the caret/selection state.

Provide the same action through a **Toggle task** command for keyboard-only use. Mouse-capable terminals are the enhanced path; editing or toggling a task never requires a mouse. In source view, literal marker editing remains available.

### Enter understands the current list

| Before Enter at the end of a nonempty item | New item prefix |
| --- | --- |
| `- First point` | `- ` |
| `* First point` or `+ First point` | Preserve `* ` or `+ ` |
| `3. Third step` | `4. ` |
| `3) Third step` | `4) ` |
| `- [ ] Open task` | `- [ ] ` |
| `- [x] Finished task` | `- [ ] ` — a new task starts unchecked |

Preserve the item's indentation and container context. In the middle of an item, split at the caret and carry trailing text into the new item. Do not automatically renumber the rest of the file. Start with incrementing the current explicit number; a configurable repeated-`1.` style can follow later.

Enter on an empty top-level item removes its marker and returns to ordinary prose. In a nested empty item, leave one list level at a time; preserve a containing quote when leaving a quoted list. Tab/Shift+Tab can indent/outdent a list item when the context is unambiguous, with equivalent commands for terminals where those bindings are unavailable. Indent/outdent can follow the initial continuation behavior.

The helper is disabled inside fenced/indented code and other literal contexts. Pasting several lines must insert those lines unchanged rather than replay Enter helpers. Provide a **Literal newline** command and an enhanced Shift+Enter binding where available. Enter's insertion and helper changes form one undo step. The same Markdown editing commands work in live and source views.

editxr's documentation already lists a keyboard task-state command, so do not describe it as lacking task interaction based on a promo video. The proposed advantage to validate is direct checkbox interaction plus consistent, automatic list editing. [editxr documented controls](https://github.com/pixdeo/editxr)

### Extend the same interaction pattern later

- A heading's gutter control folds its section; clicking the heading text still edits it.
- A code block offers **Copy code**, copying its contents without fence markers.
- A local document link has an explicit **Open link** action that opens another tab; ordinary text placement remains predictable.
- A table can eventually expose cells with Tab/Shift+Tab navigation and row insertion, without rewriting unrelated source.

Give these controls separate hit targets and equivalent keyboard commands. Avoid turning every click on formatted prose into an action when the user intended to place the caret.

## Visual polish within a character grid

Use spacing, alignment, restrained color, and consistent structure. Every element should also be understandable in monochrome; do not require a patched font.

| Element | Proposed treatment | Editing/fallback |
| --- | --- | --- |
| Headings | Bold, clear spacing, a small level indicator in the gutter; optional rule beneath H1 | Reveal `#` markers when active; no inserted blank lines in the file |
| Prose | Soft wrapping with hanging indents and configurable reading width, initially around 80 cells | Width follows narrow panes; wrapping never rewrites source |
| Emphasis | Bold, italic where supported, underline or color alternatives | Delimiters visible in the active block |
| Links | Styled label; destination in the status area when focused | Explicit open action; edit label/target from source |
| Lists | Consistent bullets and continuation indentation | Enter continues items; Enter on an empty item exits; one undo step reverses the helper |
| Tasks | Clear checked/unchecked markers | Mouse click or a discoverable command changes only the checkbox marker |
| Quotes and callouts | A vertical rail; labeled note/warning treatments | Retain source; extension callouts can fall back to ordinary quotes |
| Code | Quiet background or side rail, language label, syntax color | Preserve spaces and tabs; wrapping is separately configurable |
| Tables | Aligned columns, restrained separators, visible overflow affordance | Initially disclose the table source; dedicated cell editing is a later experiment |
| Rules | Thin line with consistent margins | A source-anchored decoration |
| Frontmatter | Subdued metadata block, optionally folded | Always expandable; editing exposes literal source |
| Images | Alt text and destination placeholder | Optional local image rendering later, with the same document flow |

Start with CommonMark semantics and a deliberately enumerated extension set: tables, task lists, and strikethrough. Style frontmatter without interpreting it as application instructions. Preserve footnotes, math, raw HTML, and unfamiliar extensions visibly even before adding specialized rendering. Do not advertise browser-equivalent rendering.

For a narrow terminal, hide optional sidebars and shorten chrome before shrinking the writing area. Large tables may need local horizontal scrolling or a stacked row presentation; choose this explicitly rather than wrapping them into an unreadable grid.

### Variable text size

Kitty's OSC 66 protocol supports text scaling and explicit cell occupancy, with a support probe. It is a real option, not something to simulate with large ASCII lettering. [Protocol specification](https://sw.kovidgoyal.net/kitty/text-sizing-protocol/)

Experiment with enlarged inactive H1/H2 headings only after ordinary layout is reliable. Test scrolling, clipping, cursor placement, and switching into source. Capability detection must distinguish width support from scaling support and account for multiplexers. Keep this opt-in initially; the default design uses one cell height.

## Familiar controls and discovery

These are proposed defaults, subject to testing in actual terminals. Each important command should also be reachable through a menu or command palette and be rebindable.

| Action | Default proposal |
| --- | --- |
| New Markdown document | Ctrl+N |
| Open file browser | Ctrl+O |
| Quick open by path/name | Ctrl+P |
| Save / Save As | Ctrl+S / menu or palette; Ctrl+Shift+S where distinguishable |
| Close document / quit | Ctrl+W / Ctrl+Q, with unsaved-work handling |
| Copy / cut / paste | Ctrl+C / Ctrl+X / Ctrl+V |
| Undo / redo | Ctrl+Z / Ctrl+Y |
| Select all | Ctrl+A |
| Extend selection | Shift+arrows; mouse drag |
| Find / next match | Ctrl+F / F3 |
| Replace | Find panel action and palette |
| Toggle live/source view | Ctrl+E and F6 |
| Previous / next tab | Ctrl+PageUp / Ctrl+PageDown; F7 / F8 alternatives |
| Command palette / help | F2 / F1 |
| Bold selection | Ctrl+B |
| Italic / insert link / toggle task | Palette or menu; optional additional bindings |
| Dismiss popup or selection | Escape; it never quits the editor |

Provide a compact contextual footer with a few useful actions. A menu and command search prevent the shortcut list from becoming homework. Formatting a selection is a source edit; without a selection, a formatting command can insert paired markers with the caret between them, as one undoable transaction.

Do not assign essential distinct actions to Ctrl+I versus Tab, Ctrl+M versus Enter, or Ctrl+H versus Backspace in legacy input. Enhanced keyboard protocols can distinguish more combinations, but should be an improvement rather than an entry requirement. [Kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)

### Clipboard reality

Ctrl+C should copy, not terminate the app. Ctrl+V should use the local clipboard when available. Support pasted text delivered by the terminal, including bracketed paste, as a first-class path; users often already have a terminal paste binding. Native terminal shortcuts may intercept keypresses before the editor sees them. [Windows Terminal action documentation](https://learn.microsoft.com/en-us/windows/terminal/customize-settings/actions)

For SSH, copying through OSC 52 may work while reading the remote user's local clipboard may not. Keep an internal clipboard and expose the selected backend in diagnostics. If a system operation is unavailable, say so instead of silently claiming the system clipboard changed. A paste must form one undo transaction and must not trigger shortcuts embedded in its contents.

## Tabs, navigation, and ordinary files

Start with document tabs, not shell sessions. Every tab retains its own selection, scroll position, undo history, and live/source preference. Show unsaved state visibly and disambiguate identical filenames with short parent paths. An untitled Ctrl+N document starts as Markdown; the user can change its language.

Ctrl+O opens a compact browser with path entry, parent navigation, directories first, and text files. Directories remain navigable even if they currently contain no visible files. Markdown is easy to spot; it is not the only supported file type.

Use extensions as a fast hint, with bounded content inspection for extensionless files such as `LICENSE` or `Makefile`. Include a **Show all files** toggle and direct-path opening. Hidden and ignored files must be reachable through explicit toggles; agent instruction files often live there. Listing a directory must not read the entire repository or follow symlink cycles.

Other supported UTF-8 text opens in source view with ordinary editing. Code highlighting is useful when cheap to provide; language servers and IDE features are outside the initial scope. Never decode invalid bytes lossily and then overwrite the original file.

A heading outline is a strong early addition for long plans. Jumping between headings, folding sections, and “select this section” make Markdown structure useful beyond decoration. Splits and workspace-wide search can follow tabs once the writing surface is solid.

## Themes

Default to a **Terminal** theme: inherit the terminal foreground/background and use its ANSI palette conservatively. This naturally respects many existing setups without needing to identify a named theme. Where supported, query background/foreground colors to choose readable accents; time out safely if the terminal does not answer. Color queries are defined in the [XTerm control-sequence reference](https://invisible-island.net/xterm/ctlseqs/ctlseqs.pdf).

Offer a clear light/dark override. “Auto” means adapting to available color information, not discovering every terminal's complete theme or configuration. Recheck on focus or an available appearance notification without flashing the screen.

Use semantic theme roles: text, muted text, syntax marker, heading, link, code, quote rail, selection, search match, and added/removed text. Custom themes should be a small readable configuration file with live preview. Keep code token colors compatible with the document palette.

For the first usable release, ship Terminal, an original light and dark pair, and a high-contrast option. Grow a curated pack toward familiar palettes such as Catppuccin, Gruvbox, Nord, Solarized, Dracula, and Tokyo Night after checking their distribution terms and validating contrast. A long theme list is less valuable than readable selections and readable inactive syntax in every theme.

## Ideas that fit work alongside agents

### Highest value

**Changes on disk are a normal event.** When a clean open file changes externally, refresh it, preserve the reader's place, and make changed regions discoverable. When both the user and another process have edited it, retain both versions and offer a review. A filesystem event cannot establish whether the writer was an agent; label it “changed on disk” unless provenance is known.

**Review by section.** Group changes under headings and allow word-level inspection alongside the exact Markdown diff. A user often wants to know what changed in “Constraints,” not just which line numbers changed. Track “since last save,” “external change,” and “Git diff” as distinct baselines.

**Copy useful context.** A command can copy the current selection or section with its relative path and heading breadcrumb for pasting into an agent. Make this an explicit action that copies only the chosen scope. Plain copy remains ordinary copy.

**Act like a good Unix editor.** Support filenames and line positions, `$EDITOR`/`$VISUAL` use, and a clearly specified wait/exit behavior. Stdin editing and reviewed patch input can follow later, with UI on the controlling terminal rather than mixed into document output.

### Later possibilities

- A table command that adds rows, moves columns, and edits cells without hand-aligning pipes.
- Link completion for nearby documents and heading anchors, with local broken-link checks.
- A section-move command that carries nested headings and their contents together.
- Reading/focus view with optional section folding and less chrome.
- Project-local scratch documents and lightweight templates for plans and decisions.
- An explicit patch-review interface for external tools, with a base revision and conflict detection.

Built-in chat, model-provider settings, autonomous edits, collaboration servers, a plugin runtime, and a terminal multiplexer would all enlarge the product substantially. Defer them until the editor itself is compelling. Interoperation through ordinary files already enables useful agent workflows.

## Relevant precedents

This is a design opportunity in an existing category, not an untouched category. The following observations come from project documentation, not hands-on quality comparisons; sources were checked for this planning session.

| Project | What its documentation establishes | Lesson for this project |
| --- | --- | --- |
| [Microsoft Edit](https://github.com/microsoft/edit) | A terminal editor aimed at approachable use with familiar modern controls | Simple terminal editing is a credible product direction |
| [Micro](https://github.com/micro-editor/micro) | An intuitive terminal editor with documented system/internal clipboard paths | Treat clipboard behavior and discoverability as core functionality |
| [editxr](https://github.com/pixdeo/editxr) | Swift terminal Markdown editor with active-line source, rendered surroundings, themes, and reviewed AI section changes | Live Markdown plus AI is already present; benchmark actual editing interactions |
| [mq-edit](https://github.com/harehare/mq-edit) | Rust editor with active-line source, Markdown rendering, and LSP; repository marked archived July 22, 2026 | Useful precedent, but not an assumed maintained foundation |

Before committing to a new core, spend one focused comparison session typing, selecting, pasting, and undoing in the closest alternatives. Record concrete problems this design improves. The recommendation to build a custom projection is a technical hypothesis, not evidence that these projects edit badly.

## Decisions to make through prototypes

1. What is the smallest comfortable disclosure unit for nested lists, quotes, tables, and long paragraphs? Active-block disclosure is chosen; its boundaries need testing.
2. Which terminal/OS combinations are the first supported release targets? Proposed starting point: local macOS/Linux, with a compatibility spike for Windows and SSH/tmux.
3. Reading width by default or fill the pane? Offer both; test the narrower default on real documents.
4. Is external-change review needed before the first public release? Basic conflict protection is required regardless.
5. Product name and visual identity, after the editing interaction is convincing.

The next step is the [small prototype and its acceptance criteria](technical-plan.md#milestones-and-decision-gates).
