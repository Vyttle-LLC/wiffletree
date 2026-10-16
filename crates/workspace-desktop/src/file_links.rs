//! File references in rendered messages. Inline code naming a file on disk, such as
//! `crates/foo/src/bar.rs:63`, becomes a link that opens the file in its default app, and a
//! Markdown link to a local path opens the same way. Web links keep opening in the browser.
use super::*;
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
    sync::{Mutex, OnceLock},
};

/// Characters that never appear in a file reference, so code holding them is not looked up.
const NOT_IN_PATHS: &[char] = &['`', '*', '?', '|', ';', '$', '"', '<', '>', '\\'];
/// Linked bodies kept before the cache starts over; enough for every message on screen.
const CACHE_LIMIT: usize = 1024;

/// `body` with its file references turned into links, resolved against `bases` in order.
/// Each body is resolved once and cached, since rendering asks for it every frame.
pub(super) fn linkify(body: &str, bases: &[PathBuf]) -> SharedString {
    if !body.contains('`') {
        return SharedString::from(body.to_owned());
    }
    struct Linked {
        body: String,
        bases: Vec<PathBuf>,
        text: SharedString,
    }
    static CACHE: OnceLock<Mutex<HashMap<u64, Linked>>> = OnceLock::new();
    let mut hasher = DefaultHasher::new();
    (body, bases).hash(&mut hasher);
    let key = hasher.finish();
    let cache = CACHE.get_or_init(Mutex::default);
    if let Some(hit) = cache.lock().unwrap().get(&key)
        && hit.body == body
        && hit.bases == bases
    {
        return hit.text.clone();
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let text = SharedString::from(link_code(body, |reference| {
        resolve(reference, bases, home.as_deref())
    }));
    let mut cache = cache.lock().unwrap();
    if cache.len() >= CACHE_LIMIT {
        cache.clear();
    }
    cache.insert(
        key,
        Linked {
            body: body.to_owned(),
            bases: bases.to_vec(),
            text: text.clone(),
        },
    );
    text
}

/// Wraps each inline code span that `resolve` finds a file for in a link to that file.
fn link_code(body: &str, resolve: impl Fn(&str) -> Option<PathBuf>) -> String {
    let mut options = markdown::ParseOptions::gfm();
    // As the text view parses, so code inside math is not mistaken for a code span.
    options.constructs.math_text = true;
    options.constructs.math_flow = true;
    let Ok(root) = markdown::to_mdast(body, &options) else {
        return body.to_owned();
    };
    let mut spans = vec![];
    code_spans(&root, &mut spans);
    let mut linked = String::with_capacity(body.len());
    let mut copied = 0;
    for (start, end, code) in spans {
        // `!` before the new link would make it an image.
        if body[..start].ends_with('!') {
            continue;
        }
        let Some(path) = reference(&code).and_then(&resolve) else {
            continue;
        };
        linked.push_str(&body[copied..start]);
        linked.push_str(&format!(
            "[{}](<{}>)",
            &body[start..end],
            destination(&path)
        ));
        copied = end;
    }
    linked.push_str(&body[copied..]);
    linked
}

/// The source range and text of every inline code span outside a link, in document order.
fn code_spans(node: &markdown::mdast::Node, spans: &mut Vec<(usize, usize, String)>) {
    use markdown::mdast::Node;
    match node {
        Node::InlineCode(code) => {
            if let Some(position) = &code.position {
                spans.push((
                    position.start.offset,
                    position.end.offset,
                    code.value.clone(),
                ));
            }
        }
        Node::Link(_) | Node::LinkReference(_) => {}
        _ => {
            for child in node.children().into_iter().flatten() {
                code_spans(child, spans);
            }
        }
    }
}

/// A path as a `<…>` link destination, which may hold spaces but not unescaped brackets. `%`, `#`
/// and `?` are encoded, since a clicked destination loses its fragment and query and is decoded.
fn destination(path: &Path) -> String {
    let mut escaped = String::new();
    for c in path.to_string_lossy().chars() {
        match c {
            '%' => escaped.push_str("%25"),
            '#' => escaped.push_str("%23"),
            '?' => escaped.push_str("%3F"),
            '<' | '>' | '\\' => {
                escaped.push('\\');
                escaped.push(c);
            }
            _ => escaped.push(c),
        }
    }
    escaped
}

/// The path that inline code may refer to, without a trailing `:line` or `:line:col`; `None` when
/// the code cannot be a path, such as a URL or a shell pipeline, so it is never looked up on disk.
fn reference(code: &str) -> Option<&str> {
    let code = code.trim().trim_end_matches(['.', ',', ';', ':', '!', '?']);
    let path = without_location(code);
    let plausible = !path.contains("://")
        && !path.contains(|c: char| c.is_control() || NOT_IN_PATHS.contains(&c))
        && !path.trim_matches(['.', '/', '~']).is_empty();
    plausible.then_some(path)
}

/// `code` without a trailing `:63`, `:63:5` or `:63-70`.
fn without_location(code: &str) -> &str {
    let mut path = code;
    for _ in 0..2 {
        match path.rsplit_once(':') {
            Some((rest, location))
                if location.starts_with(|c: char| c.is_ascii_digit())
                    && location.chars().all(|c| c.is_ascii_digit() || c == '-') =>
            {
                path = rest
            }
            _ => break,
        }
    }
    path
}

/// The existing file `reference` names: an absolute path, or `~` for `home`, as it is; a
/// relative one under the first of `bases` that holds it.
fn resolve(reference: &str, bases: &[PathBuf], home: Option<&Path>) -> Option<PathBuf> {
    let path = match reference.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            home?.join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(reference),
    };
    if path.is_absolute() {
        return path.is_file().then_some(path);
    }
    bases
        .iter()
        .map(|base| base.join(&path))
        .find(|candidate| candidate.is_file())
}

