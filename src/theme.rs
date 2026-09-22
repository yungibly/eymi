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
            Self::Dark => Palette {
                background: Color::Rgb(28, 33, 31),
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
            },
            Self::Light => Palette {
                background: Color::Rgb(248, 246, 237),
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
            },
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
    }
}

pub fn chrome_palette() -> ChromePalette {
    current_theme().chrome_palette()
}

pub fn palette() -> Palette {
    current_theme().palette()
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
