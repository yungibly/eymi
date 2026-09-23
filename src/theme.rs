//! Explicit color pairs keep the writing surface readable on any terminal.
use ratatui::style::{Color, Style};
use std::{cell::Cell, sync::OnceLock};

#[path = "theme_data.rs"]
mod data;
use data::BUILTINS;

mod chrome;
pub use chrome::ChromePalette;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Dark,
    Light,
    /// Catalog index is process-local; persist `id()` instead.
    Builtin(u16),
}

thread_local! {
    static THEME: Cell<Theme> = const { Cell::new(Theme::Dark) };
}

/// Select the palette on the UI thread. Existing editors refresh on next draw.
pub fn set_theme(theme: Theme) {
    let theme = match theme {
        Theme::Builtin(index) if usize::from(index) >= BUILTINS.len() => Theme::Dark,
        valid => valid,
    };
    THEME.set(theme);
}

pub fn current_theme() -> Theme {
    THEME.get()
}

/// A readable text/background pair; consumers need not infer contrast by
/// reversing or reusing an unrelated accent color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorPair {
    pub foreground: Color,
    pub background: Color,
}

impl ColorPair {
    pub fn style(self) -> Style {
        Style::default().fg(self.foreground).bg(self.background)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub background: Color,
    pub foreground: Color,
    pub muted: Color,
    pub chrome: Color,
    pub chrome_text: Color,
    pub chrome_muted: Color,
    pub active: Color,
    pub field: Color,
    pub accent: Color,
    pub heading: Color,
    pub link: Color,
    pub code: Color,
    pub code_background: Color,
    pub quote: Color,
    pub border: Color,
    pub warning: Color,
    pub search: Color,
    pub search_text: Color,
    pub search_active: Color,
    pub search_active_text: Color,
    /// Selection surface. Token ink is kept where it stays readable on it;
    /// `selection_text` replaces ink that would not.
    pub selection: Color,
    pub selection_text: Color,
    /// A barely lifted surface for the caret's line in code and source views.
    pub cursorline: Color,
    /// Quiet structural ink for rules, table grids, and indent guides.
    pub faint: Color,
    pub list_marker: Color,
    /// Heading ink by level, each readable on its own band and the page.
    pub headings: [Color; 6],
    pub heading_bands: [Color; 6],
    /// GitHub alert kinds: note, tip, important, warning, caution.
    pub callouts: [Color; 5],
    pub syntax: Syntax,
}

/// Token ink for highlighted code; every color reads on both the page and
/// the code-block surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Syntax {
    pub comment: Color,
    pub keyword: Color,
    pub types: Color,
    pub function: Color,
    pub string: Color,
    pub escape: Color,
    pub constant: Color,
    pub operator: Color,
    pub punctuation: Color,
    /// Macros, attributes, decorators, and labels.
    pub special: Color,
    pub property: Color,
    pub tag: Color,
    pub variable: Color,
    pub inserted: Color,
    pub deleted: Color,
}

struct BuiltinTheme {
    name: &'static str,
    id: &'static str,
    background: u32,
    foreground: u32,
    selection_background: u32,
    selection_foreground: u32,
    ansi: [u32; 16],
}

// Palette access is in the glyph-rendering hot path. Derive all imported roles
// once, then return a Copy value; no parsing, files, or color math per glyph.
static BUILTIN_PALETTES: OnceLock<Vec<Palette>> = OnceLock::new();
static SAGE_PALETTES: OnceLock<[Palette; 2]> = OnceLock::new();

fn fade(page: Color, inks: [Color; 6], amount: u16) -> [Color; 6] {
    let mut bands = inks;
    for (level, band) in bands.iter_mut().enumerate() {
        *band = mix(page, *band, amount * HEADING_STRENGTH[level] / 100);
    }
    bands
}

