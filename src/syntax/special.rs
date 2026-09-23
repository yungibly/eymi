//! Lexers for data formats and line-oriented text: JSON, YAML, TOML, INI,
//! unified diffs, and Markdown.
use super::{
    Lex, Out, Token, char_len, escape_end, find, ident_end, is_ws,
    languages::{INI_CONSTANTS, YAML_CONSTANTS},
    lookup, number_end, skip_ws, starts, string_spans,
};

/// JSON, including JSONC/JSON5 comments, single quotes and bare keys.
pub(super) struct Json<'a> {
    src: &'a str,
    comment: bool,
}

impl<'a> Json<'a> {
    pub(super) fn new(src: &'a str) -> Self {
        Self {
            src,
            comment: false,
        }
    }

    /// A string followed by `:` is an object key.
    fn string(&self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let quote = b[i];
        let mut j = i + 1;
        while j < end && b[j] != quote {
            j = if b[j] == b'\\' {
                escape_end(b, j, end)
            } else {
                j + 1
            };
        }
        let stop = (j + 1).min(end);
        let k = skip_ws(b, stop, end);
        if k < end && b[k] == b':' {
            out.push(i, stop, Token::Property);
        } else {
            string_spans(out, b, i, stop, b'\\');
        }
        stop
    }
}

impl Lex for Json<'_> {
    fn line(&mut self, start: usize, end: usize, out: &mut Out) {
        let src = self.src;
        let b = src.as_bytes();
        let mut i = start;
        while i < end {
            if self.comment {
                let stop = find(b, i, end, b"*/").map_or(end, |k| {
                    self.comment = false;
                    k + 2
                });
                out.push(i, stop, Token::Comment);
                i = stop;
                continue;
            }
            let c = b[i];
            let next = if i + 1 < end { b[i + 1] } else { 0 };
            i = match c {
                _ if is_ws(c) => skip_ws(b, i, end),
                b'/' if next == b'/' => {
                    out.push(i, end, Token::Comment);
                    end
                }
                b'/' if next == b'*' => {
                    out.push(i, i + 2, Token::Comment);
                    self.comment = true;
                    i + 2
                }
                b'"' | b'\'' => self.string(i, end, out),
                b'{' | b'}' | b'[' | b']' | b',' | b':' => {
                    out.push(i, i + 1, Token::Punctuation);
                    i + 1
                }
                b'0'..=b'9' | b'.' | b'-' | b'+' => {
                    let digit = if matches!(c, b'-' | b'+') { i + 1 } else { i };
                    let number = digit < end
                        && (b[digit].is_ascii_digit()
                            || (b[digit] == b'.'
                                && digit + 1 < end
                                && b[digit + 1].is_ascii_digit()));
                    let stop = if number {
                        number_end(b, digit, end, false)
                    } else {
                        i + 1
                    };
                    let token = if number {
                        Token::Number
                    } else {
                        Token::Operator
                    };
                    out.push(i, stop, token);
                    stop
                }
                _ if c.is_ascii_alphabetic() || c == b'_' || c == b'$' => {
                    let stop = ident_end(src, i, end, b"$");
                    let k = skip_ws(b, stop, end);
                    if matches!(
                        &src[i..stop],
                        "true" | "false" | "null" | "Infinity" | "NaN"
                    ) {
                        out.push(i, stop, Token::Constant);
                    } else if k < end && b[k] == b':' {
                        out.push(i, stop, Token::Property);
                    }
                    stop
                }
                _ => i + char_len(b, i),
            };
        }
    }
}

/// YAML: keys, scalars, anchors, tags, block scalars and flow collections.
pub(super) struct Yaml<'a> {
    src: &'a str,
    /// Quoted scalar continuing on the next line.
    quote: Option<u8>,
    /// Inside a block scalar introduced on a line with this indentation.
    block: Option<usize>,
    /// A block scalar indicator appeared on the current line.
    pending: bool,
    flow: u32,
}

impl<'a> Yaml<'a> {
    pub(super) fn new(src: &'a str) -> Self {
        Self {
            src,
            quote: None,
            block: None,
            pending: false,
            flow: 0,
        }
    }

    /// Whether `c` ends a plain scalar inside flow collections.
    fn flow_end(&self, c: u8) -> bool {
        self.flow > 0 && matches!(c, b',' | b'[' | b']' | b'{' | b'}')
    }

