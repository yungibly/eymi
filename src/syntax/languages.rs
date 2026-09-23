//! The static language registry: detection data, icons, and grammars.
//! Icons are Nerd Font code points following nvim-web-devicons, checked against
//! https://github.com/ryanoasis/nerd-fonts/blob/master/glyphnames.json.
use super::{
    Language, Lexer,
    lexer::{At, Caps, Grammar, Heredoc, Quote},
};

const fn lang(
    name: &'static str,
    icon: &'static str,
    lexer: Lexer,
    extensions: &'static [&'static str],
    file_names: &'static [&'static str],
    aliases: &'static [&'static str],
) -> Language {
    Language {
        name,
        icon,
        extensions,
        file_names,
        aliases,
        lexer,
    }
}

pub(super) static LANGUAGES: [Language; 41] = [
    lang(
        "Rust",
        "\u{e7a8}",
        Lexer::Code(&RUST),
        &["rs"],
        &[],
        &["rs", "rust"],
    ),
    lang(
        "Python",
        "\u{e606}",
        Lexer::Code(&PYTHON),
        &["py", "pyi", "pyw"],
        &[],
        &["py", "py3", "python", "python3"],
    ),
    lang(
        "JavaScript",
        "\u{e60c}",
        Lexer::Code(&JAVASCRIPT),
        &["cjs", "js", "jsx", "mjs"],
        &[],
        &["javascript", "js", "node", "nodejs"],
    ),
    lang(
        "TypeScript",
        "\u{e628}",
        Lexer::Code(&TYPESCRIPT),
        &["cts", "mts", "ts", "tsx"],
        &[],
        &["ts", "typescript"],
    ),
    lang(
        "JSON",
        "\u{e60b}",
        Lexer::Json,
        &[
            "geojson",
            "json",
            "json5",
            "jsonc",
            "jsonl",
            "ndjson",
            "webmanifest",
        ],
        &[".babelrc", "flake.lock"],
        &["json", "json5", "jsonc", "jsonl"],
    ),
    lang(
        "YAML",
        "\u{e615}",
        Lexer::Yaml,
        &["yaml", "yml"],
        &[".clang-format", ".clang-tidy"],
        &["yaml", "yml"],
    ),
    lang(
        "TOML",
        "\u{e6b2}",
        Lexer::Toml,
        &["toml"],
        &["Cargo.lock", "Pipfile", "poetry.lock", "uv.lock"],
        &["toml"],
    ),
    lang(
        "INI",
        "\u{e615}",
        Lexer::Ini,
        &["cfg", "conf", "desktop", "ini", "properties"],
        &[
            ".coveragerc",
            ".editorconfig",
            ".flake8",
            ".gitconfig",
            ".gitmodules",
            ".npmrc",
            ".pylintrc",
        ],
        &[
            "cfg",
            "conf",
            "dosini",
            "editorconfig",
            "gitconfig",
            "ini",
            "properties",
        ],
    ),
    lang(
        "Shell",
        "\u{e795}",
        Lexer::Shell,
        &["bash", "env", "ksh", "sh", "zsh"],
        &[
            ".bash_aliases",
            ".bash_logout",
            ".bash_profile",
            ".bashrc",
            ".env",
            ".envrc",
            ".kshrc",
            ".profile",
            ".zlogin",
            ".zlogout",
            ".zprofile",
            ".zshenv",
            ".zshrc",
            "APKBUILD",
            "PKGBUILD",
        ],
        &[
            "bash",
            "console",
            "ksh",
            "sh",
            "shell",
            "shellscript",
            "shellsession",
            "zsh",
        ],
    ),
    lang("C", "\u{e61e}", Lexer::Code(&C), &["c", "h"], &[], &["c"]),
    lang(
        "C++",
        "\u{e61d}",
        Lexer::Code(&CPP),
        &[
            "c++", "cc", "cpp", "cppm", "cxx", "h++", "hh", "hpp", "hxx", "inl", "ino", "ipp",
            "ixx", "tpp",
        ],
        &[],
        &["c++", "cpp", "cxx"],
    ),
    lang(
        "C#",
        "\u{f031b}",
        Lexer::Code(&CSHARP),
        &["cs", "csx"],
        &[],
        &["c#", "cs", "csharp"],
    ),
    lang(
        "Go",
        "\u{e627}",
        Lexer::Code(&GO),
        &["go"],
        &[],
        &["go", "golang"],
    ),
    lang(
        "Java",
        "\u{e738}",
        Lexer::Code(&JAVA),
        &["java"],
        &[],
        &["java"],
    ),
    lang(
        "Kotlin",
        "\u{e634}",
        Lexer::Code(&KOTLIN),
        &["kt", "kts"],
        &[],
        &["kotlin", "kt"],
    ),
    lang(
        "Swift",
        "\u{e755}",
        Lexer::Code(&SWIFT),
        &["swift"],
        &[],
        &["swift"],
    ),
    lang(
        "Ruby",
        "\u{e739}",
        Lexer::Code(&RUBY),
        &["gemspec", "rake", "rb", "rbw", "ru"],
        &[
            ".irbrc",
            ".pryrc",
            "Brewfile",
            "Capfile",
            "Gemfile",
            "Guardfile",
            "Podfile",
            "Rakefile",
            "Vagrantfile",
        ],
        &["rb", "ruby"],
    ),
    lang(
        "PHP",
        "\u{e73d}",
        Lexer::Code(&PHP),
        &["php", "phtml"],
        &[],
        &["php"],
    ),
    lang(
        "Lua",
        "\u{e620}",
        Lexer::Code(&LUA),
        &["lua", "luau", "rockspec"],
        &[],
        &["lua"],
    ),
    lang(
        "SQL",
        "\u{e706}",
        Lexer::Code(&SQL),
        &["pgsql", "sql"],
        &[],
        &[
            "mysql",
            "plpgsql",
            "postgres",
            "postgresql",
            "sql",
            "sqlite",
            "tsql",
        ],
    ),
    lang(
        "HTML",
        "\u{e736}",
        Lexer::Html,
        &["htm", "html", "xhtml"],
        &[],
        &["html", "xhtml"],
    ),
    lang(
        "XML",
        "\u{f05c0}",
        Lexer::Xml,
        &[
            "atom",
            "csproj",
            "fsproj",
            "gpx",
            "kml",
            "plist",
            "props",
            "resx",
            "rss",
            "storyboard",
            "svg",
            "targets",
            "vbproj",
            "wsdl",
            "xaml",
            "xib",
            "xml",
            "xsd",
            "xsl",
            "xslt",
        ],
        &[],
        &["xml"],
    ),
    lang(
        "CSS",
        "\u{e749}",
        Lexer::Css,
        &["css", "pcss"],
        &[],
        &["css"],
    ),
    lang("SCSS", "\u{e603}", Lexer::Scss, &["scss"], &[], &["scss"]),
    lang(
        "Diff",
        "\u{f440}",
        Lexer::Diff,
        &["diff", "patch", "rej"],
        &[],
        &["diff", "patch", "udiff"],
    ),
    lang(
        "Makefile",
        "\u{e779}",
        Lexer::Make,
        &["mak", "mk"],
        &["BSDmakefile", "GNUmakefile", "Makefile", "makefile"],
        &["make", "makefile"],
    ),
    lang(
        "Dockerfile",
        "\u{f308}",
        Lexer::Docker,
        &["containerfile", "dockerfile"],
        &["Containerfile", "Dockerfile", "containerfile", "dockerfile"],
        &["containerfile", "docker", "dockerfile"],
    ),
    lang(
        "Markdown",
        "\u{e73e}",
        Lexer::Markdown,
        &["markdown", "md", "mdown", "mdwn", "mdx", "mkd", "mkdn"],
        &[],
        &["markdown", "md"],
    ),
    lang(
        "Scala",
        "\u{e737}",
        Lexer::Code(&SCALA),
        &["sbt", "sc", "scala"],
        &[],
        &["scala"],
    ),
    lang(
        "Dart",
        "\u{e798}",
        Lexer::Code(&DART),
        &["dart"],
        &[],
        &["dart"],
    ),
    lang(
        "Zig",
        "\u{e6a9}",
        Lexer::Code(&ZIG),
        &["zig", "zon"],
        &[],
        &["zig"],
    ),
    lang(
        "Nix",
        "\u{f313}",
        Lexer::Code(&NIX),
        &["nix"],
        &[],
        &["nix"],
    ),
    lang(
        "Haskell",
        "\u{e61f}",
        Lexer::Code(&HASKELL),
        &["hs"],
        &[],
        &["haskell", "hs"],
    ),
    lang(
        "Elixir",
        "\u{e62d}",
        Lexer::Code(&ELIXIR),
        &["ex", "exs"],
        &[],
        &["elixir"],
    ),
    lang(
        "HCL",
        "\u{e69a}",
        Lexer::Code(&HCL),
        &["hcl", "tf", "tfvars"],
        &[],
        &["hcl", "terraform"],
    ),
    lang(
        "GraphQL",
        "\u{f20e}",
        Lexer::Code(&GRAPHQL),
        &["gql", "graphql", "graphqls"],
        &[],
        &["graphql"],
    ),
    lang(
        "Protobuf",
        "\u{f0fd8}",
        Lexer::Code(&PROTOBUF),
        &["proto"],
        &[],
        &["proto", "proto3", "protobuf"],
    ),
    lang(
        "PowerShell",
        "\u{f0a0a}",
        Lexer::Code(&POWERSHELL),
        &["ps1", "psd1", "psm1"],
        &[],
        &["posh", "powershell", "ps1", "pwsh"],
    ),
    lang(
        "R",
        "\u{f07d4}",
        Lexer::Code(&R),
        &["r"],
        &[".Rprofile"],
        &["r"],
    ),
    lang(
        "Julia",
        "\u{e624}",
        Lexer::Code(&JULIA),
        &["jl"],
        &[],
        &["jl", "julia"],
    ),
    lang(
        "Perl",
        "\u{e769}",
        Lexer::Code(&PERL),
        &["pl", "pm", "pod"],
        &[],
        &["perl", "pl"],
    ),
];

