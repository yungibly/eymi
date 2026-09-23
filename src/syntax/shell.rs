//! Shell scripts, and the Makefile and Dockerfile lexers that embed them.
use super::{
    Lex, Out, Token, char_len, find, is_ws,
    languages::{
        DOCKER_INSTRUCTIONS, MAKE_DIRECTIVES, MAKE_FUNCTIONS, MAKE_TARGETS, SHELL_BUILTINS,
        SHELL_KEYWORDS,
    },
    lookup, skip_ws, starts,
};

/// POSIX/Bash-style shell. Keywords and builtins count only in command position.
pub(super) struct Shell<'a> {
    src: &'a str,
    mode: Mode<'a>,
    /// The next word is a command name.
    command: bool,
    /// Arguments may be assignments (`export`, `local`, Dockerfile `ENV`).
    declaring: bool,
    /// Remaining words in which `in` is a keyword (after `for`, `case`, `select`).
    expect_in: u8,
    /// The next word names a function (after `function`).
    function: bool,
    /// Nesting of `$(` (set bit) and `(`.
    parens: u64,
    depth: u32,
    backtick: bool,
    /// The line ends with a backslash.
    continued: bool,
    pending: Option<Heredoc<'a>>,
    /// Makefile recipe: `$$` escapes `$`, and `$(VAR)`, `$<`, `$^` are make variables.
    make: bool,
    /// Dockerfile arguments that are not commands.
    arguments: bool,
    start: usize,
}

#[derive(Clone, Copy)]
struct Heredoc<'a> {
    delimiter: &'a str,
    /// `<<-` strips leading tabs.
    tabs: bool,
    /// Unquoted delimiters expand `$` references in the body.
    expand: bool,
}

#[derive(Clone, Copy)]
enum Mode<'a> {
    Normal,
    Single,
    Double,
    /// `$'...'` with backslash escapes.
    Ansi,
    Heredoc(Heredoc<'a>),
}

impl<'a> Shell<'a> {
    pub(super) fn new(src: &'a str) -> Self {
        Self {
            src,
            mode: Mode::Normal,
            command: true,
            declaring: false,
            expect_in: 0,
            function: false,
            parens: 0,
            depth: 0,
            backtick: false,
            continued: false,
            pending: None,
            make: false,
            arguments: false,
            start: 0,
        }
    }

    /// Start a new instruction or recipe; `arguments` disables command words.
    fn restart(&mut self, arguments: bool) {
        *self = Self {
            make: self.make,
            arguments,
            command: !arguments,
            declaring: arguments,
            ..Self::new(self.src)
        };
    }

    fn in_heredoc(&self) -> bool {
        matches!(self.mode, Mode::Heredoc(_))
    }

    /// Lex `start..end`, part of one line.
    fn region(&mut self, start: usize, end: usize, out: &mut Out) {
        let b = self.src.as_bytes();
        self.start = start;
        let mut i = match self.mode {
            Mode::Heredoc(heredoc) => self.heredoc_line(start, end, heredoc, out),
            _ => start,
        };
        while i < end {
            let next = match self.mode {
                Mode::Normal => self.token(i, end, out),
                Mode::Single => self.single(i, end, out),
                Mode::Double => self.double(i, end, out),
                Mode::Ansi => self.ansi(i, end, out),
                Mode::Heredoc(_) => end,
            };
            i = next.max(i + char_len(b, i));
        }
    }

    fn end_line(&mut self) {
        if matches!(self.mode, Mode::Normal) && !self.continued {
            self.command = !self.arguments;
            self.declaring = self.arguments;
            self.expect_in = 0;
            self.function = false;
        }
        self.continued = false;
        if let Some(heredoc) = self.pending.take()
            && matches!(self.mode, Mode::Normal)
        {
            self.mode = Mode::Heredoc(heredoc);
        }
    }

