//! Add source on a Space's page (OpenSpec change `assistant-does-the-work`, task 3.5, spaces
//! "Connecting a source is one form"): the MCP servers configured for this Mac's Claude and
//! Codex and the well-known CLIs found here, or one typed by hand; its tools with suggested
//! read or act kinds, Do without asking, and the source's own sign-in instructions with Try
//! again. Wiffletree runs no sign-in itself. Saving writes the source to the Space.
use super::*;
use workspace_core::{SourceCandidate, SourceKind, SourceProbe, SuggestedTool};

const CLI_ACTS: &str = "A CLI's act commands wait for a card only because the Assistant is told to run them through act; it could run the CLI in its own shell. An MCP server's act tools always wait for a card.";
const URL_ACTS: &str = "This server is reached by URL. Its read tools work; acting through it is refused for now, so its act tools never run, even with a card.";
const SIGN_IN: &str = "Sign in with the source's own tool, as its documentation says, then Try again. Wiffletree never signs in for you or keeps its tokens.";

/// One tool or command prefix as the form lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ToolChoice {
    name: String,
    description: String,
    act: bool,
    ticked: bool,
    without_asking: bool,
}
impl From<SuggestedTool> for ToolChoice {
    fn from(tool: SuggestedTool) -> Self {
        Self {
            name: tool.name,
            description: tool.description,
            act: tool.act,
            ticked: true,
            without_asking: false,
        }
    }
}

/// The source the form saves: the ticked reads and acts, and the acts marked Do without
/// asking. An MCP server keeps what the Space already holds under its name that the form does
/// not show, such as its extra fields.
pub(super) fn chosen_source(
    kind: SourceKind,
    name: &str,
    tools: &[ToolChoice],
    before: Option<&Source>,
) -> Source {
    let ticked = |act: bool| -> Vec<String> {
        (tools.iter())
            .filter(|tool| tool.ticked && tool.act == act)
            .map(|tool| tool.name.clone())
            .collect()
    };
    let without_asking: Vec<String> = (tools.iter())
        .filter(|tool| tool.ticked && tool.act && tool.without_asking)
        .map(|tool| tool.name.clone())
        .collect();
    match (kind, before) {
        (SourceKind::Mcp, Some(Source::Mcp { extra, .. })) => Source::Mcp {
            server: name.into(),
            tools: ticked(false),
            act: ticked(true),
            without_asking,
            extra: extra.clone(),
        },
        (SourceKind::Mcp, _) => Source::Mcp {
            server: name.into(),
            tools: ticked(false),
            act: ticked(true),
            without_asking,
            extra: Default::default(),
        },
        (SourceKind::Cli, before) => {
            let (check, extra) = match before {
                Some(Source::Cli { check, extra, .. }) => (check.clone(), extra.clone()),
                _ => (None, Default::default()),
            };
            Source::Cli {
                command: name.into(),
                prefixes: ticked(false),
                act: ticked(true),
                without_asking,
                check,
                extra,
            }
        }
    }
}
/// The kind a tool or prefix typed by hand suggests: for a CLI, from its last word, the verb
/// after the command and its subcommands, such as `send` in `gws gmail send`.
pub(super) fn suggests_act_by_hand(kind: Option<SourceKind>, name: &str) -> bool {
    let word = match kind {
        Some(SourceKind::Cli) => name.split_whitespace().last().unwrap_or(name),
        _ => name,
    };
    workspace_core::suggests_act(word, None)
}
/// Whether `next` is the source already picked, of the same kind and name.
pub(super) fn same_source(picked: Option<&SourceCandidate>, next: &SourceCandidate) -> bool {
    picked.is_some_and(|picked| picked.kind == next.kind && picked.name == next.name)
}
/// The Space's sources with `source` in place of the one of its kind and name, or added last.
pub(super) fn with_source(sources: &[Source], source: Source) -> Vec<Source> {
    let same = |other: &Source| {
        other.name() == source.name()
            && matches!(other, Source::Mcp { .. }) == matches!(source, Source::Mcp { .. })
    };
    let mut sources = sources.to_vec();
    match sources.iter().position(same) {
        Some(at) => sources[at] = source,
        None => sources.push(source),
    }
    sources
}

