//! Explicit color pairs keep the writing surface readable on any terminal.
use ratatui::style::{Color, Style};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

thread_local! {
    static THEME: Cell<Theme> = const { Cell::new(Theme::Dark) };
}

/// Select the palette on the UI thread. Existing editors refresh on next draw.
pub fn set_theme(theme: Theme) {
    THEME.set(theme);
}

pub fn current_theme() -> Theme {
    THEME.get()
}

#[derive(Clone, Copy, Debug)]
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

impl Theme {
    pub fn palette(self) -> Palette {
        match self {
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
                muted: Color::Rgb(107, 117, 99),
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

pub fn palette() -> Palette {
    current_theme().palette()
}

pub fn document_style() -> Style {
    let colors = palette();
    Style::default().fg(colors.foreground).bg(colors.background)
}
