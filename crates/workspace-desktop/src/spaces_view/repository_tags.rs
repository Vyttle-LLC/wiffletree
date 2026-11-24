//! App and Layer on a Space's code repositories (OpenSpec change `parent-projects-and-lenses`):
//! a menu of the Space's values on each row, a field to type a new one, the values with what
//! removing one clears, and the tags an add suggests. All of it is the form's draft, saved with
//! Save like the repositories.
use super::*;

/// The form's tags as edited, and what adds suggested until the human answers.
#[derive(Default)]
pub(super) struct Tagging {
    apps: Vec<String>,
    layers: Vec<String>,
    repository_tags: BTreeMap<String, RepositoryTags>,
    /// By repository, the tags an add suggested, applied only once the human confirms.
    suggested: BTreeMap<String, RepositoryTags>,
    /// The repository and kind whose new value is being typed.
    typing: Option<(String, TagKind)>,
}
impl Tagging {
    pub(super) fn load(space: &Space) -> Self {
        Self {
            apps: space.apps.clone(),
            layers: space.layers.clone(),
            repository_tags: space.repository_tags.clone(),
            ..Self::default()
        }
    }
    /// `space` with the form's tags, listing `repositories`; tags of repositories it no longer
    /// lists are dropped.
    pub(super) fn applied(&self, space: &Space, repositories: &[String]) -> Space {
        let mut repository_tags = self.repository_tags.clone();
        repository_tags
            .retain(|named, _| repositories.iter().any(|r| r.eq_ignore_ascii_case(named)));
        Space {
            repositories: repositories.to_vec(),
            apps: self.apps.clone(),
            layers: self.layers.clone(),
            repository_tags,
            ..space.clone()
        }
    }
    fn take(&mut self, space: Space) {
        self.apps = space.apps;
        self.layers = space.layers;
        self.repository_tags = space.repository_tags;
    }
    pub(super) fn suggestion(&self, identity: &str) -> Option<&RepositoryTags> {
        self.suggested.get(identity)
    }
    /// Suggests tags for each of `added`, from the form's repositories before the add; whether
    /// any was suggested.
    pub(super) fn suggest(&mut self, space: &Space, before: &[String], added: &[String]) -> bool {
        let draft = self.applied(space, before);
        let mut any = false;
        for identity in added {
            if let Some(tags) = draft.suggested_tags(identity) {
                self.suggested.insert(identity.clone(), tags);
                any = true;
            }
        }
        any
    }
}

/// A suggestion as the human reads it, such as "no app, layer backend".
pub(crate) fn suggestion_words(tags: &RepositoryTags) -> String {
    let words: Vec<String> = (TagKind::ALL.into_iter())
        .map(|kind| match tags.get(kind) {
            Some(value) => format!("{} {value}", kind.label().to_lowercase()),
            None => kind.none_label().to_owned(),
        })
        .collect();
    words.join(", ")
}

impl SpacesPage {
    fn draft(&self, space: &Space) -> Space {
        self.tagging.applied(space, &self.repositories)
    }
    fn set_tag(&mut self, space: &Space, identity: &str, kind: TagKind, value: Option<&str>) {
        let mut draft = self.draft(space);
        draft.set_tag(identity, kind, value);
        self.tagging.take(draft);
    }

