//! What a full-desktop run may touch, decided from what the helper reads off the screen at the
//! moment of each action. Pure, so every rule is tested without a screen.
use anyhow::{Result, bail, ensure};

/// The helper's own bundle, which no run may name: its menu holds Stop.
pub const HELPER_BUNDLE_ID: &str = "com.vyttle.wiffletree.evaluator";
/// The most apps one run may name.
const MOST_APPS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// A rectangle in screen points, from the top-left corner of the main display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x
            && point.y >= self.y
            && point.x < self.x + self.width
            && point.y < self.y + self.height
    }
}

/// One on-screen window, as the window server lists them: front to back, at every layer.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub id: u32,
    /// The bundle id of the app that owns it; `None` for a process that has none.
    pub app: Option<String>,
    pub title: Option<String>,
    pub layer: i64,
    pub bounds: Rect,
}

/// The apps a run may use, by bundle id. Nothing else is ever allowed, and nothing is allowed
/// by default.
#[derive(Clone, Debug, PartialEq)]
pub struct Allowed(Vec<String>);
impl Allowed {
    pub fn new(apps: &[String]) -> Result<Self> {
        ensure!(!apps.is_empty(), "Name at least one app the run may use");
        ensure!(
            apps.len() <= MOST_APPS,
            "Name at most {MOST_APPS} apps for one run"
        );
        for app in apps {
            ensure!(
                is_bundle_id(app),
                "{app} is not a bundle id, such as com.apple.calculator"
            );
            ensure!(
                !same_app(app, HELPER_BUNDLE_ID),
                "A run may not use Wiffletree Evaluator itself"
            );
        }
        Ok(Self(apps.to_vec()))
    }
    pub fn apps(&self) -> &[String] {
        &self.0
    }
    pub fn contains(&self, app: &str) -> bool {
        self.0.iter().any(|allowed| same_app(allowed, app))
    }
    fn owns(&self, window: &Window) -> bool {
        window.app.as_deref().is_some_and(|app| self.contains(app))
    }
}

/// Bundle ids are compared as LaunchServices compares them: without regard to case.
fn same_app(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// A reverse-DNS bundle id: at least two parts of letters, digits and `-`, no wildcard.
pub fn is_bundle_id(text: &str) -> bool {
    text.len() <= 155
        && text.split('.').count() >= 2
        && text.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}

/// Input at `point` reaches the topmost window there, whatever its layer: it must belong to an
/// allowed app. A point over no window would reach the desktop, which belongs to Finder.
pub fn check_point(windows: &[Window], point: Point, allowed: &Allowed) -> Result<()> {
    ensure!(
        point.x.is_finite() && point.y.is_finite(),
        "Give the point as finite numbers"
    );
    match windows.iter().find(|window| window.bounds.contains(point)) {
        Some(window) if allowed.owns(window) => Ok(()),
        // Another app's name would tell the evaluator what else is open, so it is not given.
        Some(_) => bail!(
            "Refused: ({}, {}) is over a window of an app this run may not use. Take a screenshot or list_windows to find this run's windows.",
            point.x,
            point.y
        ),
        None => bail!(
            "Refused: ({}, {}) is over no window of this run's apps.",
            point.x,
            point.y
        ),
    }
}

/// Keys and text go only to an allowed app that is frontmost.
pub fn check_keys(frontmost: Option<&str>, allowed: &Allowed) -> Result<()> {
    ensure!(
        frontmost.is_some_and(|app| allowed.contains(app)),
        "Refused: no app this run may use is frontmost. Click one of its windows or open_app it first."
    );
    Ok(())
}

/// The windows of the allowed apps, front to back.
pub fn allowed_windows<'a>(windows: &'a [Window], allowed: &Allowed) -> Vec<&'a Window> {
    windows
        .iter()
        .filter(|window| allowed.owns(window))
        .collect()
}

/// Escape's virtual key code.
pub const ESCAPE: u16 = 53;
/// Marks every event the helper posts, so its own Escape never stops the run.
pub const POSTED_MARK: i64 = 0x5746_4556; // "WFEV"

/// Whether a key press the helper heard is the human's Escape, which stops the run.
pub fn stops_run(key_code: i64, mark: i64) -> bool {
    key_code == i64::from(ESCAPE) && mark != POSTED_MARK
}

