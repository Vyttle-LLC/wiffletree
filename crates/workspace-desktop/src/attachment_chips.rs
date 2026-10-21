//! Attached files: dropped into a draft, and shown as chips pending in the composer or
//! sent with a message.
use super::*;
use crate::staging::Hold;
use base64::Engine as _;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use ui::icon;

const THUMBNAIL: f32 = 56.;

/// Decoded images kept at once; showing another drops the one shown longest ago.
const IMAGE_LIMIT: usize = 48;
/// How long a failed read shows its error before the next draw asks the host again.
const RETRY_AFTER: Duration = Duration::from_secs(15);

/// Which host's file an image is: `None` for This Mac's host.
type ImageKey = (Option<String>, PathBuf);

/// What is known of one image.
enum Held<T> {
    Reading,
    Read { image: T, shown: u64 },
    Failed { error: SharedString, at: Instant },
}

/// What drawing an image finds.
#[derive(Debug, PartialEq)]
enum Found<T> {
    Image(T),
    Failed(SharedString),
    Reading,
    /// Not held, or failed long enough ago: read it.
    Read,
}

/// Images read from their host, each read once while it is held. A failed read is asked for
/// again after `RETRY_AFTER`, and at most `IMAGE_LIMIT` decoded images are held.
struct ImageCache<T> {
    held: HashMap<ImageKey, Held<T>>,
    /// Counts draws, so the image shown longest ago is the first dropped.
    draws: u64,
}
impl<T: Clone> ImageCache<T> {
    fn new() -> Self {
        Self {
            held: HashMap::new(),
            draws: 0,
        }
    }
    fn find(&mut self, key: &ImageKey, now: Instant) -> Found<T> {
        self.draws += 1;
        match self.held.get_mut(key) {
            Some(Held::Read { image, shown }) => {
                *shown = self.draws;
                Found::Image(image.clone())
            }
            Some(Held::Failed { at, .. }) if now.duration_since(*at) >= RETRY_AFTER => Found::Read,
            Some(Held::Failed { error, .. }) => Found::Failed(error.clone()),
            Some(Held::Reading) => Found::Reading,
            None => Found::Read,
        }
    }
    fn reading(&mut self, key: ImageKey) {
        self.held.insert(key, Held::Reading);
    }
    /// Holds what the read of `key` brought; returns the images dropped to stay in bounds.
    fn read(&mut self, key: ImageKey, read: Result<T, SharedString>, now: Instant) -> Vec<T> {
        let held = match read {
            Ok(image) => Held::Read {
                image,
                shown: self.draws,
            },
            Err(error) => Held::Failed { error, at: now },
        };
        self.held.insert(key, held);
        let mut images: Vec<_> = (self.held.iter())
            .filter_map(|(key, held)| match held {
                Held::Read { shown, .. } => Some((*shown, key.clone())),
                _ => None,
            })
            .collect();
        let over = images.len().saturating_sub(IMAGE_LIMIT);
        images.sort();
        (images.into_iter().take(over))
            .filter_map(|(_, key)| match self.held.remove(&key) {
                Some(Held::Read { image, .. }) => Some(image),
                _ => None,
            })
            .collect()
    }
}

/// Sent messages' images, read from the attachment store of the host that holds them. The app
/// never reads a host's files itself.
pub(super) struct HostImages(ImageCache<Arc<Image>>);
impl Global for HostImages {}
impl HostImages {
    pub(super) fn new() -> Self {
        Self(ImageCache::new())
    }
}

/// A sent attachment's image, which the selected host reads; loading until its bytes arrive.
fn host_image(path: PathBuf, format: workspace_core::ImageFormat) -> ImageSource {
    ImageSource::from(move |window: &mut Window, cx: &mut App| {
        let bridge = cx.try_global::<Hosts>()?.bridge();
        let key = (bridge.remote().map(|r| r.id.clone()), path.clone());
        cx.try_global::<HostImages>()?;
        match cx.global_mut::<HostImages>().0.find(&key, Instant::now()) {
            Found::Image(image) => image.use_render_image(window, cx).map(Ok),
            Found::Failed(error) => Some(Err(ImageCacheError::Asset(error))),
            Found::Reading => None,
            Found::Read => {
                read_image(&bridge, key, format, cx);
                None
            }
        }
    })
}

