//! Lexers for markup and style sheets: HTML, XML, CSS and SCSS.
use super::{Lex, Out, Token, char_len, find, is_ws, number_end, skip_ws, starts, string_spans};

/// HTML and XML. In HTML, `<script>` and `<style>` contents stay plain.
pub(super) struct Markup<'a> {
    src: &'a str,
    html: bool,
    mode: Mode,
    kind: Kind,
    /// After `=` inside a tag: the next word is an unquoted value.
    value: bool,
    /// Raw-text element whose content follows the current tag.
    raw: Option<&'static [u8]>,
}

#[derive(Clone, Copy)]
enum Mode {
    Text,
    Comment,
    CData,
    Tag,
    /// Inside a quoted attribute value.
    Value(u8),
    /// Inside `<script>` or `<style>`, waiting for its closing tag.
    Raw(&'static [u8]),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Element,
    /// `<!DOCTYPE ...>`
    Declaration,
    /// `<?xml ...?>`
    Instruction,
}

const RAW_ELEMENTS: [&[u8]; 2] = [b"script", b"style"];

impl<'a> Markup<'a> {
    pub(super) fn new(src: &'a str, html: bool) -> Self {
        Self {
            src,
            html,
            mode: Mode::Text,
            kind: Kind::Element,
            value: false,
            raw: None,
        }
    }

    fn text(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        match b[i] {
            b'<' => self.open(i, end, out),
            b'&' => match entity_end(b, i, end) {
                Some(stop) => {
                    out.push(i, stop, Token::Escape);
                    stop
                }
                None => i + 1,
            },
            _ => (i + 1..end)
                .find(|&k| matches!(b[k], b'<' | b'&'))
                .unwrap_or(end),
        }
    }

    /// `<name`, `</name`, `<!DECL`, `<?target`, comments and CDATA sections.
    fn open(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        if starts(b, i, end, b"<!--") {
            out.push(i, i + 4, Token::Comment);
            self.mode = Mode::Comment;
            return i + 4;
        }
        if starts(b, i, end, b"<![CDATA[") {
            out.push(i, i + 9, Token::Keyword);
            self.mode = Mode::CData;
            return i + 9;
        }
        let next = if i + 1 < end { b[i + 1] } else { 0 };
        let (kind, name) = match next {
            b'!' => (Kind::Declaration, i + 2),
            b'?' => (Kind::Instruction, i + 2),
            b'/' => (Kind::Element, i + 2),
            _ => (Kind::Element, i + 1),
        };
        if !(name < end
            && (b[name].is_ascii_alphabetic() || matches!(b[name], b'_' | b':') || b[name] >= 0x80))
        {
            return i + 1;
        }
        let mut stop = name;
        while stop < end
            && (b[stop].is_ascii_alphanumeric()
                || matches!(b[stop], b'_' | b':' | b'.' | b'-')
                || b[stop] >= 0x80)
        {
            stop += 1;
        }
        match kind {
            Kind::Element => {
                out.push(i, name, Token::Punctuation);
                out.push(name, stop, Token::Tag);
            }
            Kind::Declaration => out.push(i, stop, Token::Keyword),
            Kind::Instruction => out.push(i, stop, Token::Macro),
        }
        self.kind = kind;
        self.mode = Mode::Tag;
        self.value = false;
        let tag = &b[name..stop];
        self.raw = (self.html && kind == Kind::Element && next != b'/')
            .then(|| {
                RAW_ELEMENTS
                    .into_iter()
                    .find(|raw| tag.eq_ignore_ascii_case(raw))
            })
            .flatten();
        stop
    }

