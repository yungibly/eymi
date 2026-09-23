//! Optional Nerd Font glyphs. Plain mode needs no patched font.
//! Code points follow https://github.com/ryanoasis/nerd-fonts/blob/master/glyphnames.json.
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IconSet {
    #[default]
    Plain,
    Nerd,
}
thread_local! { static ICONS: Cell<IconSet> = const { Cell::new(IconSet::Plain) }; }
pub fn current() -> IconSet {
    ICONS.get()
}
pub fn set(icons: IconSet) {
    ICONS.set(icons);
}
impl IconSet {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "plain" => Some(Self::Plain),
            "nerd" => Some(Self::Nerd),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Nerd => "nerd",
        }
    }
    /// Ghostty, Kitty, and WezTerm draw Nerd Font symbols from built-in
    /// fallbacks, so icons need no patched font there.
    pub fn detect(env: impl Fn(&str) -> Option<String>) -> Self {
        let term = env("TERM").unwrap_or_default();
        let program = env("TERM_PROGRAM").unwrap_or_default().to_ascii_lowercase();
        let bundled = ["ghostty", "wezterm", "kitty"];
        if env("TMUX").is_none()
            && (bundled
                .iter()
                .any(|name| program == *name || term.contains(name))
                || env("KITTY_WINDOW_ID").is_some())
        {
            Self::Nerd
        } else {
            Self::Plain
        }
    }
    pub fn file(self, markdown: bool) -> &'static str {
        match (self, markdown) {
            (Self::Plain, _) => "",
            (Self::Nerd, true) => "\u{e73e}",
            (Self::Nerd, false) => "\u{f15c}",
        }
    }
    /// A file's icon from its name, falling back to a generic document.
    pub fn path(self, path: &std::path::Path) -> &'static str {
        let markdown = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "md" | "markdown" | "mdown" | "mkd" | "mdx"
                )
            });
        self.file(markdown)
    }
    pub fn outline(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Nerd => "\u{eb86}",
        }
    }
    pub fn task(self, checked: bool) -> &'static str {
        match (self, checked) {
            (Self::Plain, false) => "□",
            (Self::Plain, true) => "▣",
            (Self::Nerd, false) => "\u{f0131}",
            (Self::Nerd, true) => "\u{f0c52}",
        }
    }
    /// Level markers hang in the margin beside heading bands. Plain bars
    /// thin with depth, so hierarchy reads even without color.
    pub fn heading(self, level: usize) -> &'static str {
        const NERD: [&str; 6] = [
            "\u{f0ca1}",
            "\u{f0ca3}",
            "\u{f0ca5}",
            "\u{f0ca7}",
            "\u{f0ca9}",
            "\u{f0cab}",
        ];
        const PLAIN: [&str; 6] = ["▌", "▍", "▎", "▏", "▏", "▏"];
        match self {
            Self::Plain => PLAIN[level.clamp(1, 6) - 1],
            Self::Nerd => NERD[level.clamp(1, 6) - 1],
        }
    }
    /// Note, tip, important, warning, caution.
    pub fn callout(self, kind: usize) -> &'static str {
        const NERD: [&str; 5] = [
            "\u{f02fd}",
            "\u{f0336}",
            "\u{f017e}",
            "\u{f002a}",
            "\u{f0ce6}",
        ];
        match self {
            Self::Plain => "",
            Self::Nerd => NERD[kind.min(4)],
        }
    }
    pub fn image(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Nerd => "\u{f0976}",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn terminals_with_bundled_symbols_default_to_icons_outside_multiplexers() {
        let detect = |pairs: &[(&str, &str)]| {
            let pairs: Vec<(String, String)> = pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            IconSet::detect(|key| {
                pairs
                    .iter()
                    .find(|(name, _)| name == key)
                    .map(|(_, v)| v.clone())
            })
        };
        assert_eq!(detect(&[("TERM_PROGRAM", "ghostty")]), IconSet::Nerd);
        assert_eq!(detect(&[("TERM", "xterm-ghostty")]), IconSet::Nerd);
        assert_eq!(detect(&[("TERM", "xterm-kitty")]), IconSet::Nerd);
        assert_eq!(detect(&[("KITTY_WINDOW_ID", "1")]), IconSet::Nerd);
        assert_eq!(detect(&[("TERM_PROGRAM", "WezTerm")]), IconSet::Nerd);
        assert_eq!(
            detect(&[("TERM_PROGRAM", "Apple_Terminal")]),
            IconSet::Plain
        );
        assert_eq!(detect(&[("TERM", "xterm-256color")]), IconSet::Plain);
        assert_eq!(
            detect(&[("TERM_PROGRAM", "ghostty"), ("TMUX", "/tmp/tmux")]),
            IconSet::Plain
        );
    }

    #[test]
    fn every_symbol_occupies_one_cell() {
        for icons in [IconSet::Plain, IconSet::Nerd] {
            let mut symbols = vec![icons.task(false), icons.task(true), icons.image()];
            symbols.extend((1..=6).map(|level| icons.heading(level)));
            symbols.extend((0..5).map(|kind| icons.callout(kind)));
            for symbol in symbols.into_iter().filter(|s| !s.is_empty()) {
                assert_eq!(symbol.chars().count(), 1, "{symbol:?}");
                assert_eq!(UnicodeWidthStr::width(symbol), 1, "{symbol:?}");
            }
        }
    }
}