    /// A `:` followed by whitespace, the line end, or a flow terminator.
    fn indicator(&self, b: &[u8], k: usize, end: usize) -> bool {
        k < end && b[k] == b':' && (k + 1 == end || is_ws(b[k + 1]) || self.flow_end(b[k + 1]))
    }

    /// Scan a quoted scalar from `body`; `start` is where its span begins.
    fn quoted(&mut self, start: usize, body: usize, end: usize, quote: u8, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let escape = if quote == b'"' { b'\\' } else { 0 };
        let mut j = body;
        while j < end {
            if escape != 0 && b[j] == escape {
                j = escape_end(b, j, end);
            } else if b[j] == quote && quote == b'\'' && starts(b, j + 1, end, b"'") {
                j += 2;
            } else if b[j] == quote {
                let stop = j + 1;
                let key = self.quote.is_none() && self.indicator(b, skip_ws(b, stop, end), end);
                self.quote = None;
                if key {
                    out.push(start, stop, Token::Property);
                } else {
                    string_spans(out, b, start, stop, escape);
                }
                return stop;
            } else {
                j += 1;
            }
        }
        string_spans(out, b, start, end, escape);
        self.quote = Some(quote);
        end
    }

    /// A plain scalar: a key when followed by `: `, else maybe a constant or number.
    fn plain(&self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let mut j = i;
        let mut last = i;
        while j < end {
            let c = b[j];
            if self.indicator(b, j, end)
                || (c == b'#' && j > i && is_ws(b[j - 1]))
                || self.flow_end(c)
            {
                break;
            }
            if is_ws(c) {
                j += 1;
            } else {
                j += char_len(b, j);
                last = j;
            }
        }
        let text = &self.src[i..last];
        if j < end && b[j] == b':' {
            out.push(i, last, Token::Property);
        } else if lookup(YAML_CONSTANTS, text, true) {
            out.push(i, last, Token::Constant);
        } else if is_number(text) {
            out.push(i, last, Token::Number);
        }
        j
    }

    /// `|`, `>-`, `|2+`: a block scalar starts on the next line.
    fn block_indicator(&mut self, i: usize, end: usize, out: &mut Out) -> Option<usize> {
        let b = self.src.as_bytes();
        let mut k = i + 1;
        while k < end && matches!(b[k], b'+' | b'-' | b'0'..=b'9') {
            k += 1;
        }
        if k < end && !is_ws(b[k]) {
            return None;
        }
        out.push(i, k, Token::Operator);
        self.pending = true;
        Some(k)
    }
}

impl Lex for Yaml<'_> {
    fn line(&mut self, start: usize, end: usize, out: &mut Out) {
        let b = self.src.as_bytes();
        let content = skip_ws(b, start, end);
        let indent = content - start;
        if let Some(parent) = self.block {
            if content == end || indent > parent {
                out.push(content, end, Token::String);
                return;
            }
            self.block = None;
        }
        let mut i = start;
        let marker =
            |text: &[u8]| starts(b, start, end, text) && (start + 3 == end || is_ws(b[start + 3]));
        if let Some(quote) = self.quote {
            i = self.quoted(start, start, end, quote, out);
        } else if self.flow == 0 && (marker(b"---") || marker(b"...")) {
            out.push(start, start + 3, Token::Punctuation);
            i = start + 3;
        } else if self.flow == 0 && starts(b, start, end, b"%") {
            out.push(start, end, Token::Keyword);
            return;
        }
        let mut item = true;
        while i < end {
            let c = b[i];
            if is_ws(c) {
                i = skip_ws(b, i, end);
                continue;
            }
            let spaced = i + 1 == end || is_ws(b[i + 1]);
            let named = |from: usize| {
                let mut k = from;
                while k < end && !is_ws(b[k]) && !self.flow_end(b[k]) {
                    k += 1;
                }
                k
            };
            let next = match c {
                b'#' if i == start || is_ws(b[i - 1]) => {
                    out.push(i, end, Token::Comment);
                    end
                }
                b'-' | b'?' if item && spaced => {
                    let token = if c == b'-' {
                        Token::Punctuation
                    } else {
                        Token::Operator
                    };
                    out.push(i, i + 1, token);
                    i + 1
                }
                b':' if self.indicator(b, i, end) => {
                    out.push(i, i + 1, Token::Punctuation);
                    item = false;
                    i + 1
                }
                b'"' | b'\'' => {
                    item = false;
                    self.quoted(i, i + 1, end, c, out)
                }
                b'&' | b'*' if !spaced => {
                    let stop = named(i + 1);
                    out.push(i, stop, Token::Label);
                    stop
                }
                b'!' => {
                    let stop = named(i + 1);
                    out.push(i, stop, Token::Type);
                    stop
                }
                b'[' | b'{' => {
                    out.push(i, i + 1, Token::Punctuation);
                    self.flow = self.flow.saturating_add(1);
                    item = true;
                    i + 1
                }
                b']' | b'}' => {
                    out.push(i, i + 1, Token::Punctuation);
                    self.flow = self.flow.saturating_sub(1);
                    item = false;
                    i + 1
                }
                b',' if self.flow > 0 => {
                    out.push(i, i + 1, Token::Punctuation);
                    item = true;
                    i + 1
                }
                _ => {
                    let indicator = if matches!(c, b'|' | b'>') && self.flow == 0 {
                        self.block_indicator(i, end, out)
                    } else {
                        None
                    };
                    item = false;
                    match indicator {
                        Some(stop) => stop,
                        None => self.plain(i, end, out),
                    }
                }
            };
            i = next.max(i + char_len(b, i));
        }
        if self.pending {
            self.pending = false;
            self.block = Some(indent);
        }
    }
}