/// The original hand-tuned pair; only surfaces behind headings are mixed.
fn sage_palettes() -> [Palette; 2] {
    let dark = Color::Rgb(28, 33, 31);
    let dark_headings = [
        Color::Rgb(184, 217, 155), // leaf
        Color::Rgb(142, 200, 185), // teal
        Color::Rgb(223, 195, 140), // sand
        Color::Rgb(165, 189, 224), // sky
        Color::Rgb(211, 169, 207), // heather
        Color::Rgb(226, 164, 133), // terracotta
    ];
    let light = Color::Rgb(248, 246, 237);
    let light_headings = [
        Color::Rgb(58, 100, 42),
        Color::Rgb(33, 102, 88),
        Color::Rgb(126, 91, 22),
        Color::Rgb(47, 90, 146),
        Color::Rgb(117, 66, 116),
        Color::Rgb(147, 74, 42),
    ];
    [
        Palette {
            background: dark,
            foreground: Color::Rgb(220, 224, 213),
            muted: Color::Rgb(157, 169, 158),
            chrome: Color::Rgb(35, 43, 38),
            chrome_text: Color::Rgb(224, 229, 218),
            chrome_muted: Color::Rgb(165, 181, 167),
            active: Color::Rgb(53, 69, 56),
            field: Color::Rgb(22, 28, 24),
            accent: Color::Rgb(174, 204, 160),
            heading: Color::Rgb(193, 215, 172),
            link: Color::Rgb(150, 199, 181),
            code: Color::Rgb(222, 196, 155),
            code_background: Color::Rgb(38, 43, 38),
            quote: Color::Rgb(164, 181, 159),
            border: Color::Rgb(74, 88, 75),
            warning: Color::Rgb(243, 193, 130),
            search: Color::Rgb(68, 80, 61),
            search_text: Color::Rgb(239, 237, 213),
            search_active: Color::Rgb(222, 199, 130),
            search_active_text: Color::Rgb(38, 39, 27),
            selection: Color::Rgb(56, 74, 60),
            selection_text: Color::Rgb(232, 236, 225),
            cursorline: Color::Rgb(35, 41, 38),
            faint: Color::Rgb(78, 92, 81),
            list_marker: Color::Rgb(142, 200, 185),
            headings: dark_headings,
            heading_bands: fade(dark, dark_headings, 115),
            callouts: [
                Color::Rgb(147, 194, 227),
                Color::Rgb(171, 210, 145),
                Color::Rgb(201, 173, 227),
                Color::Rgb(230, 192, 123),
                Color::Rgb(231, 159, 146),
            ],
            syntax: Syntax {
                comment: Color::Rgb(139, 153, 141),
                keyword: Color::Rgb(201, 173, 227),
                types: Color::Rgb(227, 202, 145),
                function: Color::Rgb(134, 184, 224),
                string: Color::Rgb(171, 210, 145),
                escape: Color::Rgb(143, 208, 196),
                constant: Color::Rgb(232, 171, 125),
                operator: Color::Rgb(143, 208, 196),
                punctuation: Color::Rgb(170, 181, 172),
                special: Color::Rgb(135, 200, 214),
                property: Color::Rgb(169, 200, 232),
                tag: Color::Rgb(231, 159, 146),
                variable: Color::Rgb(231, 159, 146),
                inserted: Color::Rgb(171, 210, 145),
                deleted: Color::Rgb(231, 159, 146),
            },
        },
        Palette {
            background: light,
            foreground: Color::Rgb(53, 62, 49),
            muted: Color::Rgb(105, 115, 97),
            chrome: Color::Rgb(233, 237, 223),
            chrome_text: Color::Rgb(52, 65, 48),
            chrome_muted: Color::Rgb(94, 110, 86),
            active: Color::Rgb(210, 222, 197),
            field: Color::Rgb(250, 249, 242),
            accent: Color::Rgb(64, 102, 53),
            heading: Color::Rgb(63, 91, 46),
            link: Color::Rgb(50, 104, 86),
            code: Color::Rgb(118, 81, 41),
            code_background: Color::Rgb(235, 233, 219),
            quote: Color::Rgb(99, 117, 82),
            border: Color::Rgb(186, 199, 171),
            warning: Color::Rgb(138, 69, 25),
            search: Color::Rgb(220, 227, 190),
            search_text: Color::Rgb(53, 65, 35),
            search_active: Color::Rgb(148, 178, 115),
            search_active_text: Color::Rgb(24, 41, 16),
            selection: Color::Rgb(214, 226, 196),
            selection_text: Color::Rgb(40, 48, 36),
            cursorline: Color::Rgb(240, 238, 227),
            faint: Color::Rgb(172, 181, 160),
            list_marker: Color::Rgb(33, 102, 88),
            headings: light_headings,
            heading_bands: fade(light, light_headings, 95),
            callouts: [
                Color::Rgb(47, 90, 146),
                Color::Rgb(72, 112, 40),
                Color::Rgb(117, 72, 150),
                Color::Rgb(126, 91, 22),
                Color::Rgb(158, 62, 45),
            ],
            syntax: Syntax {
                comment: Color::Rgb(96, 105, 89),
                keyword: Color::Rgb(117, 72, 150),
                types: Color::Rgb(126, 91, 22),
                function: Color::Rgb(40, 91, 140),
                string: Color::Rgb(72, 112, 40),
                escape: Color::Rgb(26, 104, 96),
                constant: Color::Rgb(158, 76, 28),
                operator: Color::Rgb(26, 104, 96),
                punctuation: Color::Rgb(85, 95, 80),
                special: Color::Rgb(22, 102, 120),
                property: Color::Rgb(40, 91, 140),
                tag: Color::Rgb(158, 62, 45),
                variable: Color::Rgb(158, 62, 45),
                inserted: Color::Rgb(72, 112, 40),
                deleted: Color::Rgb(158, 62, 45),
            },
        },
    ]
}

