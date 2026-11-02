//! New Space and Add Space from a repository (OpenSpec change `one-space-repository`, screens 1,
//! 3 and 6 of its mockup): one form each, the read-only check of a GitHub repository with
//! Protect history, and nothing written until Create Space or Add Space.
use super::*;

/// The compile time New Space proposes, in this Mac's timezone.
const DEFAULT_TIME: &str = "06:00";
const PROTECT_HISTORY: &str = "Protect history blocks force-pushes and deleting main. It doesn't require pull requests. It needs a repository admin.";

/// New Space's repository: a new one `gh` creates, or a private one the human already has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RepositoryChoice {
    Create,
    Existing,
}

/// One line of a repository's check: passed, a warning, or why it can't be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mark {
    Passed,
    Warning,
    Refused,
}

impl SpacesPage {
    /// Fills New Space as it opens: the owners to offer, the human's own account first, this
    /// Mac's timezone and the first color no Space shows.
    pub(super) fn open_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.choice = RepositoryChoice::Create;
        self.compile_here = true;
        self.new_color = SpaceColor::first_unused(self.spaces());
        self.prefix_proposed = None;
        self.repository_name_proposed = None;
        self.owner = None;
        self.folder_input.update(cx, |input, cx| {
            input.set_value(format!("{DEFAULT_ROOT}/"), window, cx)
        });
        self.time_input
            .update(cx, |input, cx| input.set_value(DEFAULT_TIME, window, cx));
        self.ask(
            ask::space_form_defaults(),
            window,
            cx,
            |view, defaults, window, cx| {
                let Ok(defaults) = defaults else {
                    return;
                };
                view.owner = defaults.owners.first().map(|owner| owner.login.clone());
                view.on_github = view.owner.is_some();
                view.timezone_input.update(cx, |input, cx| {
                    input.set_value(defaults.timezone.clone(), window, cx)
                });
                view.defaults = Some(defaults);
                cx.notify();
            },
        );
    }

    /// Proposes the prefix and the repository's name from the Space's name as the human types
    /// it, each until they type their own.
    pub(super) fn name_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shown != Some(Shown::New) {
            return;
        }
        let name = self.name.read(cx).value().trim().to_owned();
        let taken: Vec<String> = (self.shared.snapshot().into_iter())
            .flat_map(|s| s.issue_trackers.iter().filter_map(|t| t.prefix.clone()))
            .collect();
        let prefix = (!name.is_empty()).then(|| propose_prefix(&name, &taken));
        follow(
            &self.prefix_input,
            &mut self.prefix_proposed,
            prefix,
            window,
            cx,
        );
        let repository = Some(proposed_repository_name(&name));
        let input = &self.repository_name_input;
        follow(
            input,
            &mut self.repository_name_proposed,
            repository,
            window,
            cx,
        );
        cx.notify();
    }

    /// The repository Create makes, `owner/name`, once gh is signed in.
    fn repository_to_create(&self, cx: &App) -> Option<Result<String, String>> {
        let owner = self.owner.as_deref()?;
        let name = self
            .repository_name_input
            .read(cx)
            .value()
            .trim()
            .to_owned();
        Some(repository_to_create(owner, &name))
    }

    /// The private repository typed, to check and use.
    fn chosen_repository(&self, cx: &App) -> Option<String> {
        let typed = self.storage_input.read(cx).value().trim().to_owned();
        (self.choice == RepositoryChoice::Existing && !typed.is_empty()).then_some(typed)
    }

    /// What checking the typed repository found, once checked.
    fn checked_for(&self, repository: &str) -> Option<&Result<StorageCheck, String>> {
        (self.checked.as_ref())
            .filter(|(named, _)| named == repository)
            .map(|(_, check)| check)
    }

    /// The typed repository's check when it can be used.
    fn usable(&self, cx: &App) -> Option<&StorageCheck> {
        let repository = self.chosen_repository(cx)?;
        self.checked_for(&repository)?.as_ref().ok()
    }

    pub(super) fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            self.error = Some("Give the Space a name".into());
            return cx.notify();
        }
        let existing = (self.on_github && self.choice == RepositoryChoice::Existing)
            .then(|| self.usable(cx).cloned());
        let storage = match (self.on_github, self.choice) {
            (false, _) => None,
            (true, RepositoryChoice::Create) => match self.repository_to_create(cx) {
                Some(Ok(repository)) => Some(repository),
                Some(Err(error)) => {
                    self.error = Some(error);
                    return cx.notify();
                }
                None => {
                    self.error =
                        Some("Sign gh in to create a repository, or choose one you have".into());
                    return cx.notify();
                }
            },
            (true, RepositoryChoice::Existing) => match &existing {
                Some(Some(check)) => Some(check.repository.name.clone()),
                _ => {
                    self.error = Some("Check a repository that can be used first".into());
                    return cx.notify();
                }
            },
        };
        let check = existing.flatten();
        let fixed = |field: Option<&String>| field.is_some();
        let folder = self.folder_input.read(cx).value().trim().to_owned();
        let prefix = self.prefix_input.read(cx).value().trim().to_owned();
        let compile = self.compile_here.then(|| BrainCompile {
            at: self.time_input.read(cx).value().trim().to_owned(),
            timezone: self.timezone_input.read(cx).value().trim().to_owned(),
            extra: Extra::new(),
        });
        let setup = SpaceSetup {
            folder: (!fixed(check.as_ref().and_then(|c| c.folder.as_ref())) && !folder.is_empty())
                .then_some(folder),
            create_repository: self.on_github && self.choice == RepositoryChoice::Create,
            compile,
            color: Some(self.new_color),
        };
        let prefix = (!fixed(check.as_ref().and_then(|c| c.prefix.as_ref())) && !prefix.is_empty())
            .then_some(prefix);
        let ask = ask::create_space(
            name,
            self.description.read(cx).value().trim().to_owned(),
            String::new(),
            prefix,
            storage,
            setup,
        );
        self.busy = true;
        self.error = None;
        self.ask(ask, window, cx, |view, created, window, cx| {
            view.busy = false;
            match created {
                Ok(record) => view.show(Shown::Space(record.space.id), window, cx),
                Err(error) => view.error = Some(error),
            }
        });
        cx.notify();
    }

    /// Checks the typed repository, writing nothing: whether it can be used, and why not.
    pub(super) fn check_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repository = self.storage_input.read(cx).value().trim().to_owned();
        if repository.is_empty() {
            return;
        }
        self.busy = true;
        self.error = None;
        self.refused_by = None;
        let joining = self.shown == Some(Shown::FromRepository);
        if joining {
            self.joining = None;
        }
        let asked = repository.clone();
        let ask = ask::check_storage_repository(repository);
        self.ask(ask, window, cx, move |view, check, _, cx| {
            view.busy = false;
            if let (true, Ok(check)) = (joining, &check) {
                let ticked = (check.spaces.iter())
                    .filter(|listing| !listing.added)
                    .map(|listing| listing.folder.clone())
                    .collect();
                view.joining = Some(Joining {
                    repository: asked.clone(),
                    check: check.clone(),
                    ticked,
                    clones: BTreeMap::new(),
                    compile_here: false,
                });
            }
            view.checked = Some((asked, check));
            cx.notify();
        });
        cx.notify();
    }

    /// Protect history, then the check again, so the warning goes only once GitHub took it.
    fn protect_history(&mut self, repository: String, window: &mut Window, cx: &mut Context<Self>) {
        self.busy = true;
        self.ask(
            ask::protect_history(repository),
            window,
            cx,
            |view, protected, window, cx| {
                view.busy = false;
                view.protect_refused = protected.err();
                view.check_repository(window, cx);
            },
        );
        cx.notify();
    }

    /// Asks where one of a ticked Space's code repositories is cloned on this Mac.
    fn choose_clone(
        &mut self,
        folder: String,
        repository: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prompt = format!("Where is {repository} cloned?");
        let paths = folders::choose(&self.bridge, &prompt, false, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let chosen = paths.recv().await.ok().and_then(|p| p.into_iter().next());
            let _ = this.update(cx, |view, cx| {
                let (Some(path), Some(joining)) = (chosen, view.joining.as_mut()) else {
                    return;
                };
                let path = path.to_string_lossy().into_owned();
                joining.clones.insert((folder, repository), path);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn join(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(joining) = &self.joining else {
            return;
        };
        let spaces = join_choices(joining);
        if spaces.is_empty() {
            self.error = Some("Tick at least one Space to add".into());
            return cx.notify();
        }
        self.busy = true;
        self.error = None;
        self.refused_by = None;
        let ask = ask::join_spaces(joining.repository.clone(), spaces, joining.compile_here);
        self.ask(ask, window, cx, |view, joined, window, cx| {
            view.busy = false;
            match joined {
                Ok(records) => {
                    if let Some(first) = records.into_iter().next() {
                        view.show(Shown::Space(first.space.id), window, cx);
                    }
                }
                Err(error) => {
                    view.refused_by = join_refused_by(view.spaces(), &error);
                    view.error = Some(error);
                }
            }
        });
        cx.notify();
    }

    pub(super) fn new_page(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let check = self.usable(cx).cloned();
        let folder_fixed = check.as_ref().and_then(|c| c.folder.clone());
        let prefix_fixed = check.as_ref().and_then(|c| c.prefix.clone());
        let compile_fixed = check.as_ref().and_then(|c| c.compile.clone());
        let where_it_lives = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                radio(
                    "lives-github",
                    self.on_github,
                    "A private GitHub repository",
                    "Shared with everyone who can read it.",
                    p,
                )
                .on_click(cx.listener(|view, _, _, cx| {
                    view.on_github = true;
                    cx.notify();
                })),
            )
            .child(
                radio(
                    "lives-local",
                    !self.on_github,
                    "Only this Mac",
                    "Nothing is pushed. You can move it to GitHub later.",
                    p,
                )
                .on_click(cx.listener(|view, _, _, cx| {
                    view.on_github = false;
                    cx.notify();
                })),
            );
        let mut form = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(row("Name", Input::new(&self.name).small(), None, p))
            .child(row(
                "Color",
                self.new_color_field(p, cx),
                Some(COLOR_CAPTION.into()),
                p,
            ))
            .child(row("Where it lives", where_it_lives, None, p));
        if self.on_github {
            form = form.child(row("Repository", self.repository_field(p, cx), None, p));
        }
        let folder = match &folder_fixed {
            Some(folder) => mono(shown_folder(folder), p),
            None => div()
                .w(px(160.))
                .child(Input::new(&self.folder_input).small()),
        };
        let folder_caption = match &folder_fixed {
            Some(_) => "The folder this repository already holds, which every Space in it shares.",
            None => "Everything Wiffletree writes stays in this folder.",
        };
        form = form.child(row("Folder", folder, Some(folder_caption.into()), p));
        let prefix = match &prefix_fixed {
            Some(prefix) => mono(prefix.clone(), p),
            None => div()
                .w(px(160.))
                .child(Input::new(&self.prefix_input).small()),
        };
        let prefix_caption = match &prefix_fixed {
            Some(_) => {
                "This repository's issues use it already, so the Space shares it.".to_owned()
            }
            None => PREFIX_RULE.to_owned(),
        };
        form = form.child(row("Issue prefix", prefix, Some(prefix_caption), p));
        form = form.child(row(
            "Compile the wiki",
            self.compile_field(compile_fixed.as_ref(), p, cx),
            None,
            p,
        ));
        let mut page = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().font_weight(FontWeight::SEMIBOLD).child("New Space"))
            .child(form);
        let folder_name = match &folder_fixed {
            Some(folder) => shown_folder(folder),
            None => shown_folder(self.folder_input.read(cx).value().trim()),
        };
        if self.on_github {
            page = page.child(
                banner(p.edge).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Choosing this repository lets Wiffletree push to its main."),
                        )
                        .child(format!(
                            "Only inside {folder_name}. Everyone who can read it can read its issues, logs and wiki."
                        )),
                ),
            );
        }
        if let Some(error) = &self.error {
            page = page.child(self.error_line(error, p, cx));
        }
        page.child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                    button("cancel-new-space")
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|view, _, window, cx| {
                            let first = view.spaces().first().map(|s| s.id.clone());
                            if let Some(first) = first {
                                view.show(Shown::Space(first), window, cx);
                            }
                        })),
                )
                .child(
                    ui::gated(
                        button("create-space")
                            .primary()
                            .label("Create Space")
                            .disabled(self.busy),
                        &self.shared.host,
                    )
                    .on_click(cx.listener(|view, _, window, cx| view.create(window, cx))),
                ),
        )
    }

    fn new_color_field(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let weak = cx.weak_entity();
        swatches("new-space-color", self.new_color, p, move |color, _, cx| {
            let _ = weak.update(cx, |view, cx| {
                view.new_color = color;
                cx.notify();
            });
        })
    }

    /// The repository: Create under an owner with a name, or one the human has, checked.
    fn repository_field(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        const EXISTING: &str = "A private repository you already have";
        let can_create = self.owner.is_some();
        let label = match (self.choice, can_create) {
            (RepositoryChoice::Create, true) => "Create",
            _ => EXISTING,
        };
        let weak = cx.weak_entity();
        let menu = button("repository-choice")
            .label(label)
            .dropdown_caret(true)
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu;
                let choices = [
                    (can_create.then_some("Create"), RepositoryChoice::Create),
                    (Some(EXISTING), RepositoryChoice::Existing),
                ];
                for (label, choice) in choices {
                    let Some(label) = label else {
                        continue;
                    };
                    let weak = weak.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            view.choice = choice;
                            cx.notify();
                        });
                    }));
                }
                menu
            });
        let mut field = div().flex().flex_col().gap_1();
        if self.choice == RepositoryChoice::Create && can_create {
            let row = div()
                .flex()
                .items_center()
                .gap_2()
                .child(menu)
                .child(self.owner_menu(cx))
                .child(hint("/", p))
                .child(
                    Input::new(&self.repository_name_input)
                        .small()
                        .flex_1()
                        .min_w_0(),
                );
            return field
                .child(row)
                .child(hint("Or choose a private repository you already have.", p));
        }
        field = field.child(div().flex().child(menu));
        if !can_create {
            field = field.child(hint("Sign gh in and Wiffletree can create one for you.", p));
        }
        let named = self.storage_input.read(cx).value().trim().to_owned();
        field = field.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(Input::new(&self.storage_input).small().flex_1().min_w_0())
                .child(
                    ui::gated(
                        button("check-new-storage")
                            .label("Check")
                            .disabled(self.busy),
                        &self.shared.host,
                    )
                    .on_click(cx.listener(|view, _, window, cx| view.check_repository(window, cx))),
                ),
        );
        match self.checked_for(&named) {
            Some(check) => field.child(self.check_card(&named, check, p, cx)),
            None => field,
        }
    }

    /// The owner Create makes the repository in: the human's account or one of their
    /// organizations.
    fn owner_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let owners = (self.defaults.as_ref())
            .map(|defaults| defaults.owners.clone())
            .unwrap_or_default();
        let chosen = self.owner.clone().unwrap_or_default();
        let weak = cx.weak_entity();
        button("repository-owner")
            .label(chosen.clone())
            .dropdown_caret(true)
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu;
                for owner in &owners {
                    let (weak, login) = (weak.clone(), owner.login.clone());
                    let item = PopupMenuItem::new(owner_label(owner))
                        .checked(owner.login == chosen)
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |view, cx| {
                                view.owner = Some(login.clone());
                                cx.notify();
                            });
                        });
                    menu = menu.item(item);
                }
                menu
            })
    }

    /// Compile the wiki: On this Mac, daily at a time in a timezone, or the folder's own time.
    fn compile_field(
        &self,
        fixed: Option<&BrainCompile>,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let tick = Checkbox::new("compile-here")
            .label("On this Mac")
            .checked(self.compile_here)
            .on_click(cx.listener(|view, checked: &bool, _, cx| {
                view.compile_here = *checked;
                cx.notify();
            }));
        let time = match fixed {
            Some(compile) => div().child(hint(
                format!(
                    "Daily at {} {}, as the folder already says",
                    compile.at, compile.timezone
                ),
                p,
            )),
            None => div()
                .flex()
                .items_center()
                .gap_2()
                .child(hint("Daily at", p))
                .child(div().w(px(70.)).child(Input::new(&self.time_input).small()))
                .child(
                    div()
                        .w(px(180.))
                        .child(Input::new(&self.timezone_input).small()),
                ),
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(tick)
            .when(self.compile_here, |d| d.child(time))
            .child(hint(
                "It uses your model subscription. Teammates can turn this on too.",
                p,
            ))
    }

    /// What checking a repository found: Can be used, with who reads it and Protect history
    /// while its history is not protected, or Can't be used and why.
    fn check_card(
        &self,
        named: &str,
        check: &Result<StorageCheck, String>,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let (verdict, color) = match check {
            Ok(_) => ("Can be used", p.green),
            Err(_) => ("Can’t be used", p.red),
        };
        let title = match check {
            Ok(check) => check.repository.name.clone(),
            Err(_) => named.to_owned(),
        };
        let mut card = card(p).child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(div().flex_1())
                .child(hint(verdict, p).text_color(color)),
        );
        for (mark, line) in check_lines(check) {
            let (glyph, color) = match mark {
                Mark::Passed => ("✓", p.green),
                Mark::Warning => ("!", p.yellow),
                Mark::Refused => ("×", p.red),
            };
            let protect = (mark == Mark::Warning).then(|| {
                let repository = check
                    .as_ref()
                    .map(|c| c.repository.name.clone())
                    .unwrap_or_default();
                ui::gated(
                    button("protect-history")
                        .xsmall()
                        .label("Protect history")
                        .disabled(self.busy),
                    &self.shared.host,
                )
                .tooltip(PROTECT_HISTORY)
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.protect_history(repository.clone(), window, cx)
                }))
            });
            card = card.child(
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .text_size(px(font::SMALL))
                    .child(
                        div()
                            .w(px(14.))
                            .flex_none()
                            .font_weight(FontWeight::BOLD)
                            .text_color(color)
                            .child(glyph),
                    )
                    .child(div().flex_1().min_w_0().child(line))
                    .children(protect),
            );
        }
        if let Some(refused) = &self.protect_refused {
            card = card.child(hint(refused.clone(), p).text_color(p.red));
        }
        card
    }

    pub(super) fn join_page(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let named = self.storage_input.read(cx).value().trim().to_owned();
        let mut page = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Add Space from a repository"),
            )
            .child(row(
                "Repository",
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Input::new(&self.storage_input).small().flex_1().min_w_0())
                    .child(
                        ui::gated(
                            button("check-storage").label("Check").disabled(self.busy),
                            &self.shared.host,
                        )
                        .on_click(
                            cx.listener(|view, _, window, cx| view.check_repository(window, cx)),
                        ),
                    ),
                None,
                p,
            ));
        match (&self.joining, self.checked_for(&named)) {
            (Some(joining), _) => page = page.child(self.join_choices(joining, p, cx)),
            (None, Some(check @ Err(_))) => {
                page = page.child(self.check_card(&named, check, p, cx))
            }
            _ => {}
        }
        if let Some(error) = &self.error {
            page = page.child(self.error_line(error, p, cx));
        }
        if self
            .joining
            .as_ref()
            .is_some_and(|j| !j.check.spaces.is_empty())
        {
            page = page.child(
                div().flex().justify_end().child(
                    ui::gated(
                        button("join-spaces")
                            .primary()
                            .label("Add Space")
                            .disabled(self.busy),
                        &self.shared.host,
                    )
                    .on_click(cx.listener(|view, _, window, cx| view.join(window, cx))),
                ),
            );
        }
        page
    }

    /// The folder found, the Spaces in it to tick, each ticked one's code repositories here,
    /// and Also on this Mac, off until the human ticks it.
    fn join_choices(&self, joining: &Joining, p: Palette, cx: &mut Context<Self>) -> Div {
        let check = &joining.check;
        let here = self.shared.snapshot().map_or(&[][..], |s| &s.repositories);
        let found = match check.folder.as_deref() {
            Some("") => "Found at the repository’s root".to_owned(),
            Some(folder) => format!("Found in {}", shown_folder(folder)),
            None => "It holds no Wiffletree folder yet".to_owned(),
        };
        let color = if check.folder.is_some() {
            p.green
        } else {
            p.subtle
        };
        let mut list = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(hint(found, p).text_color(color));
        if check.spaces.is_empty() {
            return list.child(hint(
                "It holds no Space yet. New Space can use it, with a folder of its own.",
                p,
            ));
        }
        let mut spaces = div().flex().flex_col().gap_2();
        for listing in &check.spaces {
            let folder = listing.folder.clone();
            let ticked = joining.ticked.contains(&listing.folder);
            spaces = spaces.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Checkbox::new(SharedString::from(format!("join-{}", listing.folder)))
                            .label(listing.name.clone())
                            .checked(ticked || listing.added)
                            .disabled(listing.added)
                            .on_click(cx.listener(move |view, checked: &bool, _, cx| {
                                if let Some(joining) = &mut view.joining {
                                    match checked {
                                        true => joining.ticked.insert(folder.clone()),
                                        false => joining.ticked.remove(&folder),
                                    };
                                }
                                cx.notify();
                            })),
                    )
                    .child(hint(listing_line(listing, check), p)),
            );
        }
        list = list.child(spaces);
        let mut clones = div().flex().flex_col().gap_1();
        let ticked = (check.spaces.iter()).filter(|l| joining.ticked.contains(&l.folder));
        for listing in ticked {
            for (at, identity) in listing.repositories.iter().enumerate() {
                let name = identity.rsplit('/').next().unwrap_or(identity).to_owned();
                let cloned = (here.iter().find(|r| r.is_github(identity)))
                    .map(|r| r.path.clone())
                    .or_else(|| {
                        let key = (listing.folder.clone(), identity.clone());
                        joining.clones.get(&key).cloned()
                    });
                let line = match &cloned {
                    Some(path) => format!("{name}, {}", shown_path(path, home().as_deref())),
                    None => format!("{name}, not cloned here"),
                };
                let (glyph, color) = match cloned {
                    Some(_) => ("✓", p.green),
                    None => ("–", p.subtle),
                };
                let choose = cloned.is_none().then(|| {
                    let (folder, identity) = (listing.folder.clone(), identity.clone());
                    button(SharedString::from(format!("clone-{}-{at}", listing.folder)))
                        .xsmall()
                        .ghost()
                        .label("Choose…")
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.choose_clone(folder.clone(), identity.clone(), window, cx)
                        }))
                });
                clones = clones.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(font::SMALL))
                        .child(div().w(px(14.)).text_color(color).child(glyph))
                        .child(div().flex_1().min_w_0().child(line))
                        .children(choose),
                );
            }
        }
        list = list.child(row("Code repositories", clones, None, p));
        let compile = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                Checkbox::new("join-compile-here")
                    .label("Also on this Mac")
                    .checked(joining.compile_here)
                    .on_click(cx.listener(|view, checked: &bool, _, cx| {
                        if let Some(joining) = &mut view.joining {
                            joining.compile_here = *checked;
                        }
                        cx.notify();
                    })),
            )
            .child(hint(compiles_now(check.compilers), p));
        list.child(row("Compile the wiki", compile, None, p))
    }
}