/// TOML: headers, dotted keys, strings, numbers, dates, arrays and inline tables.
pub(super) struct Toml<'a> {
    src: &'a str,
    /// Open multi-line string: its quote character.
    multi: Option<u8>,
    /// Open value brackets; bit `n` of `tables` marks depth `n` as an inline table.
    depth: u32,
    tables: u64,
}

impl<'a> Toml<'a> {
    pub(super) fn new(src: &'a str) -> Self {
        Self {
            src,
            multi: None,
            depth: 0,
            tables: 0,
        }
    }

    fn open(&mut self, table: bool) {
        if self.depth < 64 {
            let bit = 1 << self.depth;
            if table {
                self.tables |= bit;
            } else {
                self.tables &= !bit;
            }
        }
        self.depth = self.depth.saturating_add(1);
    }

    fn in_table(&self) -> bool {
        (1..=64).contains(&self.depth) && self.tables & (1 << (self.depth - 1)) != 0
    }

    /// End of one bare or quoted key segment.
    fn key_part(&self, i: usize, end: usize) -> Option<usize> {
        let b = self.src.as_bytes();
        if i >= end {
            return None;
        }
        if matches!(b[i], b'"' | b'\'') {
            let quote = b[i];
            let mut j = i + 1;
            while j < end && b[j] != quote {
                j = if quote == b'"' && b[j] == b'\\' {
                    escape_end(b, j, end)
                } else {
                    j + 1
                };
            }
            return (j < end).then_some(j + 1);
        }
        let mut j = i;
        while j < end && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'_' | b'-')) {
            j += 1;
        }
        (j > i).then_some(j)
    }

    /// A dotted key followed by `=`; returns the position of the `=`.
    fn key(&self, i: usize, end: usize, out: &mut Out) -> Option<usize> {
        let b = self.src.as_bytes();
        let mut k = i;
        loop {
            k = skip_ws(b, self.key_part(k, end)?, end);
            if k < end && b[k] == b'.' {
                k = skip_ws(b, k + 1, end);
            } else {
                break;
            }
        }
        if !(k < end && b[k] == b'=') {
            return None;
        }
        let mut j = i;
        while j < k {
            let stop = self.key_part(j, end)?;
            out.push(j, stop, Token::Property);
            j = skip_ws(b, stop, end);
            if j < k && b[j] == b'.' {
                out.push(j, j + 1, Token::Punctuation);
                j = skip_ws(b, j + 1, end);
            }
        }
        Some(k)
    }

    /// `[table]` or `[[array.of.tables]]`.
    fn header(&self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let mut j = i + 1;
        while j < end {
            match b[j] {
                b'"' | b'\'' => j = self.key_part(j, end).unwrap_or(end),
                b']' => {
                    j += 1;
                    if starts(b, j, end, b"]") {
                        j += 1;
                    }
                    break;
                }
                _ => j += 1,
            }
        }
        out.push(i, j, Token::Heading);
        j
    }

    fn string(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let quote = b[i];
        if starts(b, i, end, &[quote; 3]) {
            out.push(i, i + 3, Token::String);
            self.multi = Some(quote);
            return self.multiline(i + 3, end, quote, out);
        }
        let escape = if quote == b'"' { b'\\' } else { 0 };
        let mut j = i + 1;
        while j < end && b[j] != quote {
            j = if escape != 0 && b[j] == escape {
                escape_end(b, j, end)
            } else {
                j + 1
            };
        }
        let stop = (j + 1).min(end);
        string_spans(out, b, i, stop, escape);
        stop
    }

    fn multiline(&mut self, i: usize, end: usize, quote: u8, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let escape = if quote == b'"' { b'\\' } else { 0 };
        let mut j = i;
        while j < end {
            if escape != 0 && b[j] == escape {
                j = escape_end(b, j, end);
            } else if starts(b, j, end, &[quote; 3]) {
                // Up to two further quotes belong to the content: `""""x"""""`.
                let mut stop = j + 3;
                while stop < end && stop < j + 5 && b[stop] == quote {
                    stop += 1;
                }
                string_spans(out, b, i, stop, escape);
                self.multi = None;
                return stop;
            } else {
                j += 1;
            }
        }
        string_spans(out, b, i, end, escape);
        end
    }

    /// Numbers, dates and times: `+1_000`, `0xff`, `6.02e23`, `1979-05-27T07:32:00Z`.
    fn number(&self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let mut j = i;
        while j < end
            && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'_' | b':' | b'.' | b'-' | b'+'))
        {
            j += 1;
        }
        let text = &self.src[i..j];
        let special = matches!(text.trim_start_matches(['+', '-']), "inf" | "nan");
        if special || text.bytes().any(|c| c.is_ascii_digit()) {
            out.push(i, j, Token::Number);
        }
        j
    }
}

