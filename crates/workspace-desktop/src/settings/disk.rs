//! Settings › Disk as the disk-use study draws it: free space with its floors, build output
//! against the budget, and source; the four settings that decide when build output is
//! reclaimed; and each ticket's use, grouped by project, with the deletes still waiting.
use super::*;
use crate::disk_use::{self, Tone};
use ui::pill;
use workspace_core::{DiskSettings, DiskUse, PendingDelete, TicketDiskUse};

/// Each setting's label, unit and caption, in the order the fields are drawn.
const FIELDS: [(&str, &str, &str); 4] = [
    (
        "Build output budget",
        "GB",
        "Over it, idle tickets' build output goes first, least recently used first.",
    ),
    (
        "Reclaim when idle for",
        "hours",
        "No agent turn in that time, or the ticket is accepted or closed.",
    ),
    (
        "Soft floor",
        "GB free",
        "Below it, idle build output is reclaimed before a build or turn starts.",
    ),
    (
        "Hard floor",
        "GB free",
        "Below it, new builds wait and you are told.",
    ),
];
const SOURCE_WIDTH: f32 = 64.;
const OUTPUT_WIDTH: f32 = 112.;
const TURN_WIDTH: f32 = 96.;
const STATE_WIDTH: f32 = 104.;
const ACTION_WIDTH: f32 = 104.;
/// The widest build output bar, for the largest ticket.
const BAR_WIDTH: f32 = 44.;

fn values(settings: &DiskSettings) -> [u32; 4] {
    [
        settings.budget_gb,
        settings.idle_hours,
        settings.soft_floor_gb,
        settings.hard_floor_gb,
    ]
}

/// The settings the four fields hold, or why they cannot be saved.
fn parse(texts: [&str; 4]) -> Result<DiskSettings, String> {
    let mut numbers = [0; 4];
    for (at, text) in texts.iter().enumerate() {
        numbers[at] = (text.trim().parse::<u32>())
            .map_err(|_| format!("{} must be a whole number", FIELDS[at].0))?;
    }
    let [budget_gb, idle_hours, soft_floor_gb, hard_floor_gb] = numbers;
    let settings = DiskSettings {
        budget_gb,
        idle_hours,
        soft_floor_gb,
        hard_floor_gb,
    };
    settings.validate().map_err(|error| error.to_string())?;
    Ok(settings)
}

/// The four disk settings' fields. A field saves all four as it loses focus or on Return.
pub(super) struct DiskForm {
    fields: [Entity<InputState>; 4],
    /// The settings the fields were last filled with.
    shown: Option<DiskSettings>,
    pub(super) error: Option<String>,
}
impl DiskForm {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Settings>) -> Self {
        let fields = std::array::from_fn(|_| cx.new(|cx| InputState::new(window, cx)));
        for field in &fields {
            cx.subscribe(field, |view: &mut Settings, _, event, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    view.save_disk_settings(cx);
                }
            })
            .detach();
        }
        Self {
            fields,
            shown: None,
            error: None,
        }
    }
    /// Shows the host's settings once they arrive or change.
    pub(super) fn fill(&mut self, settings: DiskSettings, window: &mut Window, cx: &mut App) {
        if self.shown == Some(settings) {
            return;
        }
        self.shown = Some(settings);
        for (field, value) in self.fields.iter().zip(values(&settings)) {
            field.update(cx, |input, cx| {
                input.set_value(value.to_string(), window, cx)
            });
        }
    }
}

impl Settings {
    fn save_disk_settings(&mut self, cx: &mut Context<Self>) {
        let texts = self
            .disk
            .fields
            .clone()
            .map(|f| f.read(cx).value().to_string());
        match parse(texts.each_ref().map(String::as_str)) {
            Err(error) => {
                self.disk.error = Some(error);
                cx.notify();
            }
            Ok(settings) if self.disk.shown == Some(settings) => {
                self.disk.error = None;
                cx.notify();
            }
            Ok(settings) => self.ask_host(ask::set_disk_settings(settings), cx),
        }
    }