/// Asks the host for an attachment's bytes and redraws once they arrive. While the host is out
/// of reach the request waits for it.
fn read_image(bridge: &Bridge, key: ImageKey, format: workspace_core::ImageFormat, cx: &mut App) {
    cx.global_mut::<HostImages>().0.reading(key.clone());
    let reply = bridge.ask(ask::read_file(key.1.clone(), MAX_ATTACHMENT_BYTES));
    cx.spawn(async move |cx| {
        let contents = match reply {
            Ok(reply) => reply.await,
            Err(error) => Err(error),
        };
        let image = contents.and_then(|contents| image(&contents, format).map(Arc::new));
        cx.update(|cx| {
            let image = image.map_err(SharedString::from);
            let dropped = (cx.global_mut::<HostImages>().0).read(key, image, Instant::now());
            for image in dropped {
                image.remove_asset(cx);
            }
            cx.refresh_windows();
        });
    })
    .detach();
}

/// The image in `ReadFile`'s reply.
fn image(contents: &FileContents, format: workspace_core::ImageFormat) -> Result<Image, String> {
    let bytes = (base64::engine::general_purpose::STANDARD.decode(&contents.data))
        .map_err(|e| e.to_string())?;
    let format = match format {
        workspace_core::ImageFormat::Png => gpui::ImageFormat::Png,
        workspace_core::ImageFormat::Jpeg => gpui::ImageFormat::Jpeg,
        workspace_core::ImageFormat::Gif => gpui::ImageFormat::Gif,
        workspace_core::ImageFormat::Webp => gpui::ImageFormat::Webp,
    };
    Ok(Image::from_bytes(format, bytes))
}

/// A file on its way into a draft: the path it was dropped or pasted as, which names it, and its
/// staged copy or why it could not be copied.
pub(super) type Staged = (PathBuf, Result<Hold, String>);

/// Adds staged files to a draft's attachments, as the host `inspected` their copies in order.
/// Returns why any were refused, and the holds on the copies attached; the rest are dropped.
pub(super) fn add_staged(
    attached: &mut Vec<Attachment>,
    staged: Vec<Staged>,
    inspected: Vec<Result<Attachment, String>>,
) -> (Vec<String>, Vec<Hold>) {
    let mut inspected = inspected.into_iter();
    let mut holds = vec![];
    let (paths, files): (Vec<_>, Vec<_>) = (staged.into_iter())
        .map(|(path, copy)| {
            let file = copy.and_then(|held| {
                holds.push(held);
                (inspected.next()).unwrap_or_else(|| Err("The host did not check it".into()))
            });
            (path, file)
        })
        .unzip();
    let refused = add_dropped(attached, &paths, files);
    holds.retain(|held| attached.iter().any(|a| a.path == held.path()));
    (refused, holds)
}

/// Adds dropped files, as the host `inspected` them, to a draft's attachments and returns why
/// any were refused. A file whose name is already attached, ignoring case, is refused (the same
/// file again included), since a message stores its files by name.
fn add_dropped(
    attached: &mut Vec<Attachment>,
    paths: &[PathBuf],
    inspected: Vec<Result<Attachment, String>>,
) -> Vec<String> {
    let mut refused = vec![];
    for (path, file) in paths.iter().zip(inspected) {
        let name = path.file_name().map(|n| n.to_string_lossy());
        let taken = |name: &str| attached.iter().any(|a| same_file_name(&a.name(), name));
        if let Some(name) = name.filter(|n| taken(n)) {
            refused.push(format!("{name}: a file with this name is already attached"));
            continue;
        }
        match file {
            Ok(file) => attached.push(file),
            Err(error) => refused.push(error),
        }
    }
    refused
}

/// A size such as `512 B`, `14 KB` or `3.2 MB`.
pub(super) fn file_size(bytes: u64) -> String {
    const KB: f64 = 1024.;
    let bytes_f = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes_f < KB * KB {
        format!("{:.0} KB", bytes_f / KB)
    } else {
        format!("{:.1} MB", bytes_f / (KB * KB))
    }
}