    fn token(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let c = b[i];
        if is_ws(c) {
            return skip_ws(b, i, end);
        }
        let word_start = i == self.start
            || is_ws(b[i - 1])
            || matches!(b[i - 1], b';' | b'&' | b'|' | b'(' | b')' | b'<' | b'>');
        match c {
            b'#' if word_start => {
                out.push(i, end, Token::Comment);
                end
            }
            b'\'' | b'"' => {
                self.argument();
                out.push(i, i + 1, Token::String);
                self.mode = if c == b'"' {
                    Mode::Double
                } else {
                    Mode::Single
                };
                i + 1
            }
            b'$' => self.dollar(i, end, out),
            b'`' => {
                out.push(i, i + 1, Token::Punctuation);
                self.backtick = !self.backtick;
                self.command = self.backtick;
                i + 1
            }
            b'\\' => {
                let stop = if i + 1 < end {
                    i + 1 + char_len(b, i + 1)
                } else {
                    self.continued = true;
                    end
                };
                out.push(i, stop, Token::Escape);
                stop
            }
            b'|' | b'&' | b';' | b'(' | b')' | b'<' | b'>' => self.operator(i, end, out),
            _ => self.word(i, end, out),
        }
    }

    /// A non-command word, string or expansion was consumed.
    fn argument(&mut self) {
        self.command = false;
        self.expect_in = self.expect_in.saturating_sub(1);
    }

    fn new_command(&mut self) {
        self.command = !self.arguments;
        self.declaring = self.arguments;
        self.expect_in = 0;
    }

    fn push_paren(&mut self, dollar: bool) {
        if self.depth < 64 {
            let bit = 1 << self.depth;
            if dollar {
                self.parens |= bit;
            } else {
                self.parens &= !bit;
            }
        }
        self.depth = self.depth.saturating_add(1);
    }

    /// Close a parenthesis; true if it closes a `$(`.
    fn pop_paren(&mut self) -> bool {
        if self.depth == 0 {
            return false;
        }
        self.depth -= 1;
        self.depth < 64 && self.parens & (1 << self.depth) != 0
    }

    fn operator(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let at = |k: usize| if k < end { b[k] } else { 0 };
        let (c, next) = (b[i], at(i + 1));
        let (token, stop) = match c {
            b'(' => {
                self.push_paren(false);
                self.command = !self.arguments;
                (Token::Punctuation, i + 1)
            }
            b')' => {
                let dollar = self.pop_paren();
                self.command = !dollar && !self.arguments;
                let token = if dollar {
                    Token::Variable
                } else {
                    Token::Punctuation
                };
                (token, i + 1)
            }
            b';' => {
                self.new_command();
                let stop = match (next, at(i + 2)) {
                    (b';', b'&') => i + 3,
                    (b';' | b'&', _) => i + 2,
                    _ => i + 1,
                };
                (Token::Punctuation, stop)
            }
            b'|' => {
                self.new_command();
                (
                    Token::Operator,
                    i + 1 + usize::from(matches!(next, b'|' | b'&')),
                )
            }
            b'&' if next == b'>' => (Token::Operator, i + 2 + usize::from(at(i + 2) == b'>')),
            b'&' => {
                self.new_command();
                (Token::Operator, i + 1 + usize::from(next == b'&'))
            }
            b'<' if next == b'<' && at(i + 2) == b'<' => (Token::Operator, i + 3),
            b'<' if next == b'<' => return self.heredoc_start(i, end, out),
            b'<' | b'>' if next == b'(' => {
                self.push_paren(false);
                self.command = true;
                (Token::Operator, i + 2)
            }
            _ => (
                Token::Operator,
                i + 1 + usize::from(matches!(next, b'>' | b'&' | b'|')),
            ),
        };
        out.push(i, stop, token);
        stop
    }