impl Lex for Toml<'_> {
    fn line(&mut self, start: usize, end: usize, out: &mut Out) {
        let src = self.src;
        let b = src.as_bytes();
        let mut key = self.multi.is_none() && (self.depth == 0 || self.in_table());
        let mut i = match self.multi {
            Some(quote) => self.multiline(start, end, quote, out),
            None => start,
        };
        if key && self.depth == 0 {
            let k = skip_ws(b, start, end);
            if k < end && b[k] == b'[' {
                i = self.header(k, end, out);
                key = false;
            }
        }
        while i < end {
            let c = b[i];
            if is_ws(c) {
                i = skip_ws(b, i, end);
                continue;
            }
            if c == b'#' {
                out.push(i, end, Token::Comment);
                break;
            }
            if key {
                key = false;
                if let Some(stop) = self.key(i, end, out) {
                    i = stop;
                    continue;
                }
            }
            let next = match c {
                b'"' | b'\'' => self.string(i, end, out),
                b'=' => {
                    out.push(i, i + 1, Token::Operator);
                    i + 1
                }
                b'[' | b'{' => {
                    out.push(i, i + 1, Token::Punctuation);
                    self.open(c == b'{');
                    key = c == b'{';
                    i + 1
                }
                b']' | b'}' => {
                    out.push(i, i + 1, Token::Punctuation);
                    self.depth = self.depth.saturating_sub(1);
                    i + 1
                }
                b',' | b'.' => {
                    out.push(i, i + 1, Token::Punctuation);
                    key = c == b',' && self.in_table();
                    i + 1
                }
                b'0'..=b'9' | b'+' | b'-' => self.number(i, end, out),
                _ if c.is_ascii_alphabetic() => {
                    let stop = ident_end(src, i, end, b"-");
                    match &src[i..stop] {
                        "true" | "false" => out.push(i, stop, Token::Constant),
                        "inf" | "nan" => out.push(i, stop, Token::Number),
                        _ => {}
                    }
                    stop
                }
                _ => i + char_len(b, i),
            };
            i = next.max(i + char_len(b, i));
        }
    }
}

/// INI-style configuration: sections, `key = value`/`key: value`, comments.
pub(super) struct Ini<'a> {
    src: &'a str,
}

impl<'a> Ini<'a> {
    pub(super) fn new(src: &'a str) -> Self {
        Self { src }
    }