/// A choice of New Space's Where it lives: a round mark, its name and what it means.
fn radio(
    id: &'static str,
    on: bool,
    title: &'static str,
    caption: &'static str,
    p: Palette,
) -> Stateful<Div> {
    let mark = div()
        .mt(px(3.))
        .size(px(13.))
        .flex_none()
        .rounded_full()
        .border_1()
        .border_color(p.edge)
        .when(on, |d| d.border(px(4.)).border_color(p.accent));
    div()
        .id(id)
        .flex()
        .items_start()
        .gap_2()
        .py(px(3.))
        .cursor_pointer()
        .child(mark)
        .child(div().child(title).child(hint(caption, p)))
}

/// Puts `proposed` in `input` and remembers it, unless the human typed their own value there.
fn follow(
    input: &Entity<InputState>,
    remembered: &mut Option<String>,
    proposed: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let typed = input.read(cx).value().to_string();
    if !typed.is_empty() && remembered.as_deref() != Some(typed.as_str()) {
        return;
    }
    let value = proposed.clone().unwrap_or_default();
    input.update(cx, |input, cx| input.set_value(value, window, cx));
    *remembered = proposed;
}

/// `<name>-wiffletree-storage`, the Space's name in lowercase words.
pub(super) fn proposed_repository_name(name: &str) -> String {
    let words: Vec<String> = (name.split(|c: char| !c.is_ascii_alphanumeric()))
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    match words.is_empty() {
        true => "wiffletree-storage".to_owned(),
        false => format!("{}-wiffletree-storage", words.join("-")),
    }
}

