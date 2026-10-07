use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

pub struct Assets;

const BRAND: &[(&str, &[u8])] = &[
    (
        "brand/wordmark-light.svg",
        include_bytes!("../../../design/brand/logo/wordmark-two-tone-light.svg"),
    ),
    (
        "brand/wordmark-dark.svg",
        include_bytes!("../../../design/brand/logo/wordmark-two-tone-dark.svg"),
    ),
    (
        "brand/symbol-light.svg",
        include_bytes!("../../../design/brand/logo/symbol-light.svg"),
    ),
    (
        "brand/symbol-dark.svg",
        include_bytes!("../../../design/brand/logo/symbol-dark.svg"),
    ),
];

// Small, original line drawings. GPUI caches each rasterized SVG at its display size.
const DRAWINGS: &[(&str, &str)] = &[
    ("ready", r#"<circle cx="12" cy="12" r="6"/>"#),
    (
        "working",
        r#"<circle cx="12" cy="12" r="8"/><path d="M12 7v5l3 2"/>"#,
    ),
    (
        "blocked",
        r#"<circle cx="12" cy="12" r="8"/><path d="M8 12h8"/>"#,
    ),
    (
        "disconnected",
        r#"<circle cx="12" cy="12" r="8" stroke-dasharray="2 4"/>"#,
    ),
    (
        "attention",
        r#"<circle cx="12" cy="12" r="8"/><path d="M10 9a2 2 0 1 1 3 2c-1 1-1 1-1 2m0 3v.1"/>"#,
    ),
    (
        "overview",
        r#"<rect x="4" y="4" width="16" height="16" rx="4"/><path d="M4 10h16M10 10v10"/>"#,
    ),
    (
        "project",
        r#"<rect x="3" y="3" width="7" height="7" rx="2"/><rect x="14" y="3" width="7" height="7" rx="2"/><rect x="3" y="14" width="7" height="7" rx="2"/><rect x="14" y="14" width="7" height="7" rx="2"/>"#,
    ),
    (
        "task",
        r#"<rect x="5" y="4" width="14" height="16" rx="4"/><path d="M9 10h6M9 14h4"/>"#,
    ),
    (
        "team",
        r#"<circle cx="9" cy="8" r="3"/><path d="M3 20v-2a6 6 0 0 1 12 0v2M16 5a3 3 0 0 1 0 6m2 3a5 5 0 0 1 3 4v2"/>"#,
    ),
    (
        "agent",
        r#"<rect x="4" y="7" width="16" height="13" rx="4"/><path d="M12 3v4M8 12v2m8-2v2m-7 3h6"/>"#,
    ),
    (
        "memory",
        r#"<path d="M12 5C8 3 5 3 3 4v15c3-1 6-1 9 1 3-2 6-2 9-1V4c-2-1-5-1-9 1v15"/>"#,
    ),
    ("activity", r#"<path d="M3 12h4l3-7 4 14 3-7h4"/>"#),
    (
        "git",
        r#"<circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="5" r="2"/><path d="M6 7v10m12-10a9 9 0 0 1-9 9H6"/>"#,
    ),
    (
        "inbox",
        r#"<path d="m3 14 3-9h12l3 9v5H3v-5Zm0 0h5l2 3h4l2-3h5"/>"#,
    ),
    (
        "settings",
        r#"<path d="M4 6h16M4 12h16M4 18h16"/><circle cx="9" cy="6" r="2" fill="white"/><circle cx="15" cy="12" r="2" fill="white"/><circle cx="9" cy="18" r="2" fill="white"/>"#,
    ),
    ("plus", r#"<path d="M12 5v14M5 12h14"/>"#),
    (
        "rename",
        r#"<path d="m5 15 10-10 4 4-10 10-5 1 1-5Zm8-8 4 4"/>"#,
    ),
    ("close", r#"<path d="m6 6 12 12M6 18 18 6"/>"#),
    ("chevron-down", r#"<path d="m6 9 6 6 6-6"/>"#),
    ("chevron-right", r#"<path d="m9 6 6 6-6 6"/>"#),
    ("chevron-left", r#"<path d="m15 6-6 6 6 6"/>"#),
    ("chevron-up", r#"<path d="m6 15 6-6 6 6"/>"#),
    (
        "chevrons-up-down",
        r#"<path d="m8 8 4-4 4 4m-8 8 4 4 4-4"/>"#,
    ),
    ("check", r#"<path d="m5 12 4 4L19 6"/>"#),
    ("arrow-up", r#"<path d="M12 19V5m-6 6 6-6 6 6"/>"#),
    ("pause", r#"<path d="M8 5v14M16 5v14"/>"#),
    ("play", r#"<path d="m8 4 12 8-12 8Z"/>"#),
    (
        "moon",
        r#"<path d="M20 14A9 9 0 0 1 10 3a9 9 0 1 0 10 11Z"/>"#,
    ),
    (
        "sun",
        r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1 1m12 12 1 1M5 19l1-1M18 6l1-1"/>"#,
    ),
    ("refresh", r#"<path d="M20 10a8 8 0 1 0-1 7M20 4v6h-6"/>"#),
    ("folder", r#"<path d="M3 7V5h7l2 3h9v11H3V7Z"/>"#),
    (
        "ellipsis",
        r#"<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>"#,
    ),
    (
        "search",
        r#"<circle cx="10" cy="10" r="6"/><path d="m15 15 6 6"/>"#,
    ),
    (
        "info",
        r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v6m0-10v.1"/>"#,
    ),
    (
        "circle-check",
        r#"<circle cx="12" cy="12" r="9"/><path d="m8 12 3 3 5-6"/>"#,
    ),
    (
        "circle-x",
        r#"<circle cx="12" cy="12" r="9"/><path d="m9 9 6 6m0-6-6 6"/>"#,
    ),
    (
        "triangle-alert",
        r#"<path d="M12 4 3 20h18L12 4Zm0 6v4m0 3v.1"/>"#,
    ),
    (
        "loader",
        r#"<path d="M12 3v4m0 10v4M3 12h4m10 0h4M5.6 5.6l2.9 2.9m7 7 2.9 2.9M5.6 18.4l2.9-2.9m7-7 2.9-2.9"/>"#,
    ),
    ("loader-circle", r#"<path d="M21 12a9 9 0 1 1-6.2-8.6"/>"#),
    (
        "copy",
        r#"<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V6a2 2 0 0 1 2-2h8"/>"#,
    ),
    (
        "stop",
        r#"<rect x="6" y="6" width="12" height="12" rx="2"/>"#,
    ),
    (
        "archive",
        r#"<rect x="3" y="4" width="18" height="5" rx="1"/><path d="M5 9v10h14V9M10 13h4"/>"#,
    ),
    (
        "monitor",
        r#"<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8m-4-4v4"/>"#,
    ),
];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = BRAND.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        let name = path
            .strip_prefix("icons/")
            .and_then(|p| p.strip_suffix(".svg"));
        Ok(DRAWINGS.iter().find(|(key, _)| Some(*key) == name).map(|(_, body)| {
            Cow::Owned(format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#).into_bytes())
        }))
    }
    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(DRAWINGS
            .iter()
            .map(|(name, _)| format!("icons/{name}.svg"))
            .chain(BRAND.iter().map(|(name, _)| (*name).to_string()))
            .filter(|p| p.starts_with(path))
            .map(Into::into)
            .collect())
    }
}
