//! The app driver (OpenSpec change `ui-evaluators`): the MCP server an app run's provider
//! starts, `wiffletree-host app-driver-mcp <socket> <run folder>`. Each tool becomes steps on
//! the test copy's automation socket, so it reaches that copy's window and nothing else.
//! Coordinates are window points from the top-left corner, as in a screenshot at scale 1.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Component, Path, PathBuf},
    time::Duration,
};

/// The server's name, so its tools reach Claude as `mcp__app__<tool>`.
pub const SERVER: &str = "app";
pub const TOOLS: [&str; 11] = [
    "screenshot",
    "click",
    "double_click",
    "right_click",
    "move",
    "type_text",
    "key",
    "scroll",
    "drag",
    "resize_window",
    "window",
];
/// How long a step may take on the socket.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);
/// Time for the app to draw after input, so the next screenshot shows its effect.
const SETTLE: Duration = Duration::from_millis(150);

pub fn tools() -> Value {
    let number = json!({"type":"number"});
    let point = |what: &str| json!({"type":"object","properties":{"x":{"type":"number","description":format!("Window points from the left edge {what}")},"y":{"type":"number","description":"Window points from the top edge"}},"required":["x","y"],"additionalProperties":false});
    let tool = |name: &str, description: &str, schema: Value| json!({"name":name,"description":description,"inputSchema":schema});
    json!([
        tool(
            "screenshot",
            "Capture the app's window and save it in your folder as name (a .png path relative to the folder, such as screenshots/models-dark.png). Returns the image, its size in pixels and the window's scale. scale points (the default) gives one pixel per window point, so a position in the image is the position to click; native gives the display's full resolution, for sampling pixels and measuring fine detail.",
            json!({"type":"object","properties":{"name":{"type":"string"},"scale":{"type":"string","enum":["points","native"]}},"required":["name"],"additionalProperties":false})
        ),
        tool(
            "click",
            "Click the left button once at a window point.",
            point("")
        ),
        tool(
            "double_click",
            "Double-click the left button at a window point.",
            point("")
        ),
        tool(
            "right_click",
            "Click the right button at a window point, as for a context menu.",
            point("")
        ),
        tool(
            "move",
            "Move the pointer to a window point and leave it there, as to hover.",
            point("")
        ),
        tool(
            "type_text",
            "Type text into whatever has keyboard focus.",
            json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"],"additionalProperties":false})
        ),
        tool(
            "key",
            "Press one key or shortcut, written like enter, escape, tab, shift-tab, up, cmd-n or cmd-,.",
            json!({"type":"object","properties":{"keystroke":{"type":"string"}},"required":["keystroke"],"additionalProperties":false})
        ),
        tool(
            "scroll",
            "Scroll at a window point; dy is in points, negative to move the content up and show what is below.",
            json!({"type":"object","properties":{"x":number,"y":number,"dy":{"type":"integer"}},"required":["x","y","dy"],"additionalProperties":false})
        ),
        tool(
            "drag",
            "Press the left button at (x1, y1), move to (x2, y2) in a few steps and release there.",
            json!({"type":"object","properties":{"x1":number,"y1":number,"x2":number,"y2":number},"required":["x1","y1","x2","y2"],"additionalProperties":false})
        ),
        tool(
            "resize_window",
            "Resize the window's content to width by height points. It stays within the app's minimum size and the display; the result gives the size it reached.",
            json!({"type":"object","properties":{"width":number,"height":number},"required":["width","height"],"additionalProperties":false})
        ),
        tool(
            "window",
            "The window's size in points, its scale and the largest size the display allows.",
            json!({"type":"object","properties":{},"additionalProperties":false})
        ),
    ])
}

/// One step on the socket and how long to let the app draw after it.
pub(crate) type Planned = (Value, Duration);