/// The local path a link points at, without a `#fragment` or `?query` and percent-decoded; `None`
/// for a web or other URL.
fn local_target(url: &str) -> Option<String> {
    if let Some(path) = url.strip_prefix("file://") {
        return Some(percent_decoded(path));
    }
    // `LICENSE:12` is a file and line, not a `LICENSE:` URL.
    let scheme = without_location(url)
        .split_once(':')
        .map(|(scheme, _)| scheme);
    let has_scheme = scheme.is_some_and(|s| {
        s.starts_with(|c: char| c.is_ascii_alphabetic())
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-')
    });
    let path = url.split(['#', '?']).next().unwrap_or_default();
    (!has_scheme && !path.is_empty()).then(|| percent_decoded(path))
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                i += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// What a click on a link asks for.
#[derive(Debug, PartialEq)]
enum LinkAction {
    /// Open the URL in the browser.
    Browse,
    /// Open the local file the target names.
    Open(String),
    /// Offer the file menu for the target at this point.
    Menu(String, Point<Pixels>),
    Nothing,
}

/// A left or middle click, keyboard activation or a tap opens the link, as the text view does by
/// default. A right-click on a file link offers its menu. Anything else does nothing.
fn link_action(url: &str, event: &ClickEvent) -> LinkAction {
    let opens = match event {
        ClickEvent::Mouse(click) => {
            matches!(click.up.button, MouseButton::Left | MouseButton::Middle)
        }
        ClickEvent::Keyboard(_) => true,
        ClickEvent::Touch(tap) => !tap.long_press,
    };
    match (local_target(url), event) {
        (target, _) if opens => target.map_or(LinkAction::Browse, LinkAction::Open),
        (Some(target), ClickEvent::Mouse(click)) if click.up.button == MouseButton::Right => {
            LinkAction::Menu(target, click.up.position)
        }
        _ => LinkAction::Nothing,
    }
}

/// Whether opening `path` would run it instead of showing it: an executable, or a script
/// Terminal runs.
fn runs_when_opened(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    const LAUNCHERS: [&str; 3] = ["command", "tool", "terminal"];
    let executable = path
        .metadata()
        .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0);
    let launcher = path.extension().is_some_and(|extension| {
        LAUNCHERS
            .iter()
            .any(|launcher| extension.eq_ignore_ascii_case(launcher))
    });
    executable || launcher
}