const fn quote(open: &'static str, escape: u8, multiline: bool) -> Quote {
    Quote {
        open,
        close: open,
        escape,
        doubled: false,
        multiline,
    }
}

/// Doubled delimiters escape themselves, as in SQL `'it''s'`.
const fn doubled(open: &'static str, close: &'static str, escape: u8) -> Quote {
    Quote {
        open,
        close,
        escape,
        doubled: true,
        multiline: true,
    }
}

const C_QUOTES: &[Quote] = &[quote("\"", b'\\', false)];
const SCRIPT_QUOTES: &[Quote] = &[
    quote("\"", b'\\', true),
    quote("'", b'\\', true),
    quote("`", b'\\', true),
];
const JS_QUOTES: &[Quote] = &[
    quote("`", b'\\', true),
    quote("\"", b'\\', false),
    quote("'", b'\\', false),
];
const TRIPLE_QUOTES: &[Quote] = &[quote("\"\"\"", b'\\', true), quote("\"", b'\\', false)];
const RAW_TRIPLE_QUOTES: &[Quote] = &[quote("\"\"\"", 0, true), quote("\"", b'\\', false)];

const RUST: Grammar = Grammar {
    keywords: RUST_KEYWORDS,
    types: RUST_TYPES,
    constants: BOOL_CONSTANTS,
    fn_definers: FN_DEFINERS,
    type_definers: RUST_TYPE_DEFINERS,
    nested_comments: true,
    quotes: &[quote("\"", b'\\', true)],
    prefixes: RUST_PREFIXES,
    char_literals: true,
    caps: Caps::Types,
    dollar: true,
    hash_attributes: true,
    rust: true,
    ..Grammar::BASE
};

const PYTHON: Grammar = Grammar {
    keywords: PYTHON_KEYWORDS,
    types: PYTHON_TYPES,
    constants: PYTHON_CONSTANTS,
    builtins: PYTHON_BUILTINS,
    fn_definers: DEF_DEFINERS,
    type_definers: CLASS_DEFINERS,
    line_comments: &["#"],
    block_comment: None,
    quotes: &[
        quote("\"\"\"", b'\\', true),
        quote("'''", b'\\', true),
        quote("\"", b'\\', false),
        quote("'", b'\\', false),
    ],
    prefixes: PYTHON_PREFIXES,
    caps: Caps::Types,
    at: At::LineAttribute,
    ..Grammar::BASE
};

const JAVASCRIPT: Grammar = Grammar {
    keywords: JS_KEYWORDS,
    constants: JS_CONSTANTS,
    fn_definers: FUNCTION_DEFINERS,
    type_definers: CLASS_DEFINERS,
    quotes: JS_QUOTES,
    caps: Caps::Types,
    ident_start: b"$",
    ident_extra: b"$",
    at: At::Attribute,
    ..Grammar::BASE
};

const TYPESCRIPT: Grammar = Grammar {
    keywords: TS_KEYWORDS,
    types: TS_TYPES,
    type_definers: TS_TYPE_DEFINERS,
    ..JAVASCRIPT
};

const GO: Grammar = Grammar {
    keywords: GO_KEYWORDS,
    types: GO_TYPES,
    constants: GO_CONSTANTS,
    builtins: GO_BUILTINS,
    fn_definers: GO_FN_DEFINERS,
    type_definers: GO_TYPE_DEFINERS,
    quotes: &[quote("`", 0, true), quote("\"", b'\\', false)],
    char_literals: true,
    caps: Caps::Exported,
    ..Grammar::BASE
};

const C: Grammar = Grammar {
    keywords: C_KEYWORDS,
    types: C_TYPES,
    constants: C_CONSTANTS,
    type_definers: C_TYPE_DEFINERS,
    quotes: C_QUOTES,
    prefixes: C_PREFIXES,
    char_literals: true,
    preprocessor: true,
    c_types: true,
    ..Grammar::BASE
};

