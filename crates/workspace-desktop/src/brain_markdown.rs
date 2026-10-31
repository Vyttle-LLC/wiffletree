//! Brain notes and STATUS as Markdown the app can draw safely. Brain text is untrusted: images
//! become their alt text and raw HTML shows as text, so nothing loads remote content or runs.
//! `[[wikilinks]]` become links into the browser, a broken one is struck through, and each
//! footnote marker becomes a link that selects its claim; the definitions are left out, since the
//! Sources list shows them. Every link opens only on the human's click (see [`LinkTarget`]).
use markdown::mdast::Node;
use std::ops::Range;
use workspace_core::BrainLink;

const NOTE: &str = "wiffletree-note:";
const CLAIM: &str = "wiffletree-claim:";

/// What a link in a rendered note opens.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum LinkTarget {
    /// A note by its path in the brain folder.
    Note(String),
    /// A relative Markdown link to a note, by the key a wikilink would match it by.
    NoteNamed(String),
    /// A footnote's claim, by its label.
    Claim(String),
    /// A log entry, by its id, from a relative link to its file.
    Entry(String),
    /// A web page, opened in the browser.
    Web(String),
    /// Anything else, which opens nothing.
    Ignored,
}

/// The Markdown to draw for a note or STATUS body whose wikilinks resolve as `links` says.
pub(super) fn prepare(body: &str, links: &[BrainLink]) -> String {
    let Ok(root) = markdown::to_mdast(body, &markdown::ParseOptions::gfm()) else {
        return escape(body);
    };
    let mut edits = vec![];
    collect(&root, body, links, &mut edits);
    edits.sort_by_key(|(range, _)| range.start);
    let mut out = String::with_capacity(body.len());
    let mut at = 0;
    for (range, text) in edits {
        if range.start < at {
            continue;
        }
        out.push_str(&body[at..range.start]);
        out.push_str(&text);
        at = range.end;
    }
    out.push_str(&body[at..]);
    out
}

fn span(node: &Node) -> Option<Range<usize>> {
    let position = node.position()?;
    Some(position.start.offset..position.end.offset)
}

fn collect(node: &Node, body: &str, links: &[BrainLink], edits: &mut Vec<(Range<usize>, String)>) {
    let Some(range) = span(node) else {
        return node
            .children()
            .into_iter()
            .flatten()
            .for_each(|child| collect(child, body, links, edits));
    };
    if neutralise(node, &range, edits) {
        return;
    }
    match node {
        Node::FootnoteDefinition(_) => edits.push((range, String::new())),
        Node::FootnoteReference(note) => {
            let label = &note.identifier;
            edits.push((
                range,
                format!("[\\[{}\\]]({CLAIM}{})", escape(label), encode(label)),
            ));
        }
        Node::Text(_) => wikilinks(&body[range.clone()], range.start, links, edits),
        Node::Link(_) | Node::LinkReference(_) => in_link(node, edits),
        Node::Code(_) | Node::InlineCode(_) => {}
        _ => {
            for child in node.children().into_iter().flatten() {
                collect(child, body, links, edits);
            }
        }
    }
}

/// Replaces an image with its alt text and raw HTML with itself as text; `false` for any other
/// node.
fn neutralise(node: &Node, range: &Range<usize>, edits: &mut Vec<(Range<usize>, String)>) -> bool {
    let text = match node {
        Node::Image(image) => escape(&image.alt),
        Node::ImageReference(image) => escape(&image.alt),
        Node::Html(html) if html.value.trim_start().starts_with("<!--") => String::new(),
        Node::Html(html) => escape(&html.value),
        _ => return false,
    };
    edits.push((range.clone(), text));
    true
}

/// Inside a link's text, only images and raw HTML change: a link holds no other links.
fn in_link(node: &Node, edits: &mut Vec<(Range<usize>, String)>) {
    for child in node.children().into_iter().flatten() {
        if !span(child).is_some_and(|range| neutralise(child, &range, edits)) {
            in_link(child, edits);
        }
    }
}

/// Each `[[Target|Alias#heading]]` in a text run, as a link to its note or struck through.
fn wikilinks(
    text: &str,
    offset: usize,
    links: &[BrainLink],
    edits: &mut Vec<(Range<usize>, String)>,
) {
    let mut from = 0;
    while let Some(start) = text[from..].find("[[").map(|i| from + i) {
        let Some(end) = text[start + 2..].find("]]").map(|i| start + 2 + i) else {
            break;
        };
        let inner = &text[start + 2..end];
        from = end + 2;
        if inner.contains('\n') || inner.trim().is_empty() {
            continue;
        }
        let (target, alias) = match inner.split_once('|') {
            Some((target, alias)) => (target, Some(alias.trim())),
            None => (inner, None),
        };
        let target = target.split('#').next().unwrap_or_default().trim();
        let shown = escape(alias.filter(|a| !a.is_empty()).unwrap_or(target));
        let path = (links.iter())
            .find(|link| link_key(&link.target) == link_key(target))
            .and_then(|link| link.path.as_deref());
        let replacement = match path {
            Some(path) => format!("[{shown}]({NOTE}{})", encode(path)),
            None => format!("~~{shown}~~"),
        };
        edits.push((offset + start..offset + end + 2, replacement));
    }
}