    /// `<<EOF`, `<<-EOF`, `<<'EOF'`, `<<"EOF"`, `<<\EOF`: the body starts on the next line.
    fn heredoc_start(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let tabs = starts(b, i + 2, end, b"-");
        let operator = i + 2 + usize::from(tabs);
        out.push(i, operator, Token::Operator);
        let k = skip_ws(b, operator, end);
        let quote = (k < end && matches!(b[k], b'\'' | b'"')).then(|| b[k]);
        let escaped = starts(b, k, end, b"\\");
        let name = k + usize::from(quote.is_some() || escaped);
        let mut name_end = name;
        while name_end < end
            && (b[name_end].is_ascii_alphanumeric() || matches!(b[name_end], b'_' | b'-' | b'.'))
        {
            name_end += 1;
        }
        if name_end == name {
            return operator;
        }
        let mut stop = name_end;
        if let Some(quote) = quote
            && starts(b, stop, end, &[quote])
        {
            stop += 1;
        }
        out.push(k, stop, Token::Label);
        if self.pending.is_none() {
            self.pending = Some(Heredoc {
                delimiter: &self.src[name..name_end],
                tabs,
                expand: quote.is_none() && !escaped,
            });
        }
        stop
    }

    fn heredoc_line(&mut self, start: usize, end: usize, heredoc: Heredoc, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let mut k = start;
        while heredoc.tabs && k < end && b[k] == b'\t' {
            k += 1;
        }
        if &self.src[k..end] == heredoc.delimiter {
            out.push(k, end, Token::Label);
            self.mode = Mode::Normal;
            return end;
        }
        if !heredoc.expand {
            out.push(start, end, Token::String);
            return end;
        }
        let mut run = start;
        let mut j = start;
        while j < end {
            if b[j] == b'$'
                && let Some(stop) = self.quoted_expansion(j, end)
            {
                out.push(run, j, Token::String);
                out.push(j, stop, Token::Variable);
                j = stop;
                run = stop;
            } else if b[j] == b'\\' && j + 1 < end {
                j += 1 + char_len(b, j + 1);
            } else {
                j += 1;
            }
        }
        out.push(run, end, Token::String);
        end
    }

    fn dollar(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        self.argument();
        let k = i + 1;
        if k >= end {
            return end;
        }
        match b[k] {
            b'\'' | b'"' => {
                out.push(i, k + 1, Token::String);
                self.mode = if b[k] == b'"' {
                    Mode::Double
                } else {
                    Mode::Ansi
                };
                k + 1
            }
            b'(' => {
                let name = reference_end(b, k + 1, end, self.make);
                if name > k + 1 && starts(b, name, end, b")") {
                    out.push(i, name + 1, Token::Variable);
                    return name + 1;
                }
                out.push(i, k + 1, Token::Variable);
                self.push_paren(true);
                self.command = true;
                k + 1
            }
            // Make passes `$$` to the shell as `$`.
            b'$' if self.make => match self.expansion(k, end) {
                Some(stop) => {
                    out.push(i, stop, Token::Variable);
                    stop
                }
                None => {
                    out.push(i, k + 1, Token::Escape);
                    k + 1
                }
            },
            _ => match self.expansion(i, end) {
                Some(stop) => {
                    out.push(i, stop, Token::Variable);
                    stop
                }
                None => k,
            },
        }
    }

    /// End of `$name`, `${...}`, `$1`, `$@` and similar at `i` (a `$`).
    fn expansion(&self, i: usize, end: usize) -> Option<usize> {
        let b = self.src.as_bytes();
        let k = i + 1;
        if k >= end {
            return None;
        }
        let c = b[k];
        if c == b'{' {
            return Some(matching(b, k, end, b'{', b'}'));
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            return Some(reference_end(b, k, end, false));
        }
        let special =
            c.is_ascii_digit() || matches!(c, b'@' | b'*' | b'#' | b'?' | b'-' | b'$' | b'!');
        (special || (self.make && matches!(c, b'<' | b'^' | b'+' | b'%' | b'|'))).then_some(k + 1)
    }

    /// Expansions inside double quotes and heredocs, including `$(...)` on this line.
    fn quoted_expansion(&self, i: usize, end: usize) -> Option<usize> {
        let b = self.src.as_bytes();
        let i = if self.make && starts(b, i + 1, end, b"$") {
            i + 1
        } else {
            i
        };
        if !starts(b, i + 1, end, b"(") {
            return self.expansion(i, end);
        }
        Some(matching(b, i + 1, end, b'(', b')'))
    }

