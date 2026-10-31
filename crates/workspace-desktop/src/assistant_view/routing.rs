//! The router in the assistant (stage 2, screens 1 and 2 of the mockup): the line a routed
//! message leaves with Change, the question an unsure answer asks with one-click choices and
//! "Somewhere else…", and the marks on a message Change moved.
use super::*;
use gpui_component::menu::PopupMenu;
use workspace_core::assistant::router::Destination;
use workspace_core::assistant::{HeldMessage, RoutedLine, Thread};

/// What a held message's question says it waits for, as the mockup words it.
const HELD: &str = "Held · nothing is filed in a brain until you answer";

/// The thread a message sent from a new tab went to, once it has one: a new thread takes the
/// message's id, and an existing one holds its line.
pub(super) fn thread_of(tabs: &[ThreadTab], message: &str) -> Option<String> {
    let holds = |t: &Thread| t.id == message || t.routed.iter().any(|l| l.message_id == message);
    (tabs.iter())
        .find(|tab| holds(&tab.thread))
        .map(|tab| tab.thread.id.clone())
}

/// One row of a thread's conversation: a message, or a message sent from it that went to
/// another thread.
pub(super) enum Entry<'a> {
    Message(&'a Message),
    Away(&'a RoutedLine),
}
/// A thread's messages and the messages that left it, in the order they were sent.
pub(super) fn timeline<'a>(messages: &'a [Message], thread: &'a Thread) -> Vec<Entry<'a>> {
    let away = (thread.routed.iter()).filter(|line| line.to != thread.id);
    let mut entries: Vec<(i64, Entry)> = (messages.iter())
        .map(|m| (m.created_at, Entry::Message(m)))
        .chain(away.map(|line| (line.created_at, Entry::Away(line))))
        .collect();
    entries.sort_by_key(|(at, _)| *at);
    entries.into_iter().map(|(_, entry)| entry).collect()
}

/// The question an unsure answer asks: "Is this Sage or Personal?" when its choices are in
/// several Spaces, by their names in order.
pub(super) fn question_text(spaces: &[String]) -> String {
    match spaces {
        [] => "Where does this belong?".into(),
        [_] => "Which thread is this for?".into(),
        [rest @ .., last] => format!("Is this {} or {last}?", rest.join(", ")),
    }
}

/// Where Change on `line` must not offer, as the message is already there: the thread it went
/// to, and for a thread it started, a new thread in that thread's Space. Both come from the
/// line, as the thread it went to may have left the strip.
fn already_there(line: &RoutedLine) -> Vec<Destination> {
    let thread = Destination::Thread {
        thread_id: line.to.clone(),
    };
    let new = line.new.then(|| Destination::NewThread {
        space_id: line.to_space.clone(),
    });
    std::iter::once(thread).chain(new).collect()
}

/// "New thread in Personal" or "Sent to “Dentist Tuesday” in Personal", without the Space when
/// this Mac no longer knows its name.
fn route_text(line: &RoutedLine, space: &str) -> String {
    let went = match line.new {
        true => "New thread".to_owned(),
        false => format!("Sent to “{}”", line.to_title),
    };
    match space {
        "" => went,
        space => format!("{went} in {space}"),
    }
}

/// A click that sends a message: answering a held one, or Change on a routed one.
#[derive(Clone)]
enum Pick {
    Held { id: String },
    Change { holder: String, message: String },
}

impl AssistantPage {
    /// The messages held for a tab: those sent from the thread `open`, or for a new tab those
    /// sent from one and those whose thread is no longer in the strip.
    pub(super) fn held_for(&self, open: Option<&str>) -> Vec<HeldMessage> {
        let held = self.shared.snapshot().map_or(&[][..], |s| &s.held);
        let in_strip = |id: &str| self.tabs().iter().any(|t| t.thread.id == id);
        (held.iter())
            .filter(|h| match (open, h.origin.as_deref()) {
                (Some(open), origin) => origin == Some(open),
                (None, origin) => origin.is_none_or(|o| !in_strip(o)),
            })
            .cloned()
            .collect()
    }
    fn tab_of(&self, thread: &str) -> Option<&ThreadTab> {
        self.tabs().iter().find(|t| t.thread.id == thread)
    }
    fn space_name(&self, id: &SpaceId) -> String {
        (self.spaces().iter())
            .find(|s| &s.id == id)
            .map_or_else(String::new, |s| s.name.clone())
    }
    fn destination_space(&self, destination: &Destination) -> Option<SpaceId> {
        match destination {
            Destination::Thread { thread_id } => {
                self.tab_of(thread_id).map(|t| t.thread.space_id.clone())
            }
            Destination::NewThread { space_id } => Some(space_id.clone()),
        }
    }
    fn title(&self, thread: &str) -> String {
        self.tab_of(thread).map_or("a closed thread".into(), |t| {
            format!("“{}”", t.thread.title)
        })
    }
    /// A choice's button: "Sage · new thread", "Continue “API date format”" for the thread it
    /// was sent from, or another thread's title.
    fn choice_label(&self, destination: &Destination, origin: Option<&str>) -> String {
        match destination {
            Destination::NewThread { space_id } => {
                format!("{} · new thread", self.space_name(space_id))
            }
            Destination::Thread { thread_id } if Some(thread_id.as_str()) == origin => {
                format!("Continue {}", self.title(thread_id))
            }
            Destination::Thread { thread_id } => self.title(thread_id),
        }
    }
    /// Every Space and open thread, for "Somewhere else…".
    fn everywhere(&self) -> Vec<(String, Destination)> {
        let on = |id: &SpaceId| {
            let locals = self.shared.snapshot().map(|s| &s.space_locals);
            locals.and_then(|l| l.get(id)).is_none_or(|l| l.enabled)
        };
        let spaces = (self.spaces().iter()).filter(|s| on(&s.id)).map(|s| {
            let new = Destination::NewThread {
                space_id: s.id.clone(),
            };
            (format!("New thread in {}", s.name), new)
        });
        let threads = (self.tabs().iter())
            .filter(|t| t.thread.project_id.is_none() && on(&t.thread.space_id))
            .map(|t| {
                let to = Destination::Thread {
                    thread_id: t.thread.id.clone(),
                };
                let name = self.space_name(&t.thread.space_id);
                (format!("“{}” · {name}", t.thread.title), to)
            });
        spaces.chain(threads).collect()
    }

    fn pick(&mut self, pick: Pick, to: Destination, window: &mut Window, cx: &mut Context<Self>) {
        let ask = match pick {
            Pick::Held { id } => ask::answer_held(id, to),
            Pick::Change { holder, message } => ask::change_route(holder, message, to),
        };
        self.ask(ask, window, cx, |_, done, window, cx| {
            if let Err(error) = done {
                toast_error(error, window, cx);
            }
        });
    }
    /// A menu of destinations, `first` then a separator and the rest of `everywhere`, none
    /// of them in `left_out`.
    fn destination_menu(
        &self,
        id: SharedString,
        label: &str,
        first: Vec<(String, Destination)>,
        left_out: &[Destination],
        pick: Pick,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let rest: Vec<(String, Destination)> = (self.everywhere().into_iter())
            .filter(|(_, d)| !first.iter().any(|(_, f)| f == d) && !left_out.contains(d))
            .collect();
        let weak = cx.weak_entity();
        button(id)
            .ghost()
            .xsmall()
            .label(label.to_owned())
            .dropdown_menu(move |mut menu: PopupMenu, _, _| {
                let groups = [&first, &rest];
                for (index, group) in groups.into_iter().enumerate() {
                    if index > 0 && !first.is_empty() && !group.is_empty() {
                        menu = menu.separator();
                    }
                    for (label, to) in group {
                        let (weak, pick, to) = (weak.clone(), pick.clone(), to.clone());
                        menu = menu.item(PopupMenuItem::new(label.clone()).on_click(
                            move |_, window, cx| {
                                let _ = weak.update(cx, |view, cx| {
                                    view.pick(pick.clone(), to.clone(), window, cx)
                                });
                            },
                        ));
                    }
                }
                menu
            })
    }

    pub(super) fn bubble(body: &str, p: Palette) -> Div {
        div()
            .self_end()
            .max_w(relative(0.85))
            .px_3()
            .py_2()
            .rounded_lg()
            .bg(p.overlay)
            .child(SharedString::from(body.to_owned()))
    }
    /// "New thread in Personal · Change", in the thread `holder` a routed message left.
    pub(super) fn route_line(
        &self,
        holder: &Thread,
        line: &RoutedLine,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let spaces = self.spaces().to_vec();
        let named = self.space_name(&line.to_space);
        let space = (!named.is_empty()).then(|| line.to_space.clone());
        let text = route_text(line, &named);
        let left_out = already_there(line);
        let origin = (line.to != holder.id).then_some(holder.id.as_str());
        let choices = (line.choices.iter())
            .filter(|c| !left_out.contains(c))
            .map(|c| (self.choice_label(c, origin), c.clone()))
            .collect();
        let pick = Pick::Change {
            holder: holder.id.clone(),
            message: line.message_id.clone(),
        };
        let id = SharedString::from(format!("change-{}", line.message_id));
        div()
            .self_end()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(font::CAPTION))
            .text_color(p.subtle)
            .when_some(space, |d, s| {
                d.child(space_mark(space_color(&spaces, &s, p)))
            })
            .child(format!("{text} ·"))
            .child(self.destination_menu(id, "Change", choices, &left_out, pick, cx))
    }
    pub(super) fn moved_line(&self, to: &str, p: Palette) -> Div {
        hint(format!("Moved to {}", self.title(to)), p).self_end()
    }
    /// A held message: the user's message, then the router's question, or that it is deciding.
    pub(super) fn held_message(
        &self,
        held: &HeldMessage,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let column = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(Self::bubble(&held.body, p));
        let Some(question) = &held.question else {
            return column.child(
                div()
                    .self_end()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(ui::Spinner::new().xsmall().color(p.subtle))
                    .child(hint("Finding where this goes…", p)),
            );
        };
        let spaces = self.spaces().to_vec();
        let mut named: Vec<String> = vec![];
        for choice in &question.choices {
            let name = (self.destination_space(choice)).map(|s| self.space_name(&s));
            if let Some(name) = name.filter(|n| !named.contains(n)) {
                named.push(name);
            }
        }
        let mut choices = div().flex().flex_wrap().gap(px(6.));
        for (index, choice) in question.choices.iter().enumerate() {
            let space = self.destination_space(choice);
            let (to, pick) = (
                choice.clone(),
                Pick::Held {
                    id: held.id.clone(),
                },
            );
            choices = choices.child(
                button(SharedString::from(format!("held-{}-{index}", held.id)))
                    .xsmall()
                    .when_some(space, |b, s| {
                        b.child(space_mark(space_color(&spaces, &s, p)))
                    })
                    .label(self.choice_label(choice, held.origin.as_deref()))
                    .on_click(
                        cx.listener(move |v, _, w, c| v.pick(pick.clone(), to.clone(), w, c)),
                    ),
            );
        }
        let elsewhere = SharedString::from(format!("held-elsewhere-{}", held.id));
        let pick = Pick::Held {
            id: held.id.clone(),
        };
        choices = choices.child(self.destination_menu(
            elsewhere,
            "Somewhere else…",
            vec![],
            &[],
            pick,
            cx,
        ));
        let why = format!("{} Your message waits until you choose.", question.why);
        let asked = div()
            .flex_1()
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py(px(10.))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(question_text(&named)),
            )
            .child(hint(why.trim().to_owned(), p))
            .child(choices)
            .child(hint(HELD, p).text_color(p.subtle.opacity(0.75)));
        column.child(
            div()
                .flex()
                .rounded(px(9.))
                .overflow_hidden()
                .border_1()
                .border_color(p.edge)
                .bg(p.base)
                .child(div().w(px(3.)).flex_none().bg(p.focus))
                .child(asked),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{already_there, question_text, route_text};
    use workspace_core::SpaceId;
    use workspace_core::assistant::RoutedLine;
    use workspace_core::assistant::router::Destination;

    #[test]
    fn change_never_offers_where_the_message_already_is() {
        let personal = SpaceId("personal".into());
        let new_in = |space: &str| Destination::NewThread {
            space_id: SpaceId(space.into()),
        };
        let to = Destination::Thread {
            thread_id: "dentist".into(),
        };
        let mut line = RoutedLine {
            message_id: "m1".into(),
            body: "book the dentist".into(),
            created_at: 0,
            to: "dentist".into(),
            to_space: personal.clone(),
            to_title: "Dentist Tuesday".into(),
            new: true,
            choices: vec![new_in("personal"), new_in("sage"), to.clone()],
        };
        assert_eq!(already_there(&line), [to.clone(), new_in("personal")]);
        // Sent to a thread that was open already, a new thread in its Space is a real move.
        line.new = false;
        assert_eq!(already_there(&line), [to]);
    }

    #[test]
    fn a_routed_line_names_its_space_or_leaves_it_out() {
        let mut line = RoutedLine {
            message_id: "m1".into(),
            body: "book the dentist".into(),
            created_at: 0,
            to: "dentist".into(),
            to_space: SpaceId("personal".into()),
            to_title: "Dentist Tuesday".into(),
            new: true,
            choices: vec![],
        };
        assert_eq!(route_text(&line, "Personal"), "New thread in Personal");
        assert_eq!(route_text(&line, ""), "New thread");
        line.new = false;
        assert_eq!(
            route_text(&line, "Personal"),
            "Sent to “Dentist Tuesday” in Personal"
        );
    }

    #[test]
    fn the_question_names_the_spaces_of_its_choices() {
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            question_text(&names(&["Sage", "Personal"])),
            "Is this Sage or Personal?"
        );
        assert_eq!(
            question_text(&names(&["Sage", "Vyttle", "Personal"])),
            "Is this Sage, Vyttle or Personal?"
        );
        assert_eq!(
            question_text(&names(&["Sage"])),
            "Which thread is this for?"
        );
        assert_eq!(question_text(&[]), "Where does this belong?");
    }
}