impl Theme {
    /// Sage aliases first, followed by the upstream names in stable name order.
    pub fn all() -> impl ExactSizeIterator<Item = Self> + DoubleEndedIterator {
        (0..BUILTINS.len() + 2).map(|index| match index {
            0 => Self::Dark,
            1 => Self::Light,
            _ => Self::Builtin((index - 2) as u16),
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Dark => "Sage Dark",
            Self::Light => "Sage Light",
            Self::Builtin(index) => BUILTINS
                .get(usize::from(index))
                .map_or("Sage Dark", |theme| theme.name),
        }
    }

    /// Stable identifier for configuration; numeric Builtin indices may change.
    pub fn id(self) -> &'static str {
        match self {
            Self::Dark => "sage-dark",
            Self::Light => "sage-light",
            Self::Builtin(index) => BUILTINS
                .get(usize::from(index))
                .map_or("sage-dark", |theme| theme.id),
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let normalized: Vec<_> = normalized_bytes(name).collect();
        let lookup: &[u8] = match normalized.as_slice() {
            b"dark" | b"sagedark" => return Some(Self::Dark),
            b"light" | b"sagelight" => return Some(Self::Light),
            b"catppuccin" => b"catppuccinmocha",
            b"gruvbox" => b"gruvboxdark",
            b"solarized" | b"solarizeddark" => b"iterm2solarizeddark",
            b"solarizedlight" => b"iterm2solarizedlight",
            b"onedark" => b"atomonedark",
            b"onelight" => b"atomonelight",
            other => other,
        };
        BUILTINS
            .iter()
            .position(|theme| {
                normalized_bytes(theme.name).eq(lookup.iter().copied())
                    || normalized_bytes(theme.id).eq(lookup.iter().copied())
            })
            .map(|index| Self::Builtin(index as u16))
    }

    /// Classification follows which neutral ink has greater contrast on the
    /// original background, rather than trusting inconsistent theme names.
    pub fn is_dark(self) -> bool {
        let background = match self {
            Self::Dark => return true,
            Self::Light => return false,
            Self::Builtin(index) => match BUILTINS.get(usize::from(index)) {
                Some(theme) => rgb(theme.background),
                None => return true,
            },
        };
        contrast(Color::Rgb(255, 255, 255), background) >= contrast(Color::Rgb(0, 0, 0), background)
    }

    /// Explicit contrast-safe colors for tabs, sidebar, and status segments.
    pub fn chrome_palette(self) -> ChromePalette {
        chrome::for_theme(self)
    }

    pub fn palette(self) -> Palette {
        match self {
            Self::Builtin(index) => BUILTIN_PALETTES
                .get_or_init(|| BUILTINS.iter().map(derive_palette).collect())
                .get(usize::from(index))
                .copied()
                .unwrap_or_else(|| Self::Dark.palette()),
            Self::Dark => SAGE_PALETTES.get_or_init(sage_palettes)[0],
            Self::Light => SAGE_PALETTES.get_or_init(sage_palettes)[1],
        }
    }
}

fn normalized_bytes(name: &str) -> impl Iterator<Item = u8> + '_ {
    name.bytes()
        .filter(|byte| !byte.is_ascii_whitespace() && !matches!(byte, b'-' | b'_'))
        .map(|byte| byte.to_ascii_lowercase())
}