    fn single(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let stop = match find(b, i, end, b"'") {
            Some(k) => {
                self.mode = Mode::Normal;
                k + 1
            }
            None => end,
        };
        out.push(i, stop, Token::String);
        stop
    }

    fn double(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let mut run = i;
        let mut j = i;
        while j < end {
            match b[j] {
                b'"' => {
                    out.push(run, j + 1, Token::String);
                    self.mode = Mode::Normal;
                    return j + 1;
                }
                b'\\' if j + 1 >= end || matches!(b[j + 1], b'"' | b'\\' | b'$' | b'`') => {
                    let stop = (j + 2).min(end);
                    out.push(run, j, Token::String);
                    out.push(j, stop, Token::Escape);
                    j = stop;
                    run = stop;
                }
                b'$' => match self.quoted_expansion(j, end) {
                    Some(stop) => {
                        out.push(run, j, Token::String);
                        out.push(j, stop, Token::Variable);
                        j = stop;
                        run = stop;
                    }
                    None => j += 1,
                },
                _ => j += 1,
            }
        }
        out.push(run, end, Token::String);
        end
    }

    fn ansi(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let mut run = i;
        let mut j = i;
        while j < end {
            match b[j] {
                b'\'' => {
                    out.push(run, j + 1, Token::String);
                    self.mode = Mode::Normal;
                    return j + 1;
                }
                b'\\' => {
                    let stop = super::escape_end(b, j, end);
                    out.push(run, j, Token::String);
                    out.push(j, stop, Token::Escape);
                    j = stop;
                    run = stop;
                }
                _ => j += 1,
            }
        }
        out.push(run, end, Token::String);
        end
    }

    fn word(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let src = self.src;
        let b = src.as_bytes();
        let mut stop = i;
        while stop < end
            && !is_ws(b[stop])
            && !matches!(
                b[stop],
                b'|' | b'&' | b';' | b'(' | b')' | b'<' | b'>' | b'\'' | b'"' | b'$' | b'`' | b'\\'
            )
        {
            stop += 1;
        }
        let stop = stop.max(i + char_len(b, i));
        let word = &src[i..stop];
        let expecting = self.expect_in > 0;
        self.expect_in = self.expect_in.saturating_sub(1);
        if (self.command || self.declaring)
            && let Some((operator, value)) = assignment(word)
        {
            out.push(i, i + operator, Token::Variable);
            out.push(i + operator, i + value, Token::Operator);
            if value < word.len() && word[value..].bytes().all(|c| c.is_ascii_digit()) {
                out.push(i + value, stop, Token::Number);
            }
            return stop;
        }
        let token = if self.function {
            self.function = false;
            self.command = false;
            Some(Token::Function)
        } else if word == "in" && expecting {
            self.expect_in = 0;
            self.command = false;
            Some(Token::Keyword)
        } else if self.command && lookup(SHELL_KEYWORDS, word, false) {
            self.keyword(word);
            Some(Token::Keyword)
        } else if word == "]]" {
            Some(Token::Keyword)
        } else if self.command && self.defines_function(stop, end) {
            self.command = false;
            Some(Token::Function)
        } else if self.command && lookup(SHELL_BUILTINS, word, false) {
            self.declaring = matches!(
                word,
                "declare" | "export" | "local" | "readonly" | "typeset"
            );
            self.command = false;
            Some(Token::Function)
        } else {
            self.command = false;
            word.bytes()
                .all(|c| c.is_ascii_digit())
                .then_some(Token::Number)
        };
        if let Some(token) = token {
            out.push(i, stop, token);
        }
        stop
    }

    fn keyword(&mut self, word: &str) {
        match word {
            "for" | "select" | "case" => {
                self.expect_in = 2;
                self.command = false;
            }
            "function" => {
                self.function = true;
                self.command = false;
            }
            "break" | "continue" | "done" | "esac" | "fi" | "return" | "[[" | "]]" | "}" => {
                self.command = false;
            }
            _ => self.command = true,
        }
    }

