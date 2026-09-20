//! Source-preserving editing core. All source positions are UTF-8 byte offsets.
pub mod document;
pub mod editing;
pub mod markdown;

pub use document::{Document, EditError, Selection};
pub use markdown::{Block, BlockKind, MarkdownSnapshot, Task};