fn rgb(value: u32) -> Color {
    Color::Rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

fn channels(color: Color) -> [u8; 3] {
    let Color::Rgb(r, g, b) = color else {
        unreachable!("theme colors are explicit RGB")
    };
    [r, g, b]
}

fn luminance(color: Color) -> f64 {
    let [r, g, b] = channels(color);
    let linear = |byte| {
        let value = f64::from(byte) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

fn contrast(a: Color, b: Color) -> f64 {
    let a = luminance(a);
    let b = luminance(b);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn mix(a: Color, b: Color, amount: u16) -> Color {
    let a = channels(a);
    let b = channels(b);
    let part = |index| {
        ((u32::from(a[index]) * u32::from(1000 - amount)
            + u32::from(b[index]) * u32::from(amount)
            + 500)
            / 1000) as u8
    };
    Color::Rgb(part(0), part(1), part(2))
}

fn ink(background: Color) -> Color {
    let dark = Color::Rgb(0, 0, 0);
    let light = Color::Rgb(255, 255, 255);
    if contrast(dark, background) > contrast(light, background) {
        dark
    } else {
        light
    }
}

/// Preserve a source color when readable, otherwise change it only as far as
/// needed toward the common black/white ink for these background surfaces.
fn readable(color: Color, backgrounds: &[Color], ink: Color) -> Color {
    readable_at(color, backgrounds, ink, 4.5)
}

fn readable_at(color: Color, backgrounds: &[Color], ink: Color, minimum: f64) -> Color {
    let passes = |candidate| {
        backgrounds
            .iter()
            .all(|background| contrast(candidate, *background) >= minimum)
    };
    if passes(color) {
        return color;
    }
    debug_assert!(passes(ink));
    let (mut low, mut high) = (0, 1000);
    while low < high {
        let middle = (low + high) / 2;
        if passes(mix(color, ink, middle)) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    mix(color, ink, high)
}

/// Keep surfaces on the same readable side of the palette's neutral ink.
fn surface(background: Color, tint: Color, amount: u16, ink: Color) -> Color {
    let opposite = if ink == Color::Rgb(255, 255, 255) {
        Color::Rgb(0, 0, 0)
    } else {
        Color::Rgb(255, 255, 255)
    };
    for candidate in [
        mix(background, tint, amount),
        mix(background, opposite, amount),
        mix(background, ink, 30),
    ] {
        if candidate != background && contrast(ink, candidate) >= 4.5 {
            return candidate;
        }
    }
    background
}

fn derive_palette(theme: &BuiltinTheme) -> Palette {
    let background = rgb(theme.background);
    let text = ink(background);
    let foreground = readable(rgb(theme.foreground), &[background], text);
    let ansi = theme.ansi.map(rgb);
    let chrome = surface(background, foreground, 60, text);
    let active = surface(background, ansi[4], 240, text);
    let code_background = surface(background, foreground, 35, text);
    let search = surface(background, ansi[3], 180, text);
    let mut search_active = rgb(theme.selection_background);
    if contrast(search_active, background) < 1.4 || contrast(search_active, search) < 1.25 {
        search_active = ansi[3];
    }
    if contrast(search_active, search) < 1.25 {
        search_active = text;
    }
    let page = |color, extra: &[Color]| {
        let mut surfaces = vec![background];
        surfaces.extend_from_slice(extra);
        readable(color, &surfaces, text)
    };
    let selection = selection_surface(
        rgb(theme.selection_background),
        background,
        foreground,
        ansi[4],
    );
    let orange = mix(ansi[1], ansi[3], 450);
    let tints = [ansi[4], ansi[5], ansi[6], ansi[2], ansi[3], orange];
    let mut heading_bands = tints;
    for (level, band_color) in heading_bands.iter_mut().enumerate() {
        *band_color = band(background, *band_color, foreground, level);
    }
    let mut headings = tints;
    for (heading, band) in headings.iter_mut().zip(heading_bands) {
        *heading = readable(*heading, &[background, band], text);
    }
    Palette {
        background,
        foreground,
        muted: readable(mix(foreground, background, 400), &[background], text),
        chrome,
        chrome_text: readable(foreground, &[chrome, background, active], text),
        chrome_muted: readable(mix(foreground, chrome, 350), &[chrome, active], text),
        active,
        field: background,
        accent: readable(ansi[4], &[chrome, active, background], text),
        heading: readable(ansi[5], &[background], text),
        link: readable(ansi[6], &[background], text),
        code: readable(ansi[3], &[code_background, background], text),
        code_background,
        quote: readable(mix(ansi[8], foreground, 450), &[background], text),
        border: mix(foreground, chrome, 600),
        warning: readable(ansi[1], &[chrome], text),
        search,
        search_text: readable(foreground, &[search], text),
        search_active,
        search_active_text: readable(
            rgb(theme.selection_foreground),
            &[search_active],
            ink(search_active),
        ),
        selection,
        selection_text: readable(foreground, &[selection], ink(selection)),
        cursorline: cursorline(background, foreground),
        faint: readable_at(mix(foreground, background, 700), &[background], text, 1.8),
        list_marker: readable(ansi[6], &[background], text),
        headings,
        heading_bands,
        callouts: [ansi[4], ansi[2], ansi[5], ansi[3], ansi[1]].map(|color| page(color, &[])),
        syntax: Syntax {
            comment: page(mix(ansi[8], foreground, 250), &[code_background]),
            keyword: page(ansi[5], &[code_background]),
            types: page(ansi[3], &[code_background]),
            function: page(ansi[4], &[code_background]),
            string: page(ansi[2], &[code_background]),
            escape: page(ansi[6], &[code_background]),
            constant: page(orange, &[code_background]),
            operator: page(ansi[6], &[code_background]),
            punctuation: page(mix(foreground, background, 250), &[code_background]),
            special: page(mix(ansi[6], ansi[4], 400), &[code_background]),
            property: page(mix(ansi[4], ansi[6], 350), &[code_background]),
            tag: page(ansi[1], &[code_background]),
            variable: page(ansi[1], &[code_background]),
            inserted: page(ansi[2], &[code_background]),
            deleted: page(ansi[1], &[code_background]),
        },
    }
}

/// Deeper headings fade toward the page, so level reads before color does.
const HEADING_STRENGTH: [u16; 6] = [100, 80, 62, 50, 42, 36];

/// A quiet tint behind heading rows, backing off until body text still reads.
fn band(background: Color, tint: Color, foreground: Color, level: usize) -> Color {
    let strength = HEADING_STRENGTH[level];
    [130, 100, 75, 55, 40]
        .into_iter()
        .map(|amount| mix(background, tint, amount * strength / 100))
        .find(|candidate| *candidate != background && contrast(foreground, *candidate) >= 4.5)
        .unwrap_or(background)
}

/// Prefer a visible selection that keeps body text readable: the theme's
/// own, then a tint of its blue or text. Themes whose text barely clears the
/// threshold keep their own surface and rely on `selection_text`.
fn selection_surface(theme: Color, background: Color, foreground: Color, tint: Color) -> Color {
    let visible = |candidate| contrast(candidate, background) >= 1.2;
    let usable = |candidate| visible(candidate) && contrast(foreground, candidate) >= 4.5;
    if usable(theme) {
        return theme;
    }
    [320, 260, 200, 150, 110]
        .into_iter()
        .flat_map(|amount| {
            [
                mix(background, tint, amount),
                mix(background, foreground, amount / 2),
            ]
        })
        .find(|candidate| usable(*candidate))
        .unwrap_or_else(|| {
            if visible(theme) {
                return theme;
            }
            let tint = if visible(tint) { tint } else { ink(background) };
            (320..=1000)
                .step_by(40)
                .map(|amount| mix(background, tint, amount))
                .find(|candidate| visible(*candidate))
                .unwrap_or(tint)
        })
}

/// The faintest lift that keeps body text readable, or none at all.
fn cursorline(background: Color, foreground: Color) -> Color {
    [45, 30, 20]
        .into_iter()
        .map(|amount| mix(background, foreground, amount))
        .chain([mix(background, ink(foreground), 150)])
        .find(|candidate| *candidate != background && contrast(foreground, *candidate) >= 4.5)
        .unwrap_or(background)
}

pub fn chrome_palette() -> ChromePalette {
    current_theme().chrome_palette()
}

pub fn palette() -> Palette {
    current_theme().palette()
}

/// Selected text sits on the selection surface and keeps its own ink where
/// that stays readable, as token and heading colors usually do.
pub fn selected(style: Style) -> Style {
    let colors = palette();
    let ink = style
        .fg
        .filter(|ink| matches!(ink, Color::Rgb(..)) && contrast(*ink, colors.selection) >= 3.0)
        .unwrap_or(colors.selection_text);
    style
        .fg(ink)
        .bg(colors.selection)
        .remove_modifier(ratatui::style::Modifier::REVERSED)
}

pub fn document_style() -> Style {
    let colors = palette();
    Style::default().fg(colors.foreground).bg(colors.background)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_palette_pairs_readable_text_and_distinct_surfaces() {
        for theme in Theme::all() {
            let p = theme.palette();
            for (name, text, background) in [
                ("body", p.foreground, p.background),
                ("muted", p.muted, p.background),
                ("heading", p.heading, p.background),
                ("link", p.link, p.background),
                ("quote", p.quote, p.background),
                ("code", p.code, p.code_background),
                ("chrome", p.chrome_text, p.chrome),
                ("chrome muted", p.chrome_muted, p.chrome),
                ("field", p.chrome_text, p.field),
                ("focus", p.accent, p.chrome),
                ("active tab", p.accent, p.active),
                ("warning", p.warning, p.chrome),
                ("match", p.search_text, p.search),
                ("active match", p.search_active_text, p.search_active),
            ] {
                let a = luminance(text);
                let b = luminance(background);
                let contrast = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                assert!(contrast >= 4.5, "{theme:?} {name}: {contrast:.2}");
            }
            assert_ne!(p.chrome, p.background);
            assert_ne!(p.search, p.search_active);
        }
    }

    #[test]
    fn nearly_every_palette_has_a_visible_cursorline() {
        let flat = Theme::all()
            .filter(|theme| theme.palette().cursorline == theme.palette().background)
            .count();
        assert!(flat <= 12, "{flat} themes lack a cursorline surface");
        for theme in [Theme::Dark, Theme::Light] {
            assert_ne!(theme.palette().cursorline, theme.palette().background);
        }
    }

    #[test]
    fn every_palette_keeps_document_roles_readable_on_their_surfaces() {
        for theme in Theme::all() {
            let p = theme.palette();
            let name = theme.name();
            let s = p.syntax;
            for (role, color) in [
                ("comment", s.comment),
                ("keyword", s.keyword),
                ("types", s.types),
                ("function", s.function),
                ("string", s.string),
                ("escape", s.escape),
                ("constant", s.constant),
                ("operator", s.operator),
                ("punctuation", s.punctuation),
                ("special", s.special),
                ("property", s.property),
                ("tag", s.tag),
                ("variable", s.variable),
                ("inserted", s.inserted),
                ("deleted", s.deleted),
            ] {
                for surface in [p.background, p.code_background] {
                    let ratio = contrast(color, surface);
                    assert!(ratio >= 4.5, "{name} syntax {role}: {ratio:.2}");
                }
            }
            for (level, (ink, band)) in p.headings.iter().zip(p.heading_bands).enumerate() {
                for surface in [p.background, band] {
                    let ratio = contrast(*ink, surface);
                    assert!(ratio >= 4.5, "{name} h{}: {ratio:.2}", level + 1);
                }
                assert!(contrast(p.foreground, band) >= 4.5, "{name} band text");
            }
            for color in p.callouts {
                assert!(contrast(color, p.background) >= 4.5, "{name} callout");
            }
            assert!(
                contrast(p.selection_text, p.selection) >= 4.5,
                "{name} selection"
            );
            assert!(
                contrast(p.selection, p.background) >= 1.2,
                "{name} selection"
            );
            assert!(
                contrast(p.foreground, p.cursorline) >= 4.5,
                "{name} cursorline"
            );
            assert!(contrast(p.faint, p.background) >= 1.8, "{name} faint");
            assert!(
                contrast(p.list_marker, p.background) >= 4.5,
                "{name} marker"
            );
        }
    }

    #[test]
    fn catalog_identifiers_and_names_round_trip_without_collisions() {
        use std::collections::{BTreeMap, BTreeSet};
        assert_eq!(Theme::all().len(), 624);
        let mut ids = BTreeSet::new();
        let mut names = BTreeMap::new();
        for theme in Theme::all() {
            assert!(ids.insert(theme.id()));
            for value in [theme.name(), theme.id()] {
                let normalized = normalized_bytes(value).collect::<Vec<_>>();
                if let Some(previous) = names.insert(normalized, theme) {
                    assert_eq!(previous, theme);
                }
                assert_eq!(Theme::from_name(value), Some(theme));
            }
        }
        for theme in BUILTINS {
            assert!(theme.name.is_ascii());
            assert!(
                theme
                    .id
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            );
            for color in theme.ansi.into_iter().chain([
                theme.background,
                theme.foreground,
                theme.selection_background,
                theme.selection_foreground,
            ]) {
                assert!(color <= 0xffffff);
            }
        }
    }

    #[test]
    fn popular_names_aliases_and_punctuation_resolve_unambiguously() {
        for (query, canonical) in [
            ("dark", "Sage Dark"),
            ("LIGHT", "Sage Light"),
            ("Catppuccin", "Catppuccin Mocha"),
            (" CATPPUCCIN-mocha ", "Catppuccin Mocha"),
            ("catppuccin_frappe", "Catppuccin Frappe"),
            ("catppuccin macchiato", "Catppuccin Macchiato"),
            ("catppuccin-latte", "Catppuccin Latte"),
            ("dracula", "Dracula"),
            ("Dracula+", "Dracula+"),
            ("dracula-plus", "Dracula+"),
            ("nord", "Nord"),
            ("gruvbox", "Gruvbox Dark"),
            ("solarized dark", "iTerm2 Solarized Dark"),
            ("solarized-light", "iTerm2 Solarized Light"),
            ("Tokyo Night", "TokyoNight"),
            ("Tokyo Night Storm", "TokyoNight Storm"),
            ("One Dark", "Atom One Dark"),
            ("one-light", "Atom One Light"),
        ] {
            assert_eq!(
                Theme::from_name(query).unwrap().name(),
                canonical,
                "{query}"
            );
        }
        for unknown in ["", "---", "unknown theme", "Monokai Pro", "Monokai Classic"] {
            assert_eq!(Theme::from_name(unknown), None);
        }
        for name in [
            "Sage Light",
            "Catppuccin Latte",
            "Solarized Light",
            "Gruvbox Light",
            "TokyoNight Day",
        ] {
            assert!(!Theme::from_name(name).unwrap().is_dark(), "{name}");
        }
        for name in [
            "Sage Dark",
            "Dracula",
            "Nord",
            "Catppuccin Mocha",
            "Tokyo Night",
            "One Dark",
        ] {
            assert!(Theme::from_name(name).unwrap().is_dark(), "{name}");
        }
    }

    #[test]
    fn original_backgrounds_and_readable_foregrounds_keep_palette_identity() {
        for (name, background, foreground) in [
            ("Catppuccin Mocha", 0x1e1e2e, 0xcdd6f4),
            ("Dracula", 0x282a36, 0xf8f8f2),
            ("Nord", 0x2e3440, 0xd8dee9),
        ] {
            let palette = Theme::from_name(name).unwrap().palette();
            assert_eq!(palette.background, rgb(background));
            assert_eq!(palette.foreground, rgb(foreground));
        }
        for (index, theme) in BUILTINS.iter().enumerate() {
            assert_eq!(
                Theme::Builtin(index as u16).palette().background,
                rgb(theme.background)
            );
        }
    }

    #[test]
    fn invalid_catalog_indices_are_safe_and_current_theme_is_thread_local() {
        let invalid = Theme::Builtin(u16::MAX);
        assert_eq!(invalid.palette(), Theme::Dark.palette());
        assert_eq!(invalid.name(), Theme::Dark.name());
        assert_eq!(invalid.id(), Theme::Dark.id());
        set_theme(invalid);
        assert_eq!(current_theme(), Theme::Dark);
        let chosen = Theme::from_name("Dracula").unwrap();
        set_theme(chosen);
        assert_eq!(palette(), chosen.palette());
        std::thread::spawn(|| {
            assert_eq!(current_theme(), Theme::Dark);
            set_theme(Theme::Light);
        })
        .join()
        .unwrap();
        assert_eq!(current_theme(), chosen);
        set_theme(Theme::Dark);
    }
}