    fn tag(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let c = b[i];
        if is_ws(c) {
            return skip_ws(b, i, end);
        }
        let next = if i + 1 < end { b[i + 1] } else { 0 };
        match c {
            b'>' => {
                out.push(i, i + 1, Token::Punctuation);
                self.close();
                i + 1
            }
            b'/' if next == b'>' => {
                out.push(i, i + 2, Token::Punctuation);
                self.raw = None;
                self.close();
                i + 2
            }
            b'?' if next == b'>' && self.kind == Kind::Instruction => {
                out.push(i, i + 2, Token::Macro);
                self.close();
                i + 2
            }
            b'=' => {
                out.push(i, i + 1, Token::Operator);
                self.value = true;
                i + 1
            }
            b'"' | b'\'' => {
                self.value = false;
                self.quoted(i, i + 1, end, c, out)
            }
            // An unclosed tag ends where the next one begins.
            b'<' => {
                self.mode = Mode::Text;
                self.text(i, end, out)
            }
            _ => {
                let mut stop = i + 1;
                while stop < end
                    && !is_ws(b[stop])
                    && !matches!(b[stop], b'>' | b'"' | b'\'' | b'=' | b'<')
                    && !starts(b, stop, end, b"/>")
                    && !starts(b, stop, end, b"?>")
                {
                    stop += 1;
                }
                let token = if self.value {
                    Some(Token::String)
                } else if self.kind == Kind::Declaration {
                    None
                } else {
                    Some(Token::Property)
                };
                if let Some(token) = token {
                    out.push(i, stop, token);
                }
                self.value = false;
                stop
            }
        }
    }

    fn close(&mut self) {
        self.value = false;
        self.mode = match self.raw.take() {
            Some(name) => Mode::Raw(name),
            None => Mode::Text,
        };
    }

    /// A quoted attribute value from `body`; it may continue on later lines.
    fn quoted(&mut self, start: usize, body: usize, end: usize, quote: u8, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let close = find(b, body, end, &[quote]);
        let stop = close.map_or(end, |k| k + 1);
        entities(out, b, start, stop, Token::String);
        self.mode = if close.is_some() {
            Mode::Tag
        } else {
            Mode::Value(quote)
        };
        stop
    }

    fn until(&mut self, i: usize, end: usize, close: &[u8], token: Token, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        match find(b, i, end, close) {
            Some(k) => {
                let stop = k + close.len();
                out.push(i, stop, token);
                self.mode = Mode::Text;
                stop
            }
            None => {
                out.push(i, end, token);
                end
            }
        }
    }

    fn cdata(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        match find(b, i, end, b"]]>") {
            Some(k) => {
                out.push(i, k, Token::String);
                out.push(k, k + 3, Token::Keyword);
                self.mode = Mode::Text;
                k + 3
            }
            None => {
                out.push(i, end, Token::String);
                end
            }
        }
    }

    fn raw_text(&mut self, i: usize, end: usize, name: &[u8], out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let mut j = i;
        while let Some(k) = find(b, j, end, b"</") {
            let tag = k + 2;
            if tag + name.len() <= end && b[tag..tag + name.len()].eq_ignore_ascii_case(name) {
                self.mode = Mode::Text;
                return if k > i { k } else { self.text(k, end, out) };
            }
            j = tag;
        }
        end
    }
}

impl Lex for Markup<'_> {
    fn line(&mut self, start: usize, end: usize, out: &mut Out) {
        let b = self.src.as_bytes();
        let mut i = start;
        while i < end {
            let next = match self.mode {
                Mode::Text => self.text(i, end, out),
                Mode::Comment => self.until(i, end, b"-->", Token::Comment, out),
                Mode::CData => self.cdata(i, end, out),
                Mode::Tag => self.tag(i, end, out),
                Mode::Value(quote) => self.quoted(i, i, end, quote, out),
                Mode::Raw(name) => self.raw_text(i, end, name, out),
            };
            i = next.max(i + char_len(b, i));
        }
    }
}

/// `&amp;`, `&#39;`, `&#x1F600;`.
fn entity_end(b: &[u8], i: usize, end: usize) -> Option<usize> {
    let semicolon = find(b, i + 1, end.min(i + 32), b";")?;
    let valid = match &b[i + 1..semicolon] {
        [b'#', b'x' | b'X', hex @ ..] => !hex.is_empty() && hex.iter().all(u8::is_ascii_hexdigit),
        [b'#', digits @ ..] => !digits.is_empty() && digits.iter().all(u8::is_ascii_digit),
        [first, rest @ ..] => {
            first.is_ascii_alphabetic() && rest.iter().all(u8::is_ascii_alphanumeric)
        }
        [] => false,
    };
    valid.then_some(semicolon + 1)
}

