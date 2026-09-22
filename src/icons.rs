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
    pub fn file(self, markdown: bool) -> &'static str {
        match (self, markdown) {
            (Self::Plain, _) => "",
            (Self::Nerd, true) => "\u{e73e}",
            (Self::Nerd, false) => "\u{f15c}",
        }
    }
    pub fn outline(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Nerd => "\u{eb86}",
        }
    }
}
