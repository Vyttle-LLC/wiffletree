//! A Space's page's Repository box (OpenSpec change `one-space-repository`, screens 4 and 5 of
//! its mockup): where its issues, logs and wiki are, who can read them and how they compile,
//! with Sync now, Import from Linear…, Bring in a brain… and Move into the folder. Bring in reads
//! a brain folder on this Mac and never writes there.
use super::*;

const BRING_IN_SCANNED: &str = "Scanned on this Mac before anything is pushed.";
/// After Bring in, how long the brain's index gets before the box reads its counts again.
const AFTER_BRING_IN: std::time::Duration = std::time::Duration::from_secs(5);

/// Bring in a brain…, open for one Space: the clone chosen, what it would bring, and what it did.
pub(super) struct BringingIn {
    space: SpaceId,
    from: Option<String>,
    plan: Option<Result<BringInPlan, String>>,
    done: Option<BringInPlan>,
}

impl SpacesPage {
    /// Opens Bring in a brain… for the Space, filled in from the old brain an earlier release
    /// named, when this Mac knows where its clone is.
    fn open_bring_in(&mut self, space: &Space, window: &mut Window, cx: &mut Context<Self>) {
        let from = self
            .local(&space.id)
            .and_then(|local| local.brain_path.clone());
        let folder = (space.brain.as_ref())
            .map_or(SpaceBrain::DEFAULT_FOLDER.to_owned(), |b| b.folder.clone());
        self.brain_folder_input
            .update(cx, |input, cx| input.set_value(folder, window, cx));
        self.bringing_in = Some(BringingIn {
            space: space.id.clone(),
            from,
            plan: None,
            done: None,
        });
        self.preview_bring_in(window, cx);
    }

