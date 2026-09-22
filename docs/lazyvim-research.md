# Focused editor additions from Neovim workflows

The September 22, 2026 review compared the current editor with primary documentation for [LazyVim keymaps](https://www.lazyvim.org/keymaps), [Snacks pickers](https://github.com/folke/snacks.nvim/blob/main/docs/picker.md), and [Neovim whole-line changes](https://neovim.io/doc/user/change/#%3Amove). The feature priorities below are our assessment for a non-modal Markdown editor.

| Addition | Why it fits | Interaction |
| --- | --- | --- |
| Open-document picker | Buffer lookup helps when tabs overflow. | F10 or the command palette; fuzzy title/path filtering, Enter switches, Escape cancels. |
| Heading picker | Symbol lookup translates naturally to Markdown sections. | F11 or the command palette; fuzzy heading filtering with level/source line, using the existing parser without LSP. |
| Move and duplicate lines | Reordering prose, tasks, and source blocks is a frequent editing action. | Alt+Up/Down moves; Alt+Shift+Up/Down duplicates; palette fallback; one undo per change. |

Document and heading queries operate on a separate input document and activate only on acceptance. No disk indexing or new persistent history is required. Line operations act on source lines, preserve the document's newline/BOM conventions and selection direction, and do not renumber Markdown lists automatically.

A plus button in the tab row is a separate user-requested addition and invokes the same new-document action as Ctrl+N. It yields space to the active tab at very small widths.

Back/forward jump history and recent-file history remain potential later additions. They need explicit policies for closed documents, positions after edits, missing paths, and saved history. Project indexing, LSP, and a plugin system are outside this focused pass.
