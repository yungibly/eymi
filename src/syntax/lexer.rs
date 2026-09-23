//! Configurable lexer for C-like and scripting languages.
use super::{
    Lex, Out, Token, char_at, char_len, escape_end, find, ident_end, ident_start, is_all_caps,
    is_ws, lookup, number_end, skip_ws, starts,
};
use std::ops::Range;

/// What capitalization says about an identifier.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Caps {
    Plain,
    /// Capitalized names are types, also when called (constructors).
    Types,
    /// Capitalized names are types unless called (Go and C# exports).
    Exported,
}

/// What `@name` means.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum At {
    Plain,
    Attribute,
    /// Python decorators: only as the first token on a line.
    LineAttribute,
    Macro,
    Variable,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Heredoc {
    None,
    /// `<<~ID`, `<<-ID`, `<<ID` (Ruby, Perl, HCL; bare IDs must be capitalized).
    Angle,
    /// PHP `<<<ID`.
    Php,
}

/// A string delimiter pair. `escape` is 0 for none; `doubled` means a repeated
/// closing delimiter stands for itself (SQL `''`).
pub(super) struct Quote {
    pub(super) open: &'static str,
    pub(super) close: &'static str,
    pub(super) escape: u8,
    pub(super) doubled: bool,
    pub(super) multiline: bool,
}

/// Language description for [`Code`]. Word tables are sorted for binary search;
/// with `fold_case` they are lowercase and matched case-insensitively.
pub(super) struct Grammar {
    pub(super) keywords: &'static [&'static str],
    pub(super) types: &'static [&'static str],
    pub(super) constants: &'static [&'static str],
    /// Built-in functions, highlighted even when not called.
    pub(super) builtins: &'static [&'static str],
    /// Keywords whose next identifier names a function or a type.
    pub(super) fn_definers: &'static [&'static str],
    pub(super) type_definers: &'static [&'static str],
    /// PowerShell `-eq` style operators.
    pub(super) dash_operators: &'static [&'static str],
    pub(super) line_comments: &'static [&'static str],
    pub(super) block_comment: Option<(&'static str, &'static str)>,
    pub(super) nested_comments: bool,
    /// Ordered so that longer delimiters come first.
    pub(super) quotes: &'static [Quote],
    /// Identifiers that prefix a string (`r`, `b`, `L`, `u8`); `r`/`R` means raw.
    pub(super) prefixes: &'static [&'static str],
    pub(super) char_literals: bool,
    pub(super) fold_case: bool,
    pub(super) caps: Caps,
    pub(super) caps_constants: bool,
    /// Allow whitespace between a function name and `(`.
    pub(super) call_space: bool,
    pub(super) ident_start: &'static [u8],
    pub(super) ident_extra: &'static [u8],
    /// Ruby/Elixir method names may end in `?` or `!`.
    pub(super) ident_suffix: bool,
    pub(super) at: At,
    /// `$name` is a variable.
    pub(super) dollar: bool,
    /// Perl/Ruby/PowerShell punctuation variables such as `$!` and `$_`.
    pub(super) punct_vars: bool,
    /// `:name` is a symbol constant.
    pub(super) colon_symbols: bool,
    /// `name:` is a symbol constant (Ruby hash keys, Elixir keywords).
    pub(super) key_symbols: bool,
    pub(super) heredoc: Heredoc,
    /// `#[...]` attributes (Rust, PHP 8).
    pub(super) hash_attributes: bool,
    /// Lifetimes, `name!` macros, raw strings and identifiers, `.await`.
    pub(super) rust: bool,
    /// `#include`-style directives as the first token on a line.
    pub(super) preprocessor: bool,
    /// Raw strings `R"x(...)x"` and `1'000` digit separators.
    pub(super) cpp: bool,
    /// Names ending in `_t` are types.
    pub(super) c_types: bool,
    /// `#if`/`#available` directives and `#"raw"#` strings.
    pub(super) swift: bool,
    /// `\\` multi-line string lines.
    pub(super) zig: bool,
    /// Long brackets `[==[ ... ]==]` for strings and `--` comments.
    pub(super) lua: bool,
    /// Line-start block comments: Ruby `=begin`/`=end`, Perl `=pod`/`=cut`.
    pub(super) line_block: Option<(&'static str, &'static str)>,
    /// `<?php ... ?>` islands and `->` member access.
    pub(super) php: bool,
    /// `%hash` sigils and `$pkg::name`.
    pub(super) perl: bool,
    /// `name =` highlights `name` as a property (Nix, HCL).
    pub(super) assign_property: bool,
    /// SQL `$tag$ ... $tag$` strings.
    pub(super) dollar_quotes: bool,
    /// Cmdlet names with dashes, `$scope:name` variables, `-eq` operators.
    pub(super) powershell: bool,
}

