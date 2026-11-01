//! Effective settings (settings-cascade study, screen 6): opened from a project's header, a
//! read-only list of what the project's agents get, or a ticket's, and where each value came
//! from. Agents resolve their settings when they start, so this says what the next one gets.
use super::*;
use cascade::{Behavior, EffectiveSettings, Layer, Resolved};
use ui::{from_chip, hint, section};

/// What the panel shows: the host's answer and the Behaviors it names.
pub(super) struct Effective {
    pub(super) settings: EffectiveSettings,
    pub(super) catalog: Vec<Behavior>,
}

/// One setting as the panel lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    setting: String,
    value: String,
    from: Layer,
    /// The layer named as the chip says it, such as "Sage" or "sagechat-sms in Sage".
    from_label: String,
    /// The earlier layers it replaced, as "Overrode Stock, Workspace", if any.
    overrode: Option<String>,
    /// Why a layer's value was ignored.
    notes: Vec<String>,
}

/// The panel's sections, each a heading and its rows: Behaviors, models, reviews, and the
/// instructions agents get on top. `space` names the project's Space.
fn sections(effective: &Effective, space: &str) -> Vec<(&'static str, Vec<Row>)> {
    let settings = &effective.settings;
    let repository = settings.repository.as_deref();
    let row = |setting: String, value: String, resolved: &Resolved<()>| {
        let overrode = (!resolved.overrode.is_empty()).then(|| {
            let layers: Vec<String> = (resolved.overrode.iter())
                .map(|layer| layer_label(*layer, space, repository))
                .collect();
            format!("Overrode {}", layers.join(", "))
        });
        Row {
            setting,
            value,
            from: resolved.from,
            from_label: layer_label(resolved.from, space, repository),
            overrode,
            notes: resolved.notes.clone(),
        }
    };
    let behaviors = (settings.behaviors.iter())
        .map(|resolved| {
            let behavior = effective.catalog.iter().find(|b| b.id == resolved.id);
            let name = behavior.map_or(resolved.id.clone(), |b| b.name.clone());
            let value = (behavior.and_then(|b| b.choice(&resolved.resolved.value)))
                .map_or(resolved.resolved.value.clone(), |c| c.label.clone());
            row(name, value, &erase(&resolved.resolved))
        })
        .collect();
    let line_value = |line: &Line| {
        let then = match &line.then {
            Then::Model(then) => then.short(),
            Then::Wait => "wait".into(),
            Then::Skip => "skip".into(),
        };
        format!("{}, then {then}", line.first.short())
    };
    let mut models = vec![];
    for resolved in &settings.models {
        let job = &resolved.job;
        let value = line_value(&resolved.resolved.value.main);
        models.push(row(
            format!("{} model", job.name()),
            value,
            &erase(&resolved.resolved),
        ));
        // Small work resolves on its own and may come from another layer: it runs its row's
        // small-work line, or that row's main line without one. Listed when that differs.
        let small = (settings.small_models.iter()).find(|small| small.job.is(job));
        if let Some(small) = small {
            let row_used = &small.resolved.value;
            let line = row_used.small.as_ref().unwrap_or(&row_used.main);
            let differs = *line != resolved.resolved.value.main
                || small.resolved.from != resolved.resolved.from;
            if differs {
                let setting = format!("{} model, small work", job.name());
                models.push(row(setting, line_value(line), &erase(&small.resolved)));
            }
        }
    }
    let reviews = &settings.reviews;
    let reviews = vec![
        row(
            "Review levels".into(),
            reviews.levels.value.join(", "),
            &erase(&reviews.levels),
        ),
        row(
            "Rounds per review".into(),
            reviews.max_rounds.value.to_string(),
            &erase(&reviews.max_rounds),
        ),
        row(
            "Full reviews per ticket".into(),
            reviews.max_cycles.value.to_string(),
            &erase(&reviews.max_cycles),
        ),
    ];
    let mut additions: Vec<(Layer, usize)> = vec![];
    for addition in &settings.additions {
        let lines = addition
            .text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count();
        match additions
            .iter_mut()
            .find(|(layer, _)| *layer == addition.layer)
        {
            Some((_, count)) => *count += lines,
            None => additions.push((addition.layer, lines)),
        }
    }
    let added = match additions.is_empty() {
        true => "None at any layer".to_owned(),
        false => (additions.iter())
            .map(|(layer, lines)| {
                format!(
                    "{} ({})",
                    layer_label(*layer, space, repository),
                    wording::counted(*lines, "line")
                )
            })
            .collect::<Vec<_>>()
            .join(" · "),
    };
    let fixed = |setting: &str, value: String, note: &str| Row {
        setting: setting.into(),
        value,
        from: Layer::Stock,
        from_label: note.into(),
        overrode: None,
        notes: vec![],
    };
    let instructions = vec![
        fixed("Additions", added, "in this order, after the Behaviors"),
        fixed(
            "Repository instructions",
            "AGENTS.md or CLAUDE.md".into(),
            "read by the provider",
        ),
    ];
    vec![
        ("Behaviors", behaviors),
        ("Models", models),
        ("Reviews", reviews),
        ("Instructions", instructions),
    ]
}