/// Where a chip's thumbnail comes from: a draft's files are this app's own; a sent message's
/// are in the host's attachment store.
#[derive(Clone, Copy)]
pub(super) enum Stored {
    Draft,
    Sent,
}

/// A thumbnail for an image, otherwise the file's name and size.
pub(super) fn chip(
    id: impl Into<ElementId>,
    attachment: &Attachment,
    stored: Stored,
    p: Palette,
) -> Stateful<Div> {
    let frame = div()
        .id(id)
        .relative()
        .flex_none()
        .rounded_lg()
        .border_1()
        .border_color(p.edge.opacity(edge::FIRM))
        .bg(p.overlay)
        .overflow_hidden();
    if let Some(format) = attachment.image {
        let unreadable = icon("file-text").text_color(p.subtle);
        let source = match stored {
            Stored::Draft => ImageSource::from(attachment.path.clone()),
            Stored::Sent => host_image(attachment.path.clone(), format),
        };
        frame.size(px(THUMBNAIL)).child(
            img(source)
                .size_full()
                .object_fit(ObjectFit::Cover)
                .with_fallback(move || {
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(unreadable.clone())
                        .into_any_element()
                }),
        )
    } else {
        frame
            .h(px(THUMBNAIL))
            .max_w(px(220.))
            .pl_2()
            .pr_3()
            .flex()
            .items_center()
            .gap_2()
            .child(icon("file-text").text_color(p.subtle))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(font::MESSAGE))
                            .text_color(p.text)
                            .truncate()
                            .child(attachment.name()),
                    )
                    .child(
                        div()
                            .text_size(px(font::META))
                            .text_color(p.subtle)
                            .child(file_size(attachment.size)),
                    ),
            )
    }
}

