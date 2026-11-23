//! The helper's socket: one JSON request per line, answered `{"ok":true,"result":…}` or
//! `{"ok":false,"error":"…"}`, as the app's automation socket answers.
//!
//! The host first opens the control connection, `{"do":"control"}`, and holds it for the run:
//! the helper answers with its process id and grants, and later writes `{"event":"escape"}`
//! or `{"event":"stop"}` on it. When the host closes it, the helper quits. Every other
//! connection carries one request from the desktop driver, which the helper checks against
//! the run's apps before it acts.
use crate::panel::StopSource;
use crate::policy::{self, Allowed, Keystroke, Point, Rect, Window};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// Text is typed a character at a time; before each, the run must not have been stopped and an
/// allowed app must be frontmost. So Esc or Stop ends the typing within a character.
const TYPED_PIECE: usize = 1;
/// How long a driver request may take to arrive.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Button {
    Left,
    Right,
}

/// What the screen can do, as the helper reaches it. Every check is the protocol's.
pub trait Desktop: Send + Sync {
    /// On-screen windows, front to back.
    fn windows(&self) -> Result<Vec<Window>>;
    /// The frontmost app's bundle id and process id.
    fn frontmost(&self) -> Option<(String, i32)>;
    fn click(&self, at: Point, button: Button, count: u32) -> Result<()>;
    fn move_to(&self, at: Point) -> Result<()>;
    fn scroll(&self, at: Point, dy: i32) -> Result<()>;
    fn drag(&self, from: Point, to: Point) -> Result<()>;
    /// Posts to `pid`'s app alone.
    fn type_text(&self, pid: i32, text: &str) -> Result<()>;
    fn key(&self, pid: i32, keystroke: Keystroke) -> Result<()>;
    /// The main display with only `apps`' windows on it.
    fn capture(&self, apps: &Allowed, native: bool) -> Result<Capture>;
    fn open(&self, app: &str) -> Result<()>;
}

pub struct Capture {
    pub png: Vec<u8>,
    pub width: usize,
    pub height: usize,
    /// Image pixels per screen point.
    pub scale: f64,
    /// The display the image shows, in screen points.
    pub display: Rect,
}

/// Whether the helper may see and touch the screen, and hear Escape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grants {
    pub screen_recording: bool,
    pub accessibility: bool,
    /// The Escape listener is in place; without it the run could not be stopped by the human.
    pub escape: bool,
}

/// The run's control connection, once the host has opened it, and whether the run stopped.
#[derive(Default)]
pub struct Control {
    stream: Mutex<Option<UnixStream>>,
    stopped: AtomicBool,
}
impl Control {
    /// Tells the host the run must stop, and refuses every later request. Esc, the menu bar's
    /// Stop and the panel's all end the run this one way.
    pub fn stop(&self, source: StopSource) {
        if self.stopped.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(stream) = self.stream.lock().unwrap().as_mut() {
            let _ = writeln!(stream, "{}", json!({"event": source.event()}));
        }
    }
    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }
    pub fn held(&self) -> bool {
        self.stream.lock().unwrap().is_some()
    }
}

/// Serves `listener` until the process quits. `gone` runs once the host closes its control
/// connection.
pub fn serve(
    listener: UnixListener,
    desktop: Arc<dyn Desktop>,
    allowed: Allowed,
    hello: Value,
    control: Arc<Control>,
    gone: impl Fn() + Send + Sync + 'static,
) {
    let gone = Arc::new(gone);
    for stream in listener.incoming().flatten() {
        let (desktop, allowed, hello) = (desktop.clone(), allowed.clone(), hello.clone());
        let (control, gone) = (control.clone(), gone.clone());
        std::thread::spawn(move || {
            connection(stream, &*desktop, &allowed, &hello, &control, &*gone);
        });
    }
}

fn connection(
    mut stream: UnixStream,
    desktop: &dyn Desktop,
    allowed: &Allowed,
    hello: &Value,
    control: &Control,
    gone: &dyn Fn(),
) {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let mut line = String::new();
    let Ok(reader) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(reader);
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let request: Value = serde_json::from_str(&line).unwrap_or_default();
    if request["do"] == "control" {
        return hold_control(stream, reader, hello, control, gone);
    }
    let reply = match answer(desktop, allowed, control, &request) {
        Ok(result) => json!({"ok": true, "result": result}),
        Err(error) => json!({"ok": false, "error": format!("{error:#}")}),
    };
    let _ = writeln!(stream, "{reply}");
}