/// Emit `start..stop` as `token` with entities as Escape spans.
fn entities(out: &mut Out, b: &[u8], start: usize, stop: usize, token: Token) {
    let mut run = start;
    let mut j = start;
    while let Some(k) = find(b, j, stop, b"&") {
        match entity_end(b, k, stop) {
            Some(e) => {
                out.push(run, k, token);
                out.push(k, e, Token::Escape);
                run = e;
                j = e;
            }
            None => j = k + 1,
        }
    }
    out.push(run, stop, token);
}

/// CSS and SCSS. Inside blocks, a statement is a nested rule when `{` ends it
/// on the same line and a declaration otherwise.
pub(super) struct Css<'a> {
    src: &'a str,
    scss: bool,
    comment: bool,
    depth: u32,
    context: Context,
    /// At the start of a statement inside a block.
    fresh: bool,
    /// Inside the brackets of an attribute selector.
    bracket: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Selector,
    /// Declaration before its `:`.
    Property,
    Value,
    /// At-rule prelude such as `@media screen and (...)`.
    AtRule,
}

impl<'a> Css<'a> {
    pub(super) fn new(src: &'a str, scss: bool) -> Self {
        Self {
            src,
            scss,
            comment: false,
            depth: 0,
            context: Context::Selector,
            fresh: false,
            bracket: false,
        }
    }

    fn statement(&self, i: usize, end: usize) -> Context {
        if self.depth == 0 {
            return Context::Selector;
        }
        let b = self.src.as_bytes();
        let mut k = i;
        while k < end {
            match b[k] {
                b'{' => return Context::Selector,
                b';' | b'}' => return Context::Property,
                b'"' | b'\'' => {
                    k = find(b, k + 1, end, &[b[k]]).map_or(end, |q| q + 1);
                    continue;
                }
                _ => {}
            }
            k += 1;
        }
        Context::Property
    }