impl Grammar {
    pub(super) const BASE: Grammar = Grammar {
        keywords: &[],
        types: &[],
        constants: &[],
        builtins: &[],
        fn_definers: &[],
        type_definers: &[],
        dash_operators: &[],
        line_comments: &["//"],
        block_comment: Some(("/*", "*/")),
        nested_comments: false,
        quotes: &[],
        prefixes: &[],
        char_literals: false,
        fold_case: false,
        caps: Caps::Plain,
        caps_constants: true,
        call_space: true,
        ident_start: &[],
        ident_extra: &[],
        ident_suffix: false,
        at: At::Plain,
        dollar: false,
        punct_vars: false,
        colon_symbols: false,
        key_symbols: false,
        heredoc: Heredoc::None,
        hash_attributes: false,
        rust: false,
        preprocessor: false,
        cpp: false,
        c_types: false,
        swift: false,
        zig: false,
        lua: false,
        line_block: None,
        php: false,
        perl: false,
        assign_property: false,
        dollar_quotes: false,
        powershell: false,
    };
}

pub(super) struct Code<'a> {
    g: &'static Grammar,
    src: &'a str,
    mode: Mode<'a>,
    /// Open brackets of a `#[...]` attribute.
    attribute: u32,
    /// Token for the next identifier after `fn`, `class`, ...
    define: Option<Token>,
    /// A single-line string escaped its line break.
    continued: bool,
    /// Heredoc delimiter (and whether it may be indented) for the next line.
    pending: Option<(&'a str, bool)>,
    /// PHP text outside `<?php ... ?>`.
    html: bool,
    /// Start of the current line; whether only whitespace precedes the cursor.
    start: usize,
    first: bool,
}

