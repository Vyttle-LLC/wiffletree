//! File references in rendered messages. Inline code naming a file on disk, such as
//! `crates/foo/src/bar.rs:63`, becomes a link that opens the file in its default app, and a
//! Markdown link to a local path opens the same way. Web links keep opening in the browser.
//!
//! Only the host knows which references are files, and the UI thread never waits for it: a
//! message shows as plain text at once and gains its links when the host answers.
use super::*;
use async_channel::{Receiver, Sender};
use gpui_component::{
    button::ButtonVariants,
    menu::{PopupMenu, PopupMenuItem},
    text::TextView,
};
use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

/// Characters that never appear in a file reference, so code holding them is not looked up.
const NOT_IN_PATHS: &[char] = &['`', '*', '?', '|', ';', '$', '"', '<', '>', '\\'];
/// Linked bodies kept before the cache starts over; enough for every message on screen.
const CACHE_LIMIT: usize = 1024;
/// The room for references in one request, leaving the rest of the host's line limit for the
/// bases and the request around them.
const REFERENCE_BYTES: usize = MAX_LINE_BYTES / 2;
/// How long a clicked link waits for the host to say where its file is.
const CLICK_REPLY: Duration = Duration::from_secs(2);

/// Markdown whose file references become links once the host has said which are files.
#[derive(Clone)]
pub(super) struct Linked {
    id: SharedString,
    body: SharedString,
    /// None when the body has no inline code, so there is nothing to ask.
    resolution: Option<Arc<Resolution>>,
    bases: Arc<[PathBuf]>,
    bridge: Bridge,
}
impl Linked {
    /// `body`, whose file references resolve against `bases` in order. It never waits for the
    /// host: a body resolved before shows its links at once, any other shows plain until
    /// [`link_when_resolved`] has its answer.
    pub(super) fn new(id: &str, body: &str, bases: Arc<[PathBuf]>, bridge: &Bridge) -> Self {
        Self {
            id: SharedString::from(format!("message-markdown-{id}")),
            body: SharedString::from(body.to_owned()),
            resolution: (body.contains('`'))
                .then(|| Resolution::of(body, &bases, bridge.remote().map(|r| r.id.as_str()))),
            bases,
            bridge: bridge.clone(),
        }
    }
    /// The Markdown to draw: with links once they are resolved, the plain body until then.
    pub(super) fn text(&self) -> SharedString {
        let linked = self.resolution.as_ref().and_then(|r| r.linked.get());
        linked.unwrap_or(&self.body).clone()
    }
    pub(super) fn view(&self) -> TextView {
        let (bases, bridge) = (self.bases.clone(), self.bridge.clone());
        TextView::markdown(self.id.clone(), self.text())
            .selectable(true)
            .on_link_click(move |url, event, window, cx| {
                open_link(url, event, &bases, &bridge, window, cx)
            })
            .code_block_actions(|block, _, _| {
                let code = block.code();
                button("copy-code")
                    .ghost()
                    .icon(ui::icon("copy"))
                    .label("Copy")
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()))
                    })
            })
            .text_size(px(font::BODY))
            .line_height(relative(1.5))
    }
}