/// The socket steps a tool other than `screenshot` takes, in order.
pub(crate) fn plan(name: &str, args: &Value) -> Result<Vec<Planned>> {
    let number = |key: &str| -> Result<f64> {
        let value = args[key]
            .as_f64()
            .with_context(|| format!("Give {key} as a number"))?;
        ensure!(value.is_finite(), "Give {key} as a finite number");
        Ok(value)
    };
    let text = |key: &str| -> Result<String> {
        Ok(args[key]
            .as_str()
            .with_context(|| format!("Give {key}"))?
            .to_owned())
    };
    let settle = |step: Value| (step, SETTLE);
    Ok(match name {
        "click" | "double_click" => {
            let count = if name == "click" { 1 } else { 2 };
            vec![settle(
                json!({"do":"click","x":number("x")?,"y":number("y")?,"count":count}),
            )]
        }
        "right_click" => {
            let (x, y) = (number("x")?, number("y")?);
            vec![
                (
                    json!({"do":"press","x":x,"y":y,"button":"right"}),
                    Duration::ZERO,
                ),
                settle(json!({"do":"release","x":x,"y":y,"button":"right"})),
            ]
        }
        "move" => vec![settle(
            json!({"do":"move","x":number("x")?,"y":number("y")?}),
        )],
        "type_text" => vec![settle(json!({"do":"type","text":text("text")?}))],
        "key" => vec![settle(json!({"do":"key","keystroke":text("keystroke")?}))],
        "scroll" => {
            let dy = args["dy"].as_i64().context("Give dy as a whole number")?;
            let dy = i32::try_from(dy).context("dy is too large")?;
            vec![settle(
                json!({"do":"scroll","x":number("x")?,"y":number("y")?,"dy":dy}),
            )]
        }
        "drag" => {
            let (x1, y1, x2, y2) = (number("x1")?, number("y1")?, number("x2")?, number("y2")?);
            // As a hand does: the press hit-tests a current frame, and each move draws one.
            let step = Duration::from_millis(60);
            let mut steps = vec![
                (json!({"do":"move","x":x1,"y":y1}), step),
                (json!({"do":"press","x":x1,"y":y1}), step),
            ];
            for part in [0.1, 0.5, 0.9, 1.0] {
                let (x, y) = (x1 + (x2 - x1) * part, y1 + (y2 - y1) * part);
                steps.push((json!({"do":"drag","x":x,"y":y}), step));
            }
            steps.push(settle(json!({"do":"release","x":x2,"y":y2})));
            steps
        }
        "resize_window" => {
            let (width, height) = (number("width")?, number("height")?);
            ensure!(
                width > 0. && height > 0.,
                "Give a positive width and height"
            );
            // AppKit applies a resize after the step returns, and the app then lays out.
            vec![
                (
                    json!({"do":"resize","width":width,"height":height}),
                    Duration::from_millis(500),
                ),
                (json!({"do":"window"}), Duration::ZERO),
            ]
        }
        "window" => vec![(json!({"do":"window"}), Duration::ZERO)],
        other => bail!("No tool {other}"),
    })
}

/// Sends one step to the app's automation socket and returns its result.
pub(crate) fn send(socket: &Path, step: &Value) -> Result<Value> {
    let mut stream = UnixStream::connect(socket).context("The app under test is not running")?;
    stream.set_read_timeout(Some(STEP_TIMEOUT))?;
    stream.set_write_timeout(Some(STEP_TIMEOUT))?;
    writeln!(stream, "{step}")?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    let reply: Value = serde_json::from_str(&line).context("The app under test did not answer")?;
    match reply["ok"].as_bool() {
        Some(true) => Ok(reply["result"].clone()),
        _ => bail!(
            "{}",
            reply["error"]
                .as_str()
                .unwrap_or("The app under test refused the step")
        ),
    }
}

/// Where `name` saves a screenshot: a relative `.png` path in `folder`, with no `..`.
pub(crate) fn screenshot_path(folder: &Path, name: &str) -> Result<PathBuf> {
    let relative = Path::new(name.trim());
    ensure!(
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "Give name as a path inside your folder, such as screenshots/first-run.png"
    );
    let mut path = folder.join(relative);
    if path.extension().is_none_or(|extension| extension != "png") {
        path.as_mut_os_string().push(".png");
    }
    Ok(path)
}

/// Writes `bytes` to `path` in `folder`, creating its folders, without following a link: one
/// already there could lead the write out of the folder, and this server writes outside the
/// evaluator's sandbox.
fn write_inside(folder: &Path, path: &Path, bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let relative = path.strip_prefix(folder)?;
    let mut at = folder.to_path_buf();
    let mut parts = relative.components().peekable();
    while let Some(part) = parts.next() {
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            Ok(entry) => ensure!(!entry.file_type().is_symlink(), "it is a link"),
            Err(_) if parts.peek().is_some() => std::fs::create_dir(&at)?,
            Err(_) => {}
        }
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    file.write_all(bytes)?;
    Ok(())
}