    fn value(&self, i: usize, end: usize, out: &mut Out) {
        let src = self.src;
        let b = src.as_bytes();
        let i = skip_ws(b, i, end);
        // `;` and `#` start a comment at the value start or after whitespace.
        let comment = (i..end)
            .find(|&k| matches!(b[k], b';' | b'#') && (k == i || is_ws(b[k - 1])))
            .unwrap_or(end);
        let mut stop = comment;
        while stop > i && is_ws(b[stop - 1]) {
            stop -= 1;
        }
        let text = &src[i..stop];
        if lookup(INI_CONSTANTS, text, true) {
            out.push(i, stop, Token::Constant);
        } else if is_number(text) {
            out.push(i, stop, Token::Number);
        } else {
            let mut j = i;
            while j < stop {
                j = match b[j] {
                    b'"' | b'\'' => {
                        let close = find(b, j + 1, stop, &[b[j]]).map_or(stop, |k| k + 1);
                        out.push(j, close, Token::String);
                        close
                    }
                    // `${var}` and Python's `%(name)s` interpolation.
                    b'$' if starts(b, j + 1, stop, b"{") => {
                        let close = find(b, j + 2, stop, b"}").map_or(stop, |k| k + 1);
                        out.push(j, close, Token::Variable);
                        close
                    }
                    b'%' if starts(b, j + 1, stop, b"(") => {
                        let close = find(b, j + 2, stop, b")").map_or(stop, |k| (k + 2).min(stop));
                        out.push(j, close, Token::Variable);
                        close
                    }
                    _ => j + char_len(b, j),
                };
            }
        }
        out.push(comment, end, Token::Comment);
    }
}

impl Lex for Ini<'_> {
    fn line(&mut self, start: usize, end: usize, out: &mut Out) {
        let b = self.src.as_bytes();
        let i = skip_ws(b, start, end);
        if i == end {
            return;
        }
        match b[i] {
            b';' | b'#' => out.push(i, end, Token::Comment),
            b'[' => {
                let stop = find(b, i, end, b"]").map_or(end, |k| k + 1);
                out.push(i, stop, Token::Heading);
                self.value(stop, end, out);
            }
            _ => {
                // `=` separates anywhere; `:` only before whitespace (not in URLs).
                let equals = find(b, i, end, b"=");
                let colon = (i..end).find(|&k| b[k] == b':' && (k + 1 == end || is_ws(b[k + 1])));
                let separator = match (equals, colon) {
                    (Some(e), Some(c)) => Some(e.min(c)),
                    (e, c) => e.or(c),
                };
                let Some(k) = separator else {
                    self.value(i, end, out);
                    return;
                };
                let mut key = k;
                while key > i && is_ws(b[key - 1]) {
                    key -= 1;
                }
                out.push(i, key, Token::Property);
                out.push(k, k + 1, Token::Operator);
                self.value(k + 1, end, out);
            }
        }
    }
}

/// Unified diffs. Hunk line counts disambiguate `---`/`+++` content lines
/// from file headers; without a hunk header, markers decide.
pub(super) struct Diff<'a> {
    src: &'a str,
    old: u32,
    new: u32,
}

impl<'a> Diff<'a> {
    pub(super) fn new(src: &'a str) -> Self {
        Self {
            src,
            old: 0,
            new: 0,
        }
    }
}

const DIFF_HEADERS: &[&str] = &[
    "--- ",
    "+++ ",
    "Binary files ",
    "copy from ",
    "copy to ",
    "deleted file mode ",
    "diff ",
    "dissimilarity index ",
    "index ",
    "new file mode ",
    "new mode ",
    "old mode ",
    "rename from ",
    "rename to ",
    "similarity index ",
];

impl Lex for Diff<'_> {
    fn line(&mut self, start: usize, end: usize, out: &mut Out) {
        let text = &self.src[start..end];
        let first = text.as_bytes().first().copied();
        if self.old > 0 || self.new > 0 {
            match first {
                Some(b'+') if self.new > 0 => {
                    self.new -= 1;
                    out.push(start, end, Token::Inserted);
                    return;
                }
                Some(b'-') if self.old > 0 => {
                    self.old -= 1;
                    out.push(start, end, Token::Deleted);
                    return;
                }
                Some(b' ') | None => {
                    self.old = self.old.saturating_sub(1);
                    self.new = self.new.saturating_sub(1);
                    return;
                }
                Some(b'\\') => {
                    out.push(start, end, Token::Comment);
                    return;
                }
                _ => {
                    self.old = 0;
                    self.new = 0;
                }
            }
        }
        if let Some((old, new, header)) = hunk(text) {
            self.old = old;
            self.new = new;
            out.push(start, start + header, Token::Keyword);
            return;
        }
        let token = if text.starts_with("@@") {
            Token::Keyword
        } else if text == "---" || text == "+++" || DIFF_HEADERS.iter().any(|h| text.starts_with(h))
        {
            Token::Heading
        } else {
            match first {
                Some(b'+') => Token::Inserted,
                Some(b'-') => Token::Deleted,
                Some(b'\\') => Token::Comment,
                _ => return,
            }
        };
        out.push(start, end, token);
    }
}