    /// `name()` or `name ()` defines a function.
    fn defines_function(&self, stop: usize, end: usize) -> bool {
        let b = self.src.as_bytes();
        let k = skip_ws(b, stop, end);
        starts(b, k, end, b"(") && starts(b, skip_ws(b, k + 1, end), end, b")")
    }
}

impl Lex for Shell<'_> {
    fn line(&mut self, start: usize, end: usize, out: &mut Out) {
        self.region(start, end, out);
        self.end_line();
    }
}

/// `NAME=`, `NAME+=`, `NAME[i]=`: the operator's start and the value's start.
fn assignment(word: &str) -> Option<(usize, usize)> {
    let b = word.as_bytes();
    if !(b.first()?.is_ascii_alphabetic() || b[0] == b'_') {
        return None;
    }
    let mut k = reference_end(b, 0, b.len(), false);
    if k < b.len() && b[k] == b'[' {
        k += 1 + b[k..].iter().position(|&c| c == b']')?;
    }
    let operator = k;
    if k < b.len() && b[k] == b'+' {
        k += 1;
    }
    (k < b.len() && b[k] == b'=').then_some((operator, k + 1))
}

/// Position after the bracket that closes the `open` at `i`, or `end`.
fn matching(b: &[u8], i: usize, end: usize, open: u8, close: u8) -> usize {
    let mut depth = 0usize;
    for (j, &c) in b[i..end].iter().enumerate() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return i + j + 1;
            }
        }
    }
    end
}

/// End of a variable name; make names may also contain punctuation.
fn reference_end(b: &[u8], mut i: usize, end: usize, make: bool) -> usize {
    while i < end
        && (b[i].is_ascii_alphanumeric()
            || b[i] == b'_'
            || (make
                && matches!(
                    b[i],
                    b'.' | b'-' | b'@' | b'<' | b'^' | b'*' | b'%' | b'+' | b'?' | b'/'
                )))
    {
        i += 1;
    }
    i
}

/// Makefiles: rules, assignments, directives, references, and shell recipes.
pub(super) struct Make<'a> {
    src: &'a str,
    shell: Shell<'a>,
    /// The previous recipe line ended with a backslash.
    recipe: bool,
    /// The previous line ended with a backslash.
    continued: bool,
    /// Inside `define ... endef`.
    define: bool,
    /// Open `$(`/`${` references in the current logical line.
    depth: u32,
}

impl<'a> Make<'a> {
    pub(super) fn new(src: &'a str) -> Self {
        let mut shell = Shell::new(src);
        shell.make = true;
        Self {
            src,
            shell,
            recipe: false,
            continued: false,
            define: false,
            depth: 0,
        }
    }

    /// References, comments and plain text.
    fn text(&mut self, mut i: usize, end: usize, out: &mut Out) {
        let b = self.src.as_bytes();
        while i < end {
            i = match b[i] {
                b'#' if i == 0 || b[i - 1] != b'\\' => {
                    out.push(i, end, Token::Comment);
                    end
                }
                b'$' => self.reference(i, end, out),
                b')' | b'}' if self.depth > 0 => {
                    self.depth -= 1;
                    out.push(i, i + 1, Token::Variable);
                    i + 1
                }
                _ => i + char_len(b, i),
            };
        }
    }

    /// `$(NAME)`, `${NAME}`, `$@`, `$$`, and function calls such as `$(wildcard ...)`.
    fn reference(&mut self, i: usize, end: usize, out: &mut Out) -> usize {
        let b = self.src.as_bytes();
        let k = i + 1;
        if k >= end {
            return end;
        }
        match b[k] {
            b'(' | b'{' => {
                let close = if b[k] == b'(' { b')' } else { b'}' };
                let name = k + 1;
                let mut stop = name;
                while stop < end
                    && !is_ws(b[stop])
                    && !matches!(
                        b[stop],
                        b'(' | b')' | b'{' | b'}' | b'$' | b':' | b',' | b'=' | b'#'
                    )
                {
                    stop += 1;
                }
                if stop < end && b[stop] == close {
                    out.push(i, stop + 1, Token::Variable);
                    return stop + 1;
                }
                self.depth = self.depth.saturating_add(1);
                let function = stop < end
                    && is_ws(b[stop])
                    && lookup(MAKE_FUNCTIONS, &self.src[name..stop], false);
                if function {
                    out.push(i, name, Token::Variable);
                    out.push(name, stop, Token::Function);
                } else {
                    out.push(i, stop, Token::Variable);
                }
                stop
            }
            b'$' => {
                out.push(i, k + 1, Token::Escape);
                k + 1
            }
            _ => {
                let stop = k + char_len(b, k);
                out.push(i, stop, Token::Variable);
                stop
            }
        }
    }

