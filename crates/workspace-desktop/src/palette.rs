use gpui::{Hsla, rgb};
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Clone, Deserialize)]
pub struct Tokens {
    pub base: String,
    pub surface: String,
    pub overlay: String,
    pub text: String,
    pub subtle: String,
    pub edge: String,
    pub accent: String,
    #[serde(rename = "on-accent")]
    pub on_accent: String,
    pub focus: String,
    pub green: String,
    pub red: String,
    pub yellow: String,
    pub blue: String,
    pub magenta: String,
}
#[derive(Deserialize)]
struct Variants {
    light: Tokens,
    dark: Tokens,
}
#[derive(Deserialize)]
struct ThemeFile {
    variants: Variants,
}
#[derive(Clone, Copy)]
pub struct Palette {
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
pub fn palette(dark: bool) -> Palette {
    static TOKENS: OnceLock<ThemeFile> = OnceLock::new();
    let file = TOKENS.get_or_init(|| {
        serde_json::from_str(include_str!("../../../design/themes/wiffletree-tidal.json"))
            .expect("Valid canonical Tidal tokens")
    });
    let t = if dark {
        &file.variants.dark
    } else {
        &file.variants.light
    };
    let color = |s: &str| {
        rgb(u32::from_str_radix(s.trim_start_matches('#'), 16).expect("Valid theme color")).into()
    };
    Palette {
        base: color(&t.base),
        surface: color(&t.surface),
        overlay: color(&t.overlay),
        text: color(&t.text),
        subtle: color(&t.subtle),
        edge: color(&t.edge),
        accent: color(&t.accent),
        on_accent: color(&t.on_accent),
        focus: color(&t.focus),
        green: color(&t.green),
        red: color(&t.red),
        yellow: color(&t.yellow),
        blue: color(&t.blue),
        magenta: color(&t.magenta),
    }
}
