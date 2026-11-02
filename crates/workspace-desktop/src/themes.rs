//! Theme files: the look chosen for light and for dark, read from `<data dir>/themes/*.json`
//! over the built-in Tidal. A file has Tidal's shape, so any color or font it leaves out is
//! Tidal's, and a file that cannot be used shows Tidal's variant on its side while Settings ›
//! Appearance says why. Nothing in a user's file can panic the app.
use crate::palette::{self, Fonts, Look, Palette};
use crate::preferences::Preferences;
use gpui::{App, Global, Hsla, SharedString, rgb};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, IgnoredAny, MapAccess, Visitor},
};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The color names of a variant, in the order a new theme writes them.
const KEYS: [&str; 14] = [
    "base",
    "surface",
    "overlay",
    "text",
    "subtle",
    "edge",
    "accent",
    "on-accent",
    "focus",
    "green",
    "red",
    "yellow",
    "blue",
    "magenta",
];
const SYSTEM_UI_FONT: &str = ".SystemUIFont";
const SYSTEM_MONO_FONT: &str = ".SystemMonoFont";
const TIDAL_SIZE: f32 = 14.;
const SIZES: std::ops::RangeInclusive<f32> = 11.0..=20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Light,
    Dark,
}
impl Side {
    fn word(self) -> &'static str {
        match self {
            Side::Light => "light",
            Side::Dark => "dark",
        }
    }
    fn other(self) -> Self {
        match self {
            Side::Light => Side::Dark,
            Side::Dark => Side::Light,
        }
    }
}

/// The theme a side shows: the built-in Tidal or a file in the themes folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Choice {
    #[default]
    Tidal,
    File(String),
}
impl Choice {
    /// A bare `.json` file name, never a path, so a choice cannot reach outside the folder.
    fn file(name: &str) -> Option<Self> {
        let bare = name.ends_with(".json") && !name.starts_with('.') && !name.contains('/');
        bare.then(|| Choice::File(name.into()))
    }
}
impl Serialize for Choice {
    fn serialize<S: Serializer>(&self, out: S) -> Result<S::Ok, S::Error> {
        match self {
            Choice::Tidal => out.serialize_str("tidal"),
            Choice::File(file) => out.serialize_str(file),
        }
    }
}
/// A value this version cannot read, perhaps saved by a newer one, reads as Tidal rather than
/// discarding the rest of the preferences file.
impl<'de> Deserialize<'de> for Choice {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(input)?;
        Ok(value.as_str().and_then(Choice::file).unwrap_or_default())
    }
}

/// A variant's colors as written, each already a valid `#RRGGBB`.
type Colors = BTreeMap<&'static str, String>;

#[derive(Deserialize)]
struct ThemeFile {
    name: String,
    #[serde(default)]
    fonts: FontNames,
    variants: Variants,
}
impl ThemeFile {
    fn variant(&self, side: Side) -> Option<&Colors> {
        match side {
            Side::Light => self.variants.light.as_ref(),
            Side::Dark => self.variants.dark.as_ref(),
        }
        .map(|variant| &variant.0)
    }
}

#[derive(Clone, Default, Deserialize)]
struct FontNames {
    ui: Option<String>,
    mono: Option<String>,
    #[serde(default, deserialize_with = "size_in_range")]
    size: Option<f32>,
}

#[derive(Deserialize)]
struct Variants {
    light: Option<VariantColors>,
    dark: Option<VariantColors>,
}

/// Checked while it is read, so serde_json puts the line of a bad color on the error.
struct VariantColors(Colors);
impl<'de> Deserialize<'de> for VariantColors {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        struct Read;
        impl<'de> Visitor<'de> for Read {
            type Value = VariantColors;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a variant's colors")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<VariantColors, A::Error> {
                let mut colors = Colors::new();
                while let Some(key) = map.next_key::<String>()? {
                    let Some(&known) = KEYS.iter().find(|known| **known == key) else {
                        map.next_value::<IgnoredAny>()?;
                        continue;
                    };
                    let value: String = map.next_value()?;
                    if !is_color(&value) {
                        return Err(de::Error::custom(format!(
                            "\"{key}\": \"{value}\" is not a color like #1A262D"
                        )));
                    }
                    colors.insert(known, value);
                }
                Ok(VariantColors(colors))
            }
        }
        input.deserialize_map(Read)
    }
}