/// One body's links against one list of bases, shared by every [`Linked`] showing it. Each body
/// is resolved once and cached, since a page is prepared again on each refresh; while the host
/// is out of reach it stays unlinked and leaves the cache, to be asked again when next prepared.
struct Resolution {
    body: String,
    bases: Vec<PathBuf>,
    /// The paired host whose files they are, `None` for This Mac's.
    host: Option<String>,
    linked: OnceLock<SharedString>,
    /// Taken by whoever asks the host, so a body is asked about once at a time.
    unasked: Mutex<Option<Sender<()>>>,
    /// Closes once the host has answered, or the asking gave up.
    settled: Receiver<()>,
}
impl Resolution {
    fn cache() -> &'static Mutex<HashMap<u64, Arc<Resolution>>> {
        static CACHE: OnceLock<Mutex<HashMap<u64, Arc<Resolution>>>> = OnceLock::new();
        CACHE.get_or_init(Mutex::default)
    }
    fn key(body: &str, bases: &[PathBuf], host: Option<&str>) -> u64 {
        let mut hasher = DefaultHasher::new();
        (body, bases, host).hash(&mut hasher);
        hasher.finish()
    }
    /// The cached resolution of `body` against `bases` on `host`, `None` for This Mac's, or a
    /// new one yet to be asked: the same paths on two hosts are different files.
    fn of(body: &str, bases: &[PathBuf], host: Option<&str>) -> Arc<Self> {
        let key = Self::key(body, bases, host);
        let mut cache = Self::cache().lock().unwrap();
        if let Some(hit) = cache.get(&key)
            && hit.body == body
            && hit.bases == bases
            && hit.host.as_deref() == host
        {
            return hit.clone();
        }
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        let (sender, settled) = async_channel::bounded(1);
        let resolution = Arc::new(Self {
            body: body.to_owned(),
            bases: bases.to_vec(),
            host: host.map(str::to_owned),
            linked: OnceLock::new(),
            unasked: Mutex::new(Some(sender)),
            settled,
        });
        cache.insert(key, resolution.clone());
        resolution
    }
}

/// A resolution this caller asks the host about. Dropped unanswered, it leaves the cache, and
/// its dropped sender wakes everyone waiting for it.
struct Asking {
    resolution: Arc<Resolution>,
    _settles: Sender<()>,
}
impl Drop for Asking {
    fn drop(&mut self) {
        let resolution = &self.resolution;
        if resolution.linked.get().is_none() {
            let key = Resolution::key(
                &resolution.body,
                &resolution.bases,
                resolution.host.as_deref(),
            );
            let mut cache = Resolution::cache().lock().unwrap();
            if cache.get(&key).is_some_and(|r| Arc::ptr_eq(r, resolution)) {
                cache.remove(&key);
            }
        }
    }
}

/// Resolves every body of `linked` still without links, off the UI thread, and redraws the view
/// once they have them. One request asks about all the bodies that share their bases; a body
/// already being asked about elsewhere is waited for, not asked again.
pub(super) fn link_when_resolved<'a, V: 'static>(
    linked: impl IntoIterator<Item = &'a Linked>,
    bridge: &Bridge,
    cx: &mut Context<V>,
) {
    let linked: Vec<_> = linked.into_iter().collect();
    let Some(resolving) = resolve(&linked, bridge) else {
        return;
    };
    let resolving = cx.background_spawn(resolving);
    cx.spawn(async move |view, cx| {
        resolving.await;
        let _ = view.update(cx, |_, cx| cx.notify());
    })
    .detach();
}

/// Waits until the host has answered for `linked`, as a view's redraw would.
#[cfg(test)]
pub(super) fn wait_for_links<'a>(linked: impl IntoIterator<Item = &'a Linked>, bridge: &Bridge) {
    let linked: Vec<_> = linked.into_iter().collect();
    if let Some(resolving) = resolve(&linked, bridge) {
        futures_lite::future::block_on(resolving);
    }
}