    fn token(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let c = b[i];
        if is_ws(c) {
            return skip_ws(b, i, end);
        }
        let next = if i + 1 < end { b[i + 1] } else { 0 };
        if c == b'/' && next == b'*' {
            out.push(i, i + 2, Token::Comment);
            self.comment = true;
            return i + 2;
        }
        if c == b'/' && next == b'/' && self.scss {
            out.push(i, end, Token::Comment);
            return end;
        }
        if self.fresh {
            self.fresh = false;
            self.context = self.statement(i, end);
        }
        let named = |k: usize| {
            k < end && (b[k].is_ascii_alphabetic() || matches!(b[k], b'_' | b'-') || b[k] >= 0x80)
        };
        let value = matches!(self.context, Context::Value | Context::AtRule);
        let signed = matches!(c, b'+' | b'-')
            && (next.is_ascii_digit()
                || (next == b'.' && i + 2 < end && b[i + 2].is_ascii_digit()));
        let (token, stop) = match c {
            b'"' | b'\'' => {
                let close = find(b, i + 1, end, &[c]).map_or(end, |k| k + 1);
                string_spans(out, b, i, close, b'\\');
                return close;
            }
            b'{' => {
                self.depth = self.depth.saturating_add(1);
                self.fresh = true;
                self.bracket = false;
                (Some(Token::Punctuation), i + 1)
            }
            b'}' => {
                self.depth = self.depth.saturating_sub(1);
                self.fresh = self.depth > 0;
                self.context = Context::Selector;
                (Some(Token::Punctuation), i + 1)
            }
            b';' => {
                self.fresh = self.depth > 0;
                self.context = Context::Selector;
                (Some(Token::Punctuation), i + 1)
            }
            b'@' if named(i + 1) => {
                self.context = Context::AtRule;
                (Some(Token::Keyword), ident_end(b, i + 1, end))
            }
            b'$' if self.scss && named(i + 1) => {
                if self.context == Context::Selector {
                    self.context = Context::Property;
                }
                (Some(Token::Variable), ident_end(b, i + 1, end))
            }
            b'-' if next == b'-' && named(i + 2) => (Some(Token::Variable), ident_end(b, i, end)),
            b'!' if named(i + 1) => (Some(Token::Keyword), ident_end(b, i + 1, end)),
            b'#' if next == b'{' => (Some(Token::Punctuation), i + 2),
            b'#' if value => (Some(Token::Number), ident_end(b, i + 1, end)),
            b'#' | b'.' | b'%' if self.context == Context::Selector && named(i + 1) => {
                (Some(Token::Type), ident_end(b, i + 1, end))
            }
            b':' if self.context == Context::Selector => {
                let name = if next == b':' { i + 2 } else { i + 1 };
                if named(name) {
                    (Some(Token::Attribute), ident_end(b, name, end))
                } else {
                    (Some(Token::Punctuation), i + 1)
                }
            }
            b':' => {
                if self.context == Context::Property {
                    self.context = Context::Value;
                }
                (Some(Token::Punctuation), i + 1)
            }
            b'[' if self.context == Context::Selector => {
                self.bracket = true;
                (Some(Token::Punctuation), i + 1)
            }
            b']' => {
                self.bracket = false;
                (Some(Token::Punctuation), i + 1)
            }
            b'(' | b')' | b',' => (Some(Token::Punctuation), i + 1),
            _ if c.is_ascii_digit()
                || (c == b'.' && next.is_ascii_digit())
                || (signed && value) =>
            {
                let from = if matches!(c, b'+' | b'-') { i + 1 } else { i };
                let mut stop = number_end(b, from, end, false);
                if stop < end && b[stop] == b'%' {
                    stop += 1;
                }
                (Some(Token::Number), stop)
            }
            // A lone `-` is subtraction; `-webkit-box` is a name.
            _ if named(i) && (c != b'-' || named(i + 1)) => return self.word(i, end, out),
            b'&' | b'>' | b'+' | b'~' | b'*' | b'=' | b'|' | b'^' | b'/' | b'<' | b'-' => {
                (Some(Token::Operator), i + 1)
            }
            _ => (None, i + char_len(b, i)),
        };
        if let Some(token) = token {
            out.push(i, stop, token);
        }
        stop
    }

    fn word(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let stop = ident_end(b, i, end);
        if stop < end && b[stop] == b'(' {
            out.push(i, stop, Token::Function);
            // Unquoted url(...) arguments are strings.
            let k = skip_ws(b, stop + 1, end);
            let url = b[i..stop].eq_ignore_ascii_case(b"url");
            if url && k < end && !matches!(b[k], b'"' | b'\'' | b')') {
                let close = find(b, k, end, b")").unwrap_or(end);
                out.push(stop, stop + 1, Token::Punctuation);
                out.push(k, close, Token::String);
                return close;
            }
            return stop;
        }
        let k = skip_ws(b, stop, end);
        let colon = k < end && b[k] == b':';
        let token = match self.context {
            Context::Selector if self.bracket => Some(Token::Property),
            Context::Selector => Some(Token::Tag),
            Context::Property | Context::AtRule => colon.then_some(Token::Property),
            Context::Value => None,
        };
        if let Some(token) = token {
            out.push(i, stop, token);
        }
        stop
    }
}

impl Lex for Css<'_> {
    fn line(&mut self, start: usize, end: usize, out: &mut Out) {
        let b = self.src.as_bytes();
        let mut i = start;
        while i < end {
            let next = if self.comment {
                match find(b, i, end, b"*/") {
                    Some(k) => {
                        self.comment = false;
                        out.push(i, k + 2, Token::Comment);
                        k + 2
                    }
                    None => {
                        out.push(i, end, Token::Comment);
                        end
                    }
                }
            } else {
                self.token(i, end, out)
            };
            i = next.max(i + char_len(b, i));
        }
    }
}

/// End of a CSS identifier: letters, digits, `-`, `_` and non-ASCII.
fn ident_end(b: &[u8], mut i: usize, end: usize) -> usize {
    while i < end && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'-' | b'_') || b[i] >= 0x80) {
        i += 1;
    }
    i
}