const CPP: Grammar = Grammar {
    keywords: CPP_KEYWORDS,
    types: CPP_TYPES,
    type_definers: CPP_TYPE_DEFINERS,
    prefixes: CPP_PREFIXES,
    cpp: true,
    ..C
};

const CSHARP: Grammar = Grammar {
    keywords: CS_KEYWORDS,
    types: CS_TYPES,
    constants: NULL_CONSTANTS,
    type_definers: CS_TYPE_DEFINERS,
    quotes: &[
        doubled("$@\"", "\"", 0),
        doubled("@$\"", "\"", 0),
        quote("$\"\"\"", 0, true),
        quote("\"\"\"", 0, true),
        doubled("@\"", "\"", 0),
        quote("$\"", b'\\', false),
        quote("\"", b'\\', false),
    ],
    char_literals: true,
    preprocessor: true,
    caps: Caps::Exported,
    ..Grammar::BASE
};

const JAVA: Grammar = Grammar {
    keywords: JAVA_KEYWORDS,
    types: JAVA_TYPES,
    constants: NULL_CONSTANTS,
    type_definers: JAVA_TYPE_DEFINERS,
    quotes: TRIPLE_QUOTES,
    char_literals: true,
    caps: Caps::Types,
    ident_start: b"$",
    ident_extra: b"$",
    at: At::Attribute,
    ..Grammar::BASE
};

const KOTLIN: Grammar = Grammar {
    keywords: KOTLIN_KEYWORDS,
    constants: NULL_CONSTANTS,
    fn_definers: KOTLIN_FN_DEFINERS,
    type_definers: KOTLIN_TYPE_DEFINERS,
    nested_comments: true,
    quotes: RAW_TRIPLE_QUOTES,
    char_literals: true,
    caps: Caps::Types,
    at: At::Attribute,
    ..Grammar::BASE
};

const SWIFT: Grammar = Grammar {
    keywords: SWIFT_KEYWORDS,
    constants: NIL_CONSTANTS,
    fn_definers: GO_FN_DEFINERS,
    type_definers: SWIFT_TYPE_DEFINERS,
    nested_comments: true,
    quotes: TRIPLE_QUOTES,
    caps: Caps::Types,
    at: At::Attribute,
    dollar: true,
    swift: true,
    ..Grammar::BASE
};

const RUBY: Grammar = Grammar {
    keywords: RUBY_KEYWORDS,
    constants: NIL_CONSTANTS,
    builtins: RUBY_BUILTINS,
    fn_definers: DEF_DEFINERS,
    type_definers: RUBY_TYPE_DEFINERS,
    line_comments: &["#"],
    block_comment: None,
    quotes: SCRIPT_QUOTES,
    caps: Caps::Types,
    ident_suffix: true,
    at: At::Variable,
    dollar: true,
    punct_vars: true,
    colon_symbols: true,
    key_symbols: true,
    heredoc: Heredoc::Angle,
    line_block: Some(("=begin", "=end")),
    ..Grammar::BASE
};

const PHP: Grammar = Grammar {
    keywords: PHP_KEYWORDS,
    types: PHP_TYPES,
    constants: NULL_CONSTANTS,
    fn_definers: FUNCTION_DEFINERS,
    type_definers: PHP_TYPE_DEFINERS,
    line_comments: &["//", "#"],
    quotes: SCRIPT_QUOTES,
    caps: Caps::Types,
    dollar: true,
    heredoc: Heredoc::Php,
    hash_attributes: true,
    php: true,
    ..Grammar::BASE
};

const LUA: Grammar = Grammar {
    keywords: LUA_KEYWORDS,
    constants: NIL_CONSTANTS,
    builtins: LUA_BUILTINS,
    fn_definers: FUNCTION_DEFINERS,
    line_comments: &["--"],
    block_comment: None,
    quotes: &[quote("\"", b'\\', false), quote("'", b'\\', false)],
    lua: true,
    ..Grammar::BASE
};

const SQL: Grammar = Grammar {
    keywords: SQL_KEYWORDS,
    types: SQL_TYPES,
    constants: SQL_CONSTANTS,
    line_comments: &["--"],
    quotes: &[
        doubled("'", "'", 0),
        doubled("\"", "\"", 0),
        doubled("`", "`", 0),
    ],
    prefixes: SQL_PREFIXES,
    fold_case: true,
    caps_constants: false,
    call_space: false,
    at: At::Variable,
    dollar: true,
    dollar_quotes: true,
    ..Grammar::BASE
};

const SCALA: Grammar = Grammar {
    keywords: SCALA_KEYWORDS,
    constants: NULL_CONSTANTS,
    fn_definers: DEF_DEFINERS,
    type_definers: SCALA_TYPE_DEFINERS,
    nested_comments: true,
    quotes: RAW_TRIPLE_QUOTES,
    char_literals: true,
    caps: Caps::Types,
    at: At::Attribute,
    ..Grammar::BASE
};

const DART: Grammar = Grammar {
    keywords: DART_KEYWORDS,
    types: DART_TYPES,
    constants: NULL_CONSTANTS,
    type_definers: DART_TYPE_DEFINERS,
    nested_comments: true,
    quotes: &[
        quote("'''", b'\\', true),
        quote("\"\"\"", b'\\', true),
        quote("'", b'\\', false),
        quote("\"", b'\\', false),
    ],
    prefixes: DART_PREFIXES,
    caps: Caps::Types,
    at: At::Attribute,
    ..Grammar::BASE
};

const ZIG: Grammar = Grammar {
    keywords: ZIG_KEYWORDS,
    types: ZIG_TYPES,
    constants: ZIG_CONSTANTS,
    fn_definers: FN_DEFINERS,
    block_comment: None,
    quotes: C_QUOTES,
    char_literals: true,
    caps: Caps::Types,
    at: At::Macro,
    zig: true,
    ..Grammar::BASE
};

const NIX: Grammar = Grammar {
    keywords: NIX_KEYWORDS,
    constants: NULL_CONSTANTS,
    builtins: NIX_BUILTINS,
    line_comments: &["#"],
    quotes: &[quote("''", 0, true), quote("\"", b'\\', true)],
    ident_extra: b"-'",
    assign_property: true,
    ..Grammar::BASE
};

const HASKELL: Grammar = Grammar {
    keywords: HASKELL_KEYWORDS,
    constants: HASKELL_CONSTANTS,
    type_definers: HASKELL_TYPE_DEFINERS,
    line_comments: &["--"],
    block_comment: Some(("{-", "-}")),
    nested_comments: true,
    quotes: C_QUOTES,
    char_literals: true,
    caps: Caps::Types,
    caps_constants: false,
    ident_extra: b"'",
    ..Grammar::BASE
};

const ELIXIR: Grammar = Grammar {
    keywords: ELIXIR_KEYWORDS,
    constants: NIL_CONSTANTS,
    fn_definers: ELIXIR_FN_DEFINERS,
    type_definers: ELIXIR_TYPE_DEFINERS,
    line_comments: &["#"],
    block_comment: None,
    quotes: &[
        quote("\"\"\"", b'\\', true),
        quote("'''", b'\\', true),
        quote("\"", b'\\', true),
        quote("'", b'\\', true),
    ],
    caps: Caps::Types,
    caps_constants: false,
    ident_suffix: true,
    at: At::Attribute,
    colon_symbols: true,
    key_symbols: true,
    ..Grammar::BASE
};