/// A sent message's attachments; clicking one opens it with the system default. A remote host's
/// attachment is read through that host and opened as a copy, as its file links are.
pub(super) fn sent(message: &Message, p: Palette) -> Div {
    div().flex().flex_wrap().justify_end().gap_2().children(
        message.attachments.iter().enumerate().map(|(index, a)| {
            let path = a.path.clone();
            chip(
                SharedString::from(format!("sent-{}-{index}", message.id)),
                a,
                Stored::Sent,
                p,
            )
            .cursor_pointer()
            .on_click(move |_, window, cx| {
                let bridge = cx.global::<Hosts>().bridge();
                if bridge.remote().is_none() {
                    return cx.open_with_system(&path);
                }
                // This Mac decides from the copy whether it may be opened.
                let file = ResolvedPath {
                    path: path.clone(),
                    runs_when_opened: true,
                };
                file_links::open_file(&file, &bridge, window, cx)
            })
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::{Found, IMAGE_LIMIT, ImageCache, RETRY_AFTER, file_size};
    use crate::staging::{Hold, Staging};
    use std::time::Instant;
    use std::{fs, path::PathBuf};
    use workspace_core::{Attachment, MAX_ATTACHMENT_BYTES, ask};
    use workspace_host::attachments::inspect_all;

    fn add_dropped(attached: &mut Vec<Attachment>, paths: &[PathBuf]) -> Vec<String> {
        super::add_dropped(attached, paths, inspect_all(paths))
    }

    fn key(name: &str) -> super::ImageKey {
        (None, PathBuf::from(name))
    }

    #[test]
    fn a_failed_read_shows_its_error_then_is_read_again() {
        let mut cache = ImageCache::<u32>::new();
        let now = Instant::now();
        assert_eq!(cache.find(&key("a"), now), Found::Read);
        cache.reading(key("a"));
        assert_eq!(cache.find(&key("a"), now), Found::Reading, "read once");
        cache.read(key("a"), Err("The host is busy".into()), now);
        assert_eq!(
            cache.find(&key("a"), now),
            Found::Failed("The host is busy".into())
        );
        assert_eq!(cache.find(&key("a"), now + RETRY_AFTER), Found::Read);
        cache.reading(key("a"));
        cache.read(key("a"), Ok(7), now + RETRY_AFTER);
        assert_eq!(
            cache.find(&key("a"), now + RETRY_AFTER * 9),
            Found::Image(7)
        );
    }

    #[test]
    fn past_the_bound_the_image_shown_longest_ago_is_dropped() {
        let mut cache = ImageCache::<usize>::new();
        let now = Instant::now();
        for n in 0..IMAGE_LIMIT {
            cache.reading(key(&n.to_string()));
            assert!(cache.read(key(&n.to_string()), Ok(n), now).is_empty());
        }
        // The first is shown again, so the second is now the one shown longest ago.
        assert_eq!(cache.find(&key("0"), now), Found::Image(0));
        let dropped = cache.read(key("new"), Ok(IMAGE_LIMIT), now);
        assert_eq!(dropped, [1]);
        assert_eq!(
            cache.find(&key("1"), now),
            Found::Read,
            "read again if shown"
        );
        assert_eq!(cache.find(&key("0"), now), Found::Image(0));
        let other_host = (Some("studio".to_string()), PathBuf::from("0"));
        assert_eq!(cache.find(&other_host, now), Found::Read, "each host's own");
    }

    #[test]
    fn a_file_with_an_attached_name_is_refused_and_the_rest_attach() {
        let dir = tempfile::tempdir().unwrap();
        for folder in ["a", "b"] {
            fs::create_dir(dir.path().join(folder)).unwrap();
            fs::write(dir.path().join(folder).join("notes.txt"), folder).unwrap();
        }
        fs::write(dir.path().join("shot.png"), b"png").unwrap();
        let first = dir.path().join("a/notes.txt");
        let mut attached = vec![];
        assert!(add_dropped(&mut attached, std::slice::from_ref(&first)).is_empty());

        let refused = add_dropped(
            &mut attached,
            &[
                first,
                dir.path().join("b/notes.txt"),
                dir.path().join("shot.png"),
            ],
        );
        // The same file dropped again and another file with its name read alike.
        assert_eq!(
            refused,
            ["notes.txt: a file with this name is already attached"; 2]
        );
        let names: Vec<_> = attached.iter().map(|a| a.name()).collect();
        assert_eq!(names, ["notes.txt", "shot.png"]);
        assert_eq!(attached[0].path, dir.path().join("a/notes.txt"));

        fs::write(dir.path().join("Notes.txt"), b"capital").unwrap();
        fs::write(dir.path().join("plan.md"), b"plan").unwrap();
        let refused = add_dropped(
            &mut attached,
            &[dir.path().join("Notes.txt"), dir.path().join("plan.md")],
        );
        assert_eq!(
            refused,
            ["Notes.txt: a file with this name is already attached"]
        );
        let names: Vec<_> = attached.iter().map(|a| a.name()).collect();
        assert_eq!(names, ["notes.txt", "shot.png", "plan.md"]);
    }

    /// Drops `paths` as the composer does: each is staged, the host checks the copies, and the
    /// draft takes those it accepts, with their holds.
    fn drop_staged(
        staging: &Staging,
        attached: &mut Vec<Attachment>,
        paths: &[PathBuf],
    ) -> (Vec<String>, Vec<Hold>) {
        let staged: Vec<_> = (paths.iter())
            .map(|p| (p.clone(), staging.stage(p)))
            .collect();
        let copies: Vec<_> = (staged.iter())
            .filter_map(|(_, c)| Some(c.as_ref().ok()?.path().to_path_buf()))
            .collect();
        super::add_staged(attached, staged, inspect_all(&copies))
    }

    fn staged_files(home: &std::path::Path) -> usize {
        fs::read_dir(home.join("staged-attachments")).map_or(0, Iterator::count)
    }

    #[test]
    fn a_dropped_file_is_sent_from_its_copy_after_the_original_is_gone() {
        let home = tempfile::tempdir().unwrap();
        let staging = Staging::open(home.path());
        let dropped = tempfile::tempdir().unwrap();
        let shot = dropped.path().join("Screenshot.png");
        fs::write(&shot, b"\x89PNG\r\n\x1a\nshot").unwrap();

        let mut attached = vec![];
        let (refused, _held) = drop_staged(&staging, &mut attached, std::slice::from_ref(&shot));
        assert!(refused.is_empty());
        let copy = attached[0].path.clone();
        assert!(
            copy.starts_with(home.path().join("staged-attachments")),
            "{copy:?}"
        );
        assert_eq!(attached[0].name(), "Screenshot.png");
        assert!(attached[0].image.is_some());

        // As macOS withdraws a screenshot thumbnail's file once the drop is over.
        fs::remove_file(&shot).unwrap();
        let mut host = workspace_host::Host::open_set_up(home.path()).unwrap();
        let project = host.create_project("Pictures").unwrap();
        let coordinator = (host.sessions().unwrap().into_iter())
            .find(|s| s.project_id == project.id)
            .unwrap();
        let sent = host
            .send_with_files("m1", None, &coordinator.id, "Look", &[copy])
            .unwrap();
        assert_eq!(sent.attachments[0].name(), "Screenshot.png");
        assert_eq!(
            fs::read(&sent.attachments[0].path).unwrap(),
            b"\x89PNG\r\n\x1a\nshot"
        );
    }

    #[test]
    fn a_file_that_cannot_be_copied_is_refused_at_the_drop() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let staging = Staging::open(home.path());
        let locked = home.path().join("locked.png");
        fs::write(&locked, b"png").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let notes = home.path().join("notes.txt");
        fs::write(&notes, b"notes").unwrap();

        let mut attached = vec![];
        let (refused, _held) = drop_staged(&staging, &mut attached, &[locked, notes]);
        assert_eq!(refused.len(), 1);
        assert!(
            refused[0].starts_with("Could not copy locked.png: "),
            "{refused:?}"
        );
        let names: Vec<_> = attached.iter().map(|a| a.name()).collect();
        assert_eq!(names, ["notes.txt"]);
        assert_eq!(staged_files(home.path()), 1, "only the attached copy");
    }

    #[test]
    fn copies_left_out_of_the_draft_are_released() {
        let home = tempfile::tempdir().unwrap();
        let staging = Staging::open(home.path());
        let notes = home.path().join("notes.txt");
        fs::write(&notes, b"notes").unwrap();
        let mut attached = vec![];
        let (_, _held) = drop_staged(&staging, &mut attached, std::slice::from_ref(&notes));

        // The same file again, and a folder, which the host refuses and is never copied.
        let (refused, held) = drop_staged(
            &staging,
            &mut attached,
            &[notes, home.path().join("staged-attachments")],
        );
        assert_eq!(refused.len(), 2, "{refused:?}");
        assert!(held.is_empty());
        assert_eq!(attached.len(), 1);
        assert_eq!(staged_files(home.path()), 1);
        assert!(home.path().join("notes.txt").exists());
    }

    #[test]
    fn a_sent_image_is_read_through_the_host() {
        let png = b"\x89PNG\r\n\x1a\nnot really".to_vec();
        let message = std::cell::OnceCell::new();
        let dropped = tempfile::tempdir().unwrap();
        let shot = dropped.path().join("shot.png");
        fs::write(&shot, &png).unwrap();
        let served = crate::bridge::tests::serve(|host| {
            let project = host.create_project("Pictures").unwrap();
            let coordinator = (host.sessions().unwrap().into_iter())
                .find(|s| s.project_id == project.id)
                .unwrap();
            let sent = host
                .send_with_files("m1", None, &coordinator.id, "Look", &[shot])
                .unwrap();
            message.set(sent).unwrap();
        });
        let attachment = &message.get().unwrap().attachments[0];
        let data = fs::canonicalize(served.data.path()).unwrap();
        assert!(
            attachment.path.starts_with(data.join("projects")),
            "{attachment:?}"
        );
        let format = attachment.image.unwrap();
        let read = ask::read_file(attachment.path.clone(), MAX_ATTACHMENT_BYTES);
        let contents = served.bridge.ask_now(read).unwrap();
        let image = super::image(&contents, format).unwrap();
        assert_eq!(image.bytes(), png);
        // Only what the host serves: not a file outside its roots.
        let outside = ask::read_file("/etc/hosts".into(), MAX_ATTACHMENT_BYTES);
        assert!(served.bridge.ask_now(outside).is_err());
    }

    #[test]
    fn sizes_read_in_the_nearest_unit() {
        assert_eq!(file_size(0), "0 B");
        assert_eq!(file_size(1023), "1023 B");
        assert_eq!(file_size(14 * 1024 + 300), "14 KB");
        assert_eq!(file_size(3 * 1024 * 1024 + 200 * 1024), "3.2 MB");
    }
}