/// The key a wikilink matches a note by, as the host's index keys it.
pub(super) fn link_key(target: &str) -> String {
    let target = target.rsplit('/').next().unwrap_or(target);
    target.trim_end_matches(".md").trim().to_lowercase()
}

/// What a clicked link opens.
pub(super) fn link_target(url: &str) -> LinkTarget {
    if let Some(path) = url.strip_prefix(NOTE) {
        return LinkTarget::Note(decode(path));
    }
    if let Some(label) = url.strip_prefix(CLAIM) {
        return LinkTarget::Claim(decode(label));
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return LinkTarget::Web(url.to_owned());
    }
    if lower.contains(':') || lower.starts_with("//") {
        return LinkTarget::Ignored;
    }
    let path = decode(url.split('#').next().unwrap_or_default());
    if !path.ends_with(".md") {
        return LinkTarget::Ignored;
    }
    if path.contains("logs/") {
        let stem = path
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .trim_end_matches(".md");
        return match stem.get(11..) {
            Some(id) if !id.is_empty() => LinkTarget::Entry(id.to_owned()),
            _ => LinkTarget::Ignored,
        };
    }
    LinkTarget::NoteNamed(link_key(&path))
}

/// Text shown as itself, not as Markdown.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\`*_{}[]<>()#+-.!|~&".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A link's payload, with everything but letters, digits and `-_./` percent-encoded.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%')
            .then(|| text.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::{LinkTarget, link_target, prepare};
    use workspace_core::BrainLink;

    fn links() -> Vec<BrainLink> {
        vec![
            BrainLink {
                target: "Watch Call Token Lifetime".into(),
                path: Some("notes/Watch Call Token Lifetime.md".into()),
            },
            BrainLink {
                target: "Missing Note".into(),
                path: None,
            },
        ]
    }

    #[test]
    fn wikilinks_open_their_note_and_a_broken_one_is_struck_through() {
        let shown = prepare(
            "See [[Watch Call Token Lifetime|the token]] and [[missing note]].",
            &links(),
        );
        assert_eq!(
            shown,
            "See [the token](wiffletree-note:notes/Watch%20Call%20Token%20Lifetime.md) and ~~missing note~~."
        );
        let url = "wiffletree-note:notes/Watch%20Call%20Token%20Lifetime.md";
        assert_eq!(
            link_target(url),
            LinkTarget::Note("notes/Watch Call Token Lifetime.md".into())
        );
        let code = "Write `[[Not a link]]` literally.";
        assert_eq!(prepare(code, &links()), code, "code stays as it is");
    }

    #[test]
    fn a_footnote_marker_selects_its_claim_and_its_definition_is_left_to_the_sources() {
        let shown = prepare(
            "The relay retries once.[^1]\n\n[^1]: [2026-10-20 · oscar](../logs/relay/2026/10/2026-10-20-01j.md)\n",
            &[],
        );
        assert_eq!(
            shown.trim_end(),
            "The relay retries once.[\\[1\\]](wiffletree-claim:1)"
        );
        assert_eq!(
            link_target("wiffletree-claim:1"),
            LinkTarget::Claim("1".into())
        );
    }

    #[test]
    fn images_and_html_never_load_or_run_anything() {
        let shown = prepare(
            "![a tracker](https://evil.example/p.png)\n\n<img src=\"https://evil.example/x.png\">\n\n<!-- conflict:c1 -->\n\nText <script>x()</script>.",
            &[],
        );
        assert!(!shown.contains("https://evil.example"), "{shown}");
        assert!(shown.contains("a tracker"));
        assert!(shown.contains("\\<script\\>"), "{shown}");
        assert!(!shown.contains("conflict:c1"));
        // Inside a link too, inline and by reference.
        let linked = prepare(
            "[![pixel](https://evil.example/p.png)](https://example.com) and [<img src=\"https://evil.example/x.png\">](https://example.com) and [![ref][px]][site]\n\n[px]: https://evil.example/r.png\n[site]: https://example.com\n",
            &[],
        );
        assert!(!linked.contains("evil.example/p.png"), "{linked}");
        assert!(linked.contains("[\\<img src="), "shown as text: {linked}");
        assert!(linked.contains("[pixel](https://example.com)"), "{linked}");
        assert!(linked.contains("[ref][site]"), "{linked}");
    }

    #[test]
    fn a_clicked_link_opens_a_note_an_entry_or_a_web_page_and_nothing_else() {
        assert_eq!(
            link_target("Parent%20Dashboard.md"),
            LinkTarget::NoteNamed("parent dashboard".into())
        );
        assert_eq!(
            link_target("../logs/relay/2026/10/2026-10-20-01jbzb7r5k.md#x"),
            LinkTarget::Entry("01jbzb7r5k".into())
        );
        assert_eq!(
            link_target("https://github.com/o/r/pull/81"),
            LinkTarget::Web("https://github.com/o/r/pull/81".into())
        );
        for ignored in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "notes/a.png",
            "//x.y/a.md",
        ] {
            assert_eq!(link_target(ignored), LinkTarget::Ignored, "{ignored}");
        }
    }
}
