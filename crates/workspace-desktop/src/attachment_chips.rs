//! Attached files: dropped into a draft, and shown as chips pending in the composer or
//! sent with a message.
use super::*;
use ui::icon;

const THUMBNAIL: f32 = 56.;

/// Adds dropped files to a draft's attachments and returns why any were refused. A file
/// already attached is skipped; another file with the same name is refused, since both
/// would be stored under that name.
pub(super) fn add_dropped(attached: &mut Vec<Attachment>, paths: &[PathBuf]) -> Vec<String> {
    let mut refused = vec![];
    for path in paths {
        if attached.iter().any(|a| &a.path == path) {
            continue;
        }
        match workspace_host::attachments::inspect(path) {
            Ok(file) if attached.iter().any(|a| a.name() == file.name()) => refused.push(format!(
                "{}: a file with this name is already attached",
                file.name()
            )),
            Ok(file) => attached.push(file),
            Err(error) => refused.push(error.to_string()),
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

/// A thumbnail for an image, otherwise the file's name and size.
pub(super) fn chip(id: impl Into<ElementId>, attachment: &Attachment, p: Palette) -> Stateful<Div> {
    let frame = div()
        .id(id)
        .relative()
        .flex_none()
        .rounded_lg()
        .border_1()
        .border_color(p.edge.opacity(0.7))
        .bg(p.overlay)
        .overflow_hidden();
    if attachment.image.is_some() {
        let unreadable = icon("file-text").text_color(p.subtle);
        frame.size(px(THUMBNAIL)).child(
            img(attachment.path.clone())
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
                            .text_size(px(12.5))
                            .text_color(p.text)
                            .truncate()
                            .child(attachment.name()),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(p.subtle)
                            .child(file_size(attachment.size)),
                    ),
            )
    }
}

/// A sent message's attachments; clicking one opens it with the system default.
pub(super) fn sent(message: &Message, p: Palette) -> Div {
    div().flex().flex_wrap().justify_end().gap_2().children(
        message.attachments.iter().enumerate().map(|(index, a)| {
            let path = a.path.clone();
            chip(
                SharedString::from(format!("sent-{}-{index}", message.id)),
                a,
                p,
            )
            .cursor_pointer()
            .on_click(move |_, _, cx| cx.open_with_system(&path))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::{add_dropped, file_size};
    use std::fs;

    #[test]
    fn a_second_file_with_an_attached_name_is_refused_and_the_rest_attach() {
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
        assert_eq!(
            refused,
            ["notes.txt: a file with this name is already attached"]
        );
        let names: Vec<_> = attached.iter().map(|a| a.name()).collect();
        assert_eq!(names, ["notes.txt", "shot.png"]);
        assert_eq!(attached[0].path, dir.path().join("a/notes.txt"));
    }

    #[test]
    fn sizes_read_in_the_nearest_unit() {
        assert_eq!(file_size(0), "0 B");
        assert_eq!(file_size(1023), "1023 B");
        assert_eq!(file_size(14 * 1024 + 300), "14 KB");
        assert_eq!(file_size(3 * 1024 * 1024 + 200 * 1024), "3.2 MB");
    }
}