fn is_color(value: &str) -> bool {
    value
        .strip_prefix('#')
        .is_some_and(|hex| hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()))
}

fn size_in_range<'de, D: Deserializer<'de>>(input: D) -> Result<Option<f32>, D::Error> {
    let size = f32::deserialize(input)?;
    if SIZES.contains(&size) {
        Ok(Some(size))
    } else {
        Err(de::Error::custom(format!(
            "\"size\": {size} is not a size from 11 to 20"
        )))
    }
}

/// Why a chosen file shows Tidal instead.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Fault {
    Gone,
    Unreadable(String),
    Invalid { line: Option<usize>, reason: String },
    NoVariant(Side),
}

fn parse(text: &str) -> Result<ThemeFile, Fault> {
    let file: ThemeFile = serde_json::from_str(text).map_err(|error| {
        let at = format!(" at line {} column {}", error.line(), error.column());
        let message = error.to_string();
        Fault::Invalid {
            line: (error.line() > 0).then_some(error.line()),
            reason: message.strip_suffix(&at).unwrap_or(&message).into(),
        }
    })?;
    if file.variants.light.is_none() && file.variants.dark.is_none() {
        return Err(Fault::Invalid {
            line: None,
            reason: "it has neither a light nor a dark variant".into(),
        });
    }
    Ok(file)
}

fn read_file(path: &Path) -> Result<ThemeFile, Fault> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(Fault::Gone),
        Err(error) => Err(Fault::Unreadable(error.to_string())),
    }
}

/// The compiled-in Tidal, read by the same parser as any theme file.
fn tidal() -> &'static ThemeFile {
    static TIDAL: OnceLock<ThemeFile> = OnceLock::new();
    TIDAL.get_or_init(|| {
        parse(include_str!("../../../design/themes/wiffletree-tidal.json"))
            .unwrap_or_else(|_| panic!("Valid canonical Tidal tokens"))
    })
}

fn tidal_colors(side: Side) -> &'static Colors {
    tidal()
        .variant(side)
        .expect("Tidal has a light and a dark variant")
}

/// A side's colors over Tidal's same variant.
fn palette_of(colors: &Colors, side: Side) -> Palette {
    let tidal = tidal_colors(side);
    let color = |key: &str| -> Hsla {
        let hex = colors.get(key).or_else(|| tidal.get(key));
        let value = hex.and_then(|hex| u32::from_str_radix(&hex[1..], 16).ok());
        rgb(value.unwrap_or_default()).into()
    };
    Palette {
        base: color("base"),
        surface: color("surface"),
        overlay: color("overlay"),
        text: color("text"),
        subtle: color("subtle"),
        edge: color("edge"),
        accent: color("accent"),
        on_accent: color("on-accent"),
        focus: color("focus"),
        green: color("green"),
        red: color("red"),
        yellow: color("yellow"),
        blue: color("blue"),
        magenta: color("magenta"),
    }
}

pub(crate) fn tidal_look(side: Side) -> Look {
    look(tidal_colors(side), &FontNames::default(), side, &mut |_| {
        true
    })
    .0
}

/// The look and the named fonts this Mac lacks, which fall back to the system font of their kind.
fn look(
    colors: &Colors,
    fonts: &FontNames,
    side: Side,
    installed: &mut dyn FnMut(&str) -> bool,
) -> (Look, Vec<String>) {
    let mut missing = vec![];
    let mut family = |named: &Option<String>, system: &str| -> SharedString {
        match named {
            Some(name) if installed(name) => name.clone().into(),
            Some(name) => {
                missing.push(name.clone());
                system.into()
            }
            None => system.into(),
        }
    };
    let fonts = Fonts {
        ui: family(&fonts.ui, SYSTEM_UI_FONT),
        mono: family(&fonts.mono, SYSTEM_MONO_FONT),
        size: fonts.size.unwrap_or(TIDAL_SIZE),
    };
    let look = Look {
        palette: palette_of(colors, side),
        fonts,
    };
    (look, missing)
}

/// A readable theme file, as the Light theme and Dark theme menus list it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Listed {
    pub file: String,
    pub name: String,
    light: bool,
    dark: bool,
}
impl Listed {
    pub(crate) fn has(&self, side: Side) -> bool {
        match side {
            Side::Light => self.light,
            Side::Dark => self.dark,
        }
    }
}