/// What [`link_when_resolved`] waits for; `None` when every body has its answer already.
fn resolve(linked: &[&Linked], bridge: &Bridge) -> Option<impl Future<Output = ()> + Send + use<>> {
    let mut asking: HashMap<Vec<PathBuf>, Vec<Asking>> = HashMap::new();
    let mut elsewhere = vec![];
    let waiting = (linked.iter())
        .filter_map(|linked| linked.resolution.clone())
        // A body whose asking gave up stays plain until it is prepared again.
        .filter(|resolution| resolution.linked.get().is_none() && !resolution.settled.is_closed());
    for resolution in waiting {
        let sender = resolution.unasked.lock().unwrap().take();
        match sender {
            Some(sender) => asking
                .entry(resolution.bases.clone())
                .or_default()
                .push(Asking {
                    resolution,
                    _settles: sender,
                }),
            None => elsewhere.push(resolution.settled.clone()),
        }
    }
    if asking.is_empty() && elsewhere.is_empty() {
        return None;
    }
    let bridge = bridge.clone();
    let label = bridge.remote().map(|r| r.name.clone());
    Some(async move {
        let mut replies = vec![];
        for (bases, group) in asking {
            let mut unasked = vec![];
            for asking in group {
                let candidates = candidates(&asking.resolution.body);
                if candidates.is_empty() {
                    let _ = (asking.resolution.linked).set(asking.resolution.body.clone().into());
                } else {
                    unasked.push((asking, candidates));
                }
            }
            for batch in batches(unasked) {
                let asked = ask::resolve_paths(batch.references.clone(), bases.clone());
                // Unsent, the batch is dropped, so its bodies stay unlinked and uncached.
                if let Ok(reply) = bridge.ask(asked) {
                    replies.push((batch, reply));
                }
            }
        }
        for (batch, reply) in replies {
            let Ok(resolved) = reply.await else {
                continue;
            };
            let files: HashMap<&str, PathBuf> = (batch.references.iter())
                .zip(resolved)
                .filter_map(|(reference, file)| Some((reference.as_str(), file?.path)))
                .collect();
            for (asking, candidates) in &batch.bodies {
                let body = &asking.resolution.body;
                let found = |r: &str| files.get(r).cloned();
                let text = link_code(body, candidates, label.as_deref(), found);
                let _ = asking.resolution.linked.set(text.into());
            }
        }
        for settled in elsewhere {
            let _ = settled.recv().await;
        }
    })
}

/// One request's worth of bodies that share their bases, with each reference among them once.
#[derive(Default)]
struct Batch {
    bodies: Vec<(Asking, Vec<Candidate>)>,
    references: Vec<String>,
    asked: HashSet<String>,
    /// The references' size in the request.
    bytes: usize,
}

/// `bodies`, each with its candidates, in requests small enough for the host to take. A body
/// too large for a request even alone gets one to itself, which is refused, so it stays plain.
fn batches(bodies: Vec<(Asking, Vec<Candidate>)>) -> Vec<Batch> {
    let size = |reference: &String| serde_json::to_string(reference).map_or(0, |r| r.len()) + 1;
    let mut batches: Vec<Batch> = vec![];
    for (asking, candidates) in bodies {
        let unasked: HashSet<&String> = (candidates.iter())
            .map(|(.., reference)| reference)
            .filter(|reference| !batches.last().is_some_and(|b| b.asked.contains(*reference)))
            .collect();
        let bytes: usize = unasked.into_iter().map(size).sum();
        if batches
            .last()
            .is_none_or(|b| b.bytes + bytes > REFERENCE_BYTES)
        {
            batches.push(Batch::default());
        }
        let batch = batches.last_mut().unwrap();
        for (.., reference) in &candidates {
            if batch.asked.insert(reference.clone()) {
                batch.bytes += size(reference);
                batch.references.push(reference.clone());
            }
        }
        batch.bodies.push((asking, candidates));
    }
    batches
}

/// An inline code span's source range, with its code or the file reference the code names.
type Candidate = (usize, usize, String);

/// The source range of each inline code span in `body` that may name a file, with the
/// reference it names.
fn candidates(body: &str) -> Vec<Candidate> {
    let mut options = markdown::ParseOptions::gfm();
    // As the text view parses, so code inside math is not mistaken for a code span.
    options.constructs.math_text = true;
    options.constructs.math_flow = true;
    let Ok(root) = markdown::to_mdast(body, &options) else {
        return vec![];
    };
    let mut spans = vec![];
    code_spans(&root, &mut spans);
    spans
        .into_iter()
        // `!` before the new link would make it an image.
        .filter(|(start, ..)| !body[..*start].ends_with('!'))
        .filter_map(|(start, end, code)| Some((start, end, reference(&code)?.to_owned())))
        .collect()
}