/// The modifier masks of `CGEventFlags`.
pub mod flags {
    pub const SHIFT: u64 = 0x0002_0000;
    pub const CONTROL: u64 = 0x0004_0000;
    pub const OPTION: u64 = 0x0008_0000;
    pub const COMMAND: u64 = 0x0010_0000;
}

/// One key press with its modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keystroke {
    pub key_code: u16,
    pub flags: u64,
}

/// Reads a keystroke written like `enter`, `shift-tab`, `cmd-n`, `cmd-,` or `cmd--`, as a key
/// on a US keyboard.
pub fn parse_keystroke(text: &str) -> Result<Keystroke> {
    let mut rest = text.trim();
    let mut modifiers = 0;
    while let Some((modifier, after)) = rest.split_once('-') {
        let mask = match modifier.to_ascii_lowercase().as_str() {
            "cmd" | "command" => flags::COMMAND,
            "shift" => flags::SHIFT,
            "alt" | "opt" | "option" => flags::OPTION,
            "ctrl" | "control" => flags::CONTROL,
            _ => break,
        };
        if after.is_empty() {
            break;
        }
        modifiers |= mask;
        rest = after;
    }
    let Some(key_code) = key_code(&rest.to_ascii_lowercase()) else {
        bail!(
            "Unknown key {rest:?}: write a keystroke like enter, escape, tab, shift-tab, up, cmd-n or cmd-,"
        );
    };
    Ok(Keystroke {
        key_code,
        flags: modifiers,
    })
}