/// Every `.json` theme directly in `folder`, by name; a missing folder lists none. A file that
/// can't be used is in neither menu, but keeps its name if it has one, so a menu still showing
/// it as chosen can name it.
fn list(folder: &Path) -> Vec<Listed> {
    #[derive(Deserialize)]
    struct Named {
        name: String,
    }
    let Ok(entries) = std::fs::read_dir(folder) else {
        return vec![];
    };
    let mut listed: Vec<Listed> = entries
        .flatten()
        .filter_map(|entry| {
            let file = entry.file_name().into_string().ok()?;
            Choice::file(&file)?;
            let Ok(theme) = read_file(&entry.path()) else {
                let text = std::fs::read_to_string(entry.path()).ok()?;
                let named: Named = serde_json::from_str(&text).ok()?;
                return Some(Listed {
                    file,
                    name: named.name,
                    light: false,
                    dark: false,
                });
            };
            Some(Listed {
                light: theme.variants.light.is_some(),
                dark: theme.variants.dark.is_some(),
                name: theme.name,
                file,
            })
        })
        .collect();
    listed.sort_by(|a, b| (a.name.to_lowercase(), &a.file).cmp(&(b.name.to_lowercase(), &b.file)));
    listed
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    Error,
    Warning,
}

/// A banner for Settings › Appearance, the only place a theme problem shows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Notice {
    pub tone: Tone,
    pub head: String,
    pub body: String,
    /// The sides Use Tidal sets back; none for a warning.
    pub use_tidal: Vec<Side>,
}

/// What one side shows after reading its choice.
struct Shown {
    look: Look,
    name: String,
    fault: Option<(String, Fault)>,
    missing_fonts: Vec<String>,
    named_fonts: FontNames,
}

fn show(
    folder: &Path,
    choice: &Choice,
    side: Side,
    installed: &mut dyn FnMut(&str) -> bool,
) -> Shown {
    let read = match choice {
        Choice::Tidal => Ok(None),
        Choice::File(file) => read_file(&folder.join(file))
            .and_then(|theme| match theme.variant(side) {
                Some(_) => Ok(Some(theme)),
                None => Err(Fault::NoVariant(side)),
            })
            .map_err(|fault| (file.clone(), fault)),
    };
    match read {
        Ok(Some(theme)) => {
            let colors = theme.variant(side).cloned().unwrap_or_default();
            let (look, missing_fonts) = look(&colors, &theme.fonts, side, installed);
            Shown {
                look,
                name: theme.name,
                fault: None,
                missing_fonts,
                named_fonts: theme.fonts,
            }
        }
        Ok(None) | Err(_) => Shown {
            look: tidal_look(side),
            name: "Tidal".into(),
            fault: read.err(),
            missing_fonts: vec![],
            named_fonts: FontNames::default(),
        },
    }
}

/// The two looks and the banners that explain them.
pub(crate) struct Chosen {
    pub light: Look,
    pub dark: Look,
    pub notices: Vec<Notice>,
}

pub(crate) fn choose(
    folder: &Path,
    light: &Choice,
    dark: &Choice,
    installed: &mut dyn FnMut(&str) -> bool,
) -> Chosen {
    let shown = [
        (Side::Light, show(folder, light, Side::Light, installed)),
        (Side::Dark, show(folder, dark, Side::Dark, installed)),
    ];
    let mut notices: Vec<Notice> = vec![];
    let other_name = |side: Side| &shown[usize::from(side == Side::Light)].1.name;
    let faults: Vec<_> = (shown.iter())
        .filter_map(|(side, shown)| {
            shown
                .fault
                .as_ref()
                .map(|(file, fault)| (*side, file, fault))
        })
        .collect();
    for (side, file, fault) in &faults {
        let both = faults.len() == 2 && faults[0].1 == faults[1].1 && faults[0].2 == faults[1].2;
        if both && *side == Side::Dark {
            continue;
        }
        let other = (!both).then(|| other_name(*side).as_str());
        notices.push(broken(file, fault, *side, other));
    }
    // Two themes lacking the same fonts share one warning, as the study's Celadon pair does.
    let [(_, light), (_, dark)] = shown;
    let own = |shown: &Shown| missing_fonts(shown, &format!("{}'s", shown.name));
    match (own(&light), own(&dark)) {
        (Some(a), Some(b)) if a == b => notices.push(a),
        (Some(a), Some(b)) if a.head == b.head => notices.extend(missing_fonts(
            &light,
            &format!("{}'s and {}'s", light.name, dark.name),
        )),
        (a, b) => notices.extend(a.into_iter().chain(b)),
    }
    Chosen {
        light: light.look,
        dark: dark.look,
        notices,
    }
}