/// Add source, open on one Space.
pub(super) struct AddSource {
    space: SpaceId,
    candidates: Option<Result<Vec<SourceCandidate>, String>>,
    /// The source chosen, from the list or typed by hand.
    picked: Option<SourceCandidate>,
    probe: Option<Result<SourceProbe, String>>,
    probing: bool,
    tools: Vec<ToolChoice>,
    /// A source's name typed by hand, its kind, and a tool or prefix typed by hand.
    by_hand: Entity<InputState>,
    by_hand_kind: SourceKind,
    extra_tool: Entity<InputState>,
    saving: bool,
}

impl SpacesPage {
    pub(super) fn open_add_source(
        &mut self,
        space: SpaceId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        self.adding = Some(AddSource {
            space,
            candidates: None,
            picked: None,
            probe: None,
            probing: false,
            tools: vec![],
            by_hand: input("Server or command name", window, cx),
            by_hand_kind: SourceKind::Mcp,
            extra_tool: input("A tool or command prefix", window, cx),
            saving: false,
        });
        self.ask(
            ask::source_candidates(),
            window,
            cx,
            |view, found, _, cx| {
                if let Some(adding) = &mut view.adding {
                    adding.candidates = Some(found);
                }
                cx.notify();
            },
        );
        cx.notify();
    }
    /// Starts the chosen source and lists its tools, as Try again does.
    fn probe(&mut self, picked: SourceCandidate, window: &mut Window, cx: &mut Context<Self>) {
        let Some(adding) = &mut self.adding else {
            return;
        };
        // Choices carry over only to Try again on the same source, never to another one.
        if !same_source(adding.picked.as_ref(), &picked) {
            adding.tools.clear();
        }
        adding.picked = Some(picked.clone());
        adding.probing = true;
        adding.probe = None;
        let ask = ask::probe_source(picked.kind, picked.name.clone());
        self.ask(ask, window, cx, move |view, probe, _, cx| {
            let Some(adding) = &mut view.adding else {
                return;
            };
            if adding.picked.as_ref() != Some(&picked) {
                return;
            }
            adding.probing = false;
            if let Ok(found) = &probe {
                // A tool the human already chose keeps its choice when Try again lists it.
                let kept = std::mem::take(&mut adding.tools);
                adding.tools = (found.tools.iter().cloned())
                    .map(ToolChoice::from)
                    .map(|tool| {
                        (kept.iter().find(|k| k.name == tool.name).cloned()).unwrap_or(tool)
                    })
                    .chain(
                        kept.iter()
                            .filter(|k| found.tools.iter().all(|t| t.name != k.name))
                            .cloned(),
                    )
                    .collect();
            }
            adding.probe = Some(probe);
            cx.notify();
        });
        cx.notify();
    }
    fn probe_by_hand(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(adding) = &self.adding else {
            return;
        };
        let name = adding.by_hand.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return;
        }
        let picked = SourceCandidate {
            kind: adding.by_hand_kind,
            name,
            configured_for: vec![],
            by_url: false,
        };
        self.probe(picked, window, cx);
    }
    fn add_tool_by_hand(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(adding) = &mut self.adding else {
            return;
        };
        let name = adding.extra_tool.read(cx).value().trim().to_owned();
        if name.is_empty() || adding.tools.iter().any(|t| t.name == name) {
            return;
        }
        let act = suggests_act_by_hand(adding.picked.as_ref().map(|p| p.kind), &name);
        adding.tools.push(ToolChoice {
            name,
            description: String::new(),
            act,
            ticked: true,
            without_asking: false,
        });
        adding
            .extra_tool
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }
    fn change_tool(
        &mut self,
        at: usize,
        change: impl FnOnce(&mut ToolChoice),
        cx: &mut Context<Self>,
    ) {
        if let Some(tool) = self.adding.as_mut().and_then(|a| a.tools.get_mut(at)) {
            change(tool);
            // Only an act may be done without asking.
            tool.without_asking &= tool.act;
        }
        cx.notify();
    }
    fn save_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(adding) = &mut self.adding else {
            return;
        };
        let (Some(picked), Some(latest)) = (
            adding.picked.clone(),
            (self.shared.snapshot())
                .and_then(|s| s.spaces.iter().find(|s| s.id == adding.space))
                .cloned(),
        ) else {
            return;
        };
        let before = (latest.sources.iter()).find(|s| s.name() == picked.name);
        let source = chosen_source(picked.kind, &picked.name, &adding.tools, before);
        adding.saving = true;
        let space = Space {
            sources: with_source(&latest.sources, source),
            ..latest
        };
        self.ask(
            ask::update_space(space),
            window,
            cx,
            move |view, saved, window, cx| {
                match saved {
                    Ok(record) => {
                        view.adding = None;
                        toast(
                            format!("Added {} to {}", picked.name, record.space.name),
                            window,
                            cx,
                        );
                    }
                    Err(error) => {
                        if let Some(adding) = &mut view.adding {
                            adding.saving = false;
                        }
                        toast_error(error, window, cx);
                    }
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    /// The Add source form, under the Space's Sources.
    pub(super) fn add_source_form(&self, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        let adding = self.adding.as_ref()?;
        let mut form = card(p).gap_3().child(
            div()
                .flex()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Add source"),
                )
                .child(
                    button("close-add-source")
                        .ghost()
                        .xsmall()
                        .icon(icon("close"))
                        .on_click(cx.listener(|v, _, _, c| {
                            v.adding = None;
                            c.notify();
                        })),
                ),
        );
        let mut list = div().flex().flex_wrap().gap_2();
        match &adding.candidates {
            None => list = list.child(hint("Looking for MCP servers and CLIs on this Mac…", p)),
            Some(Err(error)) => list = list.child(hint(error.clone(), p).text_color(p.red)),
            Some(Ok(found)) if found.is_empty() => {
                list = list.child(hint(
                    "No MCP server is configured for Claude or Codex, and no CLI Wiffletree knows is installed. Type one below.",
                    p,
                ));
            }
            Some(Ok(found)) => {
                for (at, candidate) in found.iter().enumerate() {
                    let picked = adding.picked.as_ref() == Some(candidate);
                    let kind = match candidate.kind {
                        SourceKind::Mcp => "MCP",
                        SourceKind::Cli => "CLI",
                    };
                    let chosen = candidate.clone();
                    list = list.child(
                        button(SharedString::from(format!("source-candidate-{at}")))
                            .xsmall()
                            .when(picked, |b| b.primary())
                            .label(format!("{} · {kind}", candidate.name))
                            .on_click(cx.listener(move |v, _, w, c| v.probe(chosen.clone(), w, c))),
                    );
                }
            }
        }
        let kind = adding.by_hand_kind;
        form = form
            .child(hint("Configured on this Mac", p))
            .child(list)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(hint("Or by hand", p))
                    .child(
                        ui::segment("by-hand-mcp", "MCP", kind == SourceKind::Mcp).on_click(
                            cx.listener(|v, _, _, c| {
                                if let Some(adding) = &mut v.adding {
                                    adding.by_hand_kind = SourceKind::Mcp;
                                }
                                c.notify();
                            }),
                        ),
                    )
                    .child(
                        ui::segment("by-hand-cli", "CLI", kind == SourceKind::Cli).on_click(
                            cx.listener(|v, _, _, c| {
                                if let Some(adding) = &mut v.adding {
                                    adding.by_hand_kind = SourceKind::Cli;
                                }
                                c.notify();
                            }),
                        ),
                    )
                    .child(div().w(px(220.)).child(Input::new(&adding.by_hand).small()))
                    .child(
                        button("probe-by-hand")
                            .xsmall()
                            .label("List its tools")
                            .on_click(cx.listener(|v, _, w, c| v.probe_by_hand(w, c))),
                    ),
            );
        let Some(picked) = &adding.picked else {
            return Some(form);
        };
        if adding.probing {
            return Some(
                form.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(ui::Spinner::new().xsmall().color(p.accent))
                        .child(hint(
                            format!("Starting {} to list its tools…", picked.name),
                            p,
                        )),
                ),
            );
        }
        if let Some(problem) = adding.probe.as_ref().and_then(|probe| match probe {
            Ok(probe) => (probe.problem.clone())
                .map(|why| (why, probe.sign_in.clone(), probe.server_said.clone())),
            Err(error) => Some((error.clone(), None, None)),
        }) {
            let (why, sign_in, said) = problem;
            // A started server's error may be an expired sign-in; a missing CLI is not.
            let may_be_signed_out =
                sign_in.is_some() || (picked.kind == SourceKind::Mcp && !picked.by_url);
            let retried = picked.clone();
            form =
                form.child(
                    banner(p.yellow)
                        .flex_col()
                        .items_start()
                        .gap_1()
                        .child(why)
                        .children(sign_in.map(|command| {
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child("Sign in with")
                                .child(mono(command, p))
                        }))
                        // Its own words, such as the command or link that signs it in.
                        .children(said.map(|said| {
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(hint("The server said:", p))
                                .child(
                                    div()
                                        .font_family(palette::mono_font())
                                        .text_size(px(font::CAPTION))
                                        .whitespace_normal()
                                        .child(said),
                                )
                        }))
                        .when(may_be_signed_out, |d| d.child(hint(SIGN_IN, p)))
                        .child(button("probe-again").xsmall().label("Try again").on_click(
                            cx.listener(move |v, _, w, c| v.probe(retried.clone(), w, c)),
                        )),
                );
        }
        form = form
            .when(picked.by_url, |d| {
                d.child(hint(URL_ACTS, p).text_color(p.yellow))
            })
            .when(picked.kind == SourceKind::Cli, |d| {
                d.child(hint(CLI_ACTS, p))
            })
            .child(self.tool_rows(adding, p, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(260.))
                            .child(Input::new(&adding.extra_tool).small()),
                    )
                    .child(
                        button("add-tool-by-hand")
                            .xsmall()
                            .label("Add")
                            .on_click(cx.listener(|v, _, w, c| v.add_tool_by_hand(w, c))),
                    ),
            )
            .child(
                div().flex().justify_end().child(
                    button("save-source")
                        .primary()
                        .small()
                        .label(format!("Add {}", picked.name))
                        .disabled(adding.saving || !adding.tools.iter().any(|t| t.ticked))
                        .on_click(cx.listener(|v, _, w, c| v.save_source(w, c))),
                ),
            );
        Some(form)
    }
    fn tool_rows(&self, adding: &AddSource, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut rows = div().flex().flex_col().gap_1();
        if adding.tools.is_empty() {
            return rows.child(hint("No tools listed yet. Add one by hand below.", p));
        }
        for (at, tool) in adding.tools.iter().enumerate() {
            let act = tool.act;
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Checkbox::new(SharedString::from(format!("source-tool-{at}")))
                            .checked(tool.ticked)
                            .on_click(cx.listener(move |v, _, _, c| {
                                v.change_tool(at, |t| t.ticked ^= true, c)
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(mono(tool.name.clone(), p))
                            .when(!tool.description.is_empty(), |d| {
                                d.child(hint(tool.description.clone(), p))
                            }),
                    )
                    .child(
                        ui::segment(SharedString::from(format!("tool-read-{at}")), "Read", !act)
                            .on_click(cx.listener(move |v, _, _, c| {
                                v.change_tool(at, |t| t.act = false, c)
                            })),
                    )
                    .child(
                        ui::segment(SharedString::from(format!("tool-act-{at}")), "Act", act)
                            .on_click(cx.listener(move |v, _, _, c| {
                                v.change_tool(at, |t| t.act = true, c)
                            })),
                    )
                    .child(
                        Checkbox::new(SharedString::from(format!("tool-without-asking-{at}")))
                            .label("Do without asking")
                            .disabled(!act)
                            .checked(tool.without_asking)
                            .on_click(cx.listener(move |v, _, _, c| {
                                v.change_tool(at, |t| t.without_asking ^= true, c)
                            })),
                    ),
            );
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::{ToolChoice, chosen_source, same_source, suggests_act_by_hand, with_source};
    use workspace_core::{Source, SourceCandidate, SourceKind};

    #[test]
    fn choices_carry_over_only_to_try_again_on_the_same_source() {
        let candidate = |kind, name: &str| SourceCandidate {
            kind,
            name: name.into(),
            configured_for: vec![],
            by_url: false,
        };
        let gh = candidate(SourceKind::Cli, "gh");
        assert!(same_source(Some(&gh), &gh));
        assert!(!same_source(
            Some(&gh),
            &candidate(SourceKind::Cli, "linctl")
        ));
        assert!(!same_source(Some(&gh), &candidate(SourceKind::Mcp, "gh")));
        assert!(!same_source(None, &gh));
    }

    #[test]
    fn a_cli_prefix_typed_by_hand_is_judged_by_its_verb() {
        let cli = Some(SourceKind::Cli);
        assert!(!suggests_act_by_hand(cli, "gws gmail list"));
        assert!(suggests_act_by_hand(cli, "gws gmail send"));
        assert!(suggests_act_by_hand(Some(SourceKind::Mcp), "send_message"));
    }

    fn tool(name: &str, act: bool, ticked: bool, without_asking: bool) -> ToolChoice {
        ToolChoice {
            name: name.into(),
            description: String::new(),
            act,
            ticked,
            without_asking,
        }
    }

    #[test]
    fn saving_writes_the_ticked_reads_and_acts_and_the_acts_done_without_asking() {
        let tools = [
            tool("list_events", false, true, false),
            tool("create_event", true, true, true),
            tool("respond_to_invite", true, true, false),
            tool("delete_calendar", true, false, false),
        ];
        let source = chosen_source(SourceKind::Mcp, "calendar", &tools, None);
        assert_eq!(source.reads(), ["list_events"]);
        assert_eq!(source.acts(), ["create_event", "respond_to_invite"]);
        assert!(source.without_asking("create_event"));
        assert!(!source.without_asking("respond_to_invite"));
    }

    #[test]
    fn an_added_source_replaces_its_namesake_and_keeps_its_sign_in_check() {
        let gh = Source::Cli {
            command: "gh".into(),
            prefixes: vec!["gh pr list".into()],
            act: vec![],
            without_asking: vec![],
            check: Some("gh auth status".into()),
            extra: Default::default(),
        };
        let tools = [
            tool("gh pr view", false, true, false),
            tool("gh pr comment", true, true, false),
        ];
        let added = chosen_source(SourceKind::Cli, "gh", &tools, Some(&gh));
        let sources = with_source(&[gh], added);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].reads(), ["gh pr view"]);
        assert_eq!(sources[0].acts(), ["gh pr comment"]);
        assert!(matches!(&sources[0], Source::Cli { check: Some(_), .. }));
        let mcp = chosen_source(SourceKind::Mcp, "gh", &tools, None);
        assert_eq!(with_source(&sources, mcp).len(), 2);
    }
}
