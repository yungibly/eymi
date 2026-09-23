use super::*;
use unicode_width::UnicodeWidthStr;

const COMMENT: Option<Token> = Some(Token::Comment);
const KEYWORD: Option<Token> = Some(Token::Keyword);
const TYPE: Option<Token> = Some(Token::Type);
const FUNCTION: Option<Token> = Some(Token::Function);
const STRING: Option<Token> = Some(Token::String);
const ESCAPE: Option<Token> = Some(Token::Escape);
const NUMBER: Option<Token> = Some(Token::Number);
const CONSTANT: Option<Token> = Some(Token::Constant);
const OPERATOR: Option<Token> = Some(Token::Operator);
const PUNCTUATION: Option<Token> = Some(Token::Punctuation);
const MACRO: Option<Token> = Some(Token::Macro);
const ATTRIBUTE: Option<Token> = Some(Token::Attribute);
const PROPERTY: Option<Token> = Some(Token::Property);
const TAG: Option<Token> = Some(Token::Tag);
const VARIABLE: Option<Token> = Some(Token::Variable);
const LABEL: Option<Token> = Some(Token::Label);
const HEADING: Option<Token> = Some(Token::Heading);
const INSERTED: Option<Token> = Some(Token::Inserted);
const DELETED: Option<Token> = Some(Token::Deleted);
/// No span may touch the text.
const PLAIN: Option<Token> = None;

fn language(alias: &str) -> &'static Language {
    Language::for_fence(alias).unwrap_or_else(|| panic!("no language for {alias:?}"))
}

fn whole(source: &str) -> Vec<Range<usize>> {
    std::iter::once(0..source.len()).collect()
}

/// Highlight and check every public invariant.
fn checked(
    language: &Language,
    source: &str,
    segments: &[Range<usize>],
) -> Vec<(Range<usize>, Token)> {
    let out = run(language, source, segments);
    assert_eq!(
        out.rejected, 0,
        "{language:?} produced invalid spans for {source:?}"
    );
    let mut last = 0;
    for (span, _) in &out.spans {
        assert!(span.start < span.end, "{language:?}: empty span {span:?}");
        assert!(
            span.start >= last,
            "{language:?}: unsorted or overlapping {span:?}"
        );
        assert!(source.is_char_boundary(span.start) && source.is_char_boundary(span.end));
        assert!(
            segments
                .iter()
                .any(|s| s.start <= span.start && span.end <= s.end),
            "{language:?}: {span:?} leaves segments {segments:?} in {source:?}"
        );
        last = span.end;
    }
    out.spans
}

/// The single token covering all of `range`, if there is one.
fn covering(spans: &[(Range<usize>, Token)], range: Range<usize>) -> Option<Token> {
    let mut at = range.start;
    let mut token = None;
    for (span, kind) in spans {
        if span.end <= at {
            continue;
        }
        if span.start > at || token.is_some_and(|t| t != *kind) {
            return None;
        }
        token = Some(*kind);
        at = span.end;
        if at >= range.end {
            return token;
        }
    }
    None
}

/// Check `(context, part, token)` cases: `part` (or all of `context` when
/// empty) inside the first occurrence of `context` must be covered by exactly
/// `token`, or by no span at all for [`PLAIN`].
fn expect(alias: &str, source: &str, cases: &[(&str, &str, Option<Token>)]) {
    let spans = checked(language(alias), source, &whole(source));
    expect_spans(alias, source, &spans, cases);
}

fn expect_spans(
    alias: &str,
    source: &str,
    spans: &[(Range<usize>, Token)],
    cases: &[(&str, &str, Option<Token>)],
) {
    for &(context, part, expected) in cases {
        let at = source
            .find(context)
            .unwrap_or_else(|| panic!("{alias}: {context:?} not in source"));
        let part = if part.is_empty() { context } else { part };
        let offset = context
            .find(part)
            .unwrap_or_else(|| panic!("{alias}: {part:?} not in {context:?}"));
        let range = at + offset..at + offset + part.len();
        let actual = match expected {
            Some(_) => covering(spans, range.clone()),
            None => spans
                .iter()
                .find(|(span, _)| span.start < range.end && range.start < span.end)
                .map(|(_, token)| *token),
        };
        let nearby: Vec<_> = spans
            .iter()
            .filter(|(span, _)| span.end + 16 > range.start && span.start < range.end + 16)
            .map(|(span, token)| (&source[span.clone()], *token))
            .collect();
        assert_eq!(
            actual, expected,
            "{alias}: {part:?} in {context:?}; nearby spans: {nearby:?}"
        );
    }
}

#[test]
fn rust_constructs() {
    let source = r####"#![allow(dead_code)]
/// Docs for `f`.
#[derive(Debug, Clone)]
struct Point<'a> { name: &'a str }
fn main() {
    let c = 'x'; let n = '\n'; let e = '\u{1F600}'; let q = '\'';
    'outer: loop { break 'outer; }
    let raw = r#"has "quotes""#;
    let bytes = br##"x"# still"##;
    let b = b"bytes"; let k = b'k';
    println!("{}", 1_000u32 + 0xFF + 2.5f64 + 1e-3);
    /* outer /* nested */ still */ let after = true;
    let s: String = String::new(); const MAX_SIZE: usize = 10;
    let r#type = x.await;
    let multi = "first
second";
}
#[cfg_attr(
    feature = "serde",
    derive(Serialize)
)]
impl<T> Trait for Point<'static> where T: Copy {}
"####;
    expect(
        "rust",
        source,
        &[
            ("#![allow(dead_code)]", "", ATTRIBUTE),
            ("/// Docs for `f`.", "", COMMENT),
            ("#[derive(Debug, Clone)]", "", ATTRIBUTE),
            ("struct Point", "struct", KEYWORD),
            ("struct Point", "Point", TYPE),
            ("Point<'a>", "'a", LABEL),
            ("&'a str", "str", TYPE),
            ("fn main", "main", FUNCTION),
            ("'x'", "", STRING),
            ("'\\n'", "\\n", ESCAPE),
            ("'\\u{1F600}'", "\\u{1F600}", ESCAPE),
            ("'\\''", "\\'", ESCAPE),
            ("'outer: loop", "'outer", LABEL),
            ("break 'outer", "'outer", LABEL),
            ("r#\"has \"quotes\"\"#", "", STRING),
            ("br##\"x\"# still\"##", "", STRING),
            ("b\"bytes\"", "", STRING),
            ("b'k'", "", STRING),
            ("println!", "", MACRO),
            ("1_000u32", "", NUMBER),
            ("0xFF", "", NUMBER),
            ("2.5f64", "", NUMBER),
            ("1e-3", "", NUMBER),
            ("/* outer /* nested */ still */", "", COMMENT),
            ("let after", "let", KEYWORD),
            ("= true", "true", CONSTANT),
            ("String::new", "String", TYPE),
            ("String::new", "::", OPERATOR),
            ("String::new", "new", FUNCTION),
            ("MAX_SIZE", "", CONSTANT),
            ("r#type", "", PLAIN),
            (".await", "await", KEYWORD),
            ("\"first", "", STRING),
            ("second\"", "", STRING),
            ("feature = \"serde\"", "feature = ", ATTRIBUTE),
            ("feature = \"serde\"", "\"serde\"", STRING),
            ("derive(Serialize)", "", ATTRIBUTE),
            (")]\nimpl", ")]", ATTRIBUTE),
            ("impl<T>", "impl", KEYWORD),
            ("Point<'static>", "'static", LABEL),
            ("where T", "where", KEYWORD),
        ],
    );
}

#[test]
fn python_constructs() {
    let source = r#"@app.route("/")
def handler(request, *args):
    """Doc string
    spanning lines"""
    x = rb'\d' + f"{name}" + u'x' + '\t' + Rb"\q" + F'{y}'
    if x is None and True:
        print(len(x), 0x1F, 1e-3, 1_000j)
    # comment
class Foo(Base):
    pass
from . import sibling
y = a @ b
"#;
    expect(
        "python",
        source,
        &[
            ("@app.route", "", ATTRIBUTE),
            ("def handler", "def", KEYWORD),
            ("def handler", "handler", FUNCTION),
            ("\"\"\"Doc string", "", STRING),
            ("spanning lines\"\"\"", "", STRING),
            ("rb'\\d'", "", STRING),
            ("f\"{name}\"", "", STRING),
            ("u'x'", "", STRING),
            ("'\\t'", "\\t", ESCAPE),
            ("Rb\"\\q\"", "", STRING),
            ("F'{y}'", "", STRING),
            ("is None", "is", KEYWORD),
            ("None", "", CONSTANT),
            ("True", "", CONSTANT),
            ("print(", "print", FUNCTION),
            ("len(x)", "len", FUNCTION),
            ("0x1F", "", NUMBER),
            ("1e-3", "", NUMBER),
            ("1_000j", "", NUMBER),
            ("# comment", "", COMMENT),
            ("class Foo", "Foo", TYPE),
            ("Base", "", TYPE),
            ("from . import", "import", KEYWORD),
            ("a @ b", "@", OPERATOR),
        ],
    );
}