/// `other` is what the other side still shows, or `None` when it chose the same broken file.
fn broken(file: &str, fault: &Fault, side: Side, other: Option<&str>) -> Notice {
    let head = match fault {
        Fault::Gone => format!("{file} is gone."),
        _ => format!("{file} can't be used."),
    };
    let reason = match fault {
        Fault::Gone => None,
        Fault::Unreadable(reason) => Some(sentence(reason)),
        Fault::Invalid {
            line: Some(line),
            reason,
        } => Some(format!("Line {line}: {reason}.")),
        Fault::Invalid { line: None, reason } => Some(sentence(reason)),
        Fault::NoVariant(side) => Some(format!("It has no {} variant.", side.word())),
    };
    let sides = match other {
        Some(other) => format!(
            "{} mode shows Tidal until the file is fixed; {} mode still shows {other}.",
            capitalized(side.word()),
            side.other().word()
        ),
        None => "Light and dark mode show Tidal until the file is fixed.".into(),
    };
    Notice {
        tone: Tone::Error,
        head,
        body: reason
            .into_iter()
            .chain([sides])
            .collect::<Vec<_>>()
            .join(" "),
        use_tidal: match other {
            Some(_) => vec![side],
            None => vec![Side::Light, Side::Dark],
        },
    }
}

