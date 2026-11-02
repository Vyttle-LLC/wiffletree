//! The colors and fonts every view draws with: the light and dark looks of the chosen themes,
//! replaced when `themes::reload` reads them again.
use crate::themes::{Side, tidal_look};
use gpui::{Hsla, SharedString};
use std::sync::{LazyLock, PoisonError, RwLock};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Palette {
    pub base: Hsla,
    pub surface: Hsla,
    pub overlay: Hsla,
    pub text: Hsla,
    pub subtle: Hsla,
    pub edge: Hsla,
    pub accent: Hsla,
    pub on_accent: Hsla,
    pub focus: Hsla,
    pub green: Hsla,
    pub red: Hsla,
    pub yellow: Hsla,
    pub blue: Hsla,
    pub magenta: Hsla,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Fonts {
    pub ui: SharedString,
    pub mono: SharedString,
    /// The base text size in points.
    pub size: f32,
}

/// What one side of Mode shows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Look {
    pub palette: Palette,
    pub fonts: Fonts,
}

struct Looks {
    light: Look,
    dark: Look,
    /// The side on show, which decides the fonts of views that do not know it.
    dark_on_show: bool,
}
impl Looks {
    fn side(&self, dark: bool) -> &Look {
        if dark { &self.dark } else { &self.light }
    }
}

static LOOKS: LazyLock<RwLock<Looks>> = LazyLock::new(|| {
    RwLock::new(Looks {
        light: tidal_look(Side::Light),
        dark: tidal_look(Side::Dark),
        dark_on_show: false,
    })
});

fn read<T>(with: impl FnOnce(&Looks) -> T) -> T {
    with(&LOOKS.read().unwrap_or_else(PoisonError::into_inner))
}

pub(crate) fn palette(dark: bool) -> Palette {
    read(|looks| looks.side(dark).palette)
}

pub(crate) fn fonts(dark: bool) -> Fonts {
    read(|looks| looks.side(dark).fonts.clone())
}

/// The UI font of the theme on show.
pub(crate) fn ui_font() -> SharedString {
    read(|looks| looks.side(looks.dark_on_show).fonts.ui.clone())
}

/// The monospace font of the theme on show, for code, paths and transcripts.
pub(crate) fn mono_font() -> SharedString {
    read(|looks| looks.side(looks.dark_on_show).fonts.mono.clone())
}

/// Replaces both looks, saying whether either changed.
pub(crate) fn show(light: Look, dark: Look) -> bool {
    let mut looks = LOOKS.write().unwrap_or_else(PoisonError::into_inner);
    let changed = looks.light != light || looks.dark != dark;
    looks.light = light;
    looks.dark = dark;
    changed
}

/// Records which side Mode shows, as `apply_theme` resolves it.
pub(crate) fn show_side(dark: bool) {
    LOOKS
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .dark_on_show = dark;
}