const HCL: Grammar = Grammar {
    keywords: HCL_KEYWORDS,
    types: HCL_TYPES,
    constants: NULL_CONSTANTS,
    line_comments: &["#", "//"],
    quotes: C_QUOTES,
    ident_extra: b"-",
    heredoc: Heredoc::Angle,
    assign_property: true,
    ..Grammar::BASE
};

const GRAPHQL: Grammar = Grammar {
    keywords: GRAPHQL_KEYWORDS,
    types: GRAPHQL_TYPES,
    constants: NULL_CONSTANTS,
    type_definers: GRAPHQL_TYPE_DEFINERS,
    line_comments: &["#"],
    block_comment: None,
    quotes: RAW_TRIPLE_QUOTES,
    caps: Caps::Types,
    at: At::Attribute,
    dollar: true,
    ..Grammar::BASE
};

const PROTOBUF: Grammar = Grammar {
    keywords: PROTO_KEYWORDS,
    types: PROTO_TYPES,
    constants: BOOL_CONSTANTS,
    fn_definers: PROTO_FN_DEFINERS,
    type_definers: PROTO_TYPE_DEFINERS,
    quotes: &[quote("\"", b'\\', false), quote("'", b'\\', false)],
    caps: Caps::Types,
    ..Grammar::BASE
};

const POWERSHELL: Grammar = Grammar {
    keywords: PS_KEYWORDS,
    types: PS_TYPES,
    fn_definers: PS_FN_DEFINERS,
    type_definers: PS_TYPE_DEFINERS,
    dash_operators: PS_OPERATORS,
    line_comments: &["#"],
    block_comment: Some(("<#", "#>")),
    quotes: &[
        Quote {
            open: "@\"",
            close: "\"@",
            escape: b'`',
            doubled: false,
            multiline: true,
        },
        quote("@'", 0, true),
        doubled("\"", "\"", b'`'),
        doubled("'", "'", 0),
    ],
    fold_case: true,
    at: At::Variable,
    dollar: true,
    punct_vars: true,
    powershell: true,
    ..Grammar::BASE
};

const R: Grammar = Grammar {
    keywords: R_KEYWORDS,
    constants: R_CONSTANTS,
    line_comments: &["#"],
    block_comment: None,
    quotes: &[quote("\"", b'\\', true), quote("'", b'\\', true)],
    ident_extra: b".",
    ..Grammar::BASE
};

const JULIA: Grammar = Grammar {
    keywords: JULIA_KEYWORDS,
    constants: JULIA_CONSTANTS,
    fn_definers: JULIA_FN_DEFINERS,
    type_definers: JULIA_TYPE_DEFINERS,
    line_comments: &["#"],
    block_comment: Some(("#=", "=#")),
    nested_comments: true,
    quotes: &[quote("\"\"\"", b'\\', true), quote("\"", b'\\', true)],
    char_literals: true,
    caps: Caps::Types,
    ident_suffix: true,
    at: At::Macro,
    colon_symbols: true,
    ..Grammar::BASE
};

const PERL: Grammar = Grammar {
    keywords: PERL_KEYWORDS,
    builtins: PERL_BUILTINS,
    fn_definers: PERL_FN_DEFINERS,
    type_definers: PERL_TYPE_DEFINERS,
    line_comments: &["#"],
    block_comment: None,
    quotes: SCRIPT_QUOTES,
    at: At::Variable,
    dollar: true,
    punct_vars: true,
    heredoc: Heredoc::Angle,
    line_block: Some(("=", "=cut")),
    perl: true,
    ..Grammar::BASE
};

// Word tables: sorted and deduplicated (checked by tests); case-insensitive
// grammars use lowercase tables.
pub(super) use words::*;