    /// Asks where the brain's clone is on this Mac.
    fn choose_brain_clone(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = folders::choose(
            &self.bridge,
            "Where is the brain cloned?",
            false,
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let chosen = paths.recv().await.ok().and_then(|p| p.into_iter().next());
            let _ = cx.update(|window, cx| {
                this.update(cx, |view, cx| {
                    let (Some(path), Some(bringing)) = (chosen, view.bringing_in.as_mut()) else {
                        return;
                    };
                    bringing.from = Some(path.to_string_lossy().into_owned());
                    view.preview_bring_in(window, cx);
                })
            });
        })
        .detach();
    }

    /// Reads what Bring in would bring, writing nothing anywhere.
    fn preview_bring_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let folder = self.brain_folder_input.read(cx).value().trim().to_owned();
        let Some(bringing) = self.bringing_in.as_mut() else {
            return;
        };
        let Some(from) = bringing.from.clone() else {
            return cx.notify();
        };
        bringing.plan = None;
        bringing.done = None;
        let ask = ask::preview_bring_in(bringing.space.clone(), from, folder);
        self.ask(ask, window, cx, |view, plan, _, cx| {
            if let Some(bringing) = view.bringing_in.as_mut() {
                bringing.plan = Some(plan);
            }
            cx.notify();
        });
        cx.notify();
    }

    fn bring_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let folder = self.brain_folder_input.read(cx).value().trim().to_owned();
        let Some(bringing) = &self.bringing_in else {
            return;
        };
        let Some(from) = bringing.from.clone() else {
            return;
        };
        self.busy = true;
        let ask = ask::bring_in(bringing.space.clone(), from, folder);
        self.ask(ask, window, cx, |view, done, window, cx| {
            view.busy = false;
            match (done, view.bringing_in.as_mut()) {
                (Ok(done), Some(bringing)) => bringing.done = Some(done),
                (Err(error), Some(bringing)) => bringing.plan = Some(Err(error)),
                _ => {}
            }
            view.read_storage(window, cx);
            // The box counts what the brain's index holds, which catches up once the files land.
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(AFTER_BRING_IN).await;
                let _ = cx
                    .update(|window, cx| this.update(cx, |view, cx| view.read_storage(window, cx)));
            })
            .detach();
        });
        cx.notify();
    }

    pub(super) fn repository_box(
        &self,
        space: &Space,
        status: Option<&Result<StorageStatus, String>>,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut head = div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Repository"));
        let status = match status {
            Some(Ok(status)) => status,
            Some(Err(error)) => {
                return card(p)
                    .child(head)
                    .child(hint(error.clone(), p).text_color(p.red));
            }
            None => return card(p).child(head).child(hint("Reading…", p)),
        };
        if let Some((line, failed)) = sync_line(status) {
            head = head
                .child(div().flex_1())
                .child(hint(line, p).text_color(if failed { p.red } else { p.green }));
        }
        let others: Vec<&str> = (self.spaces().iter())
            .filter(|s| s.id != space.id && s.storage.repository == space.storage.repository)
            .map(|s| s.name.as_str())
            .collect();
        let today = chrono::Local::now().date_naive();
        let mut facts = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(fact("Where", where_line(space), p))
            .child(fact(
                "Who can read",
                readers_line(space, status.readers, status.reader_count),
                p,
            ))
            .child(fact("Issues", self.issues_field(space, status, p, cx), p))
            .child(fact("Logs", logs_line(status), p))
            .child(fact("Wiki", wiki_line(status, today), p))
            .child(fact("Compile", self.compile_line(status, p, cx), p));
        if let Some(line) = shared_with(&others, status.prefix.as_deref()) {
            facts = facts.child(fact("Shared with", line, p));
        }
        if status.settings_repository {
            facts = facts.child(fact("Also used for", ALSO_SETTINGS, p));
        }
        let mut box_ = card(p).child(head).child(facts);
        if let Some(behind) = &status.wiki_behind {
            box_ = box_.child(self.behind_banner(space, status, behind, p, cx));
        }
        if let Some(brain) = &space.brain {
            box_ = box_.child(hint(old_brain_line(brain), p).text_color(p.yellow));
        }
        if is_shared(space) {
            box_ = box_.child(hint(SCANS, p));
        }
        box_ = box_.child(self.repository_actions(space, status, cx));
        let panel = (self.linear.as_ref())
            .filter(|(shown, _)| shown == &space.id)
            .map(|(_, panel)| panel.clone());
        let bringing = (self.bringing_in.as_ref())
            .filter(|bringing| bringing.space == space.id)
            .map(|bringing| self.bring_in_panel(bringing, p, cx));
        box_.children(panel).children(bringing)
    }

    /// Daily at 06:00 · on this Mac and 1 other · Change on Brains →.
    fn compile_line(&self, status: &StorageStatus, p: Palette, cx: &mut Context<Self>) -> Div {
        let change = button("change-on-brains")
            .link()
            .xsmall()
            .label("Change on Brains →")
            .on_click(cx.listener(|_, _, _, cx| cx.emit(Intent::OpenPage(Page::Brains))));
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .child(compile_words(status))
            .child(hint("·", p))
            .child(change)
    }

    /// No Mac compiles the wiki, with Compile on this Mac.
    fn behind_banner(
        &self,
        space: &Space,
        status: &StorageStatus,
        behind: &str,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let offer = (status.compilers == 0 && is_shared(space)).then(|| {
            let space_id = space.id.clone();
            ui::gated(
                button("compile-here").xsmall().label("Compile on this Mac"),
                &self.shared.host,
            )
            .on_click(cx.listener(move |view, _, window, cx| {
                let command = Command::SetCompileHere {
                    space_id: space_id.clone(),
                    compiles: true,
                };
                view.request(command, window, cx);
                view.read_storage(window, cx);
            }))
        });
        banner(p.yellow)
            .child(div().flex_1().min_w_0().child(behind.to_owned()))
            .children(offer)
    }

    fn repository_actions(
        &self,
        space: &Space,
        status: &StorageStatus,
        cx: &mut Context<Self>,
    ) -> Div {
        let sync_now = is_shared(space).then(|| {
            ui::gated(
                button("space-sync-now").label("Sync now"),
                &self.shared.host,
            )
            .on_click(cx.listener(|view, _, window, cx| view.request(Command::SyncNow, window, cx)))
        });
        let importing = (self.linear.as_ref()).is_some_and(|(shown, _)| shown == &space.id);
        let import = (status.prefix.is_some() && !importing).then(|| {
            let space_id = space.id.clone();
            ui::gated(
                button("space-import-linear").label("Import from Linear…"),
                &self.shared.host,
            )
            .on_click(cx.listener(move |view, _, window, cx| {
                view.open_linear_import(space_id.clone(), window, cx)
            }))
        });
        let bringing = (self.bringing_in.as_ref()).is_some_and(|b| b.space == space.id);
        let bring_in = (!bringing).then(|| {
            let shown = space.clone();
            ui::gated(
                button("space-bring-in").label("Bring in a brain…"),
                &self.shared.host,
            )
            .on_click(
                cx.listener(move |view, _, window, cx| view.open_bring_in(&shown, window, cx)),
            )
        });
        // To another repository, the folder and every Space here in it.
        let moved = space.clone();
        let move_storage =
            ui::gated(
                button("space-move-storage").label("Move…"),
                &self.shared.host,
            )
            .on_click(cx.listener(move |view, _, window, cx| {
                view.open_move_storage(moved.clone(), window, cx)
            }));
        // The repository's own files still at its root: one commit moves them into the folder.
        let move_into = (is_shared(space) && space.storage.root.is_empty()).then(|| {
            let space_id = space.id.clone();
            ui::gated(
                button("space-move-into-folder")
                    .label(format!("Move into {DEFAULT_ROOT}/"))
                    .disabled(self.busy),
                &self.shared.host,
            )
            .tooltip(MOVE_INTO_FOLDER)
            .on_click(cx.listener(move |view, _, window, cx| {
                view.confirm_move_into_folder(space_id.clone(), window, cx)
            }))
        });
        let open_issues = status.prefix.is_some().then(|| {
            let space_id = space.id.clone();
            button("space-open-issues")
                .ghost()
                .label("Open Issues")
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(Intent::OpenIssues(Some(space_id.clone())))
                }))
        });
        div()
            .flex()
            .flex_wrap()
            .gap_2()
            .children(sync_now)
            .children(import)
            .children(bring_in)
            .child(move_storage)
            .children(move_into)
            .child(div().flex_1())
            .children(open_issues)
    }

    /// Asks first: the move is one pushed commit that stops older hosts until they update.
    fn confirm_move_into_folder(
        &mut self,
        space_id: SpaceId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.weak_entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (weak, space_id) = (weak.clone(), space_id.clone());
            dialog
                .title(format!("Move into {DEFAULT_ROOT}/?"))
                .confirm()
                .ok_text("Move")
                .child(MOVE_INTO_FOLDER)
                .on_ok(move |_, window, cx| {
                    let space_id = space_id.clone();
                    let _ = weak.update(cx, |view, cx| {
                        view.busy = true;
                        view.ask(
                            ask::move_into_folder(space_id),
                            window,
                            cx,
                            |view, moved, window, cx| {
                                view.busy = false;
                                view.error = moved.err();
                                view.read_storage(window, cx);
                            },
                        );
                    });
                    true
                })
        });
    }

    /// Bring in a brain: from where, what it brings and leaves, and the warning for files that
    /// are only on this Mac.
    fn bring_in_panel(&self, bringing: &BringingIn, p: Palette, cx: &mut Context<Self>) -> Div {
        let from = (bringing.from.as_deref())
            .map_or("Choose the brain’s clone on this Mac".to_owned(), |from| {
                shown_path(from, home().as_deref())
            });
        let mut panel =
            div()
                .mt_2()
                .p_3()
                .rounded_md()
                .border_1()
                .border_color(p.edge)
                .bg(p.base)
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Bring in a brain"),
                )
                .child(row(
                    "From",
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().min_w_0().child(mono(from, p)))
                        .child(
                            button("bring-in-choose")
                                .xsmall()
                                .label("Choose…")
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.choose_brain_clone(window, cx)
                                })),
                        ),
                    None,
                    p,
                ))
                .child(row(
                    "Folder",
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .w(px(160.))
                                .child(Input::new(&self.brain_folder_input).small()),
                        )
                        .child(
                            button("bring-in-read")
                                .xsmall()
                                .ghost()
                                .label("Read again")
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.preview_bring_in(window, cx)
                                })),
                        ),
                    None,
                    p,
                ));
        if let Some(done) = &bringing.done {
            return panel
                .child(banner(p.green).child(div().flex_1().min_w_0().child(brought_line(done))))
                .child(div().flex().justify_end().child(self.close_bring_in(cx)));
        }
        match &bringing.plan {
            None if bringing.from.is_some() => panel = panel.child(hint("Reading the brain…", p)),
            None => {}
            Some(Err(error)) => panel = panel.child(hint(error.clone(), p).text_color(p.red)),
            Some(Ok(plan)) => {
                let brings =
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children(brings_lines(plan).into_iter().map(|line| {
                            div()
                                .flex()
                                .gap_2()
                                .child(div().text_color(p.green).child("✓"))
                                .child(line)
                        }));
                panel = panel.child(row("Brings in", brings, None, p)).child(row(
                    "Leaves behind",
                    hint(leaves_line(plan), p).text_color(p.text),
                    None,
                    p,
                ));
                for line in plan_notes(plan) {
                    panel = panel.child(hint(line, p));
                }
                if let Some(warning) = not_on_github_line(plan) {
                    panel = panel
                        .child(banner(p.yellow).child(div().flex_1().min_w_0().child(warning)));
                }
            }
        }
        let ready = matches!(bringing.plan, Some(Ok(_)));
        panel.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().flex_1().child(hint(BRING_IN_SCANNED, p)))
                .child(self.close_bring_in(cx))
                .child(
                    ui::gated(
                        button("bring-in")
                            .primary()
                            .label("Bring in")
                            .disabled(!ready || self.busy),
                        &self.shared.host,
                    )
                    .on_click(cx.listener(|view, _, window, cx| view.bring_in(window, cx))),
                ),
        )
    }

    fn close_bring_in(&self, cx: &mut Context<Self>) -> Button {
        button("bring-in-close")
            .ghost()
            .label("Close")
            .on_click(cx.listener(|view, _, _, cx| {
                view.bringing_in = None;
                cx.notify();
            }))
    }
}

