//! The `sample_pixels` tool both evaluator drivers offer (OpenSpec change
//! `evaluator-pixel-sampling`): exact colours and WCAG 2.x contrast read from a PNG the
//! screenshot tool saved in the run folder, so an evaluator measures rather than estimates.
//! It reads that one file and writes and sends nothing.
use crate::app_driver::path_inside;
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    io::BufReader,
    path::{Path, PathBuf},
};

/// How many points, and how many pairs, one call takes.
const MAX_LISTED: usize = 64;
/// WCAG 2.x AA: normal text, then large text and user-interface components.
const AA_NORMAL_TEXT: f64 = 4.5;
const AA_LARGE_TEXT_AND_UI: f64 = 3.0;

pub(crate) fn tool() -> Value {
    json!({
        "name": "sample_pixels",
        "description": "Read exact colours from a screenshot you saved in your folder, and measure WCAG 2.x contrast between them. Coordinates are pixels of the saved image, from its top-left corner, as the screenshot's reported width and height count them; for a native-scale screenshot, divide by the scale it reported to get window or screen points, such as to click. Returns each point's sRGB hex and WCAG relative luminance, and for each pair of points its contrast ratio, rounded to two decimals, and whether it passes AA for normal text (4.5) and for large text and UI components (3.0), judged on the unrounded ratio. Also returns the image's pixel size, to check your coordinates against. Sample inside flat areas, not on anti-aliased edges, where a pixel is a blend. Alpha is ignored. Read only.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "screenshot": {"type": "string", "description": "The name the screenshot was saved under, a .png path relative to your folder, such as screenshots/models-dark.png"},
                "points": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_LISTED,
                    "items": {
                        "type": "object",
                        "properties": {
                            "x": {"type": "integer", "description": "Pixels from the image's left edge"},
                            "y": {"type": "integer", "description": "Pixels from the image's top edge"}
                        },
                        "required": ["x", "y"],
                        "additionalProperties": false
                    }
                },
                "pairs": {
                    "type": "array",
                    "maxItems": MAX_LISTED,
                    "description": "Each is [i, j], indexes into points, to measure the contrast between those two points' colours",
                    "items": {"type": "array", "minItems": 2, "maxItems": 2, "items": {"type": "integer", "minimum": 0}}
                }
            },
            "required": ["screenshot", "points"],
            "additionalProperties": false
        }
    })
}