/// Wraps each of the `candidates` that `resolve` finds a file for in a link to that file, labelled
/// "Studio Mac: …" when the file is on the remote `host`.
fn link_code(
    body: &str,
    candidates: &[Candidate],
    host: Option<&str>,
    resolve: impl Fn(&str) -> Option<PathBuf>,
) -> String {
    let label = host.map(|host| format!("{}: ", escaped_text(host)));
    let mut linked = String::with_capacity(body.len());
    let mut copied = 0;
    for (start, end, reference) in candidates {
        let (start, end) = (*start, *end);
        let Some(path) = resolve(reference) else {
            continue;
        };
        linked.push_str(&body[copied..start]);
        linked.push_str(&format!(
            "[{}{}](<{}>)",
            label.as_deref().unwrap_or_default(),
            &body[start..end],
            destination(&path)
        ));
        copied = end;
    }
    linked.push_str(&body[copied..]);
    linked
}

/// `text` with what Markdown would read as markup escaped.
fn escaped_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii_punctuation() {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// The source range and text of every inline code span outside a link, in document order.
fn code_spans(node: &markdown::mdast::Node, spans: &mut Vec<Candidate>) {
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

/// Opens a file in its default app, or reveals it in Finder when opening would run it. A remote
/// host's file is read through that host and opened as a read-only copy on this Mac, never as
/// a file of the same path here.
pub(super) fn open_file(file: &ResolvedPath, bridge: &Bridge, window: &mut Window, cx: &mut App) {
    let Some(host) = bridge.remote() else {
        return show_file(&file.path, file.runs_when_opened, cx);
    };
    let copy = copy_path(&cache_folder(), &host.id, &file.path);
    let read = bridge.ask(ask::read_file(file.path.clone(), MAX_ATTACHMENT_BYTES));
    let name = file.path.clone();
    window
        .spawn(cx, async move |cx| {
            let copied = match read {
                Ok(read) => read.await.and_then(|contents| write_copy(&copy, &contents)),
                Err(error) => Err(error),
            };
            let _ = cx.update(|window, cx| match copied {
                Ok(()) => show_file(&copy, !opens_copy(&copy), cx),
                Err(error) => missing(&name.to_string_lossy(), error, window, cx),
            });
        })
        .detach();
}

fn show_file(path: &Path, runs_when_opened: bool, cx: &App) {
    if runs_when_opened {
        cx.reveal_path(path);
    } else {
        cx.open_with_system(path);
    }
}

/// Whether a copy of a remote host's file may be opened rather than shown in Finder. This Mac
/// decides from the copy itself, with the rule it uses for its own files, never from what the
/// host says; a copy it cannot read is shown.
fn opens_copy(copy: &Path) -> bool {
    copy.is_file() && !runs_when_opened(copy)
}

/// Where this Mac keeps read-only copies of remote hosts' files.
fn cache_folder() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join("Library/Caches/Wiffletree/hosts")
}

/// Where the copy of `path` on the host `host_id` is kept under `cache`: its folder for the host,
/// then the path's own folders, never above them.
fn copy_path(cache: &Path, host_id: &str, path: &Path) -> PathBuf {
    let mut copy = cache.join(workspace_core::pairing::short_id(host_id));
    for part in path.components() {
        if let std::path::Component::Normal(part) = part {
            copy.push(part);
        }
    }
    copy
}

/// Marks `copy` as downloaded, as a browser does, so macOS checks it before anything runs it.
fn quarantine(copy: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt as _;
    let path = std::ffi::CString::new(copy.as_os_str().as_bytes())?;
    let value = format!("0081;{:08x};Wiffletree;", ui::now() / 1000);
    // SAFETY: the name, path and value are valid for the call.
    let set = unsafe {
        libc::setxattr(
            path.as_ptr(),
            c"com.apple.quarantine".as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    match set {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

/// Writes `contents` to `copy` as a read-only, quarantined file, in place of an earlier copy.
fn write_copy(copy: &Path, contents: &FileContents) -> Result<(), String> {
    use base64::Engine as _;
    use std::os::unix::fs::PermissionsExt as _;
    let bytes = (base64::engine::general_purpose::STANDARD.decode(&contents.data))
        .map_err(|e| e.to_string())?;
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(copy.parent().unwrap_or(copy))?;
        if copy.exists() {
            std::fs::set_permissions(copy, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::write(copy, bytes)?;
        quarantine(copy)?;
        std::fs::set_permissions(copy, std::fs::Permissions::from_mode(0o444))
    };
    write().map_err(|e| format!("the copy could not be written: {e}"))
}

/// Opens a clicked link: a local file in its default app, anything else in the browser. A
/// right-click on a file offers Open, Reveal in Finder and Copy path instead. A file that is
/// gone by now, such as one in a removed worktree, says so. The host says where the file is and
/// whether opening it would run it; this app, on the host's Mac, opens it when it answers.
pub(super) fn open_link(
    url: &str,
    event: &ClickEvent,
    bases: &[PathBuf],
    bridge: &Bridge,
    window: &mut Window,
    cx: &mut App,
) {
    let (target, menu_at) = match link_action(url, event) {
        LinkAction::Browse => return cx.open_url(url),
        LinkAction::Nothing => return,
        LinkAction::Open(target) => (target, None),
        LinkAction::Menu(target, position) => (target, Some(position)),
    };
    let reference = without_location(&target).to_owned();
    let asked = ask::resolve_paths(vec![reference.clone()], bases.to_vec());
    let reply = bridge.ask(asked);
    let deadline = cx.background_executor().timer(CLICK_REPLY);
    let bridge = bridge.clone();
    window
        .spawn(cx, async move |cx| {
            let resolved = match reply {
                Ok(reply) => in_time(reply, deadline)
                    .await
                    .map(|mut found| found.pop().flatten()),
                Err(error) => Err(error),
            };
            let _ = cx.update(|window, cx| {
                let why = match (resolved, menu_at) {
                    (Ok(Some(file)), Some(position)) => {
                        return show_menu(file, position, bridge.clone(), window, cx);
                    }
                    (Ok(Some(file)), None) => return open_file(&file, &bridge, window, cx),
                    (Ok(None), _) => "the file no longer exists".to_string(),
                    (Err(error), _) => error,
                };
                missing(&reference, why, window, cx);
            });
        })
        .detach();
}

/// `reply`, or an error once `deadline` passes first, when the late reply is dropped.
async fn in_time<T>(
    reply: impl Future<Output = Result<T, String>>,
    deadline: impl Future,
) -> Result<T, String> {
    futures_lite::future::or(reply, async {
        deadline.await;
        Err("The host did not answer in time".to_string())
    })
    .await
}

/// Says that the file `reference` names could not be opened, and why.
fn missing(reference: &str, why: String, window: &mut Window, cx: &mut App) {
    struct FileMissing;
    let name = Path::new(reference)
        .file_name()
        .map_or(reference.into(), |name| name.to_string_lossy());
    window.push_notification(
        Notification::warning(format!("Can't open {name}: {why}.")).id::<FileMissing>(),
        cx,
    );
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

/// Reveal in Finder is offered only for a file on this Mac; Copy path copies the host's path.
fn show_menu(
    file: ResolvedPath,
    position: Point<Pixels>,
    bridge: Bridge,
    window: &mut Window,
    cx: &mut App,
) {
    let here = bridge.remote().is_none();
    let menu = PopupMenu::build(window, cx, |menu, _, _| {
        let (reveal, copy) = (file.path.clone(), file.path.clone());
        let open = file;
        menu.item(
            PopupMenuItem::new("Open")
                .on_click(move |_, window, cx| open_file(&open, &bridge, window, cx)),
        )
        .when(here, |menu| {
            menu.item(
                PopupMenuItem::new("Reveal in Finder")
                    .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
            )
        })
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
        Bridge, LinkAction, Linked, candidates, in_time, link_action, local_target, reference,
        resolve, wait_for_links,
    };
    use crate::bridge::tests::serve;
    use gpui::{
        ClickEvent, KeyboardClickEvent, MouseButton, MouseClickEvent, MouseUpEvent,
        TouchClickEvent, point, px,
    };
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::PathBuf,
        sync::atomic::Ordering,
        time::{Duration, Instant},
    };
    use workspace_core::{ResolvedPath, ask};

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

    /// What the host resolves `reference` to against `bases`.
    fn resolved(bridge: &Bridge, reference: &str, bases: &[PathBuf]) -> Option<ResolvedPath> {
        let asked = ask::resolve_paths(vec![reference.into()], bases.to_vec());
        bridge.ask_now(asked).unwrap().pop().flatten()
    }

    /// `body` linked against `bases`, once the host has answered.
    fn linked(body: &str, bases: &[PathBuf], bridge: &Bridge) -> Linked {
        let linked = Linked::new("m", body, bases.to_vec().into(), bridge);
        wait_for_links([&linked], bridge);
        linked
    }

    #[test]
    fn the_host_resolves_relative_paths_in_the_worktree_before_the_project_home() {
        let served = serve(|_| {});
        let worktree = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        fs::create_dir(worktree.path().join("src")).unwrap();
        fs::write(worktree.path().join("src/lib.rs"), "").unwrap();
        fs::write(home.path().join("plan.md"), "").unwrap();
        fs::write(worktree.path().join("plan.md"), "").unwrap();
        fs::write(home.path().join("only-home.md"), "").unwrap();
        let bases = [worktree.path().to_owned(), home.path().to_owned()];
        let path = |reference| resolved(&served.bridge, reference, &bases).map(|r| r.path);

        assert_eq!(path("src/lib.rs"), Some(worktree.path().join("src/lib.rs")));
        assert_eq!(path("plan.md"), Some(worktree.path().join("plan.md")));
        assert_eq!(path("only-home.md"), Some(home.path().join("only-home.md")));
        assert_eq!(path("missing.md"), None);
        assert_eq!(path("src"), None, "a directory is not a file");
        let absolute = home.path().join("plan.md");
        assert_eq!(path(absolute.to_str().unwrap()), Some(absolute));

        // Linking asks once for the whole body.
        let body = "See `src/lib.rs:12`, `plan.md` and `missing.md`, then run `cargo test`.";
        let linked = linked(body, &bases, &served.bridge);
        let lib = worktree.path().join("src/lib.rs");
        let plan = worktree.path().join("plan.md");
        assert_eq!(
            linked.text(),
            format!(
                "See [`src/lib.rs:12`](<{}>), [`plan.md`](<{}>) and `missing.md`, then run `cargo test`.",
                lib.display(),
                plan.display()
            )
        );
    }

    #[test]
    fn with_the_host_out_of_reach_a_body_stays_unlinked() {
        let served = serve(|_| {});
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("notes.md"), "").unwrap();
        served.bridge.shutdown();
        let linked = linked("`notes.md`", &[dir.path().into()], &served.bridge);
        assert_eq!(linked.text(), "`notes.md`");
    }

    #[test]
    fn a_busy_host_shows_the_message_at_once_and_links_it_when_it_answers() {
        let served = serve(|_| {});
        let (busy, held) = served.busy();
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("notes.md"), "").unwrap();
        let bases = [dir.path().to_owned()];
        held.store(true, Ordering::Relaxed);

        let started = Instant::now();
        let linked = Linked::new("m", "See `notes.md`", bases.to_vec().into(), &busy);
        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(
            linked.text(),
            "See `notes.md`",
            "plain until the host answers"
        );

        // The request is out, and the same body prepared again is not asked about twice.
        let mut resolving = Box::pin(resolve(&[&linked], &busy).unwrap());
        assert!(
            futures_lite::future::block_on(futures_lite::future::poll_once(&mut resolving))
                .is_none()
        );
        let again = Linked::new("m", "See `notes.md`", bases.to_vec().into(), &busy);
        assert_eq!(again.text(), "See `notes.md`");

        held.store(false, Ordering::Relaxed);
        futures_lite::future::block_on(resolving);
        let file = dir.path().join("notes.md");
        let expected = format!("See [`notes.md`](<{}>)", file.display());
        assert_eq!(linked.text(), expected);
        assert_eq!(again.text(), expected, "both show the one answer");
        busy.shutdown();
    }

    #[test]
    fn a_page_too_large_for_one_request_is_asked_about_in_several() {
        let served = serve(|_| {});
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("README.md"), "").unwrap();
        let bases: std::sync::Arc<[PathBuf]> = vec![dir.path().to_owned()].into();
        // 40 bodies of 300 distinct references each: about 450 KiB, past the host's 256 KiB
        // line, though each body alone fits.
        let body = |i: usize, references: usize| {
            let missing =
                (0..references).map(|j| format!("`docs/missing-{i}-{j}-of-the-page.md` "));
            format!("`README.md` {}", missing.collect::<String>())
        };
        let page: Vec<_> = (0..40)
            .map(|i| Linked::new("m", &body(i, 300), bases.clone(), &served.bridge))
            .collect();
        // One body too large for any request stays plain, as it always did.
        let huge = Linked::new("m", &body(40, 8000), bases.clone(), &served.bridge);
        wait_for_links(page.iter().chain([&huge]), &served.bridge);

        let readme = format!(
            "[`README.md`](<{}>)",
            dir.path().join("README.md").display()
        );
        assert!(page.iter().all(|linked| linked.text().starts_with(&readme)));
        assert!(huge.text().starts_with("`README.md`"));
    }

    #[test]
    fn a_click_gives_up_on_a_host_that_does_not_answer_in_time() {
        let served = serve(|_| {});
        let (busy, held) = served.busy();
        let asked = || {
            busy.ask(ask::resolve_paths(vec!["notes.md".into()], vec![]))
                .unwrap()
        };
        held.store(true, Ordering::Relaxed);
        let late = futures_lite::future::block_on(in_time(asked(), std::future::ready(())));
        assert_eq!(late.unwrap_err(), "The host did not answer in time");

        held.store(false, Ordering::Relaxed);
        let answered =
            futures_lite::future::block_on(in_time(asked(), std::future::pending::<()>()));
        assert_eq!(answered.unwrap(), vec![None]);
        busy.shutdown();
    }

    #[test]
    fn only_code_naming_a_file_becomes_a_link() {
        let link_code = |body: &str, found: fn(&str) -> Option<PathBuf>| {
            super::link_code(body, &candidates(body), None, found)
        };
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
    fn a_remote_file_is_copied_under_its_hosts_folder_and_never_above_it() {
        let cache = PathBuf::from("/c");
        let copy = |path: &str| super::copy_path(&cache, "0123456789abcdef99", path.as_ref());
        assert_eq!(
            copy("/Users/m/dev/tidepool/a.rs"),
            PathBuf::from("/c/0123456789abcdef/Users/m/dev/tidepool/a.rs")
        );
        assert_eq!(
            copy("/Users/../../etc/passwd"),
            PathBuf::from("/c/0123456789abcdef/Users/etc/passwd")
        );
    }

    #[test]
    fn a_copy_that_would_run_is_shown_whatever_the_host_says() {
        let folder = tempfile::tempdir().unwrap();
        let contents = workspace_core::FileContents {
            path: PathBuf::new(),
            data: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "rm -rf ~"),
        };
        // The host reported it as safe to open; only the copy here decides.
        let script = folder.path().join("h/x.command");
        super::write_copy(&script, &contents).unwrap();
        assert!(
            !super::opens_copy(&script),
            "a Terminal script is shown, not opened"
        );
        let notes = folder.path().join("h/notes.md");
        super::write_copy(&notes, &contents).unwrap();
        assert!(super::opens_copy(&notes));
        assert!(
            !super::opens_copy(&folder.path().join("h/gone.md")),
            "unreadable: shown"
        );

        let mut value = [0u8; 64];
        let path = std::ffi::CString::new(script.to_str().unwrap()).unwrap();
        // SAFETY: the buffer is 64 bytes and the strings are valid for the call.
        let read = unsafe {
            libc::getxattr(
                path.as_ptr(),
                c"com.apple.quarantine".as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        assert!(read > 0, "quarantined");
        assert!(value[..read as usize].ends_with(b";Wiffletree;"));
    }

    #[test]
    fn a_copy_is_read_only_and_replaces_the_one_before() {
        use std::os::unix::fs::PermissionsExt as _;
        let folder = tempfile::tempdir().unwrap();
        let copy = folder.path().join("h/notes.md");
        let contents = |text: &str| workspace_core::FileContents {
            path: PathBuf::new(),
            data: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, text),
        };
        super::write_copy(&copy, &contents("one")).unwrap();
        super::write_copy(&copy, &contents("two")).unwrap();
        assert_eq!(fs::read_to_string(&copy).unwrap(), "two");
        assert_eq!(
            fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
            0o444
        );
    }

    #[test]
    fn a_file_on_a_remote_host_is_labelled_with_the_host() {
        let body = "In `~/dev/tidepool`, see `notes.md`.";
        let found = |reference: &str| {
            (reference == "~/dev/tidepool").then(|| PathBuf::from("/Users/m/dev/tidepool"))
        };
        assert_eq!(
            super::link_code(body, &candidates(body), Some("Studio Mac"), found),
            "In [Studio Mac: `~/dev/tidepool`](</Users/m/dev/tidepool>), see `notes.md`."
        );
        assert_eq!(
            super::link_code(body, &candidates(body), Some("[Mac]"), found),
            "In [\\[Mac\\]: `~/dev/tidepool`](</Users/m/dev/tidepool>), see `notes.md`."
        );
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
        let served = serve(|_| {});
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a%20b #1.md");
        fs::write(&file, "").unwrap();
        fs::write(dir.path().join("a b.md"), "").unwrap();
        let bases = [dir.path().to_owned()];

        let linked = linked("`a%20b #1.md`", &bases, &served.bridge);
        let text = linked.text();
        let url = (text.split_once("(<"))
            .and_then(|(_, rest)| rest.split_once(">)"))
            .unwrap()
            .0;
        let target = local_target(url).unwrap();
        let clicked = resolved(&served.bridge, &target, &bases).unwrap();
        assert_eq!(clicked.path, file);
    }

    #[test]
    fn the_host_says_executables_and_terminal_scripts_run_when_opened() {
        let served = serve(|_| {});
        let dir = tempfile::tempdir().unwrap();
        let runs = |name: &str, mode: u32| {
            let path = dir.path().join(name);
            fs::write(&path, "").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            let bases = [dir.path().to_owned()];
            resolved(&served.bridge, name, &bases)
                .unwrap()
                .runs_when_opened
        };
        assert!(runs("build.sh", 0o755));
        assert!(runs("owner-only", 0o700));
        assert!(runs("Start.command", 0o644));
        assert!(runs("fix.TOOL", 0o644));
        assert!(runs("shell.terminal", 0o644));
        assert!(!runs("notes.md", 0o644));
        assert!(!runs("build.sh.txt", 0o644));
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
