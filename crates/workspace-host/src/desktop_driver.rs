//! The desktop driver (OpenSpec change `ui-evaluators`): the MCP server a full-desktop run's
//! provider starts, `wiffletree-host desktop-driver-mcp <socket> <run folder>`. Each tool is
//! one request to the run's Wiffletree Evaluator helper, which holds the screen grants and
//! checks every request against the apps the run may use. Coordinates are screen points from
//! the top-left corner of the main display.
use crate::app_driver::{ask, screenshot_path, serve_tools, write_inside};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::path::Path;

/// Who answers on the socket.
const HELPER: &str = "Wiffletree Evaluator";

fn send(socket: &Path, request: &Value) -> Result<Value> {
    ask(socket, request, HELPER)
}

/// The server's name, so its tools reach Claude as `mcp__desktop__<tool>`.
pub const SERVER: &str = "desktop";
pub const TOOLS: [&str; 12] = [
    "screenshot",
    "click",
    "double_click",
    "right_click",
    "move",
    "type_text",
    "key",
    "scroll",
    "drag",
    "open_app",
    "list_windows",
    "sample_pixels",
];

pub fn tools() -> Value {
    let number = json!({"type":"number"});
    let point = json!({"type":"object","properties":{"x":{"type":"number","description":"Screen points from the main display's left edge"},"y":{"type":"number","description":"Screen points from its top edge"}},"required":["x","y"],"additionalProperties":false});
    let tool = |name: &str, description: &str, schema: Value| json!({"name":name,"description":description,"inputSchema":schema});
    json!([
        tool(
            "screenshot",
            "Capture the main display, showing only the windows of the apps this run may use; everything else is left out. Saves it in your folder as name (a .png path relative to the folder, such as screenshots/first.png) and returns the image with its size in pixels, its scale (pixels per screen point) and the display's frame in screen points. scale points (the default) gives one pixel per point, so a position in the image is the point to click; native gives the display's full resolution, for fine detail, and is downsized before you see it when over 2000 pixels on its long edge, so sample pixels from a points shot.",
            json!({"type":"object","properties":{"name":{"type":"string"},"scale":{"type":"string","enum":["points","native"]}},"required":["name"],"additionalProperties":false})
        ),
        tool(
            "click",
            "Click the left button once at a screen point. Refused unless the point is over a window of an app this run may use.",
            point.clone()
        ),
        tool(
            "double_click",
            "Double-click the left button at a screen point over one of this run's windows.",
            point.clone()
        ),
        tool(
            "right_click",
            "Click the right button at a screen point over one of this run's windows, as for a context menu.",
            point.clone()
        ),
        tool(
            "move",
            "Move the pointer to a screen point over one of this run's windows and leave it there, as to hover.",
            point
        ),
        tool(
            "type_text",
            "Type text into the frontmost app. Refused unless an app this run may use is frontmost.",
            json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"],"additionalProperties":false})
        ),
        tool(
            "key",
            "Press one key or shortcut in the frontmost app, written like enter, escape, tab, shift-tab, up, cmd-n or cmd-,. Refused unless an app this run may use is frontmost.",
            json!({"type":"object","properties":{"keystroke":{"type":"string"}},"required":["keystroke"],"additionalProperties":false})
        ),
        tool(
            "scroll",
            "Scroll at a screen point over one of this run's windows; dy is in points, negative to move the content up and show what is below.",
            json!({"type":"object","properties":{"x":number,"y":number,"dy":{"type":"integer"}},"required":["x","y","dy"],"additionalProperties":false})
        ),
        tool(
            "drag",
            "Press the left button at (x1, y1), move to (x2, y2) in a few steps and release there. Both points must be over this run's windows.",
            json!({"type":"object","properties":{"x1":number,"y1":number,"x2":number,"y2":number},"required":["x1","y1","x2","y2"],"additionalProperties":false})
        ),
        tool(
            "open_app",
            "Open, or bring to the front, one of the apps this run may use, by its bundle id; list_windows names them.",
            json!({"type":"object","properties":{"bundle_id":{"type":"string"}},"required":["bundle_id"],"additionalProperties":false})
        ),
        tool(
            "list_windows",
            "The apps this run may use, and their on-screen windows front to back: each window's app, title, frame in screen points and whether its app is frontmost.",
            json!({"type":"object","properties":{},"additionalProperties":false})
        ),
        crate::pixel_sampling::tool(),
    ])
}