const MOVE_INTO_FOLDER: &str = "One commit moves this repository's Wiffletree files into wiffletree/, leaving a stub that tells a teammate on an older Wiffletree to update. Nothing else in the repository changes.";

/// `owner/name · wiffletree/`, or this Mac's own repository.
pub(super) fn where_line(space: &Space) -> String {
    let folder = shown_folder(&space.storage.root);
    match &space.storage.repository {
        StorageRepository::Local { .. } => format!("Only on this Mac · {folder} · no remote"),
        repository => format!("{} · {folder}", repository.key()),
    }
}

/// 214 entries · 9 waiting for the next compile.
pub(super) fn logs_line(status: &StorageStatus) -> String {
    let entries = counted_as(status.entries, "entry", "entries");
    match status.waiting {
        0 => format!("{entries} · all compiled"),
        waiting => format!("{entries} · {waiting} waiting for the next compile"),
    }
}

/// 10 pages · compiled today at 06:04 on Michael's Mac.
pub(super) fn wiki_line(status: &StorageStatus, today: chrono::NaiveDate) -> String {
    let pages = counted(status.wiki_pages, "page");
    match &status.compiled {
        Some((at, machine)) => {
            let when = wording::local(*at).map_or(String::new(), |at| wording::day_at(at, today));
            format!("{pages} · compiled {} on {machine}", when.to_lowercase())
        }
        None => format!("{pages} · no compile yet"),
    }
}

