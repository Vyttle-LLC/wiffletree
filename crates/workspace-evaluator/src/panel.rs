//! What the floating control panel shows and where it sits (`ui/control-overlay.html`, approved
//! by Michael on November 23, 2026). Pure, so it is tested without a screen.
use crate::policy::{Rect, Window};
use std::time::Duration;

/// The panel's title while the run controls the screen.
pub const TITLE: &str = "Evaluator using this Mac";
/// What the panel says while the run ends, before it disappears.
pub const STOPPING: (&str, &str) = ("Stopping…", "Giving your screen back");
/// The gap between the panel and the screen's edges, in points.
const MARGIN: f64 = 12.;

/// The panel's size in points, normally and over a full-screen app.
pub const FULL: (f64, f64) = (460., 52.);
pub const COMPACT: (f64, f64) = (210., 36.);

/// Where each part of the panel sits, in its own points from the bottom left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub size: (f64, f64),
    pub dot: Rect,
    pub title: Rect,
    pub detail: Rect,
    /// The Esc hint; the compact panel has none.
    pub esc: Option<Rect>,
    pub stop: Rect,
}

/// The panel's parts, normally or compact. Stop always sits wholly inside, clear of the text.
pub fn layout(compact: bool) -> Layout {
    let rect = |x, y, width, height| Rect {
        x,
        y,
        width,
        height,
    };
    let (width, height) = if compact { COMPACT } else { FULL };
    let stop = rect(width - 66., (height - 26.) / 2., 56., 26.);
    if compact {
        Layout {
            size: (width, height),
            dot: rect(12., height / 2. - 4.5, 9., 9.),
            title: rect(28., 10., 66., 16.),
            detail: rect(96., 10., 44., 16.),
            esc: None,
            stop,
        }
    } else {
        Layout {
            size: (width, height),
            dot: rect(12., height / 2. - 4.5, 9., 9.),
            title: rect(30., 27., 318., 17.),
            detail: rect(30., 9., 318., 16.),
            esc: Some(rect(width - 104., (height - 16.) / 2., 30., 16.)),
            stop,
        }
    }
}

/// What the run tells the panel about itself.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// As the human reads it, such as "Claude Opus 5.5".
    pub model: String,
    /// The allowed apps' names, in the order the run named them.
    pub apps: Vec<String>,
    /// How many other full-desktop runs wait for the screen after this one.
    pub queued: usize,
}

/// Reads `serve`'s options before the run's bundle ids: `--model <label>`, which the panel
/// shows, and `--queued <n>`, the full-desktop runs waiting after this one.
pub fn options(args: &[String]) -> anyhow::Result<(String, usize, &[String])> {
    let (mut model, mut queued, mut rest) = (String::new(), 0, args);
    loop {
        match rest {
            [flag, value, more @ ..] if flag == "--model" => {
                model = value.clone();
                rest = more;
            }
            [flag, value, more @ ..] if flag == "--queued" => {
                queued = value
                    .parse()
                    .map_err(|_| anyhow::anyhow!("--queued takes a count, not {value}"))?;
                rest = more;
            }
            [flag, ..] if flag.starts_with("--") => anyhow::bail!("Unknown option {flag}"),
            _ => return Ok((model, queued, rest)),
        }
    }
}

/// "0:42", "12:05", or "1:02:09" past an hour.
pub fn elapsed(time: Duration) -> String {
    let seconds = time.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    match hours {
        0 => format!("{minutes}:{seconds:02}"),
        hours => format!("{hours}:{minutes:02}:{seconds:02}"),
    }
}

/// The panel's detail line: the model, the first two apps then "+N", the time so far, and
/// "1 of N" when more full-desktop runs wait.
pub fn detail(run: &Run, time: Duration) -> String {
    let mut apps = run
        .apps
        .iter()
        .take(2)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if run.apps.len() > 2 {
        apps.push_str(&format!(" +{}", run.apps.len() - 2));
    }
    let mut parts = vec![run.model.clone(), apps, elapsed(time)];
    if run.queued > 0 {
        parts.push(format!("1 of {}", run.queued + 1));
    }
    parts.retain(|part| !part.is_empty());
    parts.join(" · ")
}