#[test]
fn javascript_and_typescript_constructs() {
    let js = r#"import { x } from "./x.js";
const tpl = `line one
line ${two}`;
function greet(name) { return `hi ${name}`; }
class Widget extends Base { static #count = 0; }
let n = null ?? undefined; // trailing
/* block */ await fetch(url).then(() => $el);
promise.catch(handle);
"#;
    expect(
        "js",
        js,
        &[
            ("import", "", KEYWORD),
            ("\"./x.js\"", "", STRING),
            ("`line one", "", STRING),
            ("line ${two}`", "", STRING),
            ("function greet", "greet", FUNCTION),
            ("class Widget", "Widget", TYPE),
            ("extends", "", KEYWORD),
            ("null", "", CONSTANT),
            ("undefined", "", CONSTANT),
            ("??", "", OPERATOR),
            ("// trailing", "", COMMENT),
            ("/* block */", "", COMMENT),
            ("fetch", "", FUNCTION),
            (".then", "then", FUNCTION),
            ("=>", "", OPERATOR),
            ("$el", "", PLAIN),
            (".catch", "catch", FUNCTION),
        ],
    );
    let ts = r#"interface Props { name: string; count: number; }
@Component({})
export class App implements Props { private x: boolean = false; }
type Id = string | unknown;
enum Color { Red }
function f(): void {}
"#;
    expect(
        "tsx",
        ts,
        &[
            ("interface Props", "interface", KEYWORD),
            ("interface Props", "Props", TYPE),
            ("name: string", "string", TYPE),
            ("number", "", TYPE),
            ("@Component", "", ATTRIBUTE),
            ("implements", "", KEYWORD),
            ("private", "", KEYWORD),
            ("boolean", "", TYPE),
            ("type Id", "Id", TYPE),
            ("unknown", "", TYPE),
            ("enum Color", "Color", TYPE),
            ("): void", "void", TYPE),
        ],
    );
}

#[test]
fn go_constructs() {
    let source = r#"package main
import "fmt"
type Server struct{ name string }
func (s *Server) Start() error {
	raw := `multi
line`
	r := 'x'
	ch := make(chan int, len(s.name))
	fmt.Println(nil, true, iota, http.Request{})
	return nil
}
"#;
    expect(
        "go",
        source,
        &[
            ("package", "", KEYWORD),
            ("\"fmt\"", "", STRING),
            ("type Server", "Server", TYPE),
            ("name string", "string", TYPE),
            ("Start()", "Start", FUNCTION),
            ("error", "", TYPE),
            ("`multi", "", STRING),
            ("line`", "", STRING),
            ("'x'", "", STRING),
            ("make(", "make", FUNCTION),
            ("chan", "", KEYWORD),
            ("int", "", TYPE),
            ("len(", "len", FUNCTION),
            ("Println", "", FUNCTION),
            ("nil", "", CONSTANT),
            ("iota", "", CONSTANT),
            ("Request", "", TYPE),
            (":=", "", OPERATOR),
        ],
    );
}

#[test]
fn c_and_cpp_constructs() {
    let c = r#"#include <stdio.h>
  #  define MAX 10
typedef struct node { int value; } node_t;
int main(void) {
    char c = 'a';
    const char *s = "hi\n";
    size_t n = sizeof(node_t);
    printf("%d", MAX); // done
    return NULL == 0 ? 0x10 : 1.5f;
}
"#;
    expect(
        "c",
        c,
        &[
            ("#include", "", MACRO),
            ("<stdio.h>", "", STRING),
            ("#  define", "", MACRO),
            ("MAX 10", "MAX", CONSTANT),
            ("struct node", "struct", KEYWORD),
            ("struct node", "node", TYPE),
            ("int value", "int", TYPE),
            ("node_t;", "node_t", TYPE),
            ("main(", "main", FUNCTION),
            ("'a'", "", STRING),
            ("\"hi\\n\"", "\\n", ESCAPE),
            ("size_t", "", TYPE),
            ("sizeof", "", KEYWORD),
            ("printf", "", FUNCTION),
            ("// done", "", COMMENT),
            ("NULL", "", CONSTANT),
            ("0x10", "", NUMBER),
            ("1.5f", "", NUMBER),
        ],
    );
    let cpp = r#"#pragma once
namespace app {
template <typename T> class Box { public: T value; };
auto raw = R"json({"a": 1})json";
std::vector<int> v{1'000'000};
auto u = u8"text"; auto w = L'x';
}
"#;
    expect(
        "cpp",
        cpp,
        &[
            ("#pragma", "", MACRO),
            ("namespace app", "namespace", KEYWORD),
            ("namespace app", "app", TYPE),
            ("template", "", KEYWORD),
            ("class Box", "Box", TYPE),
            ("R\"json({\"a\": 1})json\"", "", STRING),
            ("1'000'000", "", NUMBER),
            ("u8\"text\"", "", STRING),
            ("L'x'", "", STRING),
        ],
    );
}

#[test]
fn csharp_java_kotlin_swift_constructs() {
    let cs = r#"using System;
#region Main
public class Program {
    static void Main(string[] args) {
        var path = @"C:\dir\""quoted""";
        var msg = $"Hello {args[0]}\n";
        Console.WriteLine(msg);
    }
}
#endregion
"#;
    expect(
        "cs",
        cs,
        &[
            ("using", "", KEYWORD),
            ("#region", "", MACRO),
            ("class Program", "Program", TYPE),
            ("void", "", TYPE),
            ("string[]", "string", TYPE),
            ("var", "", KEYWORD),
            ("@\"C:\\dir\\", "", STRING),
            ("\"\"quoted", "\"\"", ESCAPE),
            ("$\"Hello", "", STRING),
            ("}\\n\"", "\\n", ESCAPE),
            ("Console", "", TYPE),
            ("WriteLine", "", FUNCTION),
            ("#endregion", "", MACRO),
        ],
    );
    let java = r#"@Override
public String toString() {
    String block = """
        text block
        """;
    char c = 'c';
    return String.format("%s", this.name) + null;
}
"#;
    expect(
        "java",
        java,
        &[
            ("@Override", "", ATTRIBUTE),
            ("public", "", KEYWORD),
            ("String", "", TYPE),
            ("toString", "", FUNCTION),
            ("text block", "", STRING),
            ("'c'", "", STRING),
            ("format", "", FUNCTION),
            ("this", "", KEYWORD),
            ("null", "", CONSTANT),
        ],
    );
    let kotlin = r#"data class User(val name: String)
fun greet(user: User): String = "Hi ${user.name}"
val raw = """no \escape"""
@JvmStatic fun main() { println(null) }
"#;
    expect(
        "kotlin",
        kotlin,
        &[
            ("data class", "class", KEYWORD),
            ("class User", "User", TYPE),
            ("val", "", KEYWORD),
            ("fun greet", "greet", FUNCTION),
            ("\"Hi ${user.name}\"", "", STRING),
            ("\"\"\"no \\escape\"\"\"", "", STRING),
            ("@JvmStatic", "", ATTRIBUTE),
            ("println", "", FUNCTION),
            ("null", "", CONSTANT),
        ],
    );
    let swift = r##"import SwiftUI
@MainActor struct ContentView: View {
    var body: some View { Text("Hi \(name)") }
}
func greet(_ name: String) -> String { return name ?? nil }
let raw = #"raw \n"#
#if DEBUG
let x = [1, 2].map { $0 * 2 }
#endif
"##;
    expect(
        "swift",
        swift,
        &[
            ("@MainActor", "", ATTRIBUTE),
            ("struct ContentView", "ContentView", TYPE),
            ("some", "", KEYWORD),
            ("func greet", "greet", FUNCTION),
            ("nil", "", CONSTANT),
            ("#\"raw \\n\"#", "", STRING),
            ("#if", "", MACRO),
            ("$0", "", VARIABLE),
        ],
    );
}

#[test]
fn ruby_php_lua_perl_constructs() {
    let ruby = r##"# frozen_string_literal: true
=begin
block comment
=end
class Greeter < Base
  attr_reader :name
  def initialize(name, greeting: "Hi")
    @name = name
    @@count += 1
    $stdout.puts "#{greeting}, #{@name}!" if valid?
  end
end
text = <<~EOS
  heredoc #{body}
EOS
"##;
    expect(
        "ruby",
        ruby,
        &[
            ("# frozen_string_literal: true", "", COMMENT),
            ("=begin", "", COMMENT),
            ("block comment", "", COMMENT),
            ("=end", "", COMMENT),
            ("class Greeter", "Greeter", TYPE),
            ("attr_reader", "", FUNCTION),
            (":name", "", CONSTANT),
            ("def initialize", "initialize", FUNCTION),
            ("greeting:", "", CONSTANT),
            ("@name =", "@name", VARIABLE),
            ("@@count", "", VARIABLE),
            ("$stdout", "", VARIABLE),
            ("valid?", "", PLAIN),
            ("<<~EOS", "<<~", OPERATOR),
            ("<<~EOS", "EOS", LABEL),
            ("heredoc #{body}", "", STRING),
            ("\nEOS", "EOS", LABEL),
        ],
    );
    let php = r#"<p>Hello</p>
<?php
namespace App;
#[Route('/home')]
function home(array $items): string {
    // comment
    # also comment
    $count = count($items);
    echo "Count: $count" . PHP_EOL;
    return <<<EOT
    Items: {$count}
    EOT;
}
?>
<footer></footer>
"#;
    expect(
        "php",
        php,
        &[
            ("Hello", "", PLAIN),
            ("<?php", "", MACRO),
            ("namespace", "", KEYWORD),
            ("#[Route('/home')]", "", ATTRIBUTE),
            ("function home", "home", FUNCTION),
            ("array", "", TYPE),
            ("$items", "", VARIABLE),
            ("): string", "string", TYPE),
            ("// comment", "", COMMENT),
            ("# also comment", "", COMMENT),
            ("count(", "count", FUNCTION),
            ("\"Count: $count\"", "", STRING),
            ("PHP_EOL", "", CONSTANT),
            ("<<<EOT", "<<<", OPERATOR),
            ("<<<EOT", "EOT", LABEL),
            ("Items: {$count}", "", STRING),
            ("    EOT;", "EOT", LABEL),
            ("?>", "", MACRO),
            ("<footer>", "", PLAIN),
        ],
    );
    let lua = r#"-- line comment
--[[ block
comment ]]
--[==[ level ]] still ]==]
local s = [[long
string]] .. "x"
local function add(a, b) return a + b end
print(nil, true, #t)
function M.helper() end
"#;
    expect(
        "lua",
        lua,
        &[
            ("-- line comment", "", COMMENT),
            ("--[[ block", "", COMMENT),
            ("comment ]]", "", COMMENT),
            ("--[==[ level ]] still ]==]", "", COMMENT),
            ("local", "", KEYWORD),
            ("[[long", "", STRING),
            ("string]]", "", STRING),
            ("..", "", OPERATOR),
            ("function add", "add", FUNCTION),
            ("print", "", FUNCTION),
            ("nil", "", CONSTANT),
            ("M.helper", "M", PLAIN),
            ("M.helper", "helper", FUNCTION),
        ],
    );
    let perl = r#"#!/usr/bin/perl
use strict;
my %hash = (key => 'value');
my @list = (1, 2);
sub greet { my ($name) = @_; print "Hello, $name\n"; }
=pod
Documentation
=cut
my $count = $#list + 1;
"#;
    expect(
        "perl",
        perl,
        &[
            ("#!/usr/bin/perl", "", COMMENT),
            ("use", "", KEYWORD),
            ("%hash", "", VARIABLE),
            ("'value'", "", STRING),
            ("@list", "", VARIABLE),
            ("sub greet", "sub", KEYWORD),
            ("sub greet", "greet", FUNCTION),
            ("@_", "", VARIABLE),
            ("print", "", FUNCTION),
            ("\"Hello, ", "", STRING),
            ("=pod", "", COMMENT),
            ("Documentation", "", COMMENT),
            ("=cut", "", COMMENT),
            ("$count", "", VARIABLE),
            ("$#list", "", VARIABLE),
        ],
    );
}

#[test]
fn sql_constructs() {
    let source = r#"-- comment
SELECT id, COUNT(*) AS total FROM users u
WHERE name = 'O''Brien' AND created_at > NOW() /* block */
CREATE TABLE t (id INTEGER PRIMARY KEY, name varchar(255), flag BOOLEAN DEFAULT NULL);
INSERT INTO users (name) VALUES ($1);
select "Quoted" from t where x = E'a';
"#;
    expect(
        "sql",
        source,
        &[
            ("-- comment", "", COMMENT),
            ("SELECT", "", KEYWORD),
            ("COUNT", "", FUNCTION),
            ("AS total", "AS", KEYWORD),
            ("FROM", "", KEYWORD),
            ("users u", "users", PLAIN),
            ("'O''Brien'", "'O", STRING),
            ("'O''Brien'", "''", ESCAPE),
            ("NOW", "", FUNCTION),
            ("/* block */", "", COMMENT),
            ("INTEGER", "", TYPE),
            ("varchar", "", TYPE),
            ("BOOLEAN", "", TYPE),
            ("NULL", "", CONSTANT),
            ("users (name)", "users", PLAIN),
            ("$1", "", VARIABLE),
            ("select", "", KEYWORD),
            ("\"Quoted\"", "", STRING),
            ("E'a'", "", STRING),
        ],
    );
}

#[test]
fn shell_constructs() {
    let source = r#"#!/bin/bash
# comment
export PATH="$HOME/bin:${PATH}"
name=world  # trailing comment
echo "Hello, $name" 'single $quoted' a#b ${#name} done
if [[ -f "$1" ]]; then
  for f in *.txt; do cat "$f" | wc -l; done
fi
greet() { local who=$1; printf '%s\n' "$who"; }
result=$(date +%s)
cat <<EOF
body $name
EOF
case "$x" in
  start) exit 0 ;;
esac
"#;
    expect(
        "bash",
        source,
        &[
            ("#!/bin/bash", "", COMMENT),
            ("# comment", "", COMMENT),
            ("export", "", FUNCTION),
            ("PATH=", "PATH", VARIABLE),
            ("\"$HOME/bin:", "\"", STRING),
            ("$HOME", "", VARIABLE),
            ("${PATH}", "", VARIABLE),
            ("name=world", "name", VARIABLE),
            ("name=world", "=", OPERATOR),
            ("# trailing comment", "", COMMENT),
            ("echo", "", FUNCTION),
            ("$name\"", "$name", VARIABLE),
            ("'single $quoted'", "", STRING),
            ("a#b", "", PLAIN),
            ("${#name}", "", VARIABLE),
            ("} done", "done", PLAIN),
            ("if", "", KEYWORD),
            ("[[", "", KEYWORD),
            ("then", "", KEYWORD),
            ("for f", "for", KEYWORD),
            ("in *.txt", "in", KEYWORD),
            ("; do", "do", KEYWORD),
            ("cat \"$f\"", "cat", PLAIN),
            ("wc -l; done", "done", KEYWORD),
            ("fi", "", KEYWORD),
            ("greet()", "greet", FUNCTION),
            ("local", "", FUNCTION),
            ("who=", "who", VARIABLE),
            ("printf", "", FUNCTION),
            ("'%s\\n'", "", STRING),
            ("$(date", "$(", VARIABLE),
            ("<<EOF", "<<", OPERATOR),
            ("<<EOF", "EOF", LABEL),
            ("body ", "", STRING),
            ("body $name", "$name", VARIABLE),
            ("\nEOF", "EOF", LABEL),
            ("case \"$x\" in", "in", KEYWORD),
            ("exit 0", "exit", FUNCTION),
            ("exit 0", "0", NUMBER),
            ("esac", "", KEYWORD),
        ],
    );
}

#[test]
fn json_constructs() {
    let source = r#"{
  "name": "eymi", "version": 1.5e3, "ok": true, "none": null,
  // jsonc comment
  "nested": { "list" : [1, -2, "three\n"] }, /* block */
  bare: 'single'
}
"#;
    expect(
        "json",
        source,
        &[
            ("\"name\"", "", PROPERTY),
            ("\"eymi\"", "", STRING),
            ("1.5e3", "", NUMBER),
            ("true", "", CONSTANT),
            ("null", "", CONSTANT),
            ("// jsonc comment", "", COMMENT),
            ("\"list\"", "", PROPERTY),
            ("-2", "", NUMBER),
            ("\"three\\n\"", "\\n", ESCAPE),
            ("/* block */", "", COMMENT),
            ("bare", "", PROPERTY),
            ("'single'", "", STRING),
            ("{", "", PUNCTUATION),
            (":", "", PUNCTUATION),
        ],
    );
}

#[test]
fn yaml_constructs() {
    let source = r#"%YAML 1.2
---
# comment
name: eymi
version: 1.5
enabled: yes
empty: ~
list:
  - item one
  - key: value # trailing
anchor: &base
  a: 1
ref: *base
tagged: !!str 123
quoted: "multi
  line"
single: 'it''s'
url: http://example.com:8080/x
block: |
  literal text: not a key
  second line
next: done
folded: >-
  folded: text
flow: {a: 1, b: [x, y]}
"quoted key": 1
...
"#;
    expect(
        "yaml",
        source,
        &[
            ("%YAML 1.2", "", KEYWORD),
            ("---", "", PUNCTUATION),
            ("# comment", "", COMMENT),
            ("name", "", PROPERTY),
            ("eymi", "", PLAIN),
            ("1.5", "", NUMBER),
            ("yes", "", CONSTANT),
            ("~", "", CONSTANT),
            ("list", "", PROPERTY),
            ("- item one", "-", PUNCTUATION),
            ("- item one", "item one", PLAIN),
            ("key: value", "key", PROPERTY),
            ("key: value", ":", PUNCTUATION),
            ("# trailing", "", COMMENT),
            ("&base", "", LABEL),
            ("*base", "", LABEL),
            ("!!str", "", TYPE),
            ("123", "", NUMBER),
            ("\"multi", "", STRING),
            ("line\"", "", STRING),
            ("'it''s'", "", STRING),
            ("http://example.com:8080/x", "", PLAIN),
            ("block: |", "|", OPERATOR),
            ("literal text: not a key", "", STRING),
            ("second line", "", STRING),
            ("next", "", PROPERTY),
            ("done", "", PLAIN),
            ("folded: >-", ">-", OPERATOR),
            ("  folded: text", "folded: text", STRING),
            ("{a: 1", "a", PROPERTY),
            ("b: [x", "b", PROPERTY),
            ("[x", "[", PUNCTUATION),
            ("\"quoted key\"", "", PROPERTY),
            ("\n...", "...", PUNCTUATION),
        ],
    );
}

#[test]
fn toml_and_ini_constructs() {
    let toml = r#"# comment
title = "TOML \u00e9"
[server]
host.name = 'literal'
"quoted key" = 1_000
date = 1979-05-27T07:32:00Z
enabled = true
[[products]]
matrix = [
  [1, 2],
  ["a", 'b'],
]
inline = { x = 1, y = -2.5 }
multi = """
line one
"""
"#;
    expect(
        "toml",
        toml,
        &[
            ("# comment", "", COMMENT),
            ("title", "", PROPERTY),
            ("\"TOML ", "", STRING),
            ("\\u00e9", "", ESCAPE),
            ("[server]", "", HEADING),
            ("host.name", "host", PROPERTY),
            ("host.name", ".", PUNCTUATION),
            ("host.name", "name", PROPERTY),
            ("'literal'", "", STRING),
            ("\"quoted key\"", "", PROPERTY),
            ("1_000", "", NUMBER),
            ("1979-05-27T07:32:00Z", "", NUMBER),
            ("true", "", CONSTANT),
            ("[[products]]", "", HEADING),
            ("  [1, 2]", "[", PUNCTUATION),
            ("\"a\"", "", STRING),
            ("{ x = 1", "x", PROPERTY),
            ("y = -2.5", "y", PROPERTY),
            ("-2.5", "", NUMBER),
            ("line one", "", STRING),
        ],
    );
    let ini = r#"; comment
# also comment
[section]
key = value ; inline
name: John
enabled = true
port = 8080
path = "C:\dir"
url = http://example.com/#anchor
interp = %(home)s/x ${HOME}
[remote "origin"]
"#;
    expect(
        "ini",
        ini,
        &[
            ("; comment", "", COMMENT),
            ("# also comment", "", COMMENT),
            ("[section]", "", HEADING),
            ("key", "", PROPERTY),
            ("value", "", PLAIN),
            ("; inline", "", COMMENT),
            ("name", "", PROPERTY),
            ("John", "", PLAIN),
            ("true", "", CONSTANT),
            ("8080", "", NUMBER),
            ("\"C:\\dir\"", "", STRING),
            ("url", "", PROPERTY),
            ("http://example.com/#anchor", "", PLAIN),
            ("%(home)s", "", VARIABLE),
            ("${HOME}", "", VARIABLE),
            ("[remote \"origin\"]", "", HEADING),
        ],
    );
}

#[test]
fn html_and_xml_constructs() {
    let html = r#"<!DOCTYPE html>
<!-- a comment
spanning lines -->
<div class="box" data-x='1' hidden>Tom &amp; Jerry &#x27;</div>
<img src=photo.png />
<script>if (a < b) { x = "<div>"; }</script>
<style>p > a { color: red; }</style>
<p
  id="multi">text</p>
"#;
    expect(
        "html",
        html,
        &[
            ("<!DOCTYPE", "", KEYWORD),
            ("<!-- a comment", "", COMMENT),
            ("spanning lines -->", "", COMMENT),
            ("<div class", "<", PUNCTUATION),
            ("<div class", "div", TAG),
            ("class", "", PROPERTY),
            ("\"box\"", "", STRING),
            ("data-x", "", PROPERTY),
            ("'1'", "", STRING),
            ("hidden", "", PROPERTY),
            ("Tom", "", PLAIN),
            ("&amp;", "", ESCAPE),
            ("&#x27;", "", ESCAPE),
            ("</div>", "</", PUNCTUATION),
            ("img", "", TAG),
            ("photo.png", "", STRING),
            ("/>", "", PUNCTUATION),
            ("<script>", "script", TAG),
            ("a < b", "", PLAIN),
            ("\"<div>\"", "", PLAIN),
            ("</script>", "script", TAG),
            ("color: red", "", PLAIN),
            ("id=", "id", PROPERTY),
            ("\"multi\"", "", STRING),
        ],
    );
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<svg:root xmlns:svg="http://www.w3.org/2000/svg">
  <![CDATA[ raw <text> ]]>
  <item key="a &lt; b"/>
</svg:root>
"#;
    expect(
        "xml",
        xml,
        &[
            ("<?xml", "", MACRO),
            ("version", "", PROPERTY),
            ("\"1.0\"", "", STRING),
            ("?>", "", MACRO),
            ("svg:root", "", TAG),
            ("xmlns:svg", "", PROPERTY),
            ("<![CDATA[", "", KEYWORD),
            (" raw <text> ", "", STRING),
            ("]]>", "", KEYWORD),
            ("&lt;", "", ESCAPE),
            ("\"/>", "/>", PUNCTUATION),
        ],
    );
}

#[test]
fn css_and_scss_constructs() {
    let css = r#"/* comment */
@import url("theme.css");
@media screen and (max-width: 600px) {
  .card > h1:hover, #main a::before {
    color: #ff0000 !important;
    margin: -2px 1.5em 50%;
    background: url(data:image/png;base64,AAAA) no-repeat;
    --accent: rgb(0, 0, 0);
    width: var(--accent);
    height: calc(100% - 2rem);
  }
}
div[data-x="1"] { font-family: "Fira Code", monospace; }
"#;
    expect(
        "css",
        css,
        &[
            ("/* comment */", "", COMMENT),
            ("@import", "", KEYWORD),
            ("url(\"", "url", FUNCTION),
            ("\"theme.css\"", "", STRING),
            ("@media", "", KEYWORD),
            ("max-width", "", PROPERTY),
            ("600px", "", NUMBER),
            (".card", "", TYPE),
            (".card > h1", ">", OPERATOR),
            ("h1", "", TAG),
            (":hover", "", ATTRIBUTE),
            ("#main", "", TYPE),
            (" a::", "a", TAG),
            ("::before", "", ATTRIBUTE),
            ("color", "", PROPERTY),
            ("#ff0000", "", NUMBER),
            ("!important", "", KEYWORD),
            ("-2px", "", NUMBER),
            ("1.5em", "", NUMBER),
            ("50%", "", NUMBER),
            ("data:image/png;base64,AAAA", "", STRING),
            ("no-repeat", "", PLAIN),
            ("--accent:", "--accent", VARIABLE),
            ("rgb", "", FUNCTION),
            ("var(", "var", FUNCTION),
            ("100% - 2rem", "-", OPERATOR),
            ("100% - 2rem", "2rem", NUMBER),
            ("div[", "div", TAG),
            ("data-x", "", PROPERTY),
            ("\"1\"", "", STRING),
            ("font-family", "", PROPERTY),
            ("\"Fira Code\"", "", STRING),
            ("monospace", "", PLAIN),
        ],
    );
    let scss = r#"// line comment
$primary: #333;
@mixin theme($color) { color: $color; }
.button {
  &:hover { background: darken($primary, 10%); }
  .icon { @include theme(red); }
  %placeholder { margin: 0; }
}
"#;
    expect(
        "scss",
        scss,
        &[
            ("// line comment", "", COMMENT),
            ("$primary:", "$primary", VARIABLE),
            ("#333", "", NUMBER),
            ("@mixin", "", KEYWORD),
            ("theme(", "theme", FUNCTION),
            ("$color)", "$color", VARIABLE),
            (".button", "", TYPE),
            ("&:hover", "&", OPERATOR),
            ("&:hover", ":hover", ATTRIBUTE),
            ("background", "", PROPERTY),
            ("darken", "", FUNCTION),
            ("10%", "", NUMBER),
            (".icon", "", TYPE),
            ("@include", "", KEYWORD),
            ("%placeholder", "", TYPE),
            ("margin", "", PROPERTY),
        ],
    );
}

#[test]
fn diff_constructs() {
    let source = r#"diff --git a/src/lib.rs b/src/lib.rs
index 83db48f..bf269f4 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,4 @@ fn main() {
 context line
--- removed header-like
+++ added header-like
+added
 tail
\ No newline at end of file
diff --git a/b b/b
"#;
    expect(
        "diff",
        source,
        &[
            ("diff --git a/src/lib.rs b/src/lib.rs", "", HEADING),
            ("index 83db48f..bf269f4 100644", "", HEADING),
            ("--- a/src/lib.rs", "", HEADING),
            ("+++ b/src/lib.rs", "", HEADING),
            ("@@ -1,3 +1,4 @@", "", KEYWORD),
            ("@@ fn main() {", "fn main() {", PLAIN),
            (" context line", "", PLAIN),
            ("--- removed header-like", "", DELETED),
            ("+++ added header-like", "", INSERTED),
            ("+added", "", INSERTED),
            (" tail", "", PLAIN),
            ("\\ No newline at end of file", "", COMMENT),
            ("diff --git a/b b/b", "", HEADING),
        ],
    );
    expect(
        "patch",
        "- old\n+ new\n context\n",
        &[("- old", "", DELETED), ("+ new", "", INSERTED)],
    );
}

#[test]
fn makefile_and_dockerfile_constructs() {
    let make = "# comment\n\
CC ?= gcc\n\
CFLAGS += -O2 $(EXTRA)\n\
SRC := $(wildcard src/*.c)\n\
.PHONY: all clean\n\
all: build/app\n\
build/%.o: src/%.c $(HEADERS)\n\
\t@echo \"Compiling $<\"\n\
\t$(CC) $(CFLAGS) -c $< -o $@ && echo $$HOME\n\
ifeq ($(OS),Windows)\n\
endif\n\
define HELP\n\
text $(VAR)\n\
endef\n";
    expect(
        "make",
        make,
        &[
            ("# comment", "", COMMENT),
            ("CC ?=", "CC", PROPERTY),
            ("?=", "", OPERATOR),
            ("CFLAGS", "", PROPERTY),
            ("+=", "", OPERATOR),
            ("$(EXTRA)", "", VARIABLE),
            ("SRC", "", PROPERTY),
            (":=", "", OPERATOR),
            ("$(wildcard", "$(", VARIABLE),
            ("wildcard", "", FUNCTION),
            (".PHONY", "", KEYWORD),
            ("all: build", "all", FUNCTION),
            ("build/%.o", "", FUNCTION),
            ("$(HEADERS)", "", VARIABLE),
            ("\t@echo", "@", OPERATOR),
            ("@echo", "echo", FUNCTION),
            ("\"Compiling ", "", STRING),
            ("$<\"", "$<", VARIABLE),
            ("$(CC)", "", VARIABLE),
            ("-o $@", "$@", VARIABLE),
            ("$$HOME", "", VARIABLE),
            ("ifeq", "", KEYWORD),
            ("$(OS)", "", VARIABLE),
            ("endif", "", KEYWORD),
            ("define", "", KEYWORD),
            ("text $(VAR)", "text", PLAIN),
            ("$(VAR)", "", VARIABLE),
            ("endef", "", KEYWORD),
        ],
    );
    let docker = r#"# syntax=docker/dockerfile:1
FROM --platform=$BUILDPLATFORM rust:1.89 AS builder
ARG VERSION=1.0
ENV PATH="/usr/local/bin:${PATH}" \
    MODE=release
WORKDIR /app
COPY --from=builder /src /app
RUN apt-get update && \
    apt-get install -y curl # comment
CMD ["./app", "--serve"]
onbuild run echo hi
"#;
    expect(
        "dockerfile",
        docker,
        &[
            ("# syntax=docker/dockerfile:1", "", COMMENT),
            ("FROM", "", KEYWORD),
            ("$BUILDPLATFORM", "", VARIABLE),
            ("AS builder", "AS", KEYWORD),
            ("ARG", "", KEYWORD),
            ("VERSION", "", VARIABLE),
            ("ENV", "", KEYWORD),
            ("ENV PATH", "PATH", VARIABLE),
            ("\"/usr/local/bin:", "", STRING),
            ("${PATH}", "", VARIABLE),
            ("MODE", "", VARIABLE),
            ("WORKDIR", "", KEYWORD),
            ("COPY", "", KEYWORD),
            ("RUN", "", KEYWORD),
            ("&&", "", OPERATOR),
            ("    apt-get install", "apt-get", PLAIN),
            ("# comment", "", COMMENT),
            ("CMD", "", KEYWORD),
            ("[\"./app\"", "[", PLAIN),
            ("\"./app\"", "", STRING),
            ("onbuild", "", KEYWORD),
            ("onbuild run", "run", KEYWORD),
            ("echo hi", "echo", FUNCTION),
        ],
    );
}

#[test]
fn markdown_constructs() {
    let source = "# Heading\n\
> quote with `code`\n\
- item [link](https://example.com) \\* escaped\n\
1. first\n\
***\n\
<!-- note -->\n\
```rust\n\
let x = 1;\n\
```\n\
Setext\n\
===\n\
``not a fence`` text\n";
    expect(
        "md",
        source,
        &[
            ("# Heading", "", HEADING),
            ("> quote", ">", PUNCTUATION),
            ("`code`", "", STRING),
            ("- item", "-", KEYWORD),
            ("(https://example.com)", "", STRING),
            ("[link]", "link", PLAIN),
            ("\\*", "", ESCAPE),
            ("1.", "", KEYWORD),
            ("***", "", PUNCTUATION),
            ("<!-- note -->", "", COMMENT),
            ("```rust", "", STRING),
            ("let x = 1;", "", STRING),
            ("Setext", "", PLAIN),
            ("===", "", HEADING),
            ("``not a fence``", "", STRING),
            ("text\n", "text", PLAIN),
        ],
    );
}

#[test]
fn more_languages() {
    expect(
        "scala",
        "@main def run(): Unit = println(s\"hi ${name}\")\ncase class Point(x: Int)\nval raw = \"\"\"a\\nb\"\"\"\nval c = 'c'\nobject Main extends App\n",
        &[
            ("@main", "", ATTRIBUTE),
            ("def run", "run", FUNCTION),
            ("Unit", "", TYPE),
            ("\"hi ${name}\"", "", STRING),
            ("case", "", KEYWORD),
            ("class Point", "Point", TYPE),
            ("\"\"\"a\\nb\"\"\"", "", STRING),
            ("'c'", "", STRING),
            ("object Main", "Main", TYPE),
        ],
    );
    expect(
        "dart",
        "@override\nWidget build(BuildContext context) {\n  final r = r'raw \\n';\n  var s = '''multi\nline''';\n  int count = null ?? 0;\n}\n",
        &[
            ("@override", "", ATTRIBUTE),
            ("Widget", "", TYPE),
            ("build", "", FUNCTION),
            ("final", "", KEYWORD),
            ("r'raw \\n'", "", STRING),
            ("'''multi", "", STRING),
            ("line'''", "", STRING),
            ("int", "", TYPE),
            ("null", "", CONSTANT),
        ],
    );
    expect(
        "zig",
        "const std = @import(\"std\");\npub fn main() !void {\n    const text =\n        \\\\multi-line \"quote\n        \\\\string\n    ;\n    var x: u8 = 'a';\n    std.debug.print(\"{d}\\n\", .{x});\n}\n",
        &[
            ("const", "", KEYWORD),
            ("@import", "", MACRO),
            ("fn main", "main", FUNCTION),
            ("void", "", TYPE),
            ("\\\\multi-line \"quote", "", STRING),
            ("u8", "", TYPE),
            ("'a'", "", STRING),
            ("print", "", FUNCTION),
            ("\\n", "", ESCAPE),
        ],
    );
    expect(
        "nix",
        "{ pkgs ? import <nixpkgs> {} }:\nlet\n  name = \"hello\";\n  script = ''\n    echo ${name}\n  '';\nin pkgs.mkShell { buildInputs = [ pkgs.hello ]; inherit name; } # comment\n",
        &[
            ("import", "", FUNCTION),
            ("let", "", KEYWORD),
            ("  name = ", "name", PROPERTY),
            ("\"hello\"", "", STRING),
            ("echo ${name}", "", STRING),
            ("in pkgs", "in", KEYWORD),
            ("mkShell", "", PLAIN),
            ("buildInputs", "", PROPERTY),
            ("inherit", "", KEYWORD),
            ("# comment", "", COMMENT),
        ],
    );
    expect(
        "haskell",
        "module Main where\nimport qualified Data.Map as M\n{- block {- nested -} comment -}\ndata Shape = Circle Double deriving (Show)\nmain :: IO ()\nmain = do\n  let x' = 'c'\n  putStrLn \"hi\\n\" -- comment\n",
        &[
            ("module", "", KEYWORD),
            ("Main", "", TYPE),
            ("qualified", "", KEYWORD),
            ("{- block {- nested -} comment -}", "", COMMENT),
            ("data Shape", "Shape", TYPE),
            ("Circle", "", TYPE),
            ("deriving", "", KEYWORD),
            ("IO", "", TYPE),
            ("x'", "", PLAIN),
            ("'c'", "", STRING),
            ("\"hi", "", STRING),
            ("-- comment", "", COMMENT),
        ],
    );
    expect(
        "elixir",
        "defmodule Greeter do\n  @moduledoc \"\"\"\n  Docs\n  \"\"\"\n  def hello(name, opts \\\\ [greeting: \"Hi\"]) do\n    IO.puts(\"#{name}\")\n    :ok\n  end\n  defp valid?(x), do: x != nil\nend\n",
        &[
            ("defmodule", "", KEYWORD),
            ("Greeter", "", TYPE),
            ("@moduledoc", "", ATTRIBUTE),
            ("  Docs", "Docs", STRING),
            ("def hello", "hello", FUNCTION),
            ("greeting:", "", CONSTANT),
            ("IO", "", TYPE),
            ("puts", "", FUNCTION),
            (":ok", "", CONSTANT),
            ("valid?", "", FUNCTION),
            ("do:", "", CONSTANT),
            ("nil", "", CONSTANT),
        ],
    );
    expect(
        "terraform",
        "# comment\nresource \"aws_instance\" \"web\" {\n  ami   = \"ami-123\"\n  count = var.enabled ? 1 : 0\n  tags = {\n    Name = \"web-${var.env}\"\n  }\n  user_data = <<-EOT\n    #!/bin/bash\n  EOT\n}\n",
        &[
            ("# comment", "", COMMENT),
            ("resource", "", KEYWORD),
            ("\"aws_instance\"", "", STRING),
            ("ami", "", PROPERTY),
            ("count", "", PROPERTY),
            ("? 1", "1", NUMBER),
            ("tags", "", PROPERTY),
            ("Name", "", PROPERTY),
            ("\"web-${var.env}\"", "", STRING),
            ("user_data", "", PROPERTY),
            ("<<-EOT", "<<-", OPERATOR),
            ("<<-EOT", "EOT", LABEL),
            ("#!/bin/bash", "", STRING),
            ("  EOT\n}", "EOT", LABEL),
        ],
    );
    expect(
        "graphql",
        "# comment\nquery GetUser($id: ID!, $with: Boolean = false) @cached {\n  user(id: $id) { name posts @include(if: $with) }\n}\ntype User implements Node { id: ID! }\n\"\"\"Block description\"\"\"\n",
        &[
            ("# comment", "", COMMENT),
            ("query", "", KEYWORD),
            ("$id", "", VARIABLE),
            ("ID!", "ID", TYPE),
            ("Boolean", "", TYPE),
            ("false", "", CONSTANT),
            ("@cached", "", ATTRIBUTE),
            ("user(", "user", FUNCTION),
            ("@include", "", ATTRIBUTE),
            ("type User", "User", TYPE),
            ("implements", "", KEYWORD),
            ("\"\"\"Block description\"\"\"", "", STRING),
        ],
    );
    expect(
        "proto",
        "syntax = \"proto3\";\npackage demo.v1;\n// comment\nmessage User {\n  string name = 1;\n  repeated int32 ids = 2 [packed = true];\n  map<string, User> children = 3;\n}\nenum Status { STATUS_UNKNOWN = 0; }\nservice Api { rpc Get(User) returns (User); }\n",
        &[
            ("syntax", "", KEYWORD),
            ("\"proto3\"", "", STRING),
            ("// comment", "", COMMENT),
            ("message User", "User", TYPE),
            ("string name", "string", TYPE),
            ("repeated", "", KEYWORD),
            ("int32", "", TYPE),
            ("true", "", CONSTANT),
            ("map<", "map", KEYWORD),
            ("enum Status", "Status", TYPE),
            ("STATUS_UNKNOWN", "", CONSTANT),
            ("rpc Get", "Get", FUNCTION),
            ("returns", "", KEYWORD),
        ],
    );
    expect(
        "powershell",
        "# comment\n<# block\ncomment #>\nfunction Get-Greeting {\n    param([string]$Name = \"World\")\n    $env:PATH += \";C:\\bin\"\n    IF ($Name -eq $null -and $true) { Write-Host \"Hello, $Name`n\" }\n    $here = @\"\nmulti $Name\n\"@\n}\n",
        &[
            ("# comment", "", COMMENT),
            ("<# block", "", COMMENT),
            ("comment #>", "", COMMENT),
            ("function", "", KEYWORD),
            ("Get-Greeting", "", FUNCTION),
            ("param", "", KEYWORD),
            ("string", "", TYPE),
            ("$Name", "", VARIABLE),
            ("\"World\"", "", STRING),
            ("$env:PATH", "", VARIABLE),
            ("IF", "", KEYWORD),
            ("-eq", "", OPERATOR),
            ("$null", "", VARIABLE),
            ("-and", "", OPERATOR),
            ("Write-Host", "", FUNCTION),
            ("`n", "", ESCAPE),
            ("@\"", "", STRING),
            ("multi $Name", "", STRING),
            ("\"@", "", STRING),
        ],
    );
    expect(
        "r",
        "# comment\nlibrary(dplyr)\nf <- function(x, na.rm = TRUE) {\n  if (is.na(x)) return(NULL)\n  x %>% filter(y > 1e-3)\n}\n",
        &[
            ("# comment", "", COMMENT),
            ("library", "", FUNCTION),
            ("<-", "", OPERATOR),
            ("function", "", KEYWORD),
            ("na.rm", "", PLAIN),
            ("TRUE", "", CONSTANT),
            ("is.na", "", FUNCTION),
            ("NULL", "", CONSTANT),
            ("%>%", "", OPERATOR),
            ("1e-3", "", NUMBER),
        ],
    );
    expect(
        "julia",
        "# comment\n#= block\ncomment =#\nfunction greet(name::String)\n    @show name'\n    x = [1, 2]'\n    s = :symbol\n    c = 'c'\n    push!(v, 1)\n    return nothing\nend\n",
        &[
            ("# comment", "", COMMENT),
            ("#= block", "", COMMENT),
            ("comment =#", "", COMMENT),
            ("function greet", "greet", FUNCTION),
            ("String", "", TYPE),
            ("@show", "", MACRO),
            ("name'", "'", OPERATOR),
            ("]'", "'", OPERATOR),
            (":symbol", "", CONSTANT),
            ("'c'", "", STRING),
            ("push!", "", FUNCTION),
            ("nothing", "", CONSTANT),
            ("end", "", KEYWORD),
        ],
    );
}

#[test]
fn segments_carry_state_and_act_as_line_breaks() {
    let rust = language("rust");
    // A fenced block inside a quote: each code line is a segment without `> `.
    let source = "> x /* start\n> end */ y\n> let s = 1; // note\n> z\n";
    let mut segments = Vec::new();
    let mut prefixes = Vec::new();
    let mut at = 0;
    for line in source.split_inclusive('\n') {
        prefixes.push(at..at + 2);
        segments.push(at + 2..at + line.len());
        at += line.len();
    }
    let spans = checked(rust, source, &segments);
    expect_spans(
        "rust",
        source,
        &spans,
        &[
            ("/* start", "", COMMENT),
            ("end */", "", COMMENT),
            ("y\n", "y", PLAIN),
            ("let", "", KEYWORD),
            ("// note", "", COMMENT),
            ("> z", "z", PLAIN),
        ],
    );
    for prefix in prefixes {
        assert!(
            spans
                .iter()
                .all(|(span, _)| span.end <= prefix.start || span.start >= prefix.end),
            "container prefix {prefix:?} received a span: {spans:?}"
        );
    }

    // A segment end without a newline still ends line comments and strings.
    let source = "a // c b";
    let spans = checked(rust, source, &[0..6, 6..8]);
    assert_eq!(covering(&spans, 2..6), Some(Token::Comment));
    assert!(
        spans
            .iter()
            .all(|(span, _)| span.end <= 6 || span.start >= 6)
    );
    assert_eq!(covering(&spans, 7..8), None);
    let js = language("js");
    let source = "s = 'abcdef + 1";
    let spans = checked(js, source, &[0..8, 8..15]);
    assert_eq!(covering(&spans, 4..8), Some(Token::String));
    assert!(
        !spans
            .iter()
            .any(|(span, token)| span.start >= 8 && *token == Token::String)
    );
    assert_eq!(covering(&spans, 12..13), Some(Token::Operator));

    // Block comments split into one span per segment, even without newlines.
    let source = "/* abc */";
    let spans = checked(rust, source, &[0..4, 4..9]);
    assert_eq!(spans, vec![(0..4, Token::Comment), (4..9, Token::Comment)]);

    // Multi-line strings continue across segments.
    let python = language("python");
    let source = "s = \"\"\"one\ntwo\"\"\"\nprint(s)\n";
    let lines = [0..11, 11..18, 18..27];
    let spans = checked(python, source, &lines);
    assert_eq!(covering(&spans, 11..17), Some(Token::String));
    assert_eq!(covering(&spans, 18..23), Some(Token::Function));
    assert_eq!(
        spans.iter().map(|(_, t)| *t).collect::<Vec<_>>(),
        checked(python, source, &whole(source))
            .iter()
            .map(|(_, t)| *t)
            .collect::<Vec<_>>(),
        "segmenting at line ends must not change classification"
    );

    // A byte order mark before the first segment is never highlighted.
    let source = "\u{feff}fn main() {}";
    let spans = checked(rust, source, std::slice::from_ref(&(3..source.len())));
    assert_eq!(covering(&spans, 3..5), Some(Token::Keyword));
}

#[test]
fn degenerate_and_invalid_segments_never_panic() {
    let source = "fn é() { \"界\" } // 😀\n/* x";
    let len = source.len();
    let cases: Vec<Vec<Range<usize>>> = vec![
        vec![],
        vec![0..0],
        vec![0..0, 0..0, len..len],
        vec![Range { start: 5, end: 3 }],
        vec![0..len + 10],
        vec![len + 1..len + 5],
        vec![4..5, 3..9, 0..len],
        vec![3..4, 4..6, 6..7],
        vec![0..len, 0..len],
    ];
    for language in Language::all() {
        for segments in &cases {
            let out = run(language, source, segments);
            assert_eq!(out.rejected, 0, "{language:?} {segments:?}");
            let mut last = 0;
            for (span, _) in &out.spans {
                assert!(span.start < span.end && span.start >= last && span.end <= len);
                assert!(source.is_char_boundary(span.start) && source.is_char_boundary(span.end));
                last = span.end;
            }
        }
    }
}

#[test]
fn detection_by_path() {
    let name = |path: &str| Language::for_path(Path::new(path)).map(Language::name);
    for (path, expected) in [
        ("src/main.rs", "Rust"),
        ("MAIN.RS", "Rust"),
        ("tool.PY", "Python"),
        ("app.tsx", "TypeScript"),
        ("lib.mjs", "JavaScript"),
        ("tsconfig.jsonc", "JSON"),
        ("config.yml", "YAML"),
        ("Cargo.toml", "TOML"),
        ("Cargo.lock", "TOML"),
        ("setup.cfg", "INI"),
        (".gitconfig", "INI"),
        (".editorconfig", "INI"),
        ("install.sh", "Shell"),
        (".bashrc", "Shell"),
        (".zshrc", "Shell"),
        (".profile", "Shell"),
        (".env", "Shell"),
        ("PKGBUILD", "Shell"),
        ("a.h", "C"),
        ("b.hpp", "C++"),
        ("c.cs", "C#"),
        ("d.go", "Go"),
        ("E.java", "Java"),
        ("build.gradle.kts", "Kotlin"),
        ("f.swift", "Swift"),
        ("Gemfile", "Ruby"),
        ("Rakefile", "Ruby"),
        ("index.php", "PHP"),
        ("init.lua", "Lua"),
        ("schema.sql", "SQL"),
        ("index.HTML", "HTML"),
        ("icon.svg", "XML"),
        ("Info.plist", "XML"),
        ("site.css", "CSS"),
        ("site.scss", "SCSS"),
        ("fix.patch", "Diff"),
        ("Makefile", "Makefile"),
        ("GNUmakefile", "Makefile"),
        ("rules.mk", "Makefile"),
        ("Dockerfile", "Dockerfile"),
        ("Containerfile", "Dockerfile"),
        ("Dockerfile.dev", "Dockerfile"),
        ("README.md", "Markdown"),
        ("main.tf", "HCL"),
        ("api.proto", "Protobuf"),
        ("run.ps1", "PowerShell"),
        ("analysis.R", "R"),
        ("script.pl", "Perl"),
    ] {
        assert_eq!(name(path), Some(expected), "{path}");
    }
    for path in [
        "notes.txt",
        "LICENSE",
        "README",
        "archive.tar.gz",
        "",
        "/",
        "dir/",
    ] {
        assert_eq!(name(path), None, "{path}");
    }
}

#[test]
fn detection_by_fence_info() {
    let name = |info: &str| Language::for_fence(info).map(Language::name);
    for (info, expected) in [
        ("rust", "Rust"),
        ("rust,ignore", "Rust"),
        ("rs", "Rust"),
        ("{.python}", "Python"),
        ("py title=x", "Python"),
        ("python3", "Python"),
        ("sh title=\"x\"", "Shell"),
        ("bash", "Shell"),
        ("zsh", "Shell"),
        ("shell", "Shell"),
        ("console", "Shell"),
        ("shellsession", "Shell"),
        ("C++", "C++"),
        ("cpp", "C++"),
        ("cxx", "C++"),
        ("  js  ", "JavaScript"),
        ("node", "JavaScript"),
        ("jsx", "JavaScript"),
        ("ts", "TypeScript"),
        ("yml", "YAML"),
        ("cs", "C#"),
        ("csharp", "C#"),
        ("golang", "Go"),
        ("rb", "Ruby"),
        ("kt", "Kotlin"),
        ("md", "Markdown"),
        ("jsonc", "JSON"),
        ("dockerfile", "Dockerfile"),
        ("make", "Makefile"),
        ("makefile", "Makefile"),
        ("patch", "Diff"),
        ("diff", "Diff"),
        ("html", "HTML"),
        ("htm", "HTML"),
        ("xml", "XML"),
        ("svg", "XML"),
        ("{.toml}", "TOML"),
        ("SQL", "SQL"),
        ("hcl", "HCL"),
    ] {
        assert_eq!(name(info), Some(expected), "{info:?}");
    }
    for info in [
        "",
        "   ",
        "{}",
        "{.}",
        ",rust",
        "text",
        "plaintext",
        "unknown-language",
    ] {
        assert_eq!(name(info), None, "{info:?}");
    }
}

#[test]
fn registry_is_consistent() {
    let languages = Language::all();
    let mut names = std::collections::HashSet::new();
    let mut extensions = std::collections::HashMap::new();
    let mut aliases = std::collections::HashMap::new();
    let mut files = std::collections::HashMap::new();
    for language in languages {
        assert!(
            names.insert(language.name()),
            "duplicate name {}",
            language.name()
        );
        let mut chars = language.icon().chars();
        let icon = chars.next().expect("icon");
        assert!(chars.next().is_none(), "{language:?} icon is not one char");
        assert_eq!(language.icon().width(), 1, "{language:?} icon width");
        assert!(
            ('\u{e000}'..='\u{f8ff}').contains(&icon)
                || ('\u{f0000}'..='\u{ffffd}').contains(&icon)
        );
        for extension in language.extensions {
            assert_eq!(*extension, extension.to_ascii_lowercase());
            let previous = extensions.insert(*extension, language.name());
            assert!(previous.is_none(), "extension {extension} is shared");
        }
        for alias in language.aliases {
            assert_eq!(*alias, alias.to_ascii_lowercase());
            let previous = aliases.insert(*alias, language.name());
            assert!(previous.is_none(), "alias {alias} is shared");
            assert_eq!(Language::for_fence(alias), Some(language), "alias {alias}");
        }
        for file in language.file_names {
            assert!(
                files.insert(*file, language.name()).is_none(),
                "file {file}"
            );
            assert_eq!(Language::for_path(Path::new(file)), Some(language));
        }
        assert_eq!(
            Language::for_fence(&language.name().to_ascii_lowercase()),
            Some(language),
            "{language:?} is not reachable by its own name"
        );
    }
    // An alias of one language must not be another language's extension.
    for (alias, owner) in &aliases {
        if let Some(other) = extensions.get(alias) {
            assert_eq!(owner, other, "alias {alias} is an extension of {other}");
        }
    }
}

fn assert_table(name: &str, table: &[&str], lowercase: bool) {
    for pair in table.windows(2) {
        assert!(
            pair[0] < pair[1],
            "{name}: {:?} is not sorted and unique",
            pair
        );
    }
    if lowercase {
        for word in table {
            assert_eq!(
                *word,
                word.to_ascii_lowercase(),
                "{name}: {word} must be lowercase"
            );
        }
    }
}

#[test]
fn word_tables_are_sorted_unique_and_folded() {
    for language in Language::all() {
        let Lexer::Code(g) = language.lexer else {
            continue;
        };
        let name = language.name();
        for table in [
            g.keywords,
            g.types,
            g.constants,
            g.builtins,
            g.fn_definers,
            g.type_definers,
            g.dash_operators,
        ] {
            assert_table(name, table, g.fold_case);
        }
        assert_table(name, g.prefixes, false);
        for definer in g.fn_definers.iter().chain(g.type_definers) {
            assert!(
                lookup(g.keywords, definer, g.fold_case),
                "{name}: definer {definer} is not a keyword"
            );
        }
    }
    use languages::*;
    for (name, table) in [
        ("SHELL_KEYWORDS", SHELL_KEYWORDS),
        ("SHELL_BUILTINS", SHELL_BUILTINS),
        ("MAKE_DIRECTIVES", MAKE_DIRECTIVES),
        ("MAKE_FUNCTIONS", MAKE_FUNCTIONS),
        ("MAKE_TARGETS", MAKE_TARGETS),
    ] {
        assert_table(name, table, false);
    }
    for (name, table) in [
        ("DOCKER_INSTRUCTIONS", DOCKER_INSTRUCTIONS),
        ("YAML_CONSTANTS", YAML_CONSTANTS),
        ("INI_CONSTANTS", INI_CONSTANTS),
    ] {
        assert_table(name, table, true);
    }
}

#[test]
fn case_insensitive_lookup_does_not_allocate_or_overflow() {
    assert!(lookup(&["select", "where"], "SeLeCt", true));
    assert!(!lookup(&["select"], "select_", true));
    assert!(!lookup(&["select"], &"s".repeat(100), true));
    assert!(!lookup(&["é"], "É", true));
    assert!(lookup(&["Select"], "Select", false));
    assert!(!lookup(&["select"], "SELECT", false));
}

/// Deterministic xorshift64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[rustfmt::skip]
const PIECES: &[&str] = &[
    "a", "Z", "_x", "fn", "let", "if", "def", "class", "end", "do", "in", "for", "0", "42", "0x1F",
    "1.5e3", "1'0", " ", "  ", "\t", "\n", "\r\n", "\r", "\"", "'", "`", "\\", "\\n", "/", "*",
    "//", "/*", "*/", "#", "#[", "#!", "--", "--[[", "]]", "[[", "[==[", "]==]", "<!--", "-->",
    "<![CDATA[", "]]>", "\"\"\"", "'''", "r#\"", "\"#", "b'", "'a", "'a'", "$", "${", "$(", "$$",
    "$'", "@", "@@ -1,2 +1,2 @@", "+", "-", "=", ":", ";", ",", ".", "(", ")", "{", "}", "[", "]",
    "<", ">", "</", "/>", "&", "&amp;", "&#", "|", "!", "?", "?>", "<?php", "%", "~", "=begin",
    "=end", "=pod", "=cut", "<<EOF", "<<-EOT", "<<~EOS", "<<<EOT", "EOF", "EOS", "EOT", "```",
    "~~~", "- ", "> ", "key: ", ">-", "&anchor", "*alias", "!!str", "[section]", "[[t]]", "R\"x(",
    ")x\"", "@\"", "\"@", "$\"", "''", "<#", "#>", "#=", "=#", "{-", "-}", "\\\\", "`n", "é", "界",
    "😀", "\u{feff}", "\u{0}", "\u{301}", "\u{2028}",
];

fn random_source(rng: &mut Rng, pieces: usize) -> String {
    let count = rng.below(pieces + 1);
    (0..count)
        .map(|_| PIECES[rng.below(PIECES.len())])
        .collect()
}

/// Sorted, non-overlapping segments on character boundaries, with gaps and
/// empty segments.
fn random_segments(rng: &mut Rng, source: &str) -> Vec<Range<usize>> {
    let boundaries: Vec<usize> = source
        .char_indices()
        .map(|(i, _)| i)
        .chain([source.len()])
        .collect();
    let mut cuts: Vec<usize> = (0..rng.below(8))
        .map(|_| boundaries[rng.below(boundaries.len())])
        .collect();
    cuts.extend([0, source.len()]);
    cuts.sort_unstable();
    cuts.windows(2)
        .map(|pair| {
            let mut start = pair[0];
            if rng.below(3) == 0 && start < pair[1] {
                start = boundaries[boundaries.partition_point(|&b| b <= start)];
            }
            start..pair[1]
        })
        .collect()
}

#[test]
fn random_input_keeps_invariants_in_every_language() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for language in Language::all() {
        for round in 0..300 {
            let source = random_source(&mut rng, if round % 10 == 0 { 200 } else { 30 });
            checked(language, &source, &whole(&source));
            let segments = random_segments(&mut rng, &source);
            checked(language, &source, &segments);
        }
    }
}

#[rustfmt::skip]
const PATHOLOGICAL: &[&str] = &[
    "` `` ``` ", "](", "'a", "\"\\", "/*", "<<A ", "${", "$(", "[==", "&a", "<a ", "#[", "r#\"",
    "\\", "{a: ", "- ? ", ")x", "=\"", "\"", "'", "`", "<!--", "[[", "--[[", "$$", "a b ", "a.b(",
    "\"\"\"", "'''", "#", "@", ":", "<", "{", "(", "*/", "<![CDATA[", "|0", "=begin\n", "~~~\n",
    "a: |\n", "[x]\n", "@@ -1 +1 @@\n", "\t$(A)\n",
];

#[test]
fn long_pathological_lines_stay_linear() {
    // Rescanning the rest of a line per character would make this take minutes.
    for pattern in PATHOLOGICAL {
        let source = pattern.repeat(20_000 / pattern.len());
        for language in Language::all() {
            checked(language, &source, &whole(&source));
        }
    }
}

#[test]
fn a_megabyte_of_rust_highlights() {
    let snippet = "/// Doc comment\nfn add<'a>(x: &'a str, y: u32) -> Result<String, Error> {\n    let s = r#\"raw\"#; // note\n    println!(\"{x} {}\", y + 0xFF);\n}\n";
    let source = snippet.repeat(1_000_000 / snippet.len() + 1);
    let spans = checked(language("rust"), &source, &whole(&source));
    assert!(spans.len() > source.len() / 20);
    assert!(spans.iter().any(|(_, token)| *token == Token::Label));
}