/// Opens a file in its default app, or reveals it in Finder when opening would run it.
fn open_file(path: &Path, cx: &App) {
    if runs_when_opened(path) {
        cx.reveal_path(path);
    } else {
        cx.open_with_system(path);
    }
}

/// Opens a clicked link: a local file in its default app, anything else in the browser. A
/// right-click on a file offers Open, Reveal in Finder and Copy path instead. A file that is
/// gone by now, such as one in a removed worktree, says so.
pub(super) fn open_link(
    url: &str,
    event: &ClickEvent,
    bases: &[PathBuf],
    window: &mut Window,
    cx: &mut App,
) {
    let (target, menu_at) = match link_action(url, event) {
        LinkAction::Browse => return cx.open_url(url),
        LinkAction::Nothing => return,
        LinkAction::Open(target) => (target, None),
        LinkAction::Menu(target, position) => (target, Some(position)),
    };
    let reference = without_location(&target);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match (resolve(reference, bases, home.as_deref()), menu_at) {
        (Some(path), Some(position)) => show_menu(path, position, window, cx),
        (Some(path), None) => open_file(&path, cx),
        (None, _) => {
            struct FileMissing;
            let name = Path::new(reference)
                .file_name()
                .map_or(reference.into(), |name| name.to_string_lossy());
            window.push_notification(
                Notification::warning(format!("Can't open {name}: the file no longer exists."))
                    .id::<FileMissing>(),
                cx,
            );
        }
    }
}

/// The menu of the right-clicked file link. The text view only reports the click, so the
/// workspace draws the menu, through [`link_menu`].
#[derive(Default)]
struct LinkMenu {
    open: Option<(Entity<PopupMenu>, Point<Pixels>)>,
    /// Closes the menu on dismiss; replaced with the next menu.
    _dismissed: Option<Subscription>,
}
impl Global for LinkMenu {}

fn show_menu(path: PathBuf, position: Point<Pixels>, window: &mut Window, cx: &mut App) {
    let menu = PopupMenu::build(window, cx, |menu, _, _| {
        let (open, reveal, copy) = (path.clone(), path.clone(), path);
        menu.item(PopupMenuItem::new("Open").on_click(move |_, _, cx| open_file(&open, cx)))
            .item(
                PopupMenuItem::new("Reveal in Finder")
                    .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
            )
            .separator()
            .item(PopupMenuItem::new("Copy path").on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(
                    copy.to_string_lossy().into_owned(),
                ))
            }))
    });
    let dismissed = window.subscribe(&menu, cx, |_, _: &DismissEvent, window, cx| {
        cx.default_global::<LinkMenu>().open = None;
        window.refresh();
    });
    menu.focus_handle(cx).focus(window, cx);
    cx.set_global(LinkMenu {
        open: Some((menu, position)),
        _dismissed: Some(dismissed),
    });
    window.refresh();
}