/// The helper request a tool other than `screenshot` makes.
pub(crate) fn request(name: &str, args: &Value) -> Result<Value> {
    let number = |key: &str| -> Result<f64> {
        let value = args[key]
            .as_f64()
            .with_context(|| format!("Give {key} as a number"))?;
        ensure!(value.is_finite(), "Give {key} as a finite number");
        Ok(value)
    };
    let text =
        |key: &str| -> Result<&str> { args[key].as_str().with_context(|| format!("Give {key}")) };
    let click = |button: &str, count: u8| -> Result<Value> {
        Ok(json!({"do":"click","x":number("x")?,"y":number("y")?,"button":button,"count":count}))
    };
    Ok(match name {
        "click" => click("left", 1)?,
        "double_click" => click("left", 2)?,
        "right_click" => click("right", 1)?,
        "move" => json!({"do":"move","x":number("x")?,"y":number("y")?}),
        "type_text" => json!({"do":"type","text":text("text")?}),
        "key" => json!({"do":"key","keystroke":text("keystroke")?}),
        "scroll" => {
            let dy = args["dy"].as_i64().context("Give dy as a whole number")?;
            let dy = i32::try_from(dy).context("dy is too large")?;
            json!({"do":"scroll","x":number("x")?,"y":number("y")?,"dy":dy})
        }
        "drag" => json!({
            "do":"drag",
            "x1":number("x1")?,
            "y1":number("y1")?,
            "x2":number("x2")?,
            "y2":number("y2")?,
        }),
        "open_app" => json!({"do":"open_app","bundle_id":text("bundle_id")?}),
        "list_windows" => json!({"do":"windows"}),
        other => bail!("No tool {other}"),
    })
}