#[rustfmt::skip]
mod words {
    pub(crate) const RUST_KEYWORDS: &[&str] = &[
        "Self", "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
        "enum", "extern", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait", "type",
        "union", "unsafe", "use", "where", "while", "yield",
    ];
    pub(crate) const RUST_TYPES: &[&str] = &[
        "bool", "char", "f32", "f64", "i128", "i16", "i32", "i64", "i8", "isize", "str", "u128",
        "u16", "u32", "u64", "u8", "usize",
    ];
    pub(crate) const RUST_PREFIXES: &[&str] = &["b", "br", "c", "cr", "r"];
    pub(crate) const RUST_TYPE_DEFINERS: &[&str] = &["enum", "struct", "trait", "type", "union"];
    pub(crate) const PYTHON_KEYWORDS: &[&str] = &[
        "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif",
        "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda",
        "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with", "yield",
    ];
    pub(crate) const PYTHON_TYPES: &[&str] = &[
        "bool", "bytearray", "bytes", "complex", "dict", "float", "frozenset", "int", "list",
        "memoryview", "object", "set", "str", "tuple", "type",
    ];
    pub(crate) const PYTHON_CONSTANTS: &[&str] = &[
        "Ellipsis", "False", "None", "NotImplemented", "True", "__debug__",
    ];
    pub(crate) const PYTHON_BUILTINS: &[&str] = &[
        "__import__", "abs", "aiter", "all", "anext", "any", "ascii", "bin", "breakpoint",
        "callable", "chr", "classmethod", "compile", "delattr", "dir", "divmod", "enumerate",
        "eval", "exec", "filter", "format", "getattr", "globals", "hasattr", "hash", "help", "hex",
        "id", "input", "isinstance", "issubclass", "iter", "len", "locals", "map", "max", "min",
        "next", "oct", "open", "ord", "pow", "print", "property", "range", "repr", "reversed",
        "round", "setattr", "slice", "sorted", "staticmethod", "sum", "super", "vars", "zip",
    ];
    pub(crate) const PYTHON_PREFIXES: &[&str] = &[
        "B", "BR", "Br", "F", "FR", "Fr", "R", "RB", "RF", "RT", "Rb", "Rf", "Rt", "T", "TR", "Tr",
        "U", "b", "bR", "br", "f", "fR", "fr", "r", "rB", "rF", "rT", "rb", "rf", "rt", "t", "tR",
        "tr", "u",
    ];
    pub(crate) const DEF_DEFINERS: &[&str] = &["def"];
    pub(crate) const CLASS_DEFINERS: &[&str] = &["class"];
    pub(crate) const JS_KEYWORDS: &[&str] = &[
        "as", "async", "await", "break", "case", "catch", "class", "const", "continue", "debugger",
        "default", "delete", "do", "else", "export", "extends", "finally", "for", "from",
        "function", "if", "import", "in", "instanceof", "let", "new", "of", "return", "static",
        "super", "switch", "this", "throw", "try", "typeof", "var", "void", "while", "with",
        "yield",
    ];
    pub(crate) const JS_CONSTANTS: &[&str] = &[
        "Infinity", "NaN", "false", "null", "true", "undefined",
    ];
    pub(crate) const TS_KEYWORDS: &[&str] = &[
        "abstract", "accessor", "as", "asserts", "async", "await", "break", "case", "catch",
        "class", "const", "continue", "debugger", "declare", "default", "delete", "do", "else",
        "enum", "export", "extends", "finally", "for", "from", "function", "if", "implements",
        "import", "in", "infer", "instanceof", "interface", "is", "keyof", "let", "module",
        "namespace", "new", "of", "override", "private", "protected", "public", "readonly",
        "return", "satisfies", "static", "super", "switch", "this", "throw", "try", "type",
        "typeof", "unique", "var", "while", "with", "yield",
    ];
    pub(crate) const TS_TYPES: &[&str] = &[
        "any", "bigint", "boolean", "never", "number", "object", "string", "symbol", "unknown",
        "void",
    ];
    pub(crate) const TS_TYPE_DEFINERS: &[&str] = &[
        "class", "enum", "interface", "namespace", "type",
    ];
    pub(crate) const FUNCTION_DEFINERS: &[&str] = &["function"];
    pub(crate) const GO_KEYWORDS: &[&str] = &[
        "break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough",
        "for", "func", "go", "goto", "if", "import", "interface", "map", "package", "range",
        "return", "select", "struct", "switch", "type", "var",
    ];
    pub(crate) const GO_TYPES: &[&str] = &[
        "any", "bool", "byte", "comparable", "complex128", "complex64", "error", "float32",
        "float64", "int", "int16", "int32", "int64", "int8", "rune", "string", "uint", "uint16",
        "uint32", "uint64", "uint8", "uintptr",
    ];
    pub(crate) const GO_CONSTANTS: &[&str] = &["false", "iota", "nil", "true"];
    pub(crate) const GO_BUILTINS: &[&str] = &[
        "append", "cap", "clear", "close", "complex", "copy", "delete", "imag", "len", "make",
        "max", "min", "new", "panic", "print", "println", "real", "recover",
    ];
    pub(crate) const GO_FN_DEFINERS: &[&str] = &["func"];
    pub(crate) const GO_TYPE_DEFINERS: &[&str] = &["type"];
    pub(crate) const C_KEYWORDS: &[&str] = &[
        "_Alignas", "_Alignof", "_Atomic", "_Generic", "_Noreturn", "_Static_assert",
        "_Thread_local", "alignas", "alignof", "asm", "auto", "break", "case", "const", "constexpr",
        "continue", "default", "do", "else", "enum", "extern", "for", "goto", "if", "inline",
        "register", "restrict", "return", "sizeof", "static", "static_assert", "struct", "switch",
        "thread_local", "typedef", "typeof", "typeof_unqual", "union", "volatile", "while",
    ];
    pub(crate) const C_TYPES: &[&str] = &[
        "FILE", "_Bool", "_Complex", "bool", "char", "double", "float", "int", "long", "short",
        "signed", "unsigned", "void",
    ];
    pub(crate) const C_CONSTANTS: &[&str] = &["NULL", "false", "nullptr", "true"];
    pub(crate) const C_PREFIXES: &[&str] = &["L", "U", "u", "u8"];
    pub(crate) const C_TYPE_DEFINERS: &[&str] = &["enum", "struct", "union"];
    pub(crate) const CPP_KEYWORDS: &[&str] = &[
        "alignas", "alignof", "and", "and_eq", "asm", "auto", "bitand", "bitor", "break", "case",
        "catch", "class", "co_await", "co_return", "co_yield", "compl", "concept", "const",
        "const_cast", "consteval", "constexpr", "constinit", "continue", "decltype", "default",
        "delete", "do", "dynamic_cast", "else", "enum", "explicit", "export", "extern", "final",
        "for", "friend", "goto", "if", "import", "inline", "module", "mutable", "namespace", "new",
        "noexcept", "not", "not_eq", "operator", "or", "or_eq", "override", "private", "protected",
        "public", "register", "reinterpret_cast", "requires", "return", "sizeof", "static",
        "static_assert", "static_cast", "struct", "switch", "template", "this", "thread_local",
        "throw", "try", "typedef", "typeid", "typename", "union", "using", "virtual", "volatile",
        "while", "xor", "xor_eq",
    ];
    pub(crate) const CPP_TYPES: &[&str] = &[
        "bool", "char", "char16_t", "char32_t", "char8_t", "double", "float", "int", "long",
        "short", "signed", "unsigned", "void", "wchar_t",
    ];
    pub(crate) const CPP_PREFIXES: &[&str] = &["L", "LR", "R", "U", "UR", "u", "u8", "u8R", "uR"];
    pub(crate) const CPP_TYPE_DEFINERS: &[&str] = &[
        "class", "concept", "enum", "namespace", "struct", "union",
    ];
    pub(crate) const CS_KEYWORDS: &[&str] = &[
        "abstract", "as", "async", "await", "base", "break", "case", "catch", "checked", "class",
        "const", "continue", "default", "delegate", "do", "else", "enum", "event", "explicit",
        "extern", "finally", "fixed", "for", "foreach", "get", "goto", "if", "implicit", "in",
        "init", "interface", "internal", "is", "lock", "nameof", "namespace", "new", "operator",
        "out", "override", "params", "partial", "private", "protected", "public", "readonly",
        "record", "ref", "required", "return", "sealed", "set", "sizeof", "stackalloc", "static",
        "struct", "switch", "this", "throw", "try", "typeof", "unchecked", "unsafe", "using", "var",
        "virtual", "volatile", "when", "where", "while", "with", "yield",
    ];
    pub(crate) const CS_TYPES: &[&str] = &[
        "bool", "byte", "char", "decimal", "double", "dynamic", "float", "int", "long", "nint",
        "nuint", "object", "sbyte", "short", "string", "uint", "ulong", "ushort", "void",
    ];
    pub(crate) const CS_TYPE_DEFINERS: &[&str] = &[
        "class", "enum", "interface", "record", "struct",
    ];
    pub(crate) const NULL_CONSTANTS: &[&str] = &["false", "null", "true"];
    pub(crate) const JAVA_KEYWORDS: &[&str] = &[
        "abstract", "assert", "break", "case", "catch", "class", "const", "continue", "default",
        "do", "else", "enum", "exports", "extends", "final", "finally", "for", "goto", "if",
        "implements", "import", "instanceof", "interface", "module", "native", "new", "open",
        "opens", "package", "permits", "private", "protected", "provides", "public", "record",
        "requires", "return", "sealed", "static", "strictfp", "super", "switch", "synchronized",
        "this", "throw", "throws", "to", "transient", "transitive", "try", "uses", "var",
        "volatile", "when", "while", "with", "yield",
    ];
    pub(crate) const JAVA_TYPES: &[&str] = &[
        "boolean", "byte", "char", "double", "float", "int", "long", "short", "void",
    ];
    pub(crate) const JAVA_TYPE_DEFINERS: &[&str] = &["class", "enum", "interface", "record"];
    pub(crate) const KOTLIN_KEYWORDS: &[&str] = &[
        "abstract", "actual", "annotation", "as", "break", "by", "catch", "class", "companion",
        "const", "constructor", "continue", "crossinline", "data", "do", "dynamic", "else", "enum",
        "expect", "external", "final", "finally", "for", "fun", "get", "if", "import", "in",
        "infix", "init", "inline", "inner", "interface", "internal", "is", "lateinit", "noinline",
        "object", "open", "operator", "out", "override", "package", "private", "protected",
        "public", "reified", "return", "sealed", "set", "super", "suspend", "tailrec", "this",
        "throw", "try", "typealias", "typeof", "val", "value", "var", "vararg", "when", "where",
        "while",
    ];
    pub(crate) const KOTLIN_FN_DEFINERS: &[&str] = &["fun"];
    pub(crate) const KOTLIN_TYPE_DEFINERS: &[&str] = &["class", "interface", "object", "typealias"];
    pub(crate) const SWIFT_KEYWORDS: &[&str] = &[
        "Self", "actor", "any", "as", "associatedtype", "async", "await", "borrowing", "break",
        "case", "catch", "class", "consume", "consuming", "continue", "convenience", "default",
        "defer", "deinit", "didSet", "do", "dynamic", "else", "enum", "extension", "fallthrough",
        "fileprivate", "final", "for", "func", "get", "guard", "if", "import", "in", "indirect",
        "infix", "init", "inout", "internal", "is", "lazy", "let", "mutating", "nonisolated",
        "nonmutating", "open", "operator", "optional", "override", "package", "postfix",
        "precedencegroup", "prefix", "private", "protocol", "public", "repeat", "required",
        "rethrows", "return", "self", "set", "some", "static", "struct", "subscript", "super",
        "switch", "throw", "throws", "try", "typealias", "unowned", "var", "weak", "where", "while",
        "willSet",
    ];
    pub(crate) const SWIFT_TYPE_DEFINERS: &[&str] = &[
        "actor", "class", "enum", "extension", "protocol", "struct", "typealias",
    ];
    pub(crate) const NIL_CONSTANTS: &[&str] = &["false", "nil", "true"];
    pub(crate) const RUBY_KEYWORDS: &[&str] = &[
        "BEGIN", "END", "__ENCODING__", "__FILE__", "__LINE__", "alias", "and", "begin", "break",
        "case", "class", "def", "defined?", "do", "else", "elsif", "end", "ensure", "for", "if",
        "in", "module", "next", "not", "or", "redo", "rescue", "retry", "return", "self", "super",
        "then", "undef", "unless", "until", "when", "while", "yield",
    ];
    pub(crate) const RUBY_BUILTINS: &[&str] = &[
        "attr_accessor", "attr_reader", "attr_writer", "extend", "include", "lambda", "loop", "p",
        "prepend", "print", "private", "proc", "protected", "public", "puts", "raise", "require",
        "require_relative",
    ];
    pub(crate) const RUBY_TYPE_DEFINERS: &[&str] = &["class", "module"];
    pub(crate) const PHP_KEYWORDS: &[&str] = &[
        "abstract", "and", "as", "break", "case", "catch", "class", "clone", "const", "continue",
        "declare", "default", "do", "echo", "else", "elseif", "empty", "enddeclare", "endfor",
        "endforeach", "endif", "endswitch", "endwhile", "enum", "extends", "final", "finally", "fn",
        "for", "foreach", "function", "global", "goto", "if", "implements", "include",
        "include_once", "instanceof", "insteadof", "interface", "isset", "list", "match",
        "namespace", "new", "or", "parent", "print", "private", "protected", "public", "readonly",
        "require", "require_once", "return", "self", "static", "switch", "throw", "trait", "try",
        "unset", "use", "var", "while", "xor", "yield",
    ];
    pub(crate) const PHP_TYPES: &[&str] = &[
        "array", "bool", "callable", "float", "int", "iterable", "mixed", "never", "object",
        "string", "void",
    ];
    pub(crate) const PHP_TYPE_DEFINERS: &[&str] = &["class", "enum", "interface", "trait"];
    pub(crate) const LUA_KEYWORDS: &[&str] = &[
        "and", "break", "do", "else", "elseif", "end", "for", "function", "goto", "if", "in",
        "local", "not", "or", "repeat", "return", "then", "until", "while",
    ];
    pub(crate) const LUA_BUILTINS: &[&str] = &[
        "assert", "collectgarbage", "dofile", "error", "getmetatable", "ipairs", "load", "loadfile",
        "next", "pairs", "pcall", "print", "rawequal", "rawget", "rawlen", "rawset", "require",
        "select", "setmetatable", "tonumber", "tostring", "type", "unpack", "xpcall",
    ];
    pub(crate) const SQL_KEYWORDS: &[&str] = &[
        "abort", "action", "add", "after", "all", "alter", "analyze", "and", "any", "as", "asc",
        "attach", "auto_increment", "before", "begin", "between", "both", "by", "call", "cascade",
        "case", "cast", "check", "collate", "column", "comment", "commit", "conflict", "constraint",
        "create", "cross", "current_date", "current_time", "current_timestamp", "database",
        "declare", "default", "deferrable", "delete", "desc", "distinct", "do", "drop", "each",
        "else", "elsif", "end", "escape", "except", "exception", "exclude", "execute", "exists",
        "explain", "extension", "fetch", "filter", "first", "following", "for", "foreign", "from",
        "full", "function", "grant", "group", "having", "if", "ignore", "ilike", "immediate", "in",
        "index", "inner", "insert", "instead", "intersect", "into", "is", "isnull", "join", "key",
        "language", "last", "lateral", "leading", "left", "like", "limit", "loop", "materialized",
        "natural", "no", "not", "nothing", "notnull", "nulls", "of", "offset", "on", "only", "or",
        "order", "outer", "over", "partition", "perform", "plan", "pragma", "preceding", "primary",
        "procedure", "raise", "range", "recursive", "references", "reindex", "release", "rename",
        "replace", "restrict", "return", "returning", "returns", "revoke", "right", "rollback",
        "row", "rows", "savepoint", "schema", "select", "sequence", "set", "similar", "table",
        "temp", "temporary", "then", "ties", "to", "trailing", "transaction", "trigger", "truncate",
        "unbounded", "union", "unique", "update", "using", "vacuum", "values", "view", "virtual",
        "when", "where", "while", "window", "with", "without",
    ];
    pub(crate) const SQL_TYPES: &[&str] = &[
        "bigint", "bigserial", "binary", "bit", "blob", "bool", "boolean", "bytea", "char",
        "character", "date", "datetime", "decimal", "double", "enum", "float", "float4", "float8",
        "inet", "int", "int2", "int4", "int8", "integer", "interval", "json", "jsonb", "longtext",
        "mediumint", "money", "nchar", "numeric", "nvarchar", "precision", "real", "serial",
        "smallint", "smallserial", "text", "time", "timestamp", "timestamptz", "tinyint",
        "tinytext", "uuid", "varbinary", "varchar", "varying", "xml", "year",
    ];
    pub(crate) const SQL_CONSTANTS: &[&str] = &["false", "null", "true", "unknown"];
    pub(crate) const SQL_PREFIXES: &[&str] = &["B", "E", "N", "X", "b", "e", "n", "x"];
    pub(crate) const SCALA_KEYWORDS: &[&str] = &[
        "abstract", "case", "catch", "class", "def", "derives", "do", "else", "end", "enum",
        "export", "extends", "extension", "final", "finally", "for", "forSome", "given", "if",
        "implicit", "import", "infix", "inline", "lazy", "match", "new", "object", "opaque", "open",
        "override", "package", "private", "protected", "return", "sealed", "super", "then", "this",
        "throw", "trait", "transparent", "try", "type", "using", "val", "var", "while", "with",
        "yield",
    ];
    pub(crate) const SCALA_TYPE_DEFINERS: &[&str] = &["class", "enum", "object", "trait", "type"];
    pub(crate) const DART_KEYWORDS: &[&str] = &[
        "abstract", "as", "assert", "async", "await", "base", "break", "case", "catch", "class",
        "const", "continue", "covariant", "default", "deferred", "do", "else", "enum", "export",
        "extends", "extension", "external", "factory", "final", "finally", "for", "get", "hide",
        "if", "implements", "import", "in", "interface", "is", "late", "library", "mixin", "new",
        "on", "operator", "part", "required", "rethrow", "return", "sealed", "set", "show",
        "static", "super", "switch", "sync", "this", "throw", "try", "typedef", "var", "when",
        "while", "with", "yield",
    ];
    pub(crate) const DART_TYPES: &[&str] = &["bool", "double", "dynamic", "int", "num", "void"];
    pub(crate) const DART_TYPE_DEFINERS: &[&str] = &[
        "class", "enum", "extension", "mixin", "typedef",
    ];
    pub(crate) const DART_PREFIXES: &[&str] = &["r"];
    pub(crate) const ZIG_KEYWORDS: &[&str] = &[
        "addrspace", "align", "allowzero", "and", "anyframe", "anytype", "asm", "async", "await",
        "break", "callconv", "catch", "comptime", "const", "continue", "defer", "else", "enum",
        "errdefer", "error", "export", "extern", "fn", "for", "if", "inline", "linksection",
        "noalias", "noinline", "nosuspend", "opaque", "or", "orelse", "packed", "pub", "resume",
        "return", "struct", "suspend", "switch", "test", "threadlocal", "try", "union",
        "unreachable", "usingnamespace", "var", "volatile", "while",
    ];
    pub(crate) const ZIG_TYPES: &[&str] = &[
        "anyerror", "anyopaque", "bool", "c_char", "c_int", "c_long", "c_longdouble", "c_longlong",
        "c_short", "c_uint", "c_ulong", "c_ulonglong", "c_ushort", "comptime_float", "comptime_int",
        "f128", "f16", "f32", "f64", "f80", "i128", "i16", "i32", "i64", "i8", "isize", "noreturn",
        "type", "u128", "u16", "u32", "u64", "u8", "usize", "void",
    ];
    pub(crate) const ZIG_CONSTANTS: &[&str] = &["false", "null", "true", "undefined"];
    pub(crate) const FN_DEFINERS: &[&str] = &["fn"];
    pub(crate) const NIX_KEYWORDS: &[&str] = &[
        "assert", "else", "if", "in", "inherit", "let", "or", "rec", "then", "with",
    ];
    pub(crate) const NIX_BUILTINS: &[&str] = &[
        "abort", "baseNameOf", "builtins", "derivation", "dirOf", "fetchGit", "fetchTarball",
        "fetchurl", "import", "isNull", "map", "placeholder", "removeAttrs", "throw", "toString",
    ];
    pub(crate) const HASKELL_KEYWORDS: &[&str] = &[
        "as", "case", "class", "data", "default", "deriving", "do", "else", "family", "forall",
        "foreign", "hiding", "if", "import", "in", "infix", "infixl", "infixr", "instance", "let",
        "mdo", "module", "newtype", "of", "proc", "qualified", "rec", "then", "type", "where",
    ];
    pub(crate) const HASKELL_CONSTANTS: &[&str] = &["False", "True"];
    pub(crate) const HASKELL_TYPE_DEFINERS: &[&str] = &[
        "class", "data", "instance", "newtype", "type",
    ];
    pub(crate) const ELIXIR_KEYWORDS: &[&str] = &[
        "after", "alias", "and", "case", "catch", "cond", "def", "defdelegate", "defexception",
        "defguard", "defguardp", "defimpl", "defmacro", "defmacrop", "defmodule", "defoverridable",
        "defp", "defprotocol", "defstruct", "do", "else", "end", "fn", "for", "if", "import", "in",
        "not", "or", "quote", "raise", "receive", "require", "reraise", "rescue", "try", "unless",
        "unquote", "use", "when", "with",
    ];
    pub(crate) const ELIXIR_FN_DEFINERS: &[&str] = &[
        "def", "defdelegate", "defguard", "defguardp", "defmacro", "defmacrop", "defp",
    ];
    pub(crate) const ELIXIR_TYPE_DEFINERS: &[&str] = &["defimpl", "defmodule", "defprotocol"];
    pub(crate) const HCL_KEYWORDS: &[&str] = &[
        "check", "data", "dynamic", "else", "endfor", "endif", "for", "if", "import", "in",
        "locals", "module", "moved", "output", "provider", "removed", "resource", "terraform",
        "variable",
    ];
    pub(crate) const HCL_TYPES: &[&str] = &[
        "any", "bool", "list", "map", "number", "object", "set", "string", "tuple",
    ];
    pub(crate) const GRAPHQL_KEYWORDS: &[&str] = &[
        "directive", "enum", "extend", "fragment", "implements", "input", "interface", "mutation",
        "on", "query", "repeatable", "scalar", "schema", "subscription", "type", "union",
    ];
    pub(crate) const GRAPHQL_TYPE_DEFINERS: &[&str] = &[
        "enum", "fragment", "input", "interface", "scalar", "type", "union",
    ];
    pub(crate) const GRAPHQL_TYPES: &[&str] = &["Boolean", "Float", "ID", "Int", "String"];
    pub(crate) const PROTO_KEYWORDS: &[&str] = &[
        "edition", "enum", "extend", "extensions", "import", "map", "max", "message", "oneof",
        "option", "optional", "package", "public", "repeated", "required", "reserved", "returns",
        "rpc", "service", "stream", "syntax", "to", "weak",
    ];
    pub(crate) const PROTO_TYPES: &[&str] = &[
        "bool", "bytes", "double", "fixed32", "fixed64", "float", "int32", "int64", "sfixed32",
        "sfixed64", "sint32", "sint64", "string", "uint32", "uint64",
    ];
    pub(crate) const PROTO_FN_DEFINERS: &[&str] = &["rpc"];
    pub(crate) const PROTO_TYPE_DEFINERS: &[&str] = &["enum", "message", "service"];
    pub(crate) const BOOL_CONSTANTS: &[&str] = &["false", "true"];
    pub(crate) const PS_KEYWORDS: &[&str] = &[
        "begin", "break", "catch", "class", "clean", "continue", "data", "define", "do",
        "dynamicparam", "else", "elseif", "end", "enum", "exit", "filter", "finally", "for",
        "foreach", "from", "function", "hidden", "if", "in", "param", "process", "return", "static",
        "switch", "throw", "trap", "try", "until", "using", "var", "while", "workflow",
    ];
    pub(crate) const PS_TYPES: &[&str] = &[
        "array", "bool", "byte", "char", "datetime", "decimal", "double", "float", "guid",
        "hashtable", "int", "int16", "int32", "int64", "long", "object", "pscustomobject",
        "psobject", "regex", "sbyte", "scriptblock", "single", "string", "timespan", "uint16",
        "uint32", "uint64", "void", "xml",
    ];
    pub(crate) const PS_OPERATORS: &[&str] = &[
        "and", "as", "band", "bnot", "bor", "bxor", "ccontains", "ceq", "cge", "cgt", "cin",
        "clike", "cmatch", "cne", "cnotcontains", "cnotlike", "cnotmatch", "contains", "creplace",
        "eq", "f", "ge", "gt", "icontains", "ieq", "ilike", "imatch", "in", "ine", "ireplace", "is",
        "isnot", "join", "le", "like", "lt", "match", "ne", "not", "notcontains", "notin",
        "notlike", "notmatch", "or", "replace", "shl", "shr", "split", "xor",
    ];
    pub(crate) const PS_FN_DEFINERS: &[&str] = &["filter", "function"];
    pub(crate) const PS_TYPE_DEFINERS: &[&str] = &["class", "enum"];
    pub(crate) const R_KEYWORDS: &[&str] = &[
        "break", "else", "for", "function", "if", "in", "next", "repeat", "while",
    ];
    pub(crate) const R_CONSTANTS: &[&str] = &[
        "FALSE", "Inf", "NA", "NA_character_", "NA_complex_", "NA_integer_", "NA_real_", "NULL",
        "NaN", "TRUE",
    ];
    pub(crate) const JULIA_KEYWORDS: &[&str] = &[
        "abstract", "baremodule", "begin", "break", "catch", "const", "continue", "do", "else",
        "elseif", "end", "export", "finally", "for", "function", "global", "if", "import", "in",
        "isa", "let", "local", "macro", "module", "mutable", "primitive", "public", "quote",
        "return", "struct", "try", "type", "using", "where", "while",
    ];
    pub(crate) const JULIA_CONSTANTS: &[&str] = &[
        "Inf", "NaN", "false", "missing", "nothing", "true",
    ];
    pub(crate) const JULIA_FN_DEFINERS: &[&str] = &["function", "macro"];
    pub(crate) const JULIA_TYPE_DEFINERS: &[&str] = &["module", "struct", "type"];
    pub(crate) const PERL_KEYWORDS: &[&str] = &[
        "BEGIN", "END", "__DATA__", "__END__", "__FILE__", "__LINE__", "__PACKAGE__", "and", "cmp",
        "continue", "default", "do", "else", "elsif", "eq", "for", "foreach", "ge", "given", "gt",
        "if", "last", "le", "local", "lt", "my", "ne", "next", "no", "not", "or", "our", "package",
        "redo", "require", "return", "state", "sub", "unless", "until", "use", "when", "while", "x",
        "xor",
    ];
    pub(crate) const PERL_BUILTINS: &[&str] = &[
        "bless", "chdir", "chomp", "chop", "chr", "close", "defined", "delete", "die", "each",
        "eval", "exec", "exists", "exit", "grep", "join", "keys", "lc", "length", "map", "mkdir",
        "open", "opendir", "ord", "pop", "print", "printf", "push", "readline", "ref", "rename",
        "reverse", "rmdir", "say", "scalar", "shift", "sort", "splice", "split", "sprintf",
        "substr", "uc", "undef", "unlink", "unshift", "values", "wait", "wantarray", "warn",
    ];
    pub(crate) const PERL_FN_DEFINERS: &[&str] = &["sub"];
    pub(crate) const PERL_TYPE_DEFINERS: &[&str] = &["package"];
    pub(crate) const SHELL_KEYWORDS: &[&str] = &[
        "!", "[[", "]]", "break", "case", "continue", "coproc", "do", "done", "elif", "else",
        "esac", "fi", "for", "function", "if", "in", "return", "select", "then", "time", "until",
        "while", "{", "}",
    ];
    pub(crate) const SHELL_BUILTINS: &[&str] = &[
        ".", ":", "[", "alias", "bg", "bind", "builtin", "caller", "cd", "command", "compgen",
        "complete", "declare", "dirs", "disown", "echo", "enable", "eval", "exec", "exit", "export",
        "false", "fc", "fg", "getopts", "hash", "help", "history", "jobs", "kill", "let", "local",
        "logout", "mapfile", "popd", "printf", "pushd", "pwd", "read", "readarray", "readonly",
        "set", "shift", "shopt", "source", "suspend", "test", "times", "trap", "true", "type",
        "typeset", "ulimit", "umask", "unalias", "unset", "wait",
    ];
    pub(crate) const MAKE_DIRECTIVES: &[&str] = &[
        "-include", "define", "else", "endef", "endif", "export", "ifdef", "ifeq", "ifndef",
        "ifneq", "include", "override", "private", "sinclude", "undefine", "unexport", "vpath",
    ];
    pub(crate) const MAKE_FUNCTIONS: &[&str] = &[
        "abspath", "addprefix", "addsuffix", "and", "basename", "call", "dir", "error", "eval",
        "file", "filter", "filter-out", "findstring", "firstword", "flavor", "foreach", "guile",
        "if", "info", "intcmp", "join", "lastword", "let", "notdir", "or", "origin", "patsubst",
        "realpath", "shell", "sort", "strip", "subst", "suffix", "value", "warning", "wildcard",
        "word", "wordlist", "words",
    ];
    pub(crate) const MAKE_TARGETS: &[&str] = &[
        ".DEFAULT", ".DELETE_ON_ERROR", ".EXPORT_ALL_VARIABLES", ".IGNORE", ".INTERMEDIATE",
        ".LOW_RESOLUTION_TIME", ".NOTINTERMEDIATE", ".NOTPARALLEL", ".ONESHELL", ".PHONY", ".POSIX",
        ".PRECIOUS", ".SECONDARY", ".SECONDEXPANSION", ".SILENT", ".SUFFIXES", ".WAIT",
    ];
    pub(crate) const DOCKER_INSTRUCTIONS: &[&str] = &[
        "add", "arg", "cmd", "copy", "entrypoint", "env", "expose", "from", "healthcheck", "label",
        "maintainer", "onbuild", "run", "shell", "stopsignal", "user", "volume", "workdir",
    ];
    pub(crate) const YAML_CONSTANTS: &[&str] = &[
        "false", "no", "null", "off", "on", "true", "yes", "~",
    ];
    pub(crate) const INI_CONSTANTS: &[&str] = &["false", "no", "off", "on", "true", "yes"];
}