/// Keeps the host's control connection, and calls `gone` once it closes.
fn hold_control(
    mut stream: UnixStream,
    mut reader: BufReader<UnixStream>,
    hello: &Value,
    control: &Control,
    gone: &dyn Fn(),
) {
    {
        let mut held = control.stream.lock().unwrap();
        if held.is_some() {
            let refused =
                json!({"ok": false, "error": "The run already has its control connection"});
            let _ = writeln!(stream, "{refused}");
            return;
        }
        let _ = writeln!(stream, "{}", json!({"ok": true, "result": hello}));
        *held = stream.try_clone().ok();
    }
    let _ = stream.set_read_timeout(None);
    let _ = reader.get_ref().set_read_timeout(None);
    // The host never writes again: anything but the end is ignored.
    let mut ignored = String::new();
    while matches!(reader.read_line(&mut ignored), Ok(read) if read > 0) {
        ignored.clear();
    }
    control.stopped.store(true, Ordering::SeqCst);
    gone();
}

/// One driver request, checked against the run's apps and the screen as it is now.
pub fn answer(
    desktop: &dyn Desktop,
    allowed: &Allowed,
    control: &Control,
    request: &Value,
) -> Result<Value> {
    ensure!(control.held(), "Refused: the run has not started");
    ensure!(!control.stopped(), "Refused: the run was stopped");
    let number = |key: &str| -> Result<f64> {
        let value = request[key]
            .as_f64()
            .with_context(|| format!("Give {key} as a number"))?;
        ensure!(value.is_finite(), "Give {key} as a finite number");
        Ok(value)
    };
    let point = |x: &str, y: &str| -> Result<Point> {
        Ok(Point {
            x: number(x)?,
            y: number(y)?,
        })
    };
    let text = |key: &str| -> Result<&str> {
        request[key].as_str().with_context(|| format!("Give {key}"))
    };
    let on_allowed_window = |at: Point| policy::check_point(&desktop.windows()?, at, allowed);
    // The frontmost app's process, once it is checked to be one of the run's.
    let frontmost_pid = || -> Result<i32> {
        let frontmost = desktop.frontmost();
        policy::check_keys(frontmost.as_ref().map(|(app, _)| app.as_str()), allowed)?;
        Ok(frontmost.map(|(_, pid)| pid).unwrap_or_default())
    };
    match request["do"].as_str().unwrap_or_default() {
        "click" => {
            let at = point("x", "y")?;
            let button = match request["button"].as_str().unwrap_or("left") {
                "left" => Button::Left,
                "right" => Button::Right,
                other => bail!("Give button as left or right, not {other}"),
            };
            let count = request["count"].as_u64().unwrap_or(1);
            ensure!((1..=2).contains(&count), "Give count as 1 or 2");
            on_allowed_window(at)?;
            desktop.click(at, button, count as u32)?;
        }
        "move" => {
            let at = point("x", "y")?;
            on_allowed_window(at)?;
            desktop.move_to(at)?;
        }
        "scroll" => {
            let at = point("x", "y")?;
            let dy = request["dy"]
                .as_i64()
                .context("Give dy as a whole number")?;
            let dy = i32::try_from(dy).context("dy is too large")?;
            on_allowed_window(at)?;
            desktop.scroll(at, dy)?;
        }
        "drag" => {
            let (from, to) = (point("x1", "y1")?, point("x2", "y2")?);
            on_allowed_window(from)?;
            on_allowed_window(to)?;
            desktop.drag(from, to)?;
        }
        "type" => {
            let text = text("text")?;
            let chars: Vec<char> = text.chars().collect();
            for piece in chars.chunks(TYPED_PIECE) {
                ensure!(!control.stopped(), "Refused: the run was stopped");
                let pid = frontmost_pid()?;
                desktop.type_text(pid, &piece.iter().collect::<String>())?;
            }
        }
        "key" => {
            let keystroke = policy::parse_keystroke(text("keystroke")?)?;
            let pid = frontmost_pid()?;
            desktop.key(pid, keystroke)?;
        }
        "open_app" => {
            let app = text("bundle_id")?;
            ensure!(
                allowed.contains(app),
                "Refused: this run may open only {}",
                allowed.apps().join(", ")
            );
            desktop.open(app)?;
        }
        "windows" => {
            let windows = desktop.windows()?;
            let frontmost = desktop.frontmost().map(|(app, _)| app);
            let listed: Vec<Value> = policy::allowed_windows(&windows, allowed)
                .into_iter()
                .map(|window| {
                    json!({
                        "app": window.app,
                        "title": window.title,
                        "frontmost": window.app == frontmost,
                        "x": window.bounds.x,
                        "y": window.bounds.y,
                        "width": window.bounds.width,
                        "height": window.bounds.height,
                    })
                })
                .collect();
            return Ok(json!({"apps": allowed.apps(), "windows": listed}));
        }
        "screenshot" => {
            let native = request["native"].as_bool().unwrap_or(false);
            let capture = desktop.capture(allowed, native)?;
            let png =
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &capture.png);
            return Ok(json!({
                "png": png,
                "width": capture.width,
                "height": capture.height,
                "scale": capture.scale,
                "display": {
                    "x": capture.display.x,
                    "y": capture.display.y,
                    "width": capture.display.width,
                    "height": capture.display.height,
                },
            }));
        }
        other => bail!("No request {other}"),
    }
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::tests::{calculator, window};

    /// A screen with Calculator in front of Mail, which records what it was asked to do.
    #[derive(Default)]
    struct Screen {
        frontmost: Mutex<Option<(String, i32)>>,
        done: Mutex<Vec<String>>,
        /// Stopped as the first text is typed, as when the human presses Esc mid-request.
        stops_while_typing: Mutex<Option<Arc<Control>>>,
    }
    impl Screen {
        fn front(app: &str) -> Self {
            let screen = Self::default();
            *screen.frontmost.lock().unwrap() = Some((app.into(), 42));
            screen
        }
        fn record(&self, what: String) -> Result<()> {
            self.done.lock().unwrap().push(what);
            Ok(())
        }
        fn done(&self) -> Vec<String> {
            self.done.lock().unwrap().clone()
        }
    }
    impl Desktop for Screen {
        fn windows(&self) -> Result<Vec<Window>> {
            let mut calculator = window("com.apple.calculator", 0, (0., 0., 230., 400.));
            calculator.title = Some("Calculator".into());
            Ok(vec![
                calculator,
                window("com.apple.mail", 0, (0., 0., 1000., 800.)),
            ])
        }
        fn frontmost(&self) -> Option<(String, i32)> {
            self.frontmost.lock().unwrap().clone()
        }
        fn click(&self, at: Point, button: Button, count: u32) -> Result<()> {
            self.record(format!("click {} {} {button:?} {count}", at.x, at.y))
        }
        fn move_to(&self, at: Point) -> Result<()> {
            self.record(format!("move {} {}", at.x, at.y))
        }
        fn scroll(&self, at: Point, dy: i32) -> Result<()> {
            self.record(format!("scroll {} {} {dy}", at.x, at.y))
        }
        fn drag(&self, from: Point, to: Point) -> Result<()> {
            self.record(format!("drag {} {} {} {}", from.x, from.y, to.x, to.y))
        }
        fn type_text(&self, pid: i32, text: &str) -> Result<()> {
            if let Some(control) = self.stops_while_typing.lock().unwrap().take() {
                control.stop(StopSource::Escape);
            }
            self.record(format!("type {pid} {text}"))
        }
        fn key(&self, pid: i32, keystroke: Keystroke) -> Result<()> {
            self.record(format!("key {pid} {}", keystroke.key_code))
        }
        fn capture(&self, apps: &Allowed, native: bool) -> Result<Capture> {
            self.record(format!("capture {} {native}", apps.apps().join(",")))?;
            Ok(Capture {
                png: b"PNG".to_vec(),
                width: 3024,
                height: 1964,
                scale: 2.,
                display: Rect {
                    x: 0.,
                    y: 0.,
                    width: 1512.,
                    height: 982.,
                },
            })
        }
        fn open(&self, app: &str) -> Result<()> {
            self.record(format!("open {app}"))
        }
    }
    fn started() -> Control {
        let control = Control::default();
        let (ours, _theirs) = UnixStream::pair().unwrap();
        *control.stream.lock().unwrap() = Some(ours);
        control
    }
    fn ask(screen: &Screen, control: &Control, request: Value) -> Result<Value> {
        answer(screen, &calculator(), control, &request)
    }

    #[test]
    fn input_is_checked_against_the_screen_before_it_is_posted() {
        let screen = Screen::front("com.apple.calculator");
        let control = started();
        ask(&screen, &control, json!({"do":"click","x":50,"y":60})).unwrap();
        ask(
            &screen,
            &control,
            json!({"do":"click","x":50,"y":60,"button":"right","count":2}),
        )
        .unwrap();
        ask(&screen, &control, json!({"do":"move","x":5,"y":6})).unwrap();
        ask(
            &screen,
            &control,
            json!({"do":"scroll","x":5,"y":6,"dy":-300}),
        )
        .unwrap();
        ask(
            &screen,
            &control,
            json!({"do":"drag","x1":5,"y1":6,"x2":100,"y2":200}),
        )
        .unwrap();
        // Over Mail, or dragged onto it.
        for refused in [
            json!({"do":"click","x":500,"y":60}),
            json!({"do":"move","x":500,"y":60}),
            json!({"do":"scroll","x":500,"y":60,"dy":10}),
            json!({"do":"drag","x1":5,"y1":6,"x2":500,"y2":60}),
            json!({"do":"click","x":50,"y":60,"count":3}),
            json!({"do":"click","x":"left","y":60}),
        ] {
            assert!(
                ask(&screen, &control, refused.clone()).is_err(),
                "{refused}"
            );
        }
        assert_eq!(
            screen.done(),
            [
                "click 50 60 Left 1",
                "click 50 60 Right 2",
                "move 5 6",
                "scroll 5 6 -300",
                "drag 5 6 100 200"
            ]
        );
    }

    #[test]
    fn keys_and_text_go_only_to_an_allowed_frontmost_app() {
        let screen = Screen::front("com.apple.calculator");
        let control = started();
        ask(&screen, &control, json!({"do":"key","keystroke":"cmd-a"})).unwrap();
        let long = "1+2=3, then 40 more characters to type!";
        ask(&screen, &control, json!({"do":"type","text":long})).unwrap();
        let typed: String = (screen.done().iter())
            .filter_map(|done| done.strip_prefix("type 42 "))
            .collect();
        assert_eq!(typed, long);
        assert_eq!(
            screen.done().len(),
            1 + long.chars().count().div_ceil(TYPED_PIECE)
        );
        let terminal = Screen::front("com.apple.Terminal");
        for request in [
            json!({"do":"key","keystroke":"enter"}),
            json!({"do":"type","text":"rm -rf ~"}),
        ] {
            let refused = ask(&terminal, &control, request).unwrap_err();
            assert!(refused.to_string().contains("frontmost"), "{refused}");
        }
        assert!(terminal.done().is_empty());
        assert!(ask(&screen, &control, json!({"do":"key","keystroke":"hyper-x"})).is_err());
    }

    /// Esc or Stop during a long type_text ends it within a character, not at its end.
    #[test]
    fn a_stop_while_typing_ends_the_typing_at_once() {
        let screen = Screen::front("com.apple.calculator");
        let control = Arc::new(started());
        *screen.stops_while_typing.lock().unwrap() = Some(control.clone());
        let long = "x".repeat(160);
        let refused = ask(&screen, &control, json!({"do":"type","text":long})).unwrap_err();
        assert!(refused.to_string().contains("stopped"), "{refused}");
        assert_eq!(screen.done(), ["type 42 x"]);
    }

    #[test]
    fn only_the_runs_apps_are_opened_listed_and_captured() {
        let screen = Screen::front("com.apple.calculator");
        let control = started();
        ask(
            &screen,
            &control,
            json!({"do":"open_app","bundle_id":"com.apple.calculator"}),
        )
        .unwrap();
        let refused = ask(
            &screen,
            &control,
            json!({"do":"open_app","bundle_id":"com.apple.Terminal"}),
        )
        .unwrap_err();
        assert!(
            refused.to_string().contains("com.apple.calculator"),
            "{refused}"
        );
        let listed = ask(&screen, &control, json!({"do":"windows"})).unwrap();
        assert_eq!(listed["windows"].as_array().unwrap().len(), 1);
        assert_eq!(listed["windows"][0]["title"], "Calculator");
        assert_eq!(listed["windows"][0]["frontmost"], true);
        assert!(!listed.to_string().contains("mail"), "{listed}");
        let shot = ask(&screen, &control, json!({"do":"screenshot","native":true})).unwrap();
        assert_eq!(shot["scale"], 2.0);
        assert_eq!(shot["display"]["width"], 1512.0);
        assert_eq!(
            screen.done(),
            [
                "open com.apple.calculator",
                "capture com.apple.calculator true"
            ]
        );
    }

    #[test]
    fn nothing_is_done_before_the_run_starts_or_after_it_stops() {
        let screen = Screen::front("com.apple.calculator");
        let click = json!({"do":"click","x":50,"y":60});
        let waiting = Control::default();
        assert!(ask(&screen, &waiting, click.clone()).is_err());
        let control = started();
        control.stop(StopSource::Escape);
        let refused = ask(&screen, &control, click).unwrap_err();
        assert!(refused.to_string().contains("stopped"), "{refused}");
        assert!(ask(&screen, &control, json!({"do":"screenshot"})).is_err());
        assert!(screen.done().is_empty());
    }

    /// The panel's Stop takes the path Esc and the menu bar's take: the host hears it once on
    /// the control connection, and every later request is refused.
    #[test]
    fn the_panels_stop_ends_the_run_as_esc_does() {
        use std::io::Read as _;
        let screen = Screen::front("com.apple.calculator");
        let control = Control::default();
        let (ours, theirs) = UnixStream::pair().unwrap();
        *control.stream.lock().unwrap() = Some(ours);
        control.stop(StopSource::Panel);
        control.stop(StopSource::Escape);
        // The host's end sees everything said so far once the helper stops writing.
        let held = control.stream.lock().unwrap();
        held.as_ref()
            .unwrap()
            .shutdown(std::net::Shutdown::Write)
            .unwrap();
        drop(held);
        let mut heard = String::new();
        BufReader::new(theirs).read_to_string(&mut heard).unwrap();
        assert_eq!(heard, "{\"event\":\"panel\"}\n");
        let refused = ask(&screen, &control, json!({"do":"click","x":50,"y":60})).unwrap_err();
        assert!(refused.to_string().contains("stopped"), "{refused}");
        assert!(screen.done().is_empty());
    }

    /// The socket end to end: the control connection gets the hello and later the stop, a
    /// second one is refused, a driver request gets its answer, and closing the control
    /// connection ends the helper.
    #[test]
    fn the_socket_holds_one_control_connection_and_answers_each_request() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("h.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let control = Arc::new(Control::default());
        let (gone_tx, gone_rx) = std::sync::mpsc::channel();
        let screen: Arc<dyn Desktop> = Arc::new(Screen::front("com.apple.calculator"));
        let hello =
            json!({"pid": 7, "screen_recording": true, "accessibility": true, "escape": true});
        {
            let control = control.clone();
            let gone_tx = Mutex::new(gone_tx);
            std::thread::spawn(move || {
                serve(listener, screen, calculator(), hello, control, move || {
                    let _ = gone_tx.lock().unwrap().send(());
                })
            });
        }
        let request = |line: Value| -> Value {
            let mut stream = UnixStream::connect(&path).unwrap();
            writeln!(stream, "{line}").unwrap();
            let mut reply = String::new();
            BufReader::new(stream).read_line(&mut reply).unwrap();
            serde_json::from_str(&reply).unwrap()
        };
        // Before the host holds control, nothing is done.
        assert_eq!(request(json!({"do":"windows"}))["ok"], false);
        let mut host = UnixStream::connect(&path).unwrap();
        writeln!(host, "{}", json!({"do":"control"})).unwrap();
        let mut events = BufReader::new(host.try_clone().unwrap());
        let mut line = String::new();
        events.read_line(&mut line).unwrap();
        let hello: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(hello["result"]["pid"], 7);
        assert_eq!(request(json!({"do":"control"}))["ok"], false);
        let listed = request(json!({"do":"windows"}));
        assert_eq!(listed["ok"], true, "{listed}");
        assert_eq!(request(json!({"do":"click","x":500,"y":5}))["ok"], false);
        // The menu bar's Stop reaches the host, and later requests are refused.
        control.stop(StopSource::MenuBar);
        line.clear();
        events.read_line(&mut line).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap(),
            json!({"event":"stop"})
        );
        assert_eq!(request(json!({"do":"windows"}))["ok"], false);
        drop(events);
        host.shutdown(std::net::Shutdown::Both).unwrap();
        gone_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}