fn screenshot(socket: &Path, folder: &Path, args: &Value) -> Result<Value> {
    let name = args["name"].as_str().context("Give name")?;
    let path = screenshot_path(folder, name)?;
    let native = match args["scale"].as_str().unwrap_or("points") {
        "points" => false,
        "native" => true,
        other => bail!("Give scale as points or native, not {other}"),
    };
    // Asks for a frame, and gives the window's size and scale.
    let window = send(socket, &json!({"do":"window"}))?;
    std::thread::sleep(Duration::from_millis(200));
    let image = send(socket, &json!({"do":"capture","native":native}))?;
    let png = image["png"].as_str().context("The app sent no image")?;
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, png)?;
    write_inside(folder, &path, &bytes).with_context(|| format!("Could not save {name}"))?;
    let saved = path.strip_prefix(folder).unwrap_or(&path);
    let about = json!({
        "saved": saved,
        "width": image["width"],
        "height": image["height"],
        "window_points": {"width": window["width"], "height": window["height"]},
        "scale": window["scale"],
    });
    Ok(json!([
        {"type":"image","data":png,"mimeType":"image/png"},
        {"type":"text","text":about.to_string()},
    ]))
}

fn call(socket: &Path, folder: &Path, name: &str, args: &Value) -> Result<Value> {
    if name == "screenshot" {
        return screenshot(socket, folder, args);
    }
    let mut result = Value::Null;
    for (step, pause) in plan(name, args)? {
        result = send(socket, &step)?;
        std::thread::sleep(pause);
    }
    let text = match result {
        Value::Null => "Done.".to_owned(),
        result => result.to_string(),
    };
    Ok(json!([{"type":"text","text":text}]))
}

