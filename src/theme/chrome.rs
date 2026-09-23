//! Editor chrome uses explicit text/background pairs instead of reversing an
//! accent color. The writing surface and imported palette data stay unchanged.
use super::{ColorPair, Palette, Theme, contrast, ink, mix, readable, readable_at};
use ratatui::style::Color;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChromePalette {
    pub sidebar: ColorPair,
    /// Readable on `sidebar.background`.
    pub sidebar_muted: Color,
    /// Decorative separator readable on both the sidebar and document.
    pub separator: Color,
    pub tab_inactive: ColorPair,
    /// Consumers should also mark the active tab with bold and a visible
    /// indicator, so active state never depends on color alone.
    pub tab_active: ColorPair,
    /// Readable on `tab_active.background`, for a marker such as `>` or `|`.
    pub tab_indicator: Color,
    pub status: ColorPair,
    pub status_accent: ColorPair,
    /// A second solid badge, distinguishing Markdown source view.
    pub status_alternate: ColorPair,
    pub status_secondary: ColorPair,
    pub status_warning: ColorPair,
}

static PALETTES: OnceLock<Vec<ChromePalette>> = OnceLock::new();

pub(super) fn for_theme(theme: Theme) -> ChromePalette {
    let palettes =
        PALETTES.get_or_init(|| Theme::all().map(|theme| derive(theme.palette())).collect());
    let index = match theme {
        Theme::Dark => 0,
        Theme::Light => 1,
        Theme::Builtin(index) => usize::from(index) + 2,
    };
    palettes.get(index).copied().unwrap_or(palettes[0])
}

fn pair(foreground: Color, background: Color) -> ColorPair {
    ColorPair {
        foreground: readable(foreground, &[background], ink(background)),
        background,
    }
}