fn screenshot(socket: &Path, folder: &Path, args: &Value) -> Result<Value> {
    let name = args["name"].as_str().context("Give name")?;
    let path = screenshot_path(folder, name)?;
    let native = match args["scale"].as_str().unwrap_or("points") {
        "points" => false,
        "native" => true,
        other => bail!("Give scale as points or native, not {other}"),
    };
    let image = send(socket, &json!({"do":"screenshot","native":native}))?;
    let png = image["png"].as_str().context("The helper sent no image")?;
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, png)?;
    write_inside(folder, &path, &bytes).with_context(|| format!("Could not save {name}"))?;
    let saved = path.strip_prefix(folder).unwrap_or(&path);
    let about = json!({
        "saved": saved,
        "width": image["width"],
        "height": image["height"],
        "scale": image["scale"],
        "display_points": image["display"],
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
    if name == "sample_pixels" {
        return crate::pixel_sampling::sample(folder, args);
    }
    let text = match send(socket, &request(name, args)?)? {
        Value::Null => "Done.".to_owned(),
        result => result.to_string(),
    };
    Ok(json!([{"type":"text","text":text}]))
}

/// Serves the driver's tools on standard input and output.
pub fn serve_stdio(socket: &Path, folder: &Path) -> Result<()> {
    serve_tools("wiffletree-desktop-driver", tools(), |name, args| {
        call(socket, folder, name, args)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};

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
    fn each_tool_maps_to_one_helper_request() {
        let at = json!({"x": 10, "y": 20.5});
        let cases = [
            (
                "click",
                at.clone(),
                json!({"do":"click","x":10.0,"y":20.5,"button":"left","count":1}),
            ),
            (
                "double_click",
                at.clone(),
                json!({"do":"click","x":10.0,"y":20.5,"button":"left","count":2}),
            ),
            (
                "right_click",
                at.clone(),
                json!({"do":"click","x":10.0,"y":20.5,"button":"right","count":1}),
            ),
            ("move", at, json!({"do":"move","x":10.0,"y":20.5})),
            (
                "type_text",
                json!({"text":"12+7"}),
                json!({"do":"type","text":"12+7"}),
            ),
            (
                "key",
                json!({"keystroke":"cmd-n"}),
                json!({"do":"key","keystroke":"cmd-n"}),
            ),
            (
                "scroll",
                json!({"x":1,"y":2,"dy":-300}),
                json!({"do":"scroll","x":1.0,"y":2.0,"dy":-300}),
            ),
            (
                "drag",
                json!({"x1":0,"y1":0,"x2":100,"y2":40}),
                json!({"do":"drag","x1":0.0,"y1":0.0,"x2":100.0,"y2":40.0}),
            ),
            (
                "open_app",
                json!({"bundle_id":"com.apple.calculator"}),
                json!({"do":"open_app","bundle_id":"com.apple.calculator"}),
            ),
            ("list_windows", json!({}), json!({"do":"windows"})),
        ];
        for (name, args, expected) in cases {
            assert_eq!(request(name, &args).unwrap(), expected, "{name}");
        }
        // Every tool but screenshot, which captures, and sample_pixels, which reads a file,
        // makes a request.
        for tool in (TOOLS.iter()).filter(|tool| !matches!(**tool, "screenshot" | "sample_pixels"))
        {
            assert!(
                request(tool, &json!({})).is_err() || *tool == "list_windows",
                "{tool}"
            );
        }
        assert!(request("resize_window", &json!({"width":1,"height":1})).is_err());
        assert!(request("click", &json!({"x":"left","y":1})).is_err());
    }

    /// A stand-in helper that answers one screenshot request.
    #[test]
    fn a_screenshot_is_saved_in_the_folder_and_returned_with_its_scale_and_display() {
        use std::os::unix::net::UnixListener;
        let folder = tempfile::tempdir().unwrap();
        let socket = folder.path().join("h.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let png = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"PNG");
        let answer = json!({"ok":true,"result":{"png":png,"width":3024,"height":1964,"scale":2.0,"display":{"x":0.0,"y":0.0,"width":1512.0,"height":982.0}}});
        let helper = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            writeln!(stream, "{answer}").unwrap();
            serde_json::from_str::<Value>(&line).unwrap()
        });
        let content = call(
            &socket,
            folder.path(),
            "screenshot",
            &json!({"name":"shots/calc","scale":"native"}),
        )
        .unwrap();
        assert_eq!(
            helper.join().unwrap(),
            json!({"do":"screenshot","native":true})
        );
        assert_eq!(content[0]["type"], "image");
        let about: Value = serde_json::from_str(content[1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(about["saved"], "shots/calc.png");
        assert_eq!(about["scale"], 2.0);
        assert_eq!(about["display_points"]["width"], 1512.0);
        assert_eq!(
            std::fs::read(folder.path().join("shots/calc.png")).unwrap(),
            b"PNG"
        );
        // Out of the folder, nothing is asked of the helper.
        for outside in ["../x", "/tmp/x"] {
            assert!(
                call(
                    &socket,
                    folder.path(),
                    "screenshot",
                    &json!({"name":outside})
                )
                .is_err()
            );
        }
    }

    #[test]
    fn sample_pixels_reads_a_saved_screenshot_without_asking_the_helper() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(
            folder.path().join("shot.png"),
            crate::pixel_sampling::tests::one_white_pixel(),
        )
        .unwrap();
        let args = json!({"screenshot":"shot.png","points":[{"x":0,"y":0}]});
        // No helper is listening on this socket, so the call reads only the file.
        let content = call(
            Path::new("/nowhere.sock"),
            folder.path(),
            "sample_pixels",
            &args,
        )
        .unwrap();
        assert!(content[0]["text"].as_str().unwrap().contains("#FFFFFF"));
    }

    #[test]
    fn a_refusal_from_the_helper_is_the_tools_error() {
        use std::os::unix::net::UnixListener;
        let folder = tempfile::tempdir().unwrap();
        let socket = folder.path().join("h.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let refused = json!({"ok":false,"error":"Refused: (500, 5) is over a window of an app this run may not use."});
            writeln!(stream, "{refused}").unwrap();
        });
        let error = call(&socket, folder.path(), "click", &json!({"x":500,"y":5})).unwrap_err();
        assert!(error.to_string().starts_with("Refused"), "{error}");
    }
}