    /// An assignment (`NAME op value`) or a rule (`targets: prerequisites`).
    fn statement(&mut self, p: usize, end: usize, out: &mut Out) {
        let src = self.src;
        let b = src.as_bytes();
        let mut k = p;
        let mut depth = 0;
        let found = loop {
            if k >= end {
                break None;
            }
            match b[k] {
                b'$' if k + 1 < end && matches!(b[k + 1], b'(' | b'{') => {
                    depth += 1;
                    k += 2;
                    continue;
                }
                b')' | b'}' if depth > 0 => depth -= 1,
                b'#' if depth == 0 => break None,
                b'=' | b':' if depth == 0 => break Some(k),
                _ => {}
            }
            k += 1;
        };
        let Some(k) = found else {
            self.text(p, end, out);
            return;
        };
        let mut colons = k;
        while colons < end && b[colons] == b':' {
            colons += 1;
        }
        let assignment = if b[k] == b'=' {
            let operator = if k > p && matches!(b[k - 1], b'?' | b'+' | b'!') {
                k - 1
            } else {
                k
            };
            Some((operator, k + 1))
        } else {
            starts(b, colons, end, b"=").then_some((k, colons + 1))
        };
        if let Some((operator, value)) = assignment {
            let mut name = operator;
            while name > p && is_ws(b[name - 1]) {
                name -= 1;
            }
            if src[p..name].contains('$') {
                self.text(p, name, out);
            } else {
                out.push(p, name, Token::Property);
            }
            out.push(operator, value, Token::Operator);
            self.text(value, end, out);
            return;
        }
        let mut j = p;
        while j < k {
            if is_ws(b[j]) {
                j += 1;
                continue;
            }
            let mut w = j;
            while w < k && !is_ws(b[w]) {
                w += 1;
            }
            let target = &src[j..w];
            if target.contains('$') {
                self.text(j, w, out);
            } else if lookup(MAKE_TARGETS, target, false) {
                out.push(j, w, Token::Keyword);
            } else {
                out.push(j, w, Token::Function);
            }
            j = w;
        }
        out.push(k, colons, Token::Operator);
        match find(b, colons, end, b";") {
            Some(semicolon) => {
                self.text(colons, semicolon, out);
                out.push(semicolon, semicolon + 1, Token::Punctuation);
                self.shell.region(semicolon + 1, end, out);
                self.shell.end_line();
            }
            None => self.text(colons, end, out),
        }
    }
}