/// Prefer a restrained tint, increasing it only as far as needed for a
/// visibly distinct segment. This is a background separation target, not a
/// substitute for the independent text-contrast checks in `pair`.
fn distinct_tint(base: Color, tint: Color, amount: u16, minimum: f64) -> Color {
    let tint = if contrast(tint, base) >= minimum {
        tint
    } else {
        ink(base)
    };
    let candidate = mix(base, tint, amount);
    if contrast(candidate, base) >= minimum {
        return candidate;
    }
    let (mut low, mut high) = (amount, 1000);
    while low < high {
        let middle = (low + high) / 2;
        if contrast(mix(base, tint, middle), base) >= minimum {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    mix(base, tint, high)
}

fn derive(palette: Palette) -> ChromePalette {
    // A large sidebar should stay quiet. Color is concentrated in small tabs
    // and badges where it conveys hierarchy without tinting the whole editor.
    let sidebar_background = mix(palette.background, palette.chrome, 650);
    let tab_background = distinct_tint(palette.chrome, palette.accent, 300, 1.45);
    // The secondary segment draws from the heading/link family, independently
    // of the blue/green accent used by the primary badge and selected tab.
    let secondary_tint = mix(palette.heading, palette.link, 250);
    let secondary_background = distinct_tint(palette.chrome, secondary_tint, 300, 1.35);
    let warning_background = mix(palette.chrome, palette.warning, 160);
    ChromePalette {
        sidebar: pair(palette.foreground, sidebar_background),
        sidebar_muted: readable(
            palette.chrome_muted,
            &[sidebar_background],
            ink(sidebar_background),
        ),
        separator: readable_at(
            palette.border,
            &[sidebar_background, palette.background],
            ink(palette.background),
            3.0,
        ),
        tab_inactive: pair(palette.chrome_muted, palette.chrome),
        tab_active: pair(palette.foreground, tab_background),
        tab_indicator: readable(palette.accent, &[tab_background], ink(tab_background)),
        status: pair(palette.chrome_text, palette.chrome),
        // Preserve the actual theme accent as the small solid badge. Derive
        // its own ink; document foreground or reverse-video may be unreadable.
        status_accent: pair(palette.background, palette.accent),
        status_alternate: pair(palette.background, palette.headings[1]),
        status_secondary: pair(palette.foreground, secondary_background),
        status_warning: pair(palette.warning, warning_background),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{chrome_palette, current_theme, set_theme};

    #[test]
    fn every_theme_has_readable_chrome_and_distinct_selected_tabs() {
        for theme in Theme::all() {
            let p = theme.palette();
            let c = theme.chrome_palette();
            for (name, pair) in [
                ("sidebar", c.sidebar),
                ("inactive tab", c.tab_inactive),
                ("active tab", c.tab_active),
                ("status", c.status),
                ("status accent", c.status_accent),
                ("status alternate", c.status_alternate),
                ("status secondary", c.status_secondary),
                ("status warning", c.status_warning),
            ] {
                let ratio = contrast(pair.foreground, pair.background);
                assert!(ratio >= 4.5, "{} {name}: {ratio}", theme.name());
                assert_eq!(pair.style().fg, Some(pair.foreground));
                assert_eq!(pair.style().bg, Some(pair.background));
            }
            assert!(
                contrast(c.sidebar_muted, c.sidebar.background) >= 4.5,
                "{} sidebar muted",
                theme.name()
            );
            assert!(
                contrast(c.tab_indicator, c.tab_active.background) >= 4.5,
                "{} indicator",
                theme.name()
            );
            assert!(
                contrast(c.tab_active.background, c.tab_inactive.background) >= 1.4,
                "{} tab backgrounds",
                theme.name()
            );
            assert!(
                contrast(c.status_secondary.background, c.status.background) >= 1.3,
                "{} secondary segment",
                theme.name()
            );
            assert!(
                contrast(c.status_accent.background, c.status.background) >= 4.5,
                "{} accent segment",
                theme.name()
            );
            assert!(
                contrast(c.separator, c.sidebar.background) >= 3.0,
                "{} sidebar separator",
                theme.name()
            );
            assert!(
                contrast(c.separator, p.background) >= 3.0,
                "{} document separator",
                theme.name()
            );
        }
    }

    #[test]
    fn popular_themes_keep_their_own_accents_and_varied_segments() {
        use std::collections::BTreeSet;
        let mut signatures = BTreeSet::new();
        for name in [
            "Sage Dark",
            "Sage Light",
            "Catppuccin Mocha",
            "Catppuccin Latte",
            "Dracula",
            "Nord",
            "Gruvbox Dark",
            "Solarized Light",
            "Tokyo Night",
            "One Dark",
        ] {
            let theme = Theme::from_name(name).unwrap();
            let p = theme.palette();
            let c = theme.chrome_palette();
            assert_eq!(c.status_accent.background, p.accent, "{name}");
            assert_ne!(
                c.status_accent.background, c.status_secondary.background,
                "{name}"
            );
            signatures.insert((
                super::super::channels(c.status_accent.background),
                super::super::channels(c.status_secondary.background),
            ));
            assert_eq!(theme.palette(), p, "chrome must not change document colors");
        }
        assert_eq!(
            signatures.len(),
            10,
            "popular themes must not collapse to one generic chrome palette"
        );
    }

    #[test]
    fn current_theme_preview_changes_chrome_and_invalid_index_is_safe() {
        let original = current_theme();
        let dark = Theme::from_name("Catppuccin Mocha").unwrap();
        let light = Theme::from_name("Catppuccin Latte").unwrap();
        set_theme(dark);
        assert_eq!(chrome_palette(), dark.chrome_palette());
        set_theme(light);
        assert_eq!(chrome_palette(), light.chrome_palette());
        assert_ne!(light.chrome_palette(), dark.chrome_palette());
        assert_eq!(
            Theme::Builtin(u16::MAX).chrome_palette(),
            Theme::Dark.chrome_palette()
        );
        set_theme(original);
    }
}
