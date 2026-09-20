# md-term-editor

A proposed terminal editor for writing, reading, and reviewing Markdown, with familiar shortcuts and useful support for other text files.

**Status: planning.** There is no editor implementation yet. The directory name is a working label, not a product name. These documents describe recommendations and experiments, not settled requirements or implemented features.

The central idea: make a Markdown document pleasant to work in directly, with dependable cursor movement and selection, while preserving the underlying file exactly outside intentional edits.

- [Product and interaction plan](docs/product-plan.md): live editing, rendering, controls, tabs, themes, and ideas for working alongside agents.
- [Technical plan and milestones](docs/technical-plan.md): proposed Rust stack, source mapping, file safety, compatibility, and the first prototype.

## Proposed starting point

Build a small Rust prototype with one document, source and live views, clickable task checkboxes, automatic list continuation, selection, undo, and save. The chosen interaction reveals syntax in the active block and renders the surrounding document. Validate cursor stability and selection before building out the workspace UI.

The product should be useful with ordinary terminal capabilities. Larger headings and images are optional experiments. Its files remain ordinary Markdown; no account, service, or proprietary document format is required.