    /// The whole Disk section: the measurement's tiles, the settings, and each ticket's use.
    pub(super) fn disk_section(&self, p: Palette, window: &Window, cx: &mut Context<Self>) -> Div {
        let handle = window.window_handle();
        let reclaim_all = button("reclaim-idle-now")
            .xsmall()
            .outline()
            .label("Reclaim idle build output now")
            .map(|b| ui::gated(b, &self.driving))
            .on_click(move |_, _, cx| {
                disk_use::ask(ask::reclaim_build_output(None), Some(handle), cx)
            });
        let mut body = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(section("DISK", p).child(reclaim_all));
        let disk = disk_use::current(cx).cloned();
        let settings = self.host.as_ref().map(|h| h.disk);
        match (&disk, settings) {
            (Some(disk), Some(settings)) => body = body.child(tiles(disk, &settings, p)),
            _ => body = body.child(hint("Measuring disk use…", p)),
        }
        body = body.child(self.disk_fields(p));
        if let Some(disk) = &disk {
            body = body.child(self.disk_table(disk, p, window, cx));
        }
        body
    }

    fn disk_fields(&self, p: Palette) -> Div {
        let mut fields = div().flex().gap_3();
        for ((label, unit, caption), field) in FIELDS.into_iter().zip(&self.disk.fields) {
            fields = fields.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(hint(label, p))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .child(div().w(px(64.)).child(Input::new(field).small()))
                            .child(hint(unit, p)),
                    )
                    .child(hint(caption, p).text_size(px(font::META))),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(fields)
            .children((self.disk.error.clone()).map(|error| hint(error, p).text_color(p.red)))
    }

    /// Each project's tickets, largest first, then the deletes still waiting.
    fn disk_table(&self, disk: &DiskUse, p: Palette, window: &Window, cx: &App) -> Div {
        let index = main_window_index(cx);
        let largest = (disk.tickets.iter()).map(|t| t.build_output).max();
        let mut table = div().flex().flex_col().child(header_row(p));
        for group in disk_use::groups(disk) {
            let label = group_label(index.as_deref(), group.project_id, &group.tickets);
            table = table.child(group_row(label, p));
            for ticket in group.tickets {
                let row = self.ticket_row(index.as_deref(), ticket, largest, p, window);
                table = table.child(row);
            }
        }
        if !disk.deletes.is_empty() {
            table = table.child(group_row("Removed worktrees still being deleted".into(), p));
            let on_this_mac = self.bridge.remote().is_none();
            for delete in &disk.deletes {
                table = table.child(delete_row(delete, on_this_mac, p));
            }
        }
        table
    }

    fn ticket_row(
        &self,
        index: Option<&WorkspaceIndex>,
        disk: &TicketDiskUse,
        largest: Option<u64>,
        p: Palette,
        window: &Window,
    ) -> Div {
        let ticket = index.and_then(|index| index.ticket(&disk.ticket_id));
        let worktree = ticket.map_or("", |t| t.worktree.as_str());
        let pr_open = ticket
            .and_then(|t| t.pull_request.as_ref())
            .is_some_and(|pr| pr.state == PrState::Open);
        let (state, tone) = match disk_use::state_label(disk) {
            (_, Tone::Quiet) if pr_open => ("PR open".to_owned(), Tone::Quiet),
            label => label,
        };
        let bar = largest
            .filter(|largest| *largest > 0)
            .map_or(0., |largest| {
                disk.build_output as f32 / largest as f32 * BAR_WIDTH
            });
        let handle = window.window_handle();
        let ticket_id = disk.ticket_id.clone();
        let action = disk_use::can_reclaim(disk).then(|| {
            button(SharedString::from(format!("reclaim-{ticket_id}")))
                .xsmall()
                .outline()
                .label("Reclaim")
                .map(|b| ui::gated(b, &self.driving))
                .on_click(move |_, _, cx| {
                    let ask = ask::reclaim_build_output(Some(ticket_id.clone()));
                    disk_use::ask(ask, Some(handle), cx)
                })
        });
        let title = div()
            .flex_1()
            .min_w_0()
            .child(div().text_ellipsis().child(disk.title.clone()))
            .children(
                disk_use::where_line(disk, worktree)
                    .map(|line| hint(line, p).text_size(px(font::META)).text_ellipsis()),
            );
        let output = div()
            .flex()
            .items_center()
            .justify_end()
            .gap_1p5()
            .child(
                div()
                    .w(px(bar))
                    .h(px(6.))
                    .rounded_full()
                    .bg(p.accent.opacity(0.8)),
            )
            .child(disk_use::size(disk.build_output));
        row(p)
            .child(title)
            .child(number(SOURCE_WIDTH).child(disk_use::size(disk.source)))
            .child(number(OUTPUT_WIDTH).child(output))
            .child(column(TURN_WIDTH).child(disk.last_turn_at.map_or("—".into(), wording::age)))
            .child(column(STATE_WIDTH).child(pill(state, tone_color(tone, p))))
            .child(cell(ACTION_WIDTH).children(action))
    }
}