/// Daily at 06:00 · on this Mac and 1 other, or who compiles it when this Mac does not.
pub(super) fn compile_words(status: &StorageStatus) -> String {
    let time = match &status.compile {
        Some(compile) => format!("Daily at {}", compile.at),
        None => "On Run now only".to_owned(),
    };
    let who = match (status.compiles_here, status.compilers) {
        (true, 0 | 1) => "on this Mac".to_owned(),
        (true, 2) => "on this Mac and 1 other".to_owned(),
        (true, n) => format!("on this Mac and {} others", n - 1),
        (false, 0) => "on no Mac".to_owned(),
        (false, n) => format!("on {}", counted(n, "Mac")),
    };
    format!("{time} · {who}")
}

/// What an earlier release named as this Space's brain, which Bring in copies.
fn old_brain_line(brain: &SpaceBrain) -> String {
    let named = match &brain.repository {
        BrainRepository::GitHub { owner, name, .. } => format!("{owner}/{name}"),
        BrainRepository::Local { .. } => "a folder on this Mac".into(),
    };
    format!(
        "An earlier release kept this Space’s brain in {named}. It takes no new entries; Bring in a brain… copies it here."
    )
}

/// What Bring in brings: the pages and STATUS.md, and the entries with how many files hold them.
pub(super) fn brings_lines(plan: &BringInPlan) -> Vec<String> {
    let pages = counted(plan.pages, "wiki page");
    let pages = match plan.status {
        true => format!("{pages} and STATUS.md"),
        false => pages,
    };
    let entries = counted_as(plan.entries, "log entry", "log entries");
    let files = counted(plan.entry_files, "file");
    vec![
        pages,
        format!("{entries} from {files}, each with its date and repository"),
    ]
}