#[derive(Clone, Copy)]
enum Mode<'a> {
    Code,
    Comment(u32),
    /// Ruby `=begin` or Perl POD block.
    Block,
    /// Lua long bracket: level, and whether it is a comment.
    Long(usize, bool),
    /// Inside a string; the flag disables escapes (raw strings).
    Str(&'static Quote, bool),
    /// Rust `r#"..."#` and Swift `#"..."#`: `"` followed by this many `#`.
    Raw(usize),
    /// C++ `R"delim(...)delim"`.
    Delimited(&'a str),
    /// SQL `$tag$ ... $tag$`.
    Dollar(&'a str),
    Heredoc(&'a str, bool),
}

impl<'a> Code<'a> {
    pub(super) fn new(g: &'static Grammar, src: &'a str, segments: &[Range<usize>]) -> Self {
        // PHP files that open a PHP island start as HTML; snippets start as code.
        let html = g.php
            && segments.iter().any(|range| {
                src.get(range.clone())
                    .is_some_and(|text| text.contains("<?php") || text.contains("<?="))
            });
        Self {
            g,
            src,
            mode: Mode::Code,
            attribute: 0,
            define: None,
            continued: false,
            pending: None,
            html,
            start: 0,
            first: true,
        }
    }

    fn token(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let g = self.g;
        let src = self.src;
        let b = src.as_bytes();
        let c = b[i];
        if is_ws(c) {
            return skip_ws(b, i, end);
        }
        let word = c.is_ascii_alphabetic()
            || c == b'_'
            || g.ident_start.contains(&c)
            || (c >= 0x80 && char_at(src, i).is_some_and(ident_start));
        if !(word || c == b'.' || c == b'*') {
            self.define = None;
        }
        if i == self.start
            && let Some((open, _)) = g.line_block
            && self.block_opens(i, end, open)
        {
            self.mode = Mode::Block;
            out.push(i, end, Token::Comment);
            return end;
        }
        if g.preprocessor && c == b'#' && self.first {
            return self.directive(i, end, out);
        }
        if g.hash_attributes && c == b'#' {
            let open = if starts(b, i + 1, end, b"[") {
                i + 2
            } else if starts(b, i + 1, end, b"![") {
                i + 3
            } else {
                i
            };
            if open > i {
                out.push(i, open, Token::Attribute);
                self.attribute = 1;
                return open;
            }
        }
        if let Some(next) = self.comment_start(i, end, out) {
            return next;
        }
        if let Some(quote) = self.quote_at(i, end) {
            return self.open_string(i, i, quote, false, out);
        }
        if c == b'\'' && g.char_literals {
            return self.char_literal(i, i, end, out);
        }
        if c.is_ascii_digit()
            || (c == b'.' && i + 1 < end && b[i + 1].is_ascii_digit() && !self.follows_value(i))
        {
            let stop = number_end(b, i, end, g.cpp);
            out.push(i, stop, Token::Number);
            return stop;
        }
        if word {
            return self.word(i, end, out);
        }
        if let Some(next) = self.special(i, end, out) {
            return next;
        }
        self.punctuation(i, end, out)
    }

    fn word(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let g = self.g;
        let src = self.src;
        let b = src.as_bytes();
        let mut j = ident_end(src, i, end, g.ident_extra);
        if g.ident_suffix && j < end && matches!(b[j], b'?' | b'!') && !starts(b, j + 1, end, b"=")
        {
            j += 1;
        }
        let word = &src[i..j];
        if j < end
            && g.prefixes.binary_search(&word).is_ok()
            && let Some(next) = self.prefixed(i, j, end, word, out)
        {
            return next;
        }
        if g.rust {
            if word == "r"
                && starts(b, j, end, b"#")
                && char_at(src, j + 1).is_some_and(ident_start)
            {
                // Raw identifier such as r#type.
                return ident_end(src, j + 1, end, &[]);
            }
            if j < end && b[j] == b'!' && !starts(b, j + 1, end, b"=") {
                out.push(i, j + 1, Token::Macro);
                return j + 1;
            }
        }
        let mut cmdlet = false;
        if g.powershell {
            while j + 1 < end && b[j] == b'-' && b[j + 1].is_ascii_alphabetic() {
                j = ident_end(src, j + 1, end, &[]);
                cmdlet = true;
            }
        }
        if g.key_symbols
            && j < end
            && b[j] == b':'
            && !starts(b, j + 1, end, b":")
            && !(i > self.start && b[i - 1] == b':')
        {
            out.push(i, j + 1, Token::Constant);
            return j + 1;
        }
        let word = &src[i..j];
        if let Some(token) = self.classify(i, j, end, word, cmdlet) {
            out.push(i, j, token);
        }
        j
    }

    fn classify(
        &mut self,
        i: usize,
        j: usize,
        end: usize,
        word: &str,
        cmdlet: bool,
    ) -> Option<Token> {
        let g = self.g;
        let b = self.src.as_bytes();
        let fold = g.fold_case;
        let member = self.after_dot(i);
        let keyword = if member {
            g.rust && word == "await"
        } else {
            lookup(g.keywords, word, fold)
        };
        if keyword {
            if lookup(g.fn_definers, word, fold) {
                self.define = Some(Token::Function);
            } else if lookup(g.type_definers, word, fold) {
                self.define = Some(Token::Type);
            }
            return Some(Token::Keyword);
        }
        if let Some(token) = self.define {
            // `function M.name`, `def self.name`, `function M:name`: the last part is defined.
            let dotted = j + 1 < end
                && (b[j] == b'.'
                    || (b[j] == b':' && (b[j + 1].is_ascii_alphabetic() || b[j + 1] == b'_')));
            if !dotted {
                self.define = None;
                return Some(token);
            }
        }
        if g.assign_property && self.assigned(j, end) {
            return Some(Token::Property);
        }
        if !member {
            if lookup(g.constants, word, fold) {
                return Some(Token::Constant);
            }
            if lookup(g.types, word, fold) {
                return Some(Token::Type);
            }
        }
        if g.c_types && word.len() > 2 && word.ends_with("_t") {
            return Some(Token::Type);
        }
        let caps = g.caps_constants && is_all_caps(word);
        let upper = word.chars().next().is_some_and(char::is_uppercase);
        if cmdlet || self.called(j, end) {
            let constructor = upper && !caps && g.caps == Caps::Types;
            return Some(if constructor {
                Token::Type
            } else {
                Token::Function
            });
        }
        if !member && lookup(g.builtins, word, fold) {
            return Some(Token::Function);
        }
        if caps {
            return Some(Token::Constant);
        }
        (upper && g.caps != Caps::Plain).then_some(Token::Type)
    }

    /// String prefixes: `r"..."`, `b'x'`, `br#"..."#`, `L"..."`, `R"x(...)x"`.
    fn prefixed(
        &mut self,
        i: usize,
        j: usize,
        end: usize,
        word: &str,
        out: &mut Out,
    ) -> Option<usize> {
        let g = self.g;
        let b = self.src.as_bytes();
        let raw = word.contains(['r', 'R']);
        if g.rust && raw {
            let mut k = j;
            while k < end && b[k] == b'#' {
                k += 1;
            }
            if k < end && b[k] == b'"' {
                out.push(i, k + 1, Token::String);
                self.mode = Mode::Raw(k - j);
                return Some(k + 1);
            }
            return None;
        }
        if g.cpp && word.ends_with('R') {
            if b[j] != b'"' {
                return None;
            }
            let open = j + 1;
            let paren = (open..end.min(open + 17)).find(|&k| b[k] == b'(')?;
            let delimiter = &b[open..paren];
            if delimiter
                .iter()
                .any(|&c| !c.is_ascii_graphic() || matches!(c, b')' | b'\\' | b'"'))
            {
                return None;
            }
            out.push(i, paren + 1, Token::String);
            self.mode = Mode::Delimited(&self.src[open..paren]);
            return Some(paren + 1);
        }
        if b[j] == b'\'' && g.char_literals {
            return Some(self.char_literal(i, j, end, out));
        }
        let quote = self.quote_at(j, end)?;
        Some(self.open_string(i, j, quote, raw, out))
    }

    fn quote_at(&self, i: usize, end: usize) -> Option<&'static Quote> {
        let b = self.src.as_bytes();
        self.g
            .quotes
            .iter()
            .find(|quote| starts(b, i, end, quote.open.as_bytes()))
    }

    /// Open a string whose prefix starts at `start` and delimiter at `at`.
    fn open_string(
        &mut self,
        start: usize,
        at: usize,
        quote: &'static Quote,
        raw: bool,
        out: &mut Out,
    ) -> usize {
        let body = at + quote.open.len();
        out.push(start, body, Token::String);
        self.mode = Mode::Str(quote, raw);
        body
    }

    fn string(
        &mut self,
        i: usize,
        end: usize,
        quote: &'static Quote,
        raw: bool,
        out: &mut Out,
    ) -> usize {
        let b = self.src.as_bytes();
        let close = quote.close.as_bytes();
        let escape = if raw { 0 } else { quote.escape };
        let mut run = i;
        let mut j = i;
        while j < end {
            if escape != 0 && b[j] == escape {
                let stop = escape_end(b, j, end);
                if j + 1 >= end {
                    self.continued = true;
                }
                out.push(run, j, Token::String);
                out.push(j, stop, Token::Escape);
                j = stop;
                run = stop;
            } else if starts(b, j, end, close) {
                let doubled = j + 2 * close.len();
                if quote.doubled && starts(b, j + close.len(), end, close) {
                    out.push(run, j, Token::String);
                    out.push(j, doubled, Token::Escape);
                    j = doubled;
                    run = doubled;
                    continue;
                }
                let stop = j + close.len();
                out.push(run, stop, Token::String);
                self.mode = Mode::Code;
                return stop;
            } else {
                j += 1;
            }
        }
        out.push(run, end, Token::String);
        end
    }

    /// `'x'`, `'\n'`, and prefixed forms; Rust lifetimes; transposes (`a'`).
    fn char_literal(&mut self, start: usize, i: usize, end: usize, out: &mut Out) -> usize {
        let g = self.g;
        let src = self.src;
        let b = src.as_bytes();
        let next = i + 1;
        if g.rust && start == i && next < end && char_at(src, next).is_some_and(ident_start) {
            let stop = ident_end(src, next, end, &[]);
            if !(stop < end && b[stop] == b'\'') {
                out.push(i, stop, Token::Label);
                return stop;
            }
        }
        if !g.rust && start == i && self.follows_value(i) {
            out.push(i, next, Token::Operator);
            return next;
        }
        if next < end {
            let escaped = b[next] == b'\\';
            let body = if escaped {
                escape_end(b, next, end)
            } else if b[next] == b'\'' {
                next
            } else {
                next + char_len(b, next)
            };
            if body < end && b[body] == b'\'' {
                if escaped {
                    out.push(start, next, Token::String);
                    out.push(next, body, Token::Escape);
                    out.push(body, body + 1, Token::String);
                } else {
                    out.push(start, body + 1, Token::String);
                }
                return body + 1;
            }
            // Multi-character constants such as 'abcd'.
            if let Some(close) = find(b, next, end.min(next + 16), b"'") {
                out.push(start, close + 1, Token::String);
                return close + 1;
            }
        }
        out.push(start, next, Token::Operator);
        next
    }

    fn comment_start(&mut self, i: usize, end: usize, out: &mut Out) -> Option<usize> {
        let g = self.g;
        let b = self.src.as_bytes();
        if let Some((open, _)) = g.block_comment
            && starts(b, i, end, open.as_bytes())
        {
            let stop = i + open.len();
            out.push(i, stop, Token::Comment);
            self.mode = Mode::Comment(1);
            return Some(stop);
        }
        for marker in g.line_comments {
            if !starts(b, i, end, marker.as_bytes()) {
                continue;
            }
            if g.lua
                && let Some(level) = long_level(b, i + 2, end)
            {
                let stop = i + level + 4;
                out.push(i, stop, Token::Comment);
                self.mode = Mode::Long(level, true);
                return Some(stop);
            }
            out.push(i, end, Token::Comment);
            return Some(end);
        }
        None
    }

    /// Whether a comment starts at `i` (to end operator runs such as `=/*`).
    fn at_comment(&self, i: usize, end: usize) -> bool {
        let b = self.src.as_bytes();
        let g = self.g;
        g.block_comment
            .is_some_and(|(open, _)| starts(b, i, end, open.as_bytes()))
            || g.line_comments
                .iter()
                .any(|marker| starts(b, i, end, marker.as_bytes()))
    }

    fn comment(&mut self, i: usize, end: usize, mut depth: u32, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let Some((open, close)) = self.g.block_comment else {
            out.push(i, end, Token::Comment);
            return end;
        };
        let mut j = i;
        while j < end {
            if starts(b, j, end, close.as_bytes()) {
                j += close.len();
                depth -= 1;
                if depth == 0 {
                    out.push(i, j, Token::Comment);
                    self.mode = Mode::Code;
                    return j;
                }
            } else if self.g.nested_comments && starts(b, j, end, open.as_bytes()) {
                j += open.len();
                depth = depth.saturating_add(1);
            } else {
                j += 1;
            }
        }
        out.push(i, end, Token::Comment);
        self.mode = Mode::Comment(depth);
        end
    }

    fn block_opens(&self, i: usize, end: usize, open: &str) -> bool {
        let b = self.src.as_bytes();
        let k = i + open.len();
        starts(b, i, end, open.as_bytes())
            && if open == "=" {
                k < end && b[k].is_ascii_alphabetic()
            } else {
                k == end || is_ws(b[k])
            }
    }

    fn block_line(&mut self, start: usize, end: usize, out: &mut Out) -> usize {
        if let Some((_, close)) = self.g.line_block
            && self.block_opens(start, end, close)
        {
            self.mode = Mode::Code;
        }
        out.push(start, end, Token::Comment);
        end
    }

    fn heredoc_line(
        &mut self,
        start: usize,
        end: usize,
        delimiter: &str,
        indented: bool,
        out: &mut Out,
    ) -> usize {
        let b = self.src.as_bytes();
        let k = skip_ws(b, start, end);
        let at = if indented { k } else { start };
        let stop = at + delimiter.len();
        if starts(b, at, end, delimiter.as_bytes())
            && !(stop < end && (b[stop].is_ascii_alphanumeric() || b[stop] == b'_'))
        {
            out.push(at, stop, Token::Label);
            self.mode = Mode::Code;
            return stop;
        }
        out.push(k, end, Token::String);
        end
    }

    /// C preprocessor directive: `#` and the directive word; `<file>` after include.
    fn directive(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let src = self.src;
        let b = src.as_bytes();
        let name = skip_ws(b, i + 1, end);
        let stop = ident_end(src, name, end, &[]);
        out.push(i, stop, Token::Macro);
        if matches!(&src[name..stop], "include" | "include_next" | "import") {
            let k = skip_ws(b, stop, end);
            if starts(b, k, end, b"<")
                && let Some(close) = find(b, k + 1, end, b">")
            {
                out.push(k, close + 1, Token::String);
                return close + 1;
            }
        }
        stop
    }

    /// The body of a `#[...]` attribute; strings inside keep their own color.
    fn attribute(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        if is_ws(b[i]) {
            return skip_ws(b, i, end);
        }
        let mut j = i;
        while j < end {
            match b[j] {
                b'"' => break,
                b'[' => self.attribute = self.attribute.saturating_add(1),
                b']' => {
                    self.attribute -= 1;
                    if self.attribute == 0 {
                        j += 1;
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        out.push(i, j, Token::Attribute);
        if j < end
            && b[j] == b'"'
            && let Some(quote) = self.quote_at(j, end)
        {
            return self.open_string(j, j, quote, false, out);
        }
        j
    }

    fn html_text(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let Some(k) = find(b, i, end, b"<?") else {
            return end;
        };
        let mut stop = k + 2;
        if stop + 3 <= end && b[stop..stop + 3].eq_ignore_ascii_case(b"php") {
            stop += 3;
        } else if starts(b, stop, end, b"=") {
            stop += 1;
        }
        out.push(k, stop, Token::Macro);
        self.html = false;
        stop
    }

    /// Sigils, symbols, heredocs and other language-specific punctuation.
    fn special(&mut self, i: usize, end: usize, out: &mut Out) -> Option<usize> {
        let g = self.g;
        let src = self.src;
        let b = src.as_bytes();
        let next = if i + 1 < end { b[i + 1] } else { 0 };
        let name_follows = || i + 1 < end && char_at(src, i + 1).is_some_and(ident_start);
        match b[i] {
            b'#' if g.swift => {
                let mut k = i;
                while k < end && b[k] == b'#' {
                    k += 1;
                }
                if k < end && b[k] == b'"' {
                    out.push(i, k + 1, Token::String);
                    self.mode = Mode::Raw(k - i);
                    return Some(k + 1);
                }
                // Consume the whole run so that long `###` lines stay linear.
                let (token, stop) = if k == i + 1 && name_follows() {
                    (Token::Macro, ident_end(src, i + 1, end, &[]))
                } else {
                    (Token::Operator, k)
                };
                out.push(i, stop, token);
                Some(stop)
            }
            b'@' => self.at(i, end, out),
            b'$' if g.dollar => self.dollar(i, end, out),
            b':' if g.colon_symbols
                && (next.is_ascii_alphabetic() || next == b'_')
                && !self.follows_value(i)
                && !(i > self.start && b[i - 1] == b':') =>
            {
                let mut stop = ident_end(src, i + 1, end, g.ident_extra);
                if g.ident_suffix && stop < end && matches!(b[stop], b'?' | b'!') {
                    stop += 1;
                }
                out.push(i, stop, Token::Constant);
                Some(stop)
            }
            b'%' if g.perl
                && (next.is_ascii_alphabetic() || next == b'_')
                && (i == self.start
                    || matches!(b[i - 1], b' ' | b'\t' | b'(' | b',' | b'=' | b'{' | b'\\')) =>
            {
                let stop = ident_end(src, i + 1, end, &[]);
                out.push(i, stop, Token::Variable);
                Some(stop)
            }
            b'\\' if g.zig && next == b'\\' => {
                out.push(i, end, Token::String);
                Some(end)
            }
            b'<' if g.heredoc != Heredoc::None && next == b'<' => self.heredoc(i, end, out),
            b'?' if g.php && next == b'>' => {
                out.push(i, i + 2, Token::Macro);
                self.html = true;
                Some(i + 2)
            }
            b'-' if g.powershell && next.is_ascii_alphabetic() && !self.follows_value(i) => {
                let stop = ident_end(src, i + 1, end, &[]);
                if lookup(g.dash_operators, &src[i + 1..stop], true) {
                    out.push(i, stop, Token::Operator);
                }
                Some(stop)
            }
            b'[' if g.lua => {
                let level = long_level(b, i, end)?;
                let stop = i + level + 2;
                out.push(i, stop, Token::String);
                self.mode = Mode::Long(level, false);
                Some(stop)
            }
            _ => None,
        }
    }

    fn at(&mut self, i: usize, end: usize, out: &mut Out) -> Option<usize> {
        let g = self.g;
        let src = self.src;
        let b = src.as_bytes();
        let named = |k: usize| k < end && char_at(src, k).is_some_and(ident_start);
        let (token, stop) = match g.at {
            At::Attribute | At::LineAttribute
                if named(i + 1) && (g.at == At::Attribute || self.first) =>
            {
                let mut stop = ident_end(src, i + 1, end, g.ident_extra);
                while stop < end && b[stop] == b'.' && named(stop + 1) {
                    stop = ident_end(src, stop + 1, end, g.ident_extra);
                }
                (Token::Attribute, stop)
            }
            At::Macro if named(i + 1) => (Token::Macro, ident_end(src, i + 1, end, &[])),
            At::Variable => {
                let k = if starts(b, i + 1, end, b"@") {
                    i + 2
                } else {
                    i + 1
                };
                if !named(k) {
                    return None;
                }
                (Token::Variable, ident_end(src, k, end, g.ident_extra))
            }
            _ => return None,
        };
        out.push(i, stop, token);
        Some(stop)
    }

    fn dollar(&mut self, i: usize, end: usize, out: &mut Out) -> Option<usize> {
        let g = self.g;
        let src = self.src;
        let b = src.as_bytes();
        let k = i + 1;
        if k >= end {
            return None;
        }
        let named = |k: usize| k < end && char_at(src, k).is_some_and(ident_start);
        if g.dollar_quotes {
            let tag = ident_end(src, k, end, &[]);
            if tag < end && b[tag] == b'$' && tag - k <= 64 && !b[k].is_ascii_digit() {
                let stop = tag + 1;
                out.push(i, stop, Token::String);
                self.mode = Mode::Dollar(&src[i..stop]);
                return Some(stop);
            }
        }
        let stop = if b[k] == b'{' {
            find(b, k + 1, end, b"}").map_or(end, |close| close + 1)
        } else if b[k].is_ascii_digit() {
            let mut stop = k;
            while stop < end && b[stop].is_ascii_digit() {
                stop += 1;
            }
            stop
        } else if named(k) || (g.perl && b[k] == b'#' && named(k + 1)) {
            let from = if b[k] == b'#' { k + 1 } else { k };
            let mut stop = ident_end(src, from, end, &[]);
            loop {
                if g.powershell && starts(b, stop, end, b":") && named(stop + 1) {
                    stop = ident_end(src, stop + 1, end, &[]);
                } else if g.perl && starts(b, stop, end, b"::") && named(stop + 2) {
                    stop = ident_end(src, stop + 2, end, &[]);
                } else {
                    break;
                }
            }
            stop
        } else if g.punct_vars
            && b[k].is_ascii_punctuation()
            && !matches!(
                b[k],
                b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'"' | b'\'' | b',' | b';'
            )
        {
            k + 1
        } else {
            return None;
        };
        out.push(i, stop, Token::Variable);
        Some(stop)
    }

    fn heredoc(&mut self, i: usize, end: usize, out: &mut Out) -> Option<usize> {
        let g = self.g;
        let src = self.src;
        let b = src.as_bytes();
        let mut k = i + 2;
        let mut indented = g.heredoc == Heredoc::Php;
        if indented {
            if !starts(b, k, end, b"<") {
                return None;
            }
            k = skip_ws(b, k + 1, end);
        } else if k < end && matches!(b[k], b'~' | b'-') {
            indented = true;
            k += 1;
        }
        let operator = k;
        let quote = (k < end && matches!(b[k], b'"' | b'\'')).then(|| b[k]);
        let name = k + usize::from(quote.is_some());
        let name_end = ident_end(src, name, end, &[]);
        if name_end == name || name_end - name > 64 {
            return None;
        }
        // Bare delimiters must look like constants so that `a <<b` stays a shift.
        if quote.is_none() && g.heredoc == Heredoc::Angle && !b[name].is_ascii_uppercase() {
            return None;
        }
        let mut stop = name_end;
        if let Some(quote) = quote {
            if !starts(b, stop, end, &[quote]) {
                return None;
            }
            stop += 1;
        }
        out.push(i, operator, Token::Operator);
        out.push(operator, stop, Token::Label);
        if self.pending.is_none() {
            self.pending = Some((&src[name..name_end], indented));
        }
        Some(stop)
    }

    fn punctuation(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let c = b[i];
        let next = if i + 1 < end { b[i + 1] } else { 0 };
        let (token, stop) = match c {
            b'.' if next == b'.' => {
                let third = i + 2 < end && matches!(b[i + 2], b'.' | b'=' | b'<');
                (Token::Operator, i + 2 + usize::from(third))
            }
            b':' if matches!(next, b':' | b'=') => (Token::Operator, i + 2),
            b'(' | b')' | b'[' | b']' | b'{' | b'}' | b',' | b';' | b'.' | b':' => {
                (Token::Punctuation, i + 1)
            }
            _ if is_operator(c) => {
                let mut stop = i + 1;
                while stop < end && is_operator(b[stop]) && !self.at_comment(stop, end) {
                    stop += 1;
                }
                (Token::Operator, stop)
            }
            b'#' | b'@' | b'$' | b'\\' | b'`' => (Token::Operator, i + 1),
            _ => return i + char_len(b, i),
        };
        out.push(i, stop, token);
        stop
    }

    fn long(&mut self, i: usize, end: usize, level: usize, comment: bool, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let token = if comment {
            Token::Comment
        } else {
            Token::String
        };
        let mut j = i;
        while let Some(k) = find(b, j, end, b"]") {
            let mut e = k + 1;
            while e < end && b[e] == b'=' {
                e += 1;
            }
            if e - k - 1 == level && e < end && b[e] == b']' {
                out.push(i, e + 1, token);
                self.mode = Mode::Code;
                return e + 1;
            }
            j = k + 1;
        }
        out.push(i, end, token);
        end
    }

    fn raw(&mut self, i: usize, end: usize, hashes: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let mut j = i;
        while let Some(k) = find(b, j, end, b"\"") {
            let mut e = k + 1;
            while e < end && e - k - 1 < hashes && b[e] == b'#' {
                e += 1;
            }
            if e - k - 1 == hashes {
                out.push(i, e, Token::String);
                self.mode = Mode::Code;
                return e;
            }
            j = k + 1;
        }
        out.push(i, end, Token::String);
        end
    }

    fn delimited(&mut self, i: usize, end: usize, delimiter: &str, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let mut j = i;
        while let Some(k) = find(b, j, end, b")") {
            let quote = k + 1 + delimiter.len();
            if starts(b, k + 1, end, delimiter.as_bytes()) && starts(b, quote, end, b"\"") {
                out.push(i, quote + 1, Token::String);
                self.mode = Mode::Code;
                return quote + 1;
            }
            j = k + 1;
        }
        out.push(i, end, Token::String);
        end
    }

    fn dollar_quoted(&mut self, i: usize, end: usize, tag: &str, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        match find(b, i, end, tag.as_bytes()) {
            Some(k) => {
                let stop = k + tag.len();
                out.push(i, stop, Token::String);
                self.mode = Mode::Code;
                stop
            }
            None => {
                out.push(i, end, Token::String);
                end
            }
        }
    }

    /// The previous character ends a value: `a'` is a transpose, `x.5` a member.
    fn follows_value(&self, i: usize) -> bool {
        let b = self.src.as_bytes();
        i > self.start && {
            let p = b[i - 1];
            p.is_ascii_alphanumeric()
                || p >= 0x80
                || matches!(p, b'_' | b')' | b']' | b'}' | b'\'' | b'"')
        }
    }

    /// Member access (`x.name`, PHP `$x->name`): keywords do not apply.
    fn after_dot(&self, i: usize) -> bool {
        let b = self.src.as_bytes();
        let from = i - self.start;
        let arrow = self.g.php && from >= 2 && &b[i - 2..i] == b"->";
        arrow || (from >= 1 && b[i - 1] == b'.' && !(from >= 2 && b[i - 2] == b'.'))
    }

    fn called(&self, j: usize, end: usize) -> bool {
        let b = self.src.as_bytes();
        let k = if self.g.call_space {
            skip_ws(b, j, end)
        } else {
            j
        };
        k < end && b[k] == b'('
    }

    fn assigned(&self, j: usize, end: usize) -> bool {
        let b = self.src.as_bytes();
        let k = skip_ws(b, j, end);
        k < end && b[k] == b'=' && !(k + 1 < end && matches!(b[k + 1], b'=' | b'>' | b'~'))
    }

    fn end_line(&mut self) {
        if let Mode::Str(quote, _) = self.mode
            && !quote.multiline
            && !self.continued
        {
            self.mode = Mode::Code;
        }
        self.continued = false;
        if let Some((delimiter, indented)) = self.pending.take()
            && matches!(self.mode, Mode::Code)
        {
            self.mode = Mode::Heredoc(delimiter, indented);
        }
        self.define = None;
    }
}

impl Lex for Code<'_> {
    fn line(&mut self, start: usize, end: usize, out: &mut Out) {
        self.start = start;
        self.first = true;
        let b = self.src.as_bytes();
        let mut i = match self.mode {
            Mode::Block => self.block_line(start, end, out),
            Mode::Heredoc(delimiter, indented) => {
                self.heredoc_line(start, end, delimiter, indented, out)
            }
            _ => start,
        };
        while i < end {
            let next = match self.mode {
                Mode::Code if self.html => self.html_text(i, end, out),
                Mode::Code if self.attribute > 0 => self.attribute(i, end, out),
                Mode::Code => self.token(i, end, out),
                Mode::Comment(depth) => self.comment(i, end, depth, out),
                Mode::Long(level, comment) => self.long(i, end, level, comment, out),
                Mode::Str(quote, raw) => self.string(i, end, quote, raw, out),
                Mode::Raw(hashes) => self.raw(i, end, hashes, out),
                Mode::Delimited(delimiter) => self.delimited(i, end, delimiter, out),
                Mode::Dollar(tag) => self.dollar_quoted(i, end, tag, out),
                Mode::Block | Mode::Heredoc(..) => end,
            };
            if !is_ws(b[i]) {
                self.first = false;
            }
            // Every step consumes at least one character.
            i = next.max(i + char_len(b, i)).min(end);
        }
        self.end_line();
    }
}

fn is_operator(c: u8) -> bool {
    matches!(
        c,
        b'+' | b'-'
            | b'*'
            | b'/'
            | b'='
            | b'<'
            | b'>'
            | b'!'
            | b'&'
            | b'|'
            | b'^'
            | b'%'
            | b'~'
            | b'?'
    )
}

/// Level of a Lua long bracket `[==[` at `i`.
fn long_level(b: &[u8], i: usize, end: usize) -> Option<usize> {
    if !starts(b, i, end, b"[") {
        return None;
    }
    let mut k = i + 1;
    while k < end && b[k] == b'=' {
        k += 1;
    }
    (k < end && b[k] == b'[').then_some(k - i - 1)
}
