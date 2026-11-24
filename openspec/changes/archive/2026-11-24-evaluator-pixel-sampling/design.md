## Context

Both drivers already save screenshots in the run folder with `write_inside`, which follows no link. The evaluator cannot measure those files itself: it has no shell and sees only a downsized image.

## Decisions

### One shared pure module, one more tool

`pixel_sampling` holds the tool's schema and its whole behaviour; each driver lists its schema and routes the call to it before its own handling, so the two cannot disagree. The module does not depend on the app or desktop socket, so a test reads a PNG written to a temporary folder.

### Read by the same rules, which for a read also refuse a link

The name resolves as a screenshot's does: relative, no `..`, root or prefix. The file must end in `.png`, and no part of its path inside the folder may be a link, nor the file be anything but a regular file; the open itself refuses a final link. A file that is not a PNG fails to decode and is refused by name. Decoding is the `png` crate's, with its default size limits, and normalizes 16-bit, palette and grey images to 8-bit samples.

### Colours are the file's sRGB samples

The app and the helper save sRGB images, so the sample is the pixel's value as stored; alpha is ignored and said so. Relative luminance uses the WCAG 2.x definition with the sRGB transfer function (threshold 0.04045). A pass is judged on the unrounded ratio, so 4.499 shows as 4.5 and does not pass.

### Whole-number pixels only

Points are whole, non-negative pixels, as the model reads them from a screenshot's reported size. A fraction is refused rather than rounded, because rounding would hide a coordinate mistake. For a native-scale screenshot the tool says to divide by the reported scale to get points.

## Risks

- A model may sample a window's anti-aliased edge and read an in-between colour. The description asks for points inside flat areas.