/// The open file-link menu, at the pointer and above everything else.
pub(super) fn link_menu(cx: &App) -> Option<impl IntoElement> {
    let (menu, position) = cx.try_global::<LinkMenu>()?.open.clone()?;
    Some(
        deferred(
            anchored()
                .position(position)
                .snap_to_window_with_margin(px(8.))
                .child(menu),
        )
        .with_priority(1),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        LinkAction, link_action, link_code, local_target, reference, resolve, runs_when_opened,
    };
    use gpui::{
        ClickEvent, KeyboardClickEvent, MouseButton, MouseClickEvent, MouseUpEvent,
        TouchClickEvent, point, px,
    };
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

    fn mouse(button: MouseButton) -> ClickEvent {
        ClickEvent::Mouse(MouseClickEvent {
            up: MouseUpEvent {
                button,
                position: point(px(4.), px(8.)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    #[test]
    fn paths_with_locations_are_references() {
        assert_eq!(
            reference("crates/foo/src/bar.rs"),
            Some("crates/foo/src/bar.rs")
        );
        assert_eq!(reference("bar.rs:63"), Some("bar.rs"));
        assert_eq!(reference("bar.rs:63:5"), Some("bar.rs"));
        assert_eq!(reference("bar.rs:63-70"), Some("bar.rs"));
        assert_eq!(reference("/abs/bar.rs"), Some("/abs/bar.rs"));
        assert_eq!(reference("~/notes.md"), Some("~/notes.md"));
        assert_eq!(reference(" README.md, "), Some("README.md"));
        assert_eq!(reference("bar.rs:63."), Some("bar.rs"));
        assert_eq!(reference("LICENSE:12"), Some("LICENSE"));
    }

    #[test]
    fn urls_and_shell_syntax_are_not_references() {
        assert_eq!(reference("cargo test | tail"), None);
        assert_eq!(reference("https://example.com/a.rs"), None);
        assert_eq!(reference("`bar.rs`"), None);
        assert_eq!(reference("echo $HOME/x.rs"), None);
        assert_eq!(reference(".."), None);
        assert_eq!(reference("~"), None);
    }

    #[test]
    fn relative_paths_resolve_in_the_worktree_before_the_project_home() {
        let worktree = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        fs::create_dir(worktree.path().join("src")).unwrap();
        fs::write(worktree.path().join("src/lib.rs"), "").unwrap();
        fs::write(home.path().join("plan.md"), "").unwrap();
        fs::write(worktree.path().join("plan.md"), "").unwrap();
        fs::write(home.path().join("only-home.md"), "").unwrap();
        fs::write(worktree.path().join("LICENSE"), "").unwrap();
        let bases = [worktree.path().to_owned(), home.path().to_owned()];

        assert_eq!(
            resolve("src/lib.rs", &bases, None),
            Some(worktree.path().join("src/lib.rs"))
        );
        assert_eq!(
            resolve("plan.md", &bases, None),
            Some(worktree.path().join("plan.md"))
        );
        assert_eq!(
            resolve("only-home.md", &bases, None),
            Some(home.path().join("only-home.md"))
        );
        assert_eq!(
            resolve("LICENSE", &bases, None),
            Some(worktree.path().join("LICENSE"))
        );
        assert_eq!(resolve("missing.md", &bases, None), None);
        assert_eq!(
            resolve("src", &bases, None),
            None,
            "a directory is not a file"
        );
    }

    #[test]
    fn absolute_and_home_paths_resolve_as_they_are() {
        let home = tempfile::tempdir().unwrap();
        fs::write(home.path().join("notes.md"), "").unwrap();
        let absolute = home.path().join("notes.md");

        assert_eq!(
            resolve(absolute.to_str().unwrap(), &[], None),
            Some(absolute.clone())
        );
        assert_eq!(
            resolve("~/notes.md", &[], Some(home.path())),
            Some(absolute)
        );
        assert_eq!(resolve("~/gone.md", &[], Some(home.path())), None);
        assert_eq!(resolve("~/notes.md", &[], None), None);
    }

    #[test]
    fn only_code_naming_a_file_becomes_a_link() {
        let found = |reference: &str| {
            (reference == "src/a b.rs" || reference == "x.rs")
                .then(|| PathBuf::from("/w").join(reference))
        };
        assert_eq!(
            link_code("See `src/a b.rs:63`, then run `cargo test`.", found),
            "See [`src/a b.rs:63`](</w/src/a b.rs>), then run `cargo test`."
        );
        assert_eq!(link_code("`missing.rs` stays", found), "`missing.rs` stays");
        assert_eq!(link_code("`a1b2c3d` stays", found), "`a1b2c3d` stays");
        assert_eq!(
            link_code("[`x.rs`](https://example.com) and ``x.rs``", found),
            "[`x.rs`](https://example.com) and [``x.rs``](</w/x.rs>)"
        );
        assert_eq!(link_code("```\n`x.rs`\n```", found), "```\n`x.rs`\n```");
        assert_eq!(link_code("Wow!`x.rs`", found), "Wow!`x.rs`");
    }

    #[test]
    fn link_targets_tell_local_paths_from_web_urls() {
        assert_eq!(local_target("https://github.com/x"), None);
        assert_eq!(local_target("mailto:a@b.c"), None);
        assert_eq!(local_target("#heading"), None);
        assert_eq!(
            local_target("file:///tmp/a%20b.md"),
            Some("/tmp/a b.md".into())
        );
        assert_eq!(local_target("/tmp/a%20b.md"), Some("/tmp/a b.md".into()));
        assert_eq!(local_target("LICENSE:12"), Some("LICENSE:12".into()));
        assert_eq!(local_target("src/main.rs#L12"), Some("src/main.rs".into()));
        assert_eq!(
            local_target("docs/PRD.md#agent-message-formatting"),
            Some("docs/PRD.md".into())
        );
        assert_eq!(local_target("docs/a.md?plain=1"), Some("docs/a.md".into()));
        assert_eq!(local_target("docs/PRD.md"), Some("docs/PRD.md".into()));
        assert_eq!(
            local_target("src/main.rs:12"),
            Some("src/main.rs:12".into())
        );
    }

    #[test]
    fn literal_percent_and_hash_signs_survive_the_link_and_the_click() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a%20b #1.md");
        fs::write(&file, "").unwrap();
        fs::write(dir.path().join("a b.md"), "").unwrap();
        let bases = [dir.path().to_owned()];

        let linked = link_code("`a%20b #1.md`", |r| resolve(r, &bases, None));
        let url = linked
            .split_once("(<")
            .and_then(|(_, rest)| rest.split_once(">)"))
            .unwrap()
            .0;
        let target = local_target(url).unwrap();
        assert_eq!(resolve(&target, &bases, None), Some(file));
    }

    #[test]
    fn executables_and_terminal_scripts_run_when_opened() {
        let dir = tempfile::tempdir().unwrap();
        let file = |name: &str, mode: u32| {
            let path = dir.path().join(name);
            fs::write(&path, "").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            path
        };
        assert!(runs_when_opened(&file("build.sh", 0o755)));
        assert!(runs_when_opened(&file("owner-only", 0o700)));
        assert!(runs_when_opened(&file("Start.command", 0o644)));
        assert!(runs_when_opened(&file("fix.TOOL", 0o644)));
        assert!(runs_when_opened(&file("shell.terminal", 0o644)));
        assert!(!runs_when_opened(&file("notes.md", 0o644)));
        assert!(!runs_when_opened(&file("build.sh.txt", 0o644)));
    }

    #[test]
    fn clicks_open_right_clicks_offer_the_menu_for_files_only() {
        let file = "src/lib.rs:12";
        let web = "https://example.com";
        let open = LinkAction::Open(file.into());
        assert_eq!(link_action(file, &mouse(MouseButton::Left)), open);
        assert_eq!(link_action(file, &mouse(MouseButton::Middle)), open);
        assert_eq!(
            link_action(file, &ClickEvent::Keyboard(KeyboardClickEvent::default())),
            open
        );
        assert_eq!(
            link_action(file, &ClickEvent::Touch(TouchClickEvent::default())),
            open
        );
        assert_eq!(
            link_action(web, &mouse(MouseButton::Left)),
            LinkAction::Browse
        );
        assert_eq!(
            link_action(file, &mouse(MouseButton::Right)),
            LinkAction::Menu(file.into(), point(px(4.), px(8.)))
        );
        assert_eq!(
            link_action(web, &mouse(MouseButton::Right)),
            LinkAction::Nothing
        );
        let long_press = ClickEvent::Touch(TouchClickEvent {
            long_press: true,
            ..Default::default()
        });
        assert_eq!(link_action(file, &long_press), LinkAction::Nothing);
        let back = mouse(MouseButton::Navigate(gpui::NavigationDirection::Back));
        assert_eq!(link_action(file, &back), LinkAction::Nothing);
    }
}