/// The three tiles: free space with the floors marked, build output against the budget, and
/// source.
fn tiles(disk: &DiskUse, settings: &DiskSettings, p: Palette) -> Div {
    let share = |part: u64, whole: u64| match whole {
        0 => 0.,
        whole => (part as f32 / whole as f32).min(1.),
    };
    let free_color = if disk.free < settings.hard_floor() {
        p.red
    } else if disk.free < settings.soft_floor() {
        p.yellow
    } else {
        p.green
    };
    let free = tile(
        "Free on this Mac's disk",
        disk_use::size(disk.free),
        format!("of {}", disk_use::size(disk.total)),
        meter(share(disk.free, disk.total), free_color, p)
            .child(mark(share(settings.soft_floor(), disk.total), p.yellow))
            .child(mark(share(settings.hard_floor(), disk.total), p.red)),
        hint(
            format!(
                "hard floor {} GB · soft floor {} GB",
                settings.hard_floor_gb, settings.soft_floor_gb
            ),
            p,
        ),
        p,
    );
    let (budget, over) = disk_use::budget_line(disk, settings);
    let output_color = if over { p.red.opacity(0.75) } else { p.accent };
    let output = tile(
        "Build output, all tickets",
        disk_use::size(disk.build_output),
        format!("budget {} GB", settings.budget_gb),
        meter(share(disk.build_output, settings.budget()), output_color, p),
        hint(budget, p).when(over, |d| d.text_color(p.red)),
        p,
    );
    let source = tile(
        "Source and checkouts",
        disk_use::size(disk.source),
        wording::counted(disk.tickets.len(), "worktree"),
        meter(share(disk.source, disk.total), p.focus, p),
        hint("never reclaimed", p),
        p,
    );
    div().flex().gap_3().child(free).child(output).child(source)
}

fn tile(
    label: &'static str,
    figure: String,
    of: String,
    meter: Div,
    legend: Div,
    p: Palette,
) -> Div {
    card(p)
        .flex_1()
        .min_w_0()
        .gap_1()
        .child(hint(label, p))
        .child(
            div()
                .flex()
                .items_baseline()
                .gap_1p5()
                .child(div().text_size(px(font::FIGURE)).child(figure))
                .child(hint(of, p)),
        )
        .child(meter)
        .child(legend.text_size(px(font::META)))
}

/// A track filled to `share` in `color`; marks are its children.
fn meter(share: f32, color: Hsla, p: Palette) -> Div {
    div()
        .relative()
        .my_1()
        .h(px(10.))
        .rounded(px(5.))
        .bg(p.overlay)
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(relative(share))
                .rounded(px(5.))
                .bg(color),
        )
}

/// A floor's mark on a meter, at `share` of its width.
fn mark(share: f32, color: Hsla) -> Div {
    div()
        .absolute()
        .left(relative(share))
        .top(px(-3.))
        .bottom(px(-3.))
        .w(px(2.))
        .bg(color)
}

fn tone_color(tone: Tone, p: Palette) -> Hsla {
    match tone {
        Tone::Busy => p.focus,
        Tone::Quiet => p.subtle,
        Tone::Working => p.yellow,
        Tone::Done => p.green,
        Tone::Failing => p.red,
    }
}