/// What stays in the original.
pub(super) fn leaves_line(plan: &BringInPlan) -> String {
    let others = match plan.left_behind.as_slice() {
        [] => String::new(),
        [one] => format!(", along with {one}"),
        [rest @ .., last] => format!(", along with {} and {last}", rest.join(", ")),
    };
    format!("The original folder is left as it is{others}. You can delete it once you’re happy.")
}

/// Entries shortened, entries already here, compiled ones and pages left as they are.
pub(super) fn plan_notes(plan: &BringInPlan) -> Vec<String> {
    let mut notes = vec![];
    if plan.shortened > 0 {
        notes.push(format!(
            "{} shortened to fit the 2,000-character cap. Their full text stays in the original.",
            counted_as(plan.shortened, "entry is", "entries are")
        ));
    }
    if plan.already > 0 {
        notes.push(format!(
            "{} already here, so bringing them in adds nothing.",
            counted_as(plan.already, "file is", "files are")
        ));
    }
    if plan.compiled > 0 {
        notes.push(format!(
            "{} the original had compiled stay compiled here.",
            counted_as(plan.compiled, "entry", "entries")
        ));
    }
    if !plan.skipped.is_empty() {
        notes.push(format!(
            "Left as they are, as this folder has them with other text or they don’t fit the entry contract: {}.",
            plan.skipped.join(", ")
        ));
    }
    notes
}

/// The warning for files only this Mac has.
pub(super) fn not_on_github_line(plan: &BringInPlan) -> Option<String> {
    if plan.not_on_github == 0 {
        return None;
    }
    let files = match plan.not_on_github {
        1 => "1 of these files isn’t".to_owned(),
        n => format!("{n} of these files aren’t"),
    };
    let never = match plan.never_committed {
        0 => String::new(),
        1 => ", and 1 has never been committed".to_owned(),
        n => format!(", and {n} have never been committed"),
    };
    Some(format!(
        "{files} on GitHub yet. They exist only in this Mac’s clone{never}. Bringing in reads this Mac’s copy, so none of them are lost."
    ))
}

/// What a finished Bring in did.
pub(super) fn brought_line(plan: &BringInPlan) -> String {
    let entries = counted_as(plan.entries, "log entry", "log entries");
    let pages = counted(plan.pages, "wiki page");
    format!(
        "Brought in {entries} and {pages}. They land on main as they pass the scan; a file that fails stays on this Mac with its card."
    )
}