/// Serves the driver's tools on standard input and output.
pub fn serve_stdio(socket: &Path, folder: &Path) -> Result<()> {
    let tools = tools();
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    while let Some(request) = crate::mcp::read_json(&mut input)? {
        let Some(id) = request.get("id") else {
            continue;
        };
        let result = match request["method"].as_str().unwrap_or_default() {
            "initialize" => Ok(
                json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"wiffletree-app-driver","version":"0.1.0"}}),
            ),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools":tools})),
            "tools/call" => {
                let params = &request["params"];
                let name = params["name"].as_str().unwrap_or_default();
                Ok(match call(socket, folder, name, &params["arguments"]) {
                    Ok(content) => json!({"isError":false,"content":content}),
                    Err(error) => {
                        json!({"isError":true,"content":[{"type":"text","text":format!("{error:#}")}]})
                    }
                })
            }
            _ => Err("Method not found"),
        };
        let response = match result {
            Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
            Err(message) => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":message}})
            }
        };
        writeln!(output, "{response}")?;
        output.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_is_listed_with_a_closed_schema() {
        let tools = tools();
        let names: Vec<&str> = (tools.as_array().unwrap().iter())
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, TOOLS);
        for tool in tools.as_array().unwrap() {
            assert_eq!(tool["inputSchema"]["additionalProperties"], false, "{tool}");
        }
    }

    #[test]
    fn each_tool_maps_to_its_socket_steps() {
        let steps = |name: &str, args: Value| -> Vec<Value> {
            plan(name, &args)
                .unwrap()
                .into_iter()
                .map(|(step, _)| step)
                .collect()
        };
        let at = json!({"x": 10, "y": 20});
        assert_eq!(
            steps("click", at.clone()),
            [json!({"do":"click","x":10.0,"y":20.0,"count":1})]
        );
        assert_eq!(
            steps("double_click", at.clone()),
            [json!({"do":"click","x":10.0,"y":20.0,"count":2})]
        );
        assert_eq!(
            steps("right_click", at.clone()),
            [
                json!({"do":"press","x":10.0,"y":20.0,"button":"right"}),
                json!({"do":"release","x":10.0,"y":20.0,"button":"right"})
            ]
        );
        assert_eq!(steps("move", at), [json!({"do":"move","x":10.0,"y":20.0})]);
        assert_eq!(
            steps("type_text", json!({"text":"Ship it"})),
            [json!({"do":"type","text":"Ship it"})]
        );
        assert_eq!(
            steps("key", json!({"keystroke":"cmd-n"})),
            [json!({"do":"key","keystroke":"cmd-n"})]
        );
        assert_eq!(
            steps("scroll", json!({"x":1,"y":2,"dy":-300})),
            [json!({"do":"scroll","x":1.0,"y":2.0,"dy":-300})]
        );
        let drag = steps("drag", json!({"x1":0,"y1":0,"x2":100,"y2":40}));
        assert_eq!(drag[0], json!({"do":"move","x":0.0,"y":0.0}));
        assert_eq!(drag[1], json!({"do":"press","x":0.0,"y":0.0}));
        assert_eq!(drag[5], json!({"do":"drag","x":100.0,"y":40.0}));
        assert_eq!(
            drag.last().unwrap(),
            &json!({"do":"release","x":100.0,"y":40.0})
        );
        assert_eq!(
            steps("resize_window", json!({"width":960,"height":600})),
            [
                json!({"do":"resize","width":960.0,"height":600.0}),
                json!({"do":"window"})
            ]
        );
        assert_eq!(steps("window", json!({})), [json!({"do":"window"})]);
        // The schemas' every tool but screenshot, which captures, is planned.
        for tool in TOOLS.iter().filter(|tool| **tool != "screenshot") {
            assert!(
                plan(tool, &json!({})).is_err() || *tool == "window",
                "{tool}"
            );
        }
        assert!(plan("drop", &json!({"x":1,"y":1})).is_err());
        assert!(plan("click", &json!({"x":"left","y":1})).is_err());
    }

    #[test]
    fn a_screenshot_never_follows_a_link_out_of_the_folder() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("run");
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("kept.png"), b"KEPT").unwrap();
        std::os::unix::fs::symlink(outside.join("kept.png"), folder.join("shot.png")).unwrap();
        std::os::unix::fs::symlink(&outside, folder.join("shots")).unwrap();
        for name in ["shot", "shots/new", "shots/deeper/new"] {
            let path = screenshot_path(&folder, name).unwrap();
            assert!(write_inside(&folder, &path, b"PNG").is_err(), "{name}");
        }
        assert_eq!(std::fs::read(outside.join("kept.png")).unwrap(), b"KEPT");
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
        let path = screenshot_path(&folder, "fresh/one").unwrap();
        write_inside(&folder, &path, b"PNG").unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"PNG");
    }

    #[test]
    fn a_screenshot_is_saved_only_inside_the_folder() {
        let folder = Path::new("/runs/r");
        assert_eq!(
            screenshot_path(folder, "screenshots/models-dark").unwrap(),
            Path::new("/runs/r/screenshots/models-dark.png")
        );
        assert_eq!(
            screenshot_path(folder, "first.png").unwrap(),
            Path::new("/runs/r/first.png")
        );
        for outside in ["../x.png", "/tmp/x.png", "a/../../x.png", ""] {
            assert!(screenshot_path(folder, outside).is_err(), "{outside}");
        }
    }

    /// A stand-in socket that records each step and answers as the app does.
    #[test]
    fn a_screenshot_saves_the_apps_image_and_returns_it_with_its_size_and_scale() {
        use std::os::unix::net::UnixListener;
        let folder = tempfile::tempdir().unwrap();
        let socket = folder.path().join("s.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let png = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"PNG");
        let answers = [
            json!({"ok":true,"result":{"width":960.0,"height":600.0,"scale":2.0}}),
            json!({"ok":true,"result":{"png":png,"width":1920,"height":1200}}),
        ];
        let server = std::thread::spawn(move || {
            let mut steps = vec![];
            for answer in answers {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                steps.push(serde_json::from_str::<Value>(&line).unwrap());
                writeln!(stream, "{answer}").unwrap();
            }
            steps
        });
        let content = call(
            &socket,
            folder.path(),
            "screenshot",
            &json!({"name":"shots/one","scale":"native"}),
        )
        .unwrap();
        assert_eq!(
            server.join().unwrap(),
            [
                json!({"do":"window"}),
                json!({"do":"capture","native":true})
            ]
        );
        assert_eq!(content[0]["type"], "image");
        assert_eq!(content[0]["mimeType"], "image/png");
        let about: Value = serde_json::from_str(content[1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(about["saved"], "shots/one.png");
        assert_eq!(
            (about["width"].clone(), about["height"].clone()),
            (json!(1920), json!(1200))
        );
        assert_eq!(about["scale"], 2.0);
        assert_eq!(
            std::fs::read(folder.path().join("shots/one.png")).unwrap(),
            b"PNG"
        );
    }
}