impl Lex for Make<'_> {
    fn line(&mut self, start: usize, end: usize, out: &mut Out) {
        let src = self.src;
        let b = src.as_bytes();
        let continued = self.continued;
        self.continued = end > start && b[end - 1] == b'\\';
        if self.define {
            let k = skip_ws(b, start, end);
            if starts(b, k, end, b"endef") && (k + 5 == end || is_ws(b[k + 5])) {
                out.push(k, k + 5, Token::Keyword);
                self.define = false;
                self.text(k + 5, end, out);
            } else {
                self.text(start, end, out);
            }
            return;
        }
        if self.recipe || starts(b, start, end, b"\t") {
            let mut i = start;
            if !self.recipe {
                i = skip_ws(b, i, end);
                while i < end && matches!(b[i], b'@' | b'-' | b'+') {
                    out.push(i, i + 1, Token::Operator);
                    i += 1;
                }
            }
            self.shell.region(i, end, out);
            self.shell.end_line();
            self.recipe = self.continued;
            return;
        }
        if !continued {
            self.depth = 0;
            self.shell.restart(false);
        }
        let i = skip_ws(b, start, end);
        if i == end {
            return;
        }
        if continued {
            self.text(i, end, out);
            return;
        }
        if b[i] == b'#' {
            out.push(i, end, Token::Comment);
            return;
        }
        let word_end = (i..end)
            .find(|&k| is_ws(b[k]) || b[k] == b'(')
            .unwrap_or(end);
        let word = &src[i..word_end];
        let mut p = i;
        if lookup(MAKE_DIRECTIVES, word, false) {
            out.push(i, word_end, Token::Keyword);
            self.define |= word == "define";
            if !matches!(word, "export" | "override" | "private" | "unexport") {
                self.text(word_end, end, out);
                return;
            }
            p = skip_ws(b, word_end, end);
            if p == end {
                return;
            }
            if starts(b, p, end, b"define") && (p + 6 == end || is_ws(b[p + 6])) {
                out.push(p, p + 6, Token::Keyword);
                self.define = true;
                self.text(p + 6, end, out);
                return;
            }
        }
        self.statement(p, end, out);
    }
}

/// Dockerfiles: instructions, comments, and shell or argument lexing.
pub(super) struct Docker<'a> {
    src: &'a str,
    shell: Shell<'a>,
    /// The current instruction continues on the next line.
    continued: bool,
}

impl<'a> Docker<'a> {
    pub(super) fn new(src: &'a str) -> Self {
        Self {
            src,
            shell: Shell::new(src),
            continued: false,
        }
    }

    /// The end of an instruction word at `i`, if it is one.
    fn instruction(&self, i: usize, end: usize) -> Option<usize> {
        let b = self.src.as_bytes();
        let stop = (i..end)
            .find(|&k| !b[k].is_ascii_alphabetic())
            .unwrap_or(end);
        (stop > i && lookup(DOCKER_INSTRUCTIONS, &self.src[i..stop], true)).then_some(stop)
    }
}

impl Lex for Docker<'_> {
    fn line(&mut self, start: usize, end: usize, out: &mut Out) {
        let src = self.src;
        let b = src.as_bytes();
        if self.shell.in_heredoc() {
            self.shell.region(start, end, out);
            self.shell.end_line();
            return;
        }
        let continued = self.continued;
        let i = skip_ws(b, start, end);
        if i == end || b[i] == b'#' {
            out.push(i, end, Token::Comment);
            return;
        }
        let mut trimmed = end;
        while trimmed > i && is_ws(b[trimmed - 1]) {
            trimmed -= 1;
        }
        self.continued = b[trimmed - 1] == b'\\';
        let mut p = i;
        if !continued {
            let Some(stop) = self.instruction(i, end) else {
                self.shell.restart(true);
                self.shell.region(i, end, out);
                self.shell.end_line();
                return;
            };
            out.push(i, stop, Token::Keyword);
            let mut instruction = &src[i..stop];
            p = stop;
            if instruction.eq_ignore_ascii_case("onbuild") {
                let k = skip_ws(b, stop, end);
                if let Some(next) = self.instruction(k, end) {
                    out.push(k, next, Token::Keyword);
                    instruction = &src[k..next];
                    p = next;
                }
            }
            // The JSON exec form (`CMD ["app", "--flag"]`) holds no shell words.
            let command = ["run", "cmd", "entrypoint"]
                .iter()
                .any(|name| instruction.eq_ignore_ascii_case(name))
                && !starts(b, skip_ws(b, p, end), end, b"[");
            self.shell.restart(!command);
            if instruction.eq_ignore_ascii_case("from")
                && let Some(k) = (p + 1..end.saturating_sub(1)).find(|&k| {
                    b[k..k + 2].eq_ignore_ascii_case(b"as")
                        && is_ws(b[k - 1])
                        && (k + 2 == end || is_ws(b[k + 2]))
                })
            {
                self.shell.region(p, k, out);
                out.push(k, k + 2, Token::Keyword);
                p = k + 2;
            }
        }
        self.shell.region(p, end, out);
        self.shell.end_line();
    }
}