/// The virtual key code of a key on a US keyboard.
fn key_code(key: &str) -> Option<u16> {
    const LETTERS: &str = "asdfhgzxcv_bqweryt";
    let named = match key {
        "enter" | "return" => 36,
        "tab" => 48,
        "space" => 49,
        "delete" | "backspace" => 51,
        "escape" | "esc" => ESCAPE,
        "forward-delete" | "forwarddelete" => 117,
        "home" => 115,
        "end" => 119,
        "pageup" | "page-up" => 116,
        "pagedown" | "page-down" => 121,
        "left" => 123,
        "right" => 124,
        "down" => 125,
        "up" => 126,
        "f1" => 122,
        "f2" => 120,
        "f3" => 99,
        "f4" => 118,
        "f5" => 96,
        "f6" => 97,
        "f7" => 98,
        "f8" => 100,
        "f9" => 101,
        "f10" => 109,
        "f11" => 103,
        "f12" => 111,
        "minus" => 27,
        _ => {
            let mut chars = key.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return None;
            };
            if let Some(at) = LETTERS.find(c).filter(|_| c != '_') {
                return u16::try_from(at).ok();
            }
            match c {
                'i' => 34,
                'o' => 31,
                'u' => 32,
                'p' => 35,
                'l' => 37,
                'j' => 38,
                'k' => 40,
                'n' => 45,
                'm' => 46,
                '1' => 18,
                '2' => 19,
                '3' => 20,
                '4' => 21,
                '5' => 23,
                '6' => 22,
                '7' => 26,
                '8' => 28,
                '9' => 25,
                '0' => 29,
                '=' => 24,
                '-' => 27,
                ']' => 30,
                '[' => 33,
                '\'' => 39,
                ';' => 41,
                '\\' => 42,
                ',' => 43,
                '/' => 44,
                '.' => 47,
                '`' => 50,
                _ => return None,
            }
        }
    };
    Some(named)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn window(app: &str, layer: i64, bounds: (f64, f64, f64, f64)) -> Window {
        Window {
            id: 0,
            app: (!app.is_empty()).then(|| app.to_owned()),
            title: None,
            layer,
            bounds: Rect {
                x: bounds.0,
                y: bounds.1,
                width: bounds.2,
                height: bounds.3,
            },
        }
    }
    pub(crate) fn calculator() -> Allowed {
        Allowed::new(&["com.apple.calculator".into()]).unwrap()
    }

    #[test]
    fn a_run_names_its_apps_by_bundle_id_and_never_the_helper() {
        assert!(Allowed::new(&[]).is_err());
        for bad in [
            "Calculator",
            "com.apple.*",
            "com..apple",
            "",
            "com.apple.calc ulator",
        ] {
            assert!(Allowed::new(&[bad.into()]).is_err(), "{bad}");
        }
        assert!(Allowed::new(&["COM.vyttle.wiffletree.Evaluator".into()]).is_err());
        let eleven: Vec<String> = (0..11).map(|n| format!("com.example.app{n}")).collect();
        assert!(Allowed::new(&eleven).is_err());
        let allowed = calculator();
        assert!(allowed.contains("com.apple.Calculator"));
        assert!(!allowed.contains("com.apple.finder"));
    }

    #[test]
    fn input_reaches_only_a_point_whose_topmost_window_is_an_allowed_apps() {
        let allowed = calculator();
        let at = |x, y| Point { x, y };
        // Front to back: a notification banner over part of Calculator, then Calculator, then
        // Mail behind it.
        let windows = [
            window("com.apple.notificationcenterui", 23, (100., 0., 300., 80.)),
            window("com.apple.calculator", 0, (0., 0., 230., 400.)),
            window("com.apple.mail", 0, (0., 0., 1000., 800.)),
        ];
        check_point(&windows, at(50., 200.), &allowed).unwrap();
        // Calculator lies under the banner here, but the banner takes the click.
        let covered = check_point(&windows, at(150., 40.), &allowed).unwrap_err();
        assert!(covered.to_string().starts_with("Refused"), "{covered}");
        assert!(!covered.to_string().contains("notification"), "{covered}");
        // Only Mail is here.
        assert!(check_point(&windows, at(500., 500.), &allowed).is_err());
        // Nothing is here: the desktop.
        assert!(check_point(&windows, at(1500., 900.), &allowed).is_err());
        // A window with no bundle id is no allowed app's.
        let unknown = [window("", 0, (0., 0., 500., 500.))];
        assert!(check_point(&unknown, at(10., 10.), &allowed).is_err());
        assert!(check_point(&windows, at(f64::NAN, 10.), &allowed).is_err());
        // A window's far edges are outside it.
        assert!(check_point(&windows, at(230., 200.), &allowed).is_err());
    }

    #[test]
    fn keys_need_an_allowed_app_frontmost() {
        let allowed = calculator();
        check_keys(Some("com.apple.calculator"), &allowed).unwrap();
        assert!(check_keys(Some("com.apple.Terminal"), &allowed).is_err());
        assert!(check_keys(None, &allowed).is_err());
    }

    #[test]
    fn only_the_allowed_apps_windows_are_listed() {
        let windows = [
            window("com.apple.mail", 0, (0., 0., 10., 10.)),
            window("com.apple.calculator", 0, (0., 0., 10., 10.)),
        ];
        let listed = allowed_windows(&windows, &calculator());
        assert_eq!(listed, [&windows[1]]);
    }

    #[test]
    fn only_the_humans_escape_stops_the_run() {
        assert!(stops_run(53, 0));
        assert!(!stops_run(53, POSTED_MARK));
        assert!(!stops_run(36, 0));
    }

    #[test]
    fn keystrokes_read_as_a_us_keyboards_keys_with_their_modifiers() {
        let key = |text| parse_keystroke(text).unwrap();
        assert_eq!(
            key("enter"),
            Keystroke {
                key_code: 36,
                flags: 0
            }
        );
        assert_eq!(
            key("shift-tab"),
            Keystroke {
                key_code: 48,
                flags: flags::SHIFT
            }
        );
        assert_eq!(
            key("cmd-n"),
            Keystroke {
                key_code: 45,
                flags: flags::COMMAND
            }
        );
        assert_eq!(key("cmd-,").key_code, 43);
        assert_eq!(
            key("cmd--"),
            Keystroke {
                key_code: 27,
                flags: flags::COMMAND
            }
        );
        assert_eq!(
            key("ctrl-alt-cmd-Q"),
            Keystroke {
                key_code: 12,
                flags: flags::CONTROL | flags::OPTION | flags::COMMAND
            }
        );
        assert_eq!(key("a").key_code, 0);
        assert_eq!(key("v").key_code, 9);
        assert_eq!(key("b").key_code, 11);
        assert_eq!(key("t").key_code, 17);
        assert_eq!(key("escape").key_code, ESCAPE);
        for bad in ["", "cmd-", "hyper-a", "ab", "_"] {
            assert!(parse_keystroke(bad).is_err(), "{bad}");
        }
    }
}