/// `whose` names the themes whose colors are in use, such as "Celadon Jade's".
fn missing_fonts(shown: &Shown, whose: &str) -> Option<Notice> {
    let (ui, mono) = (&shown.named_fonts.ui, &shown.named_fonts.mono);
    let lacks = |font: &Option<String>| {
        font.as_ref()
            .is_some_and(|f| shown.missing_fonts.contains(f))
    };
    let (no_ui, no_mono) = (lacks(ui), lacks(mono));
    let (head, body) = match (no_ui, no_mono) {
        (false, false) => return None,
        (true, true) => (
            format!(
                "{} and {} aren't installed.",
                ui.as_deref()?,
                mono.as_deref()?
            ),
            format!("{whose} colors are in use; the UI text and code use the system fonts."),
        ),
        (true, false) => (
            format!("{} isn't installed.", ui.as_deref()?),
            match mono {
                Some(mono) => format!(
                    "{whose} colors and {mono} are in use; the UI text uses the system font."
                ),
                None => format!("{whose} colors are in use; the UI text uses the system font."),
            },
        ),
        (false, true) => (
            format!("{} isn't installed.", mono.as_deref()?),
            match ui {
                Some(ui) => {
                    format!("{whose} colors and {ui} are in use; code uses the system font.")
                }
                None => format!("{whose} colors are in use; code uses the system font."),
            },
        ),
    };
    Some(Notice {
        tone: Tone::Warning,
        head,
        body,
        use_tidal: vec![],
    })
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn sentence(reason: &str) -> String {
    format!("{}.", capitalized(reason.trim_end_matches('.')))
}

/// A new theme holding both of Tidal's variants, named after its file.
fn new_theme_text(name: &str) -> String {
    let variant = |side: Side| {
        let colors = tidal_colors(side);
        let lines: Vec<String> = KEYS
            .iter()
            .map(|key| format!("      \"{key}\": \"{}\"", colors[key]))
            .collect();
        format!("    \"{}\": {{\n{}\n    }}", side.word(), lines.join(",\n"))
    };
    format!(
        "{{\n  \"version\": 1,\n  \"name\": {},\n  \"variants\": {{\n{},\n{}\n  }}\n}}\n",
        serde_json::Value::from(name),
        variant(Side::Light),
        variant(Side::Dark)
    )
}

/// The first free `my-theme.json`, `my-theme-2.json`, … and the name to give it.
fn free_file(folder: &Path) -> (String, String) {
    (1..)
        .map(|n| match n {
            1 => ("my-theme.json".to_string(), "My theme".to_string()),
            n => (format!("my-theme-{n}.json"), format!("My theme {n}")),
        })
        .find(|(file, _)| !folder.join(file).exists())
        .expect("a free file name")
}

/// The themes folder and what Settings › Appearance shows about it.
pub(crate) struct Themes {
    pub folder: PathBuf,
    pub listed: Vec<Listed>,
    pub notices: Vec<Notice>,
    /// Why the last Open themes folder or New theme from Tidal failed.
    pub failure: Option<String>,
    /// Font families on this Mac, listed when a theme first names one it might lack.
    installed_fonts: Vec<String>,
}
impl Global for Themes {}

impl Themes {
    pub(crate) fn new(folder: PathBuf) -> Self {
        Self {
            folder,
            listed: vec![],
            notices: vec![],
            failure: None,
            installed_fonts: vec![],
        }
    }
    /// The name a menu shows for `choice`: the file's own, or its file name while unreadable.
    pub(crate) fn label(&self, choice: &Choice) -> String {
        match choice {
            Choice::Tidal => "Tidal".into(),
            Choice::File(file) => (self.listed.iter())
                .find(|listed| &listed.file == file)
                .map_or_else(|| file.clone(), |listed| listed.name.clone()),
        }
    }
}

/// Reads the folder and both chosen files again, applying the looks and redrawing what changed.
pub(crate) fn reload(cx: &mut App) {
    let preferences = cx.global::<Preferences>();
    let (light, dark) = (
        preferences.light_theme.clone(),
        preferences.dark_theme.clone(),
    );
    let folder = cx.global::<Themes>().folder.clone();
    let listed = list(&folder);
    // Listing fonts costs about 100 ms, so it happens again only when a name is not found,
    // which also picks up a font installed since.
    let mut fonts = std::mem::take(&mut cx.global_mut::<Themes>().installed_fonts);
    let mut relisted = false;
    let chosen = choose(&folder, &light, &dark, &mut |name| {
        if !fonts.iter().any(|font| font == name) && !relisted {
            fonts = cx.text_system().all_font_names();
            relisted = true;
        }
        fonts.iter().any(|font| font == name)
    });
    let looks_changed = palette::show(chosen.light, chosen.dark);
    let themes = cx.global_mut::<Themes>();
    themes.installed_fonts = fonts;
    let page_changed = themes.listed != listed || themes.notices != chosen.notices;
    themes.listed = listed;
    themes.notices = chosen.notices;
    if looks_changed {
        crate::apply_theme(cx);
    } else if page_changed {
        cx.refresh_windows();
    }
}

/// Chooses a theme for one side, saved for the next launch, and shows it at once.
pub(crate) fn set(side: Side, choice: Choice, cx: &mut App) {
    crate::change_preferences(cx, |preferences| match side {
        Side::Light => preferences.light_theme = choice,
        Side::Dark => preferences.dark_theme = choice,
    });
    reload(cx);
}

/// Opens the themes folder in Finder, creating it first if it is missing.
pub(crate) fn open_folder(cx: &mut App) {
    let folder = cx.global::<Themes>().folder.clone();
    match std::fs::create_dir_all(&folder) {
        Ok(()) => cx.open_with_system(&folder),
        Err(error) => fail(format!("The themes folder can't be created: {error}."), cx),
    }
}

/// Writes a copy of Tidal to edit, chooses it for both sides and opens the folder.
pub(crate) fn new_from_tidal(cx: &mut App) {
    let folder = cx.global::<Themes>().folder.clone();
    let written = std::fs::create_dir_all(&folder).and_then(|()| {
        let (file, name) = free_file(&folder);
        std::fs::write(folder.join(&file), new_theme_text(&name)).map(|()| file)
    });
    match written {
        Ok(file) => {
            cx.global_mut::<Themes>().failure = None;
            crate::change_preferences(cx, |preferences| {
                preferences.light_theme = Choice::File(file.clone());
                preferences.dark_theme = Choice::File(file);
            });
            reload(cx);
            cx.open_with_system(&folder);
        }
        Err(error) => fail(format!("The new theme can't be written: {error}."), cx),
    }
}

fn fail(failure: String, cx: &mut App) {
    cx.global_mut::<Themes>().failure = Some(failure);
    cx.refresh_windows();
}

#[cfg(test)]
mod tests {
    use super::*;

    const JADE: &str = r##"{
  "version": 1,
  "name": "Celadon Jade",
  "fonts": { "ui": "Figtree", "mono": "JetBrains Mono", "size": 15 },
  "variants": {
    "dark": {
      "base": "#060d05", "surface": "#0b150a", "overlay": "#182315",
      "text": "#dbe7d8", "subtle": "#9ba799", "edge": "#748072",
      "accent": "#ffaee7", "on-accent": "#060d05", "focus": "#4ce7d8",
      "green": "#acde83", "red": "#ffb7b0", "yellow": "#ebca60",
      "blue": "#99d4ff", "magenta": "#ffaee7"
    }
  }
}"##;
    const SKY: &str =
        r##"{ "name": "Celadon Sky", "variants": { "light": { "base": "#eaf6e8" } } }"##;

    fn folder(files: &[(&str, &str)]) -> tempfile::TempDir {
        let folder = tempfile::tempdir().unwrap();
        for (name, text) in files {
            std::fs::write(folder.path().join(name), text).unwrap();
        }
        folder
    }
    fn file(name: &str) -> Choice {
        Choice::File(name.into())
    }
    fn hex(color: &str) -> Hsla {
        rgb(u32::from_str_radix(&color[1..], 16).unwrap()).into()
    }
    fn every_font(_: &str) -> bool {
        true
    }
    fn invalid(text: &str) -> (Option<usize>, String) {
        match parse(text) {
            Err(Fault::Invalid { line, reason }) => (line, reason),
            Err(other) => panic!("{other:?}"),
            Ok(_) => panic!("parsed"),
        }
    }

    #[test]
    fn tidal_is_a_theme_with_every_color_in_both_variants() {
        for side in [Side::Light, Side::Dark] {
            assert_eq!(tidal_colors(side).len(), KEYS.len(), "{side:?}");
        }
        let light = tidal_look(Side::Light);
        assert_eq!(light.palette.accent, hex("#902818"));
        assert_eq!(light.fonts.ui.as_ref(), SYSTEM_UI_FONT);
        assert_eq!(light.fonts.mono.as_ref(), SYSTEM_MONO_FONT);
        assert_eq!(light.fonts.size, 14.);
    }

    #[test]
    fn a_complete_dark_only_theme_shows_on_dark_with_its_fonts() {
        let themes = folder(&[("celadon-jade.json", JADE)]);
        let chosen = choose(
            themes.path(),
            &Choice::Tidal,
            &file("celadon-jade.json"),
            &mut every_font,
        );
        assert_eq!(chosen.dark.palette.base, hex("#060d05"));
        assert_eq!(chosen.dark.palette.on_accent, hex("#060d05"));
        assert_eq!(chosen.dark.fonts.ui.as_ref(), "Figtree");
        assert_eq!(chosen.dark.fonts.mono.as_ref(), "JetBrains Mono");
        assert_eq!(chosen.dark.fonts.size, 15.);
        assert_eq!(chosen.light, tidal_look(Side::Light));
        assert!(chosen.notices.is_empty());
    }

    #[test]
    fn a_light_only_theme_fills_its_gaps_from_tidal_light() {
        let themes = folder(&[("celadon-sky.json", SKY)]);
        let chosen = choose(
            themes.path(),
            &file("celadon-sky.json"),
            &Choice::Tidal,
            &mut every_font,
        );
        let tidal = tidal_look(Side::Light);
        assert_eq!(chosen.light.palette.base, hex("#EAF6E8"));
        assert_eq!(chosen.light.palette.accent, tidal.palette.accent);
        assert_eq!(chosen.light.fonts, tidal.fonts);
        assert!(chosen.notices.is_empty());
    }

    #[test]
    fn a_partial_variant_changes_only_what_it_names() {
        let partial = r##"{ "name": "Tidal, rust accent", "variants": { "dark": { "accent": "#FF7A59" } } }"##;
        let themes = folder(&[("rust-accent.json", partial)]);
        let chosen = choose(
            themes.path(),
            &Choice::Tidal,
            &file("rust-accent.json"),
            &mut every_font,
        );
        let tidal = tidal_look(Side::Dark).palette;
        assert_eq!(chosen.dark.palette.accent, hex("#FF7A59"));
        assert_eq!(
            Palette {
                accent: tidal.accent,
                ..chosen.dark.palette
            },
            tidal
        );
    }

    #[test]
    fn a_bad_color_names_its_line_and_value() {
        let broken = JADE.replace("\"accent\": \"#ffaee7\"", "\"accent\": \"#ffaee\"");
        assert_eq!(
            invalid(&broken),
            (
                Some(9),
                "\"accent\": \"#ffaee\" is not a color like #1A262D".into()
            )
        );
        let themes = folder(&[("celadon-jade.json", &broken), ("celadon-sky.json", SKY)]);
        let chosen = choose(
            themes.path(),
            &file("celadon-sky.json"),
            &file("celadon-jade.json"),
            &mut every_font,
        );
        assert_eq!(chosen.dark, tidal_look(Side::Dark));
        assert_eq!(
            chosen.notices,
            [Notice {
                tone: Tone::Error,
                head: "celadon-jade.json can't be used.".into(),
                body: "Line 9: \"accent\": \"#ffaee\" is not a color like #1A262D. Dark mode \
                       shows Tidal until the file is fixed; light mode still shows Celadon Sky."
                    .into(),
                use_tidal: vec![Side::Dark],
            }]
        );
    }

    #[test]
    fn a_file_with_neither_variant_or_no_name_is_an_error() {
        assert_eq!(
            invalid(r#"{ "name": "Empty", "variants": {} }"#),
            (None, "it has neither a light nor a dark variant".into())
        );
        assert!(
            invalid(r#"{ "variants": { "dark": {} } }"#)
                .1
                .contains("`name`")
        );
        assert_eq!(invalid("{ \"name\": \"x\",\n}").0, Some(2));
    }

    #[test]
    fn a_size_outside_11_to_20_is_an_error() {
        let text = "{ \"name\": \"Big\",\n  \"fonts\": { \"size\": 24 },\n  \"variants\": { \"dark\": {} } }";
        assert_eq!(
            invalid(text),
            (Some(2), "\"size\": 24 is not a size from 11 to 20".into())
        );
        for size in ["11", "20", "13.5"] {
            let text = text.replace("24", size);
            assert!(parse(&text).is_ok(), "{size}");
        }
    }

    #[test]
    fn unknown_keys_are_ignored_everywhere() {
        let text = r##"{ "name": "Odd", "brand": { "rust": "#902818" }, "heading": 3,
            "fonts": { "heading": "Caprasimo" },
            "variants": { "sepia": {}, "dark": { "cyan": "#86C1BC", "muted": 1, "accent": "#abcdef" } } }"##;
        let theme = parse(text).unwrap();
        assert_eq!(theme.variant(Side::Dark).unwrap().len(), 1);
        assert!(theme.variant(Side::Light).is_none());
    }

    #[test]
    fn each_menu_lists_the_files_with_its_variant_by_name() {
        let tidal = include_str!("../../../design/themes/wiffletree-tidal.json");
        let themes = folder(&[
            ("celadon-jade.json", JADE),
            ("celadon-sky.json", SKY),
            ("a-tidal.json", tidal),
            ("broken.json", "{"),
            ("notes.txt", SKY),
        ]);
        let listed = list(themes.path());
        let names = |side| -> Vec<&str> {
            (listed.iter())
                .filter(|listed| listed.has(side))
                .map(|listed| listed.file.as_str())
                .collect()
        };
        assert_eq!(names(Side::Light), ["celadon-sky.json", "a-tidal.json"]);
        assert_eq!(names(Side::Dark), ["celadon-jade.json", "a-tidal.json"]);
        assert!(list(&themes.path().join("missing")).is_empty());

        // A broken file is in neither menu, but a menu that still has it chosen names it.
        let bad = JADE.replace("#ffaee7", "#ffaee");
        let themes = folder(&[("celadon-jade.json", &bad), ("broken.json", "{")]);
        let page = Themes {
            listed: list(themes.path()),
            ..Themes::new(themes.path().into())
        };
        assert!(!page.listed.iter().any(|l| l.has(Side::Dark)));
        assert_eq!(page.label(&file("celadon-jade.json")), "Celadon Jade");
        assert_eq!(page.label(&file("broken.json")), "broken.json");
    }

    #[test]
    fn a_deleted_file_shows_tidal_and_keeps_the_choice() {
        let themes = folder(&[("celadon-sky.json", SKY)]);
        let chosen = choose(
            themes.path(),
            &file("celadon-sky.json"),
            &file("celadon-jade.json"),
            &mut every_font,
        );
        assert_eq!(chosen.dark, tidal_look(Side::Dark));
        assert_eq!(chosen.notices[0].head, "celadon-jade.json is gone.");
        assert_eq!(
            chosen.notices[0].body,
            "Dark mode shows Tidal until the file is fixed; light mode still shows Celadon Sky."
        );
    }

    #[test]
    fn a_file_chosen_for_a_side_it_lacks_shows_tidal_there() {
        let themes = folder(&[("celadon-jade.json", JADE)]);
        let chosen = choose(
            themes.path(),
            &file("celadon-jade.json"),
            &file("celadon-jade.json"),
            &mut every_font,
        );
        assert_eq!(chosen.light, tidal_look(Side::Light));
        assert_eq!(chosen.dark.palette.base, hex("#060d05"));
        assert_eq!(chosen.notices.len(), 1);
        assert!(
            chosen.notices[0]
                .body
                .starts_with("It has no light variant.")
        );
        assert_eq!(chosen.notices[0].use_tidal, [Side::Light]);
    }

    #[test]
    fn one_broken_file_on_both_sides_is_one_banner_for_both() {
        let themes = folder(&[("mine.json", "not json")]);
        let chosen = choose(
            themes.path(),
            &file("mine.json"),
            &file("mine.json"),
            &mut every_font,
        );
        assert_eq!(chosen.notices.len(), 1);
        assert!(
            chosen.notices[0]
                .body
                .ends_with("Light and dark mode show Tidal until the file is fixed.")
        );
        assert_eq!(chosen.notices[0].use_tidal, [Side::Light, Side::Dark]);
    }

    #[test]
    fn a_missing_font_falls_back_and_warns_while_the_colors_apply() {
        let themes = folder(&[("celadon-jade.json", JADE)]);
        let chosen = choose(
            themes.path(),
            &Choice::Tidal,
            &file("celadon-jade.json"),
            &mut |name| name == "JetBrains Mono",
        );
        assert_eq!(chosen.dark.fonts.ui.as_ref(), SYSTEM_UI_FONT);
        assert_eq!(chosen.dark.fonts.mono.as_ref(), "JetBrains Mono");
        assert_eq!(chosen.dark.palette.base, hex("#060d05"));
        assert_eq!(
            chosen.notices,
            [Notice {
                tone: Tone::Warning,
                head: "Figtree isn't installed.".into(),
                body: "Celadon Jade's colors and JetBrains Mono are in use; the UI text uses \
                       the system font."
                    .into(),
                use_tidal: vec![],
            }]
        );
    }

    #[test]
    fn two_themes_lacking_the_same_fonts_share_one_warning() {
        let sky = SKY.replace(
            "\"variants\"",
            "\"fonts\": { \"ui\": \"Figtree\", \"mono\": \"JetBrains Mono\" }, \"variants\"",
        );
        let themes = folder(&[("celadon-jade.json", JADE), ("celadon-sky.json", &sky)]);
        let chosen = choose(
            themes.path(),
            &file("celadon-sky.json"),
            &file("celadon-jade.json"),
            &mut |_| false,
        );
        assert_eq!(chosen.notices.len(), 1);
        assert_eq!(
            chosen.notices[0].head,
            "Figtree and JetBrains Mono aren't installed."
        );
        assert_eq!(
            chosen.notices[0].body,
            "Celadon Sky's and Celadon Jade's colors are in use; the UI text and code use the \
             system fonts."
        );
    }

    #[test]
    fn a_new_theme_is_tidal_under_a_free_name() {
        let themes = folder(&[("my-theme.json", SKY)]);
        let (file, name) = free_file(themes.path());
        assert_eq!(
            (file.as_str(), name.as_str()),
            ("my-theme-2.json", "My theme 2")
        );
        let theme = parse(&new_theme_text(&name)).unwrap();
        assert_eq!(theme.name, "My theme 2");
        for side in [Side::Light, Side::Dark] {
            assert_eq!(theme.variant(side), Some(tidal_colors(side)));
        }
    }

    #[test]
    fn a_choice_is_a_bare_json_file_name_or_tidal() {
        let read = |json: &str| serde_json::from_str::<Choice>(json).unwrap();
        assert_eq!(read(r#""celadon.json""#), file("celadon.json"));
        for other in [
            r#""tidal""#,
            r#""../x.json""#,
            r#""a/b.json""#,
            "7",
            "null",
            r#""x.txt""#,
        ] {
            assert_eq!(read(other), Choice::Tidal, "{other}");
        }
        assert_eq!(serde_json::to_string(&Choice::Tidal).unwrap(), r#""tidal""#);
    }
}