/// `@@ -a[,b] +c[,d] @@`: old and new line counts, and the header's length.
fn hunk(text: &str) -> Option<(u32, u32, usize)> {
    let rest = text.strip_prefix("@@ -")?;
    let (old, rest) = hunk_range(rest)?;
    let rest = rest.strip_prefix(" +")?;
    let (new, rest) = hunk_range(rest)?;
    let rest = rest.strip_prefix(" @@")?;
    Some((old, new, text.len() - rest.len()))
}

/// `start[,count]`; the count defaults to one.
fn hunk_range(text: &str) -> Option<(u32, &str)> {
    let digits = |t: &str| t.bytes().take_while(u8::is_ascii_digit).count();
    let n = digits(text);
    if n == 0 {
        return None;
    }
    let rest = &text[n..];
    let Some(count) = rest.strip_prefix(',') else {
        return Some((1, rest));
    };
    let m = digits(count);
    let value = count[..m].bytes().fold(0u32, |n, d| {
        n.saturating_mul(10).saturating_add(u32::from(d - b'0'))
    });
    (m > 0).then_some((value, &count[m..]))
}

/// Basic Markdown for ```` ```markdown ```` fences: headings, quotes, list
/// markers, code, link destinations, escapes and HTML comments.
pub(super) struct Markdown<'a> {
    src: &'a str,
    /// Open code fence: its character and length.
    fence: Option<(u8, usize)>,
    comment: bool,
}

impl<'a> Markdown<'a> {
    pub(super) fn new(src: &'a str) -> Self {
        Self {
            src,
            fence: None,
            comment: false,
        }
    }

    fn inline(&mut self, mut i: usize, end: usize, out: &mut Out) {
        let b = self.src.as_bytes();
        // Code spans: the position after which no closing run of each length
        // exists, so unmatched backticks cost linear time.
        let mut unmatched = [usize::MAX; 16];
        while i < end {
            let c = b[i];
            i = match c {
                b'`' => {
                    let run = count(b, i, end, b'`');
                    let from = i + run;
                    let slot = unmatched.get_mut(run - 1);
                    match slot {
                        Some(slot) if *slot > from => match closing_run(b, from, end, run) {
                            Some(k) => {
                                out.push(i, k + run, Token::String);
                                k + run
                            }
                            None => {
                                *slot = from;
                                from
                            }
                        },
                        _ => from,
                    }
                }
                b'\\' if i + 1 < end && b[i + 1].is_ascii_punctuation() => {
                    out.push(i, i + 2, Token::Escape);
                    i + 2
                }
                b'<' if starts(b, i, end, b"<!--") => match find(b, i + 4, end, b"-->") {
                    Some(k) => {
                        out.push(i, k + 3, Token::Comment);
                        k + 3
                    }
                    None => {
                        out.push(i, end, Token::Comment);
                        self.comment = true;
                        end
                    }
                },
                b'<' if starts(b, i, end, b"<http") || starts(b, i, end, b"<mailto:") => {
                    match find(b, i, end, b">") {
                        Some(k) => {
                            out.push(i, k + 1, Token::String);
                            k + 1
                        }
                        None => i + 1,
                    }
                }
                // Link destination after `](`, up to `)` or whitespace.
                b']' if starts(b, i + 1, end, b"(") => {
                    let mut j = i + 2;
                    while j < end && !is_ws(b[j]) && b[j] != b')' {
                        j += 1;
                    }
                    let stop = if j < end && b[j] == b')' { j + 1 } else { j };
                    out.push(i + 1, stop, Token::String);
                    stop
                }
                _ => i + char_len(b, i),
            };
        }
    }
}