fn row(p: Palette) -> Div {
    div()
        .flex()
        .items_start()
        .gap_2()
        .px_2()
        .py(px(7.))
        .border_b_1()
        .border_color(p.edge.opacity(edge::HAIRLINE))
        .text_size(px(font::SMALL))
}
fn column(width: f32) -> Div {
    div().w(px(width)).flex_none().flex()
}
/// A right-aligned column.
fn cell(width: f32) -> Div {
    column(width).justify_end()
}
fn number(width: f32) -> Div {
    cell(width).whitespace_nowrap()
}

fn header_row(p: Palette) -> Div {
    let head = |text: &'static str| hint(text, p).text_size(px(font::CAPTION));
    row(p)
        .border_color(p.edge)
        .child(div().flex_1().child(head("Ticket")))
        .child(cell(SOURCE_WIDTH).child(head("Source")))
        .child(cell(OUTPUT_WIDTH).child(head("Build output")))
        .child(column(TURN_WIDTH).child(head("Last agent turn")))
        .child(column(STATE_WIDTH).child(head("State")))
        .child(cell(ACTION_WIDTH))
}

fn group_row(label: String, p: Palette) -> Div {
    div()
        .px_2()
        .py(px(5.))
        .bg(p.base)
        .text_size(px(font::CAPTION))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.subtle)
        .child(label)
}

/// "Wiffletree: Build Fixes · Vyttle-LLC/wiffletree": the project, and its repository when
/// every ticket listed is in the same one.
fn group_label(
    index: Option<&WorkspaceIndex>,
    project_id: &str,
    tickets: &[&TicketDiskUse],
) -> String {
    let project = index.and_then(|index| index.project(project_id));
    let name = project.map_or_else(|| "Project".to_owned(), |p| p.name.clone());
    let repository_id = &tickets[0].repository_id;
    if tickets.iter().any(|t| &t.repository_id != repository_id) {
        return name;
    }
    let repositories = index.map_or(&[][..], |index| &index.snapshot().repositories);
    match repositories.iter().find(|r| &r.id == repository_id) {
        Some(r) => match &r.github {
            Some(github) => format!("{name} · {}/{}", github.owner, github.name),
            None => format!("{name} · {}", r.name),
        },
        None => name,
    }
}

fn delete_row(delete: &PendingDelete, on_this_mac: bool, p: Palette) -> Div {
    let shown = repository_tree::shown_path(&delete.path, spaces_view::home().as_deref());
    let (state, color) = match delete.failing {
        true => ("delete failing", p.red),
        false => ("deleting", p.subtle),
    };
    let path = std::path::PathBuf::from(&delete.path);
    let finder = on_this_mac.then(|| {
        button(SharedString::from(format!("show-{}", delete.path)))
            .xsmall()
            .outline()
            .label("Show in Finder")
            .on_click(move |_, _, cx| cx.reveal_path(&path))
    });
    row(p)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(mono(shown, p).text_ellipsis())
                .child(hint(disk_use::delete_line(delete), p).text_size(px(font::META))),
        )
        .child(number(SOURCE_WIDTH).child("—"))
        .child(number(OUTPUT_WIDTH).child("—"))
        .child(column(TURN_WIDTH).child("—"))
        .child(column(STATE_WIDTH).child(pill(state, color)))
        .child(cell(ACTION_WIDTH).children(finder))
}

/// The index of the workspace the main window shows, which is the host this window drives.
fn main_window_index(cx: &App) -> Option<Rc<WorkspaceIndex>> {
    let workspace = (cx.try_global::<MainWindow>())
        .and_then(|main| main.0.clone())
        .and_then(|(_, workspace)| workspace.upgrade())?;
    workspace.read(cx).shared.index.clone()
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn the_fields_save_only_whole_numbers_the_host_would_accept() {
        let saved = parse(["100", " 24 ", "25", "5"]).unwrap();
        assert_eq!(
            (
                saved.budget_gb,
                saved.idle_hours,
                saved.soft_floor_gb,
                saved.hard_floor_gb
            ),
            (100, 24, 25, 5)
        );
        assert_eq!(
            parse(["100 GB", "24", "25", "5"]).unwrap_err(),
            "Build output budget must be a whole number"
        );
        assert_eq!(
            parse(["100", "24", "5", "5"]).unwrap_err(),
            "The soft floor must be above the hard floor"
        );
        assert!(parse(["100", "0", "25", "5"]).is_err());
    }
}