/// `owner/name` for gh to create, or why GitHub would refuse it.
pub(super) fn repository_to_create(owner: &str, name: &str) -> Result<String, String> {
    validate_new_repository(owner, name).map_err(|error| error.to_string())?;
    Ok(format!("{owner}/{name}"))
}

/// How the Owner menu names an owner.
pub(super) fn owner_label(owner: &RepositoryOwner) -> String {
    let kind = match owner.organization {
        true => "organization",
        false => "your account",
    };
    format!("{} — {kind}", owner.login)
}

/// A folder as the forms show it, `wiffletree/`, or the root for an empty one.
pub(super) fn shown_folder(folder: &str) -> String {
    match folder.trim_end_matches('/') {
        "" => "the repository’s root".to_owned(),
        folder => format!("{folder}/"),
    }
}

/// The lines of a repository's check, in the mockup's words.
pub(super) fn check_lines(check: &Result<StorageCheck, String>) -> Vec<(Mark, String)> {
    let check = match check {
        Ok(check) => check,
        Err(why) => return vec![(Mark::Refused, why.clone())],
    };
    let repository = &check.repository;
    let owner = repository_owner(&repository.name);
    let readers = match (repository.readers, check.reader_count) {
        (Readers::EveryMember, Some(count)) => {
            format!("{} of {owner} can read it.", counted(count, "member"))
        }
        (Readers::Invited, Some(count)) => {
            format!("{} can read it.", counted_as(count, "person", "people"))
        }
        (readers, _) => readers_sentence(readers, &repository.name),
    };
    let mut lines = vec![
        (Mark::Passed, format!("Private. {readers}")),
        (
            Mark::Passed,
            format!("Wiffletree can write to {}.", repository.branch),
        ),
    ];
    if !check.history_protected {
        lines.push((
            Mark::Warning,
            "History isn’t protected. Anyone with access could force-push over it.".into(),
        ));
    }
    lines
}