/// The provenance of a resolved value, without the value.
fn erase<T>(resolved: &Resolved<T>) -> Resolved<()> {
    Resolved {
        value: (),
        from: resolved.from,
        overrode: resolved.overrode.clone(),
        notes: resolved.notes.clone(),
    }
}

/// A layer as the panel names it: the Space by its name, and a repository within it.
fn layer_label(layer: Layer, space: &str, repository: Option<&str>) -> String {
    match layer {
        Layer::Space => space.to_owned(),
        Layer::Repository => {
            let name = repository.map_or("The repository", |r| r.rsplit('/').next().unwrap_or(r));
            format!("{name} in {space}")
        }
        other => other.name().to_owned(),
    }
}

/// The panel's body: what the agents get, section by section.
pub(super) fn panel(effective: &Effective, space: &str, p: Palette) -> Div {
    let whose = match &effective.settings.repository {
        Some(repository) => format!("What this ticket's agents get, on {repository}"),
        None => "What this project's agents get".into(),
    };
    let mut body = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .child(div().font_weight(FontWeight::SEMIBOLD).child(whose))
                .child(hint(
                    "Read-only. Each agent resolves these when it starts; change them on Behaviors and Models.",
                    p,
                )),
        );
    for (title, rows) in sections(effective, space) {
        if rows.is_empty() {
            continue;
        }
        let mut list = div()
            .flex()
            .flex_col()
            .child(section(title.to_uppercase(), p));
        for row in rows {
            let chip = match row.setting.as_str() {
                // These rows say where they come from in words, not as a layer.
                "Additions" | "Repository instructions" => hint(row.from_label, p),
                _ => from_chip(row.from, row.from_label, p),
            };
            list = list.child(
                div()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .border_b_1()
                    .border_color(p.edge.opacity(edge::CARD))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .gap_2()
                            .child(div().font_weight(FontWeight::MEDIUM).child(row.setting))
                            .child(div().text_right().child(row.value)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(chip)
                            .children(row.overrode.map(|overrode| hint(overrode, p))),
                    )
                    .children(
                        (row.notes.into_iter()).map(|note| hint(note, p).text_color(p.yellow)),
                    ),
            );
        }
        body = body.child(list);
    }
    body
}

#[cfg(test)]
mod tests {
    use super::{Effective, Row, sections};
    use workspace_core::cascade::*;
    use workspace_core::*;

    fn resolved<T>(value: T, from: Layer, overrode: &[Layer]) -> Resolved<T> {
        Resolved {
            value,
            from,
            overrode: overrode.to_vec(),
            notes: vec![],
        }
    }
    fn choice(value: &str, label: &str) -> Choice {
        Choice {
            value: value.into(),
            label: label.into(),
            sentence: String::new(),
        }
    }