/// Runs the tool on `args` for the run in `folder`, returning its MCP content.
pub(crate) fn sample(folder: &Path, args: &Value) -> Result<Value> {
    let name = args["screenshot"].as_str().context("Give screenshot")?;
    let points = coordinates(&args["points"])?;
    let pairs = index_pairs(&args["pairs"], points.len())?;
    let image = Image::read(folder, name)?;
    let colours = (points.iter())
        .enumerate()
        .map(|(index, &(x, y))| {
            image.colour_at(x, y).with_context(|| {
                format!(
                    "Point {index} ({x}, {y}) is outside the image, which is {} × {} pixels: x runs 0 to {} and y runs 0 to {}",
                    image.width,
                    image.height,
                    image.width - 1,
                    image.height - 1
                )
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let result = json!({
        "screenshot": name.trim(),
        "width": image.width,
        "height": image.height,
        "points": points.iter().zip(&colours).map(|(&(x, y), colour)| json!({
            "x": x,
            "y": y,
            "hex": colour.hex(),
            "luminance": round(colour.luminance(), 4),
        })).collect::<Vec<_>>(),
        "pairs": pairs.iter().map(|&(a, b)| {
            let ratio = contrast(colours[a], colours[b]);
            json!({
                "points": [a, b],
                "contrast": round(ratio, 2),
                "aa_normal_text": ratio >= AA_NORMAL_TEXT,
                "aa_large_text_and_ui": ratio >= AA_LARGE_TEXT_AND_UI,
            })
        }).collect::<Vec<_>>(),
    });
    Ok(json!([{"type": "text", "text": result.to_string()}]))
}

fn round(value: f64, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    (value * scale).round() / scale
}

fn coordinates(points: &Value) -> Result<Vec<(u32, u32)>> {
    let points = points.as_array().context("Give points as a list")?;
    ensure!(!points.is_empty(), "Give at least one point");
    ensure!(
        points.len() <= MAX_LISTED,
        "Give at most {MAX_LISTED} points, not {}",
        points.len()
    );
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let whole = |key: &str| -> Result<u32> {
                let value = point[key].as_f64().filter(|value| value.is_finite());
                match value {
                    Some(value)
                        if value >= 0. && value.fract() == 0. && value < f64::from(u32::MAX) =>
                    {
                        Ok(value as u32)
                    }
                    _ => bail!("Give point {index}'s {key} as a whole number of pixels, 0 or more"),
                }
            };
            Ok((whole("x")?, whole("y")?))
        })
        .collect()
}

fn index_pairs(pairs: &Value, points: usize) -> Result<Vec<(usize, usize)>> {
    if pairs.is_null() {
        return Ok(vec![]);
    }
    let pairs = pairs
        .as_array()
        .context("Give pairs as a list of [i, j] pairs")?;
    ensure!(
        pairs.len() <= MAX_LISTED,
        "Give at most {MAX_LISTED} pairs, not {}",
        pairs.len()
    );
    pairs
        .iter()
        .enumerate()
        .map(|(index, pair)| {
            let ends = pair.as_array().map(Vec::as_slice).unwrap_or_default();
            let [a, b] = ends else {
                bail!("Give pair {index} as two point indexes, [i, j]");
            };
            let point = |end: &Value| match end.as_u64() {
                Some(point) if (point as usize) < points => Ok(point as usize),
                _ => bail!("Pair {index} names a point that is not in points, which has {points}"),
            };
            Ok((point(a)?, point(b)?))
        })
        .collect()
}

#[derive(Clone, Copy)]
struct Colour([u8; 3]);

impl Colour {
    fn hex(self) -> String {
        let [red, green, blue] = self.0;
        format!("#{red:02X}{green:02X}{blue:02X}")
    }

    /// WCAG 2.x relative luminance of an sRGB colour, from 0 for black to 1 for white.
    fn luminance(self) -> f64 {
        let linear = |channel: u8| {
            let value = f64::from(channel) / 255.;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        let [red, green, blue] = self.0;
        0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)
    }
}

/// WCAG 2.x contrast ratio of two colours, from 1 to 21, whichever is lighter.
fn contrast(a: Colour, b: Colour) -> f64 {
    let (a, b) = (a.luminance(), b.luminance());
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// A decoded PNG: 8-bit samples, `channels` to a pixel, row by row.
struct Image {
    width: u32,
    height: u32,
    channels: usize,
    samples: Vec<u8>,
}

impl Image {
    fn read(folder: &Path, name: &str) -> Result<Self> {
        let path = path_inside(folder, name)?;
        ensure!(
            path.extension().is_some_and(|extension| extension == "png"),
            "{name} is not a .png file; name a screenshot you saved"
        );
        let file = open_inside(folder, &path).with_context(|| format!("Could not read {name}"))?;
        let mut decoder = png::Decoder::new(BufReader::new(file));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder
            .read_info()
            .with_context(|| format!("{name} is not a PNG image"))?;
        let size = reader
            .output_buffer_size()
            .context("The image is too large")?;
        let mut samples = vec![0; size];
        let frame = reader
            .next_frame(&mut samples)
            .with_context(|| format!("{name} is not a readable PNG image"))?;
        samples.truncate(frame.buffer_size());
        let channels = match frame.color_type {
            png::ColorType::Grayscale => 1,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            png::ColorType::Indexed => bail!("{name} has an unexpanded palette"),
        };
        Ok(Self {
            width: frame.width,
            height: frame.height,
            channels,
            samples,
        })
    }

    fn colour_at(&self, x: u32, y: u32) -> Option<Colour> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let start = (y as usize * self.width as usize + x as usize) * self.channels;
        let pixel = self.samples.get(start..start + self.channels)?;
        Some(Colour(match *pixel {
            [grey] | [grey, _] => [grey; 3],
            [red, green, blue] | [red, green, blue, _] => [red, green, blue],
            _ => return None,
        }))
    }
}

/// Opens the regular file `path` in `folder` for reading, through no link: one in the way
/// could lead the read out of the folder, and this server reads outside the evaluator's
/// sandbox.
fn open_inside(folder: &Path, path: &Path) -> Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut at = PathBuf::from(folder);
    for part in path.strip_prefix(folder)?.components() {
        at.push(part);
        ensure!(
            !std::fs::symlink_metadata(&at)?.file_type().is_symlink(),
            "it is a link"
        );
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        // Not blocking, so a pipe with no writer is opened and refused, not waited on.
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    ensure!(file.metadata()?.is_file(), "it is not a file");
    Ok(file)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn rgb(hex: &str) -> Colour {
        let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap();
        Colour([channel(1), channel(3), channel(5)])
    }

    /// A PNG of `pixels`, row by row, `width` to a row.
    fn png_bytes(width: u32, pixels: &[[u8; 3]]) -> Vec<u8> {
        let mut bytes = vec![];
        let height = pixels.len() as u32 / width;
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&pixels.concat()).unwrap();
        writer.finish().unwrap();
        bytes
    }

    /// A folder holding `shots/one.png`: black, white, #1A262D and #D1D1CD, two to a row.
    fn run_folder() -> tempfile::TempDir {
        let folder = tempfile::tempdir().unwrap();
        std::fs::create_dir(folder.path().join("shots")).unwrap();
        let pixels = [
            [0, 0, 0],
            [255, 255, 255],
            [0x1A, 0x26, 0x2D],
            [0xD1, 0xD1, 0xCD],
        ];
        std::fs::write(folder.path().join("shots/one.png"), png_bytes(2, &pixels)).unwrap();
        folder
    }

    /// A one-pixel white PNG, for a driver's test.
    pub(crate) fn one_white_pixel() -> Vec<u8> {
        png_bytes(1, &[[255; 3]])
    }

    fn measured(folder: &Path, args: Value) -> Value {
        let content = sample(folder, &args).unwrap();
        serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
    }

    #[test]
    fn known_colours_read_back_as_hex_and_luminance() {
        let folder = run_folder();
        let result = measured(
            folder.path(),
            json!({"screenshot":"shots/one.png","points":[{"x":0,"y":0},{"x":1,"y":0},{"x":0,"y":1},{"x":1,"y":1}]}),
        );
        assert_eq!(
            (result["width"].clone(), result["height"].clone()),
            (json!(2), json!(2))
        );
        let points = result["points"].as_array().unwrap();
        let read: Vec<(&str, f64)> = (points.iter())
            .map(|point| {
                (
                    point["hex"].as_str().unwrap(),
                    point["luminance"].as_f64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            read,
            [
                ("#000000", 0.0),
                ("#FFFFFF", 1.0),
                ("#1A262D", 0.018),
                ("#D1D1CD", 0.6356),
            ]
        );
        assert_eq!(result["pairs"], json!([]));
    }

    #[test]
    fn contrast_matches_the_wcag_reference_values() {
        let (black, white, grey) = (rgb("#000000"), rgb("#FFFFFF"), rgb("#777777"));
        assert_eq!(round(contrast(black, white), 2), 21.0);
        assert_eq!(round(contrast(white, black), 2), 21.0);
        assert_eq!(round(contrast(white, white), 2), 1.0);
        assert_eq!(round(contrast(grey, white), 2), 4.48);
        // The Tidal text colour on its background, a pairing the rubric measures.
        assert_eq!(round(contrast(rgb("#D1D1CD"), rgb("#1A262D")), 2), 10.09);
    }

    #[test]
    fn a_pair_reports_its_ratio_and_whether_it_passes_aa() {
        let folder = tempfile::tempdir().unwrap();
        let pixels = [[0x77; 3], [255; 3], [0x76; 3], [0; 3]];
        std::fs::write(folder.path().join("g.png"), png_bytes(2, &pixels)).unwrap();
        let result = measured(
            folder.path(),
            json!({"screenshot":"g.png","points":[{"x":0,"y":0},{"x":1,"y":0},{"x":0,"y":1},{"x":1,"y":1}],"pairs":[[0,1],[2,1],[3,1]]}),
        );
        let pairs = result["pairs"].as_array().unwrap();
        // #777777 on white is 4.48, so it fails normal text but passes large text and UI;
        // #767676 is the lightest grey that passes both.
        assert_eq!(
            pairs[0],
            json!({"points":[0,1],"contrast":4.48,"aa_normal_text":false,"aa_large_text_and_ui":true})
        );
        assert_eq!(
            pairs[1],
            json!({"points":[2,1],"contrast":4.54,"aa_normal_text":true,"aa_large_text_and_ui":true})
        );
        assert_eq!(
            pairs[2],
            json!({"points":[3,1],"contrast":21.0,"aa_normal_text":true,"aa_large_text_and_ui":true})
        );
    }

    #[test]
    fn grey_and_sixteen_bit_images_read_as_8_bit_colour() {
        let folder = tempfile::tempdir().unwrap();
        let mut bytes = vec![];
        let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Sixteen);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[0x80, 0x00]).unwrap();
        writer.finish().unwrap();
        std::fs::write(folder.path().join("g.png"), bytes).unwrap();
        let result = measured(
            folder.path(),
            json!({"screenshot":"g.png","points":[{"x":0,"y":0}]}),
        );
        assert_eq!(result["points"][0]["hex"], "#808080");
    }

    fn refusal(folder: &Path, args: Value) -> String {
        format!("{:#}", sample(folder, &args).unwrap_err())
    }

    #[test]
    fn a_name_outside_the_folder_or_a_link_or_a_non_png_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("run");
        let outside = root.path().join("outside");
        std::fs::create_dir_all(folder.join("shots")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let png = png_bytes(1, &[[1, 2, 3]]);
        std::fs::write(outside.join("kept.png"), &png).unwrap();
        std::fs::write(folder.join("shots/real.png"), &png).unwrap();
        std::fs::write(folder.join("notes.txt"), &png).unwrap();
        std::fs::write(folder.join("fake.png"), b"not a png").unwrap();
        std::os::unix::fs::symlink(outside.join("kept.png"), folder.join("link.png")).unwrap();
        std::os::unix::fs::symlink(&outside, folder.join("linked")).unwrap();
        let at = |screenshot: &str| json!({"screenshot":screenshot,"points":[{"x":0,"y":0}]});
        for name in [
            "../outside/kept.png",
            "/etc/hosts.png",
            "shots/../../outside/kept.png",
            "",
        ] {
            assert!(
                refusal(&folder, at(name)).contains("path inside your folder"),
                "{name}"
            );
        }
        for name in ["link.png", "linked/kept.png"] {
            assert!(
                refusal(&folder, at(name)).contains("it is a link"),
                "{name}"
            );
        }
        assert!(refusal(&folder, at("notes.txt")).contains("not a .png file"));
        assert!(refusal(&folder, at("shots/real")).contains("not a .png file"));
        assert!(refusal(&folder, at("fake.png")).contains("not a PNG image"));
        assert!(refusal(&folder, at("shots")).contains("not a .png file"));
        // A pipe with no writer is refused at once, not waited on.
        let pipe = std::ffi::CString::new(folder.join("stalled.png").to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
        assert!(refusal(&folder, at("stalled.png")).contains("it is not a file"));
        assert!(refusal(&folder, at("missing.png")).contains("Could not read"));
        measured(&folder, at("shots/real.png"));
    }

    #[test]
    fn a_point_outside_the_image_is_refused_naming_it_and_the_size() {
        let folder = run_folder();
        let message = refusal(
            folder.path(),
            json!({"screenshot":"shots/one.png","points":[{"x":0,"y":0},{"x":2,"y":1}]}),
        );
        assert!(
            message.contains("Point 1 (2, 1) is outside the image, which is 2 × 2 pixels"),
            "{message}"
        );
        assert!(
            refusal(
                folder.path(),
                json!({"screenshot":"shots/one.png","points":[{"x":0,"y":2}]})
            )
            .contains("outside")
        );
        for bad in [
            json!({"x":-1,"y":0}),
            json!({"x":0.5,"y":0}),
            json!({"x":"0","y":0}),
            json!({"y":0}),
        ] {
            let message = refusal(
                folder.path(),
                json!({"screenshot":"shots/one.png","points":[bad]}),
            );
            assert!(message.contains("whole number of pixels"), "{message}");
        }
    }

    #[test]
    fn too_many_or_empty_points_and_pairs_are_refused() {
        let folder = run_folder();
        let point = json!({"x":0,"y":0});
        let call = |points: usize, pairs: Value| {
            refusal(
                folder.path(),
                json!({"screenshot":"shots/one.png","points":vec![point.clone(); points],"pairs":pairs}),
            )
        };
        assert!(call(65, json!([])).contains("at most 64 points, not 65"));
        assert!(call(0, json!([])).contains("at least one point"));
        assert!(call(2, json!(vec![json!([0, 1]); 65])).contains("at most 64 pairs, not 65"));
        assert!(call(2, json!([[0, 2]])).contains("not in points, which has 2"));
        assert!(call(2, json!([[0, -1]])).contains("not in points"));
        assert!(call(2, json!([[0]])).contains("two point indexes"));
        // The most each takes is accepted.
        let content = sample(
            folder.path(),
            &json!({"screenshot":"shots/one.png","points":vec![point; 64],"pairs":vec![json!([0, 63]); 64]}),
        );
        assert!(content.is_ok());
    }

    #[test]
    fn the_schema_is_closed_and_bounds_its_lists() {
        let tool = tool();
        let schema = &tool["inputSchema"];
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["screenshot", "points"]));
        assert_eq!(schema["properties"]["points"]["maxItems"], 64);
        assert_eq!(schema["properties"]["pairs"]["maxItems"], 64);
        assert_eq!(
            schema["properties"]["points"]["items"]["additionalProperties"],
            false
        );
    }
}