    /// Keeps a value being typed within the host's limit, so Save never refuses it.
    pub(super) fn clip_tag_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.tag_input.read(cx).value();
        if typed.len() > MAX_TAG_BYTES {
            let clipped = within_tag_limit(&typed).to_owned();
            (self.tag_input).update(cx, |input, cx| input.set_value(clipped, window, cx));
        }
    }

    /// Gives the row being typed in the value typed, which joins the Space's values.
    pub(super) fn type_value(&mut self, space: &Space, cx: &mut Context<Self>) {
        let value = self.tag_input.read(cx).value().trim().to_owned();
        if let Some((identity, kind)) = self.tagging.typing.take()
            && !value.is_empty()
        {
            self.set_tag(space, &identity, kind, Some(&value));
        }
        cx.notify();
    }

    fn use_suggestion(&mut self, space: &Space, identity: &str, cx: &mut Context<Self>) {
        if let Some(tags) = self.tagging.suggested.remove(identity) {
            for kind in TagKind::ALL {
                if let Some(value) = tags.get(kind) {
                    self.set_tag(space, identity, kind, Some(value));
                }
            }
        }
        cx.notify();
    }

    /// Removes a value, first naming the repositories it would clear when any has it.
    fn confirm_remove_value(
        &mut self,
        space: &Space,
        kind: TagKind,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let draft = self.draft(space);
        let users = draft.using(kind, &value);
        if users.is_empty() {
            return self.remove_value(draft, kind, &value, cx);
        }
        let (weak, name) = (cx.weak_entity(), kind.label().to_lowercase());
        let body = removal_sentence(&users, &name);
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (weak, value) = (weak.clone(), value.clone());
            dialog
                .title(format!("Remove the {name} {value}?"))
                .confirm()
                .ok_text("Remove")
                .child(body.clone())
                .on_ok(move |_, _, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        let Some(space) = view.shown_space().cloned() else {
                            return;
                        };
                        let draft = view.draft(&space);
                        view.remove_value(draft, kind, &value, cx)
                    });
                    true
                })
        });
    }
    fn remove_value(
        &mut self,
        mut draft: Space,
        kind: TagKind,
        value: &str,
        cx: &mut Context<Self>,
    ) {
        draft.remove_value(kind, value);
        self.tagging.take(draft);
        cx.notify();
    }

    /// A repository row's App and Layer menus, or the field a new value is typed in.
    pub(super) fn tag_controls(
        &self,
        space: &Space,
        at: usize,
        identity: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let draft = self.draft(space);
        let typing = (self.tagging.typing.as_ref())
            .filter(|(typed, _)| typed == identity)
            .map(|(_, kind)| *kind);
        let mut controls = div().flex().items_center().gap_1();
        for kind in TagKind::ALL {
            if typing == Some(kind) {
                controls = controls.child(
                    div()
                        .w(px(120.))
                        .child(Input::new(&self.tag_input).xsmall()),
                );
                continue;
            }
            controls = controls.child(self.tag_menu(&draft, at, identity, kind, cx));
        }
        controls
    }

    fn tag_menu(
        &self,
        draft: &Space,
        at: usize,
        identity: &str,
        kind: TagKind,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let current = (draft.tags_of(identity).and_then(|tags| tags.get(kind))).map(str::to_owned);
        let values = draft.values(kind).to_vec();
        let (weak, identity) = (cx.weak_entity(), identity.to_owned());
        let id = SharedString::from(format!("space-repository-{}-{at}", kind.label()));
        let label = current.clone().unwrap_or_else(|| kind.label().to_owned());
        let shown = draft.clone();
        button(id)
            .ghost()
            .xsmall()
            .label(label)
            .dropdown_caret(true)
            .tooltip(format!("{} of {identity}; saved with Save", kind.label()))
            .dropdown_menu(move |mut menu, _, _| {
                let choices = std::iter::once(None).chain(values.iter().cloned().map(Some));
                for value in choices {
                    let checked = value == current;
                    let name = value.clone().unwrap_or_else(|| "None".into());
                    let (weak, identity, shown) = (weak.clone(), identity.clone(), shown.clone());
                    menu = menu.item(PopupMenuItem::new(name).checked(checked).on_click(
                        move |_, _, cx| {
                            let _ = weak.update(cx, |view, cx| {
                                view.set_tag(&shown, &identity, kind, value.as_deref());
                                cx.notify();
                            });
                        },
                    ));
                }
                let (weak, identity) = (weak.clone(), identity.clone());
                menu.separator().item(
                    PopupMenuItem::new(format!("New {}…", kind.label().to_lowercase())).on_click(
                        move |_, window, cx| {
                            let _ = weak.update(cx, |view, cx| {
                                view.tagging.typing = Some((identity.clone(), kind));
                                view.tag_input.update(cx, |input, cx| {
                                    input.set_value("", window, cx);
                                    input.focus(window, cx);
                                });
                                cx.notify();
                            });
                        },
                    ),
                )
            })
    }

    pub(super) fn suggested_tags_prompt(
        &self,
        space: &Space,
        at: usize,
        identity: &str,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let words = suggestion_words(self.tagging.suggestion(identity)?);
        let (used, dismissed) = (identity.to_owned(), identity.to_owned());
        let shown = space.clone();
        Some(
            div()
                .pl_2()
                .flex()
                .items_center()
                .gap_2()
                .child(hint(format!("Suggested: {words}"), p))
                .child(
                    button(SharedString::from(format!("use-suggested-tags-{at}")))
                        .xsmall()
                        .label("Use")
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.use_suggestion(&shown, &used, cx)
                        })),
                )
                .child(
                    button(SharedString::from(format!("dismiss-suggested-tags-{at}")))
                        .ghost()
                        .xsmall()
                        .label("Dismiss")
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.tagging.suggested.remove(&dismissed);
                            cx.notify();
                        })),
                ),
        )
    }

    pub(super) fn removable_tag_values(
        &self,
        space: &Space,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let draft = self.draft(space);
        if draft.apps.is_empty() && draft.layers.is_empty() {
            return None;
        }
        let mut line = div().mt_1().flex().flex_wrap().items_center().gap_1();
        for kind in TagKind::ALL {
            let values = draft.values(kind);
            if values.is_empty() {
                continue;
            }
            line = line.child(hint(format!("{}s:", kind.label()), p));
            for (at, value) in values.iter().enumerate() {
                let (removed, shown) = (value.clone(), space.clone());
                line = line.child(
                    button(SharedString::from(format!("remove-{}-{at}", kind.label())))
                        .ghost()
                        .xsmall()
                        .label(value.clone())
                        .icon(icon("close"))
                        .tooltip(format!(
                            "Remove {value} from {}; saved with Save",
                            space.name
                        ))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.confirm_remove_value(&shown, kind, removed.clone(), window, cx)
                        })),
                );
            }
        }
        Some(line)
    }
}

/// The longest start of `typed`, in whole characters, within `MAX_TAG_BYTES`.
pub(super) fn within_tag_limit(typed: &str) -> &str {
    let mut end = typed.len().min(MAX_TAG_BYTES);
    while !typed.is_char_boundary(end) {
        end -= 1;
    }
    &typed[..end]
}

/// What removing a value clears, naming the repositories that have it.
pub(super) fn removal_sentence(users: &[String], kind: &str) -> String {
    let named = match users {
        [one] => format!("{one} has it"),
        [rest @ .., last] => format!("{} and {last} have it", rest.join(", ")),
        [] => return String::new(),
    };
    format!("{named}, and will have no {kind}.")
}