    #[test]
    fn a_tickets_settings_in_a_space_say_where_each_value_came_from() {
        let publishing = Behavior {
            id: "publishing".into(),
            name: "Publishing".into(),
            skill: Skill::Coordinating,
            layers: vec![Layer::Workspace, Layer::Space, Layer::Repository],
            choices: vec![
                choice("allowed_owners", "As allowed owners say"),
                choice("always_ask", "Always ask first"),
            ],
            tightens: true,
        };
        let opus = ModelProfile {
            provider: Provider::Claude,
            model: "opus".into(),
            effort: "high".into(),
        };
        let reviewer = JobRow {
            job: Job::Verifier {
                focus: "Claude reviewer".into(),
            },
            main: Line {
                first: opus,
                then: Then::Wait,
            },
            small: None,
        };
        // Small work falls back to this Mac's row, whose small-work line is Sonnet.
        let this_macs_small = JobRow {
            small: Some(Line {
                first: ModelProfile {
                    provider: Provider::Claude,
                    model: "sonnet".into(),
                    effort: "medium".into(),
                },
                then: Then::Wait,
            }),
            ..reviewer.clone()
        };
        // Sage's Implementer row runs its main line here, but its small-work line is on Codex,
        // signed out: small work falls back to this Mac's row, which has no small-work line, so
        // it runs that row's main line, Opus.
        let profile = |provider, model: &str, effort: &str| ModelProfile {
            provider,
            model: model.into(),
            effort: effort.into(),
        };
        let line = |first| Line {
            first,
            then: Then::Wait,
        };
        let sage_implementer = JobRow {
            job: Job::Implementer,
            main: line(profile(Provider::Claude, "sonnet", "high")),
            small: Some(line(profile(Provider::Codex, "gpt-6.1-sol", "high"))),
        };
        let this_macs_implementer = JobRow {
            job: Job::Implementer,
            main: line(profile(Provider::Claude, "opus", "high")),
            small: None,
        };
        // A row whose small work is its main line, from the same layer, is listed once.
        let tester = JobRow {
            job: Job::Tester,
            main: line(profile(Provider::Claude, "sonnet", "medium")),
            small: None,
        };
        let mut levels = resolved(vec!["Full".to_owned()], Layer::Space, &[Layer::Stock]);
        levels.notes = vec!["The Space's levels Quick are not configured here".into()];
        let settings = EffectiveSettings {
            space_id: SpaceId("sage".into()),
            repository: Some("Sage-Social/sagechat-sms".into()),
            behaviors: vec![ResolvedBehavior {
                id: "publishing".into(),
                resolved: resolved(
                    "always_ask".into(),
                    Layer::Repository,
                    &[Layer::Stock, Layer::Space],
                ),
            }],
            additions: vec![
                Addition {
                    layer: Layer::Space,
                    skill: None,
                    text: "Never log child PII.\n\nFail closed on auth paths.".into(),
                },
                Addition {
                    layer: Layer::Repository,
                    skill: None,
                    text: "Verify the Telnyx signature first.".into(),
                },
            ],
            models: vec![
                ResolvedRow {
                    job: reviewer.job.clone(),
                    resolved: resolved(reviewer.clone(), Layer::Space, &[Layer::ThisMac]),
                },
                ResolvedRow {
                    job: Job::Implementer,
                    resolved: resolved(sage_implementer.clone(), Layer::Space, &[Layer::ThisMac]),
                },
                ResolvedRow {
                    job: Job::Tester,
                    resolved: resolved(tester.clone(), Layer::ThisMac, &[]),
                },
            ],
            small_models: vec![
                ResolvedRow {
                    job: reviewer.job.clone(),
                    resolved: resolved(this_macs_small, Layer::ThisMac, &[]),
                },
                ResolvedRow {
                    job: Job::Implementer,
                    resolved: resolved(this_macs_implementer, Layer::ThisMac, &[]),
                },
                ResolvedRow {
                    job: Job::Tester,
                    resolved: resolved(tester, Layer::ThisMac, &[]),
                },
            ],
            reviews: ResolvedReviews {
                levels,
                max_rounds: resolved(2, Layer::Workspace, &[Layer::Stock]),
                max_cycles: resolved(3, Layer::Space, &[Layer::Stock, Layer::Workspace]),
            },
        };
        let effective = Effective {
            settings,
            catalog: vec![publishing],
        };
        let sections = sections(&effective, "Sage");
        let titles: Vec<&str> = sections.iter().map(|(title, _)| *title).collect();
        assert_eq!(titles, ["Behaviors", "Models", "Reviews", "Instructions"]);
        assert_eq!(
            sections[0].1,
            [Row {
                setting: "Publishing".into(),
                value: "Always ask first".into(),
                from: Layer::Repository,
                from_label: "sagechat-sms in Sage".into(),
                overrode: Some("Overrode Stock, Sage".into()),
                notes: vec![],
            }]
        );
        let model = &sections[1].1[0];
        assert_eq!(
            (model.setting.as_str(), model.value.as_str()),
            ("Claude reviewer model", "Opus · high, then wait")
        );
        assert_eq!(
            (model.from_label.as_str(), model.overrode.as_deref()),
            ("Sage", Some("Overrode This Mac"))
        );
        let small = &sections[1].1[1];
        assert_eq!(
            (small.setting.as_str(), small.value.as_str()),
            (
                "Claude reviewer model, small work",
                "Sonnet · medium, then wait"
            )
        );
        assert_eq!(
            (small.from, small.from_label.as_str()),
            (Layer::ThisMac, "This Mac")
        );
        let listed: Vec<(&str, &str, Layer)> = (sections[1].1.iter())
            .skip(2)
            .map(|row| (row.setting.as_str(), row.value.as_str(), row.from))
            .collect();
        assert_eq!(
            listed,
            [
                (
                    "Implementer model",
                    "Sonnet · high, then wait",
                    Layer::Space
                ),
                (
                    "Implementer model, small work",
                    "Opus · high, then wait",
                    Layer::ThisMac
                ),
                ("Tester model", "Sonnet · medium, then wait", Layer::ThisMac),
            ]
        );
        let reviews = &sections[2].1;
        assert_eq!(reviews[0].notes.len(), 1, "an ignored value says why");
        assert_eq!(reviews[1].from_label, "Workspace");
        assert_eq!(reviews[2].value, "3");
        assert_eq!(
            sections[3].1[0].value,
            "Sage (2 lines) · sagechat-sms in Sage (1 line)"
        );
    }
}