impl Lex for Markdown<'_> {
    fn line(&mut self, start: usize, end: usize, out: &mut Out) {
        let b = self.src.as_bytes();
        let mut i = start;
        if self.comment {
            let Some(k) = find(b, i, end, b"-->") else {
                out.push(i, end, Token::Comment);
                return;
            };
            out.push(i, k + 3, Token::Comment);
            self.comment = false;
            self.inline(k + 3, end, out);
            return;
        }
        loop {
            let k = skip_ws(b, i, end);
            if k < end && b[k] == b'>' {
                out.push(k, k + 1, Token::Punctuation);
                i = k + 1;
            } else {
                break;
            }
        }
        let k = skip_ws(b, i, end);
        if let Some((fence, len)) = self.fence {
            let run = count(b, k, end, fence);
            if run >= len && skip_ws(b, k + run, end) == end {
                self.fence = None;
            }
            out.push(k, end, Token::String);
            return;
        }
        if k == end {
            return;
        }
        let c = b[k];
        let run = count(b, k, end, c);
        let fence = matches!(c, b'`' | b'~')
            && run >= 3
            && !(c == b'`' && find(b, k + run, end, b"`").is_some());
        if fence {
            self.fence = Some((c, run));
            out.push(k, end, Token::String);
            return;
        }
        if c == b'#' && run <= 6 && (k + run == end || is_ws(b[k + run])) {
            out.push(k, end, Token::Heading);
            return;
        }
        let rule = b[k..end].iter().all(|&x| x == c || is_ws(x));
        if rule && c == b'=' {
            out.push(k, end, Token::Heading);
            return;
        }
        if rule
            && matches!(c, b'-' | b'*' | b'_')
            && b[k..end].iter().filter(|&&x| x == c).count() >= 3
        {
            out.push(k, end, Token::Punctuation);
            return;
        }
        let mut i = k;
        if matches!(c, b'-' | b'*' | b'+') && (k + 1 == end || is_ws(b[k + 1])) {
            out.push(k, k + 1, Token::Keyword);
            i = k + 1;
        } else if c.is_ascii_digit() {
            let digits = count_digits(b, k, end);
            let m = k + digits;
            if digits <= 9
                && m < end
                && matches!(b[m], b'.' | b')')
                && (m + 1 == end || is_ws(b[m + 1]))
            {
                out.push(k, m + 1, Token::Keyword);
                i = m + 1;
            }
        }
        self.inline(i, end, out);
    }
}

fn count(b: &[u8], i: usize, end: usize, c: u8) -> usize {
    b[i.min(end)..end].iter().take_while(|&&x| x == c).count()
}

fn count_digits(b: &[u8], i: usize, end: usize) -> usize {
    b[i..end].iter().take_while(|x| x.is_ascii_digit()).count()
}

/// Start of the next backtick run of exactly `run` characters.
fn closing_run(b: &[u8], from: usize, end: usize, run: usize) -> Option<usize> {
    let mut j = from;
    while let Some(k) = find(b, j, end, b"`") {
        let length = count(b, k, end, b'`');
        if length == run {
            return Some(k);
        }
        j = k + length;
    }
    None
}

/// Integers, decimals and exponents with an optional sign and `_` separators,
/// radix-prefixed integers, and YAML's `.inf`/`.nan`.
fn is_number(text: &str) -> bool {
    fn digits(b: &[u8], k: &mut usize) -> usize {
        let from = *k;
        while *k < b.len() && (b[*k].is_ascii_digit() || b[*k] == b'_') {
            *k += 1;
        }
        *k - from
    }
    let t = text.strip_prefix(['+', '-']).unwrap_or(text);
    if t.eq_ignore_ascii_case(".inf") || t.eq_ignore_ascii_case(".nan") {
        return true;
    }
    let b = t.as_bytes();
    if b.len() > 2 && b[0] == b'0' && matches!(b[1] | 0x20, b'x' | b'o' | b'b') {
        return b[2..].iter().all(|c| c.is_ascii_hexdigit() || *c == b'_');
    }
    let mut k = 0;
    let mut count = digits(b, &mut k);
    if k < b.len() && b[k] == b'.' {
        k += 1;
        count += digits(b, &mut k);
    }
    if count == 0 || !b[..k].iter().any(u8::is_ascii_digit) {
        return false;
    }
    if k < b.len() && b[k] | 0x20 == b'e' {
        k += 1;
        if k < b.len() && matches!(b[k], b'+' | b'-') {
            k += 1;
        }
        if digits(b, &mut k) == 0 {
            return false;
        }
    }
    k == b.len()
}