/// A Space found in a repository, as joining lists it.
fn listing_line(listing: &SpaceListing, check: &StorageCheck) -> String {
    let prefix =
        (check.prefix.as_deref()).map_or("No tracker yet".to_owned(), |p| format!("Prefix {p}"));
    let already = if listing.added {
        " · already on this Mac"
    } else {
        ""
    };
    format!(
        "{prefix} · {} · {}{already}",
        counted(check.issues, "issue"),
        counted(check.wiki_pages, "wiki page"),
    )
}

/// How many Macs compile the wiki now, and what turning it on costs.
pub(super) fn compiles_now(compilers: usize) -> String {
    let now = match compilers {
        0 => "No Mac compiles this wiki now.".to_owned(),
        1 => "1 Mac compiles this wiki now.".to_owned(),
        n => format!("{n} Macs compile this wiki now."),
    };
    format!("{now} Turning this on would use your model subscription.")
}

/// The ticked Spaces with the clones of their repositories on this Mac.
pub(super) fn join_choices(joining: &Joining) -> Vec<JoinSpace> {
    (joining.check.spaces.iter())
        .filter(|listing| joining.ticked.contains(&listing.folder))
        .map(|listing| JoinSpace {
            folder: listing.folder.clone(),
            clones: (joining.clones.iter())
                .filter(|((folder, _), _)| folder == &listing.folder)
                .map(|(_, path)| path.clone())
                .collect(),
        })
        .collect()
}