/// The corner the panel sits in; the human may drag it to another.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Corner {
    #[default]
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
}
impl Corner {
    /// As stored in the helper's defaults.
    pub fn key(self) -> &'static str {
        match self {
            Self::TopRight => "top-right",
            Self::TopLeft => "top-left",
            Self::BottomRight => "bottom-right",
            Self::BottomLeft => "bottom-left",
        }
    }
    /// A stored corner, or top right for anything else.
    pub fn from_key(key: &str) -> Self {
        [
            Self::TopRight,
            Self::TopLeft,
            Self::BottomRight,
            Self::BottomLeft,
        ]
        .into_iter()
        .find(|corner| corner.key() == key)
        .unwrap_or_default()
    }
}

/// The panel's frame in `corner` of `visible`, the screen less its menu bar and Dock, in
/// AppKit's coordinates, whose origin is at the bottom left.
pub fn frame(corner: Corner, visible: Rect, width: f64, height: f64) -> Rect {
    let left = visible.x + MARGIN;
    let right = visible.x + visible.width - width - MARGIN;
    let bottom = visible.y + MARGIN;
    let top = visible.y + visible.height - height - MARGIN;
    let (x, y) = match corner {
        Corner::TopRight => (right, top),
        Corner::TopLeft => (left, top),
        Corner::BottomRight => (right, bottom),
        Corner::BottomLeft => (left, bottom),
    };
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// The corner nearest where the human left the panel.
pub fn nearest(panel: Rect, visible: Rect) -> Corner {
    let (center_x, center_y) = (panel.x + panel.width / 2., panel.y + panel.height / 2.);
    let right = center_x >= visible.x + visible.width / 2.;
    let top = center_y >= visible.y + visible.height / 2.;
    match (top, right) {
        (true, true) => Corner::TopRight,
        (true, false) => Corner::TopLeft,
        (false, true) => Corner::BottomRight,
        (false, false) => Corner::BottomLeft,
    }
}

/// Whether a full-screen app has the screen, so the panel shrinks to its essentials: an
/// ordinary window, front to back the first, covers the whole display.
pub fn full_screen(windows: &[Window], display: Rect) -> bool {
    windows
        .iter()
        .find(|window| window.layer == 0)
        .is_some_and(|window| window.bounds == display)
}

/// Where a request to stop the run came from. Each takes the same path: the helper logs it,
/// tells the host on the control connection and refuses every later request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopSource {
    Escape,
    MenuBar,
    Panel,
}
impl StopSource {
    /// The event the host reads on the control connection.
    pub fn event(self) -> &'static str {
        match self {
            Self::Escape => "escape",
            Self::MenuBar => "stop",
            Self::Panel => "panel",
        }
    }
    /// What the helper writes to the unified log.
    pub fn note(self) -> &'static str {
        match self {
            Self::Escape => "Esc heard: stopping the run",
            Self::MenuBar => "Stop chosen from the menu bar: stopping the run",
            Self::Panel => "Stop chosen from the panel: stopping the run",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::tests::window;

    fn run(apps: &[&str], queued: usize) -> Run {
        Run {
            model: "Claude Opus 5.5".into(),
            apps: apps.iter().map(|app| (*app).to_owned()).collect(),
            queued,
        }
    }

    #[test]
    fn the_detail_names_the_model_two_apps_the_time_and_the_queue() {
        let time = Duration::from_secs(42);
        assert_eq!(
            detail(&run(&["Calculator"], 0), time),
            "Claude Opus 5.5 · Calculator · 0:42"
        );
        assert_eq!(
            detail(&run(&["Calculator", "TextEdit"], 0), time),
            "Claude Opus 5.5 · Calculator, TextEdit · 0:42"
        );
        assert_eq!(
            detail(
                &run(&["Calculator", "TextEdit", "Preview", "Notes"], 2),
                time
            ),
            "Claude Opus 5.5 · Calculator, TextEdit +2 · 0:42 · 1 of 3"
        );
        assert_eq!(elapsed(Duration::from_secs(725)), "12:05");
        assert_eq!(elapsed(Duration::from_secs(3729)), "1:02:09");
        assert_eq!(elapsed(Duration::ZERO), "0:00");
    }

    /// The human saw the Esc hint but no Stop: Stop must lie wholly inside the panel, clear of
    /// everything else, in both forms.
    #[test]
    fn stop_is_inside_the_panel_and_clear_of_its_text_in_both_forms() {
        let inside = |part: Rect, (width, height): (f64, f64)| {
            part.x >= 0.
                && part.y >= 0.
                && part.x + part.width <= width
                && part.y + part.height <= height
        };
        let apart = |a: Rect, b: Rect| {
            a.x + a.width <= b.x
                || b.x + b.width <= a.x
                || a.y + a.height <= b.y
                || b.y + b.height <= a.y
        };
        for compact in [false, true] {
            let parts = layout(compact);
            assert!(
                parts.stop.width >= 44. && parts.stop.height >= 24.,
                "{parts:?}"
            );
            let others = [
                Some(parts.dot),
                Some(parts.title),
                Some(parts.detail),
                parts.esc,
            ];
            for part in others.into_iter().flatten().chain([parts.stop]) {
                assert!(inside(part, parts.size), "{compact} {part:?}");
            }
            for part in others.into_iter().flatten() {
                assert!(apart(part, parts.stop), "{compact} {part:?} overlaps Stop");
            }
            assert_eq!(parts.esc.is_some(), !compact);
        }
    }

    #[test]
    fn the_panel_sits_in_its_corner_and_a_drag_picks_the_nearest_one() {
        // A 3008×1692 screen with a 33-point menu bar.
        let visible = Rect {
            x: 0.,
            y: 0.,
            width: 3008.,
            height: 1659.,
        };
        let top_right = frame(Corner::TopRight, visible, 360., 54.);
        assert_eq!(
            (top_right.x, top_right.y),
            (3008. - 360. - 12., 1659. - 54. - 12.)
        );
        let bottom_left = frame(Corner::BottomLeft, visible, 360., 54.);
        assert_eq!((bottom_left.x, bottom_left.y), (12., 12.));
        for corner in [
            Corner::TopRight,
            Corner::TopLeft,
            Corner::BottomRight,
            Corner::BottomLeft,
        ] {
            assert_eq!(nearest(frame(corner, visible, 360., 54.), visible), corner);
            assert_eq!(Corner::from_key(corner.key()), corner);
        }
        let dropped = Rect {
            x: 400.,
            y: 300.,
            width: 360.,
            height: 54.,
        };
        assert_eq!(nearest(dropped, visible), Corner::BottomLeft);
        assert_eq!(Corner::from_key("middle"), Corner::TopRight);
    }

    #[test]
    fn the_panel_is_compact_only_over_a_full_screen_app() {
        let display = Rect {
            x: 0.,
            y: 0.,
            width: 3008.,
            height: 1692.,
        };
        let menu = window("com.apple.controlcenter", 25, (0., 0., 3008., 33.));
        let full = window("com.apple.calculator", 0, (0., 0., 3008., 1692.));
        let small = window("com.apple.calculator", 0, (1418., 838., 230., 408.));
        assert!(full_screen(&[menu.clone(), full.clone()], display));
        assert!(!full_screen(&[menu, small.clone(), full], display));
        assert!(!full_screen(&[], display));
    }

    #[test]
    fn serve_reads_the_model_and_queue_before_the_apps() {
        let args: Vec<String> = [
            "--model",
            "Claude Opus 5.5",
            "--queued",
            "2",
            "com.apple.calculator",
        ]
        .map(String::from)
        .into();
        let (model, queued, apps) = options(&args).unwrap();
        assert_eq!((model.as_str(), queued), ("Claude Opus 5.5", 2));
        assert_eq!(apps, ["com.apple.calculator"]);
        let bare: Vec<String> = vec!["com.apple.calculator".into()];
        assert_eq!(options(&bare).unwrap(), (String::new(), 0, &bare[..]));
        for bad in [
            vec!["--queued", "two", "a.b"],
            vec!["--colour", "red", "a.b"],
        ] {
            let bad: Vec<String> = bad.into_iter().map(String::from).collect();
            assert!(options(&bad).is_err());
        }
    }

    #[test]
    fn every_way_to_stop_has_its_own_event_and_log_line() {
        let sources = [StopSource::Escape, StopSource::MenuBar, StopSource::Panel];
        let events: Vec<&str> = sources.iter().map(|source| source.event()).collect();
        assert_eq!(events, ["escape", "stop", "panel"]);
        assert_eq!(
            StopSource::Panel.note(),
            "Stop chosen from the panel: stopping the run"
        );
    }
}
