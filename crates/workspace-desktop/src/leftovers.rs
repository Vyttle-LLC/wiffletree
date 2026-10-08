//! The leftover-worktrees sheet: a dry-run listing first, then removal of only what the human
//! selects and confirms. Branches are always kept.
use super::*;
use gpui_component::{
    ActiveTheme, Disableable, Sizable, button::ButtonVariants, checkbox::Checkbox,
};
use std::collections::BTreeSet;
use ui::{hint, mono, pill, section};

/// What the human has selected from a listing. Nothing starts selected, and only clean or
/// unpushed rows can be.
pub struct LeftoverSelection {
    listing: Vec<LeftoverWorktree>,
    selected: BTreeSet<String>,
    confirming: bool,
}

impl LeftoverSelection {
    pub fn new(listing: Vec<LeftoverWorktree>) -> Self {
        Self {
            listing,
            selected: BTreeSet::new(),
            confirming: false,
        }
    }
    /// Rows by project, in listing order; worktrees no ticket records come last.
    pub fn groups(&self) -> Vec<(String, Vec<&LeftoverWorktree>)> {
        let mut groups: Vec<(String, Vec<&LeftoverWorktree>)> = vec![];
        for leftover in &self.listing {
            let project = if leftover.project.is_empty() {
                LeftoverClass::UntrackedByWiffletree.label().to_owned()
            } else {
                leftover.project.clone()
            };
            match groups.iter_mut().find(|(name, _)| *name == project) {
                Some((_, rows)) => rows.push(leftover),
                None => groups.push((project, vec![leftover])),
            }
        }
        groups
    }
    pub fn selectable(leftover: &LeftoverWorktree) -> bool {
        leftover.ticket_id.is_some() && leftover.class.is_removable()
    }
    /// Selects or clears a removable row; protected rows never change.
    pub fn toggle(&mut self, ticket_id: &str) {
        let removable = self
            .listing
            .iter()
            .any(|l| l.ticket_id.as_deref() == Some(ticket_id) && Self::selectable(l));
        if removable && !self.selected.remove(ticket_id) {
            self.selected.insert(ticket_id.to_owned());
        }
        self.confirming = false;
    }
    pub fn is_selected(&self, ticket_id: &str) -> bool {
        self.selected.contains(ticket_id)
    }
    /// Asks for confirmation, naming the count; `None` when nothing is selected.
    pub fn confirm(&mut self) -> Option<String> {
        self.confirming = !self.selected.is_empty();
        self.confirmation()
    }
    pub fn confirmation(&self) -> Option<String> {
        let count = self.selected.len();
        (self.confirming && count > 0).then(|| {
            format!(
                "Remove {count} leftover {}? Their branches and commits are kept.",
                if count == 1 { "worktree" } else { "worktrees" }
            )
        })
    }
    pub fn cancel(&mut self) {
        self.confirming = false;
    }
    /// The removal the human confirmed, or `None` before confirmation.
    pub fn command(&self) -> Option<Command> {
        self.confirmation()
            .map(|_| Command::RemoveLeftoverWorktrees {
                ticket_ids: self.selected.iter().cloned().collect(),
            })
    }
}

pub(super) struct LeftoverSheet {
    bridge: Bridge,
    selection: Option<LeftoverSelection>,
    outcomes: Vec<LeftoverRemoval>,
    error: Option<String>,
    pending: bool,
}

impl LeftoverSheet {
    pub(super) fn new(bridge: Bridge, cx: &mut Context<Self>) -> Self {
        let mut sheet = Self {
            bridge,
            selection: None,
            outcomes: vec![],
            error: None,
            pending: false,
        };
        sheet.list(cx);
        sheet
    }

    fn list(&mut self, cx: &mut Context<Self>) {
        self.ask(Command::LeftoverWorktrees, false, cx, |sheet, value| {
            sheet.selection = Some(LeftoverSelection::new(serde_json::from_value(value)?));
            Ok(())
        });
    }

    fn remove(&mut self, cx: &mut Context<Self>) {
        let Some(command) = self.selection.as_ref().and_then(LeftoverSelection::command) else {
            return;
        };
        // The listing after removal shows what was kept.
        self.ask(command, true, cx, |sheet, value| {
            sheet.outcomes = serde_json::from_value(value)?;
            sheet.selection = None;
            Ok(())
        });
    }

    fn ask(
        &mut self,
        command: Command,
        then_list: bool,
        cx: &mut Context<Self>,
        apply: impl FnOnce(&mut Self, serde_json::Value) -> serde_json::Result<()> + 'static,
    ) {
        let receiver = match self.bridge.request(command) {
            Ok(receiver) => receiver,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        self.pending = true;
        cx.spawn(async move |this, cx| {
            let answer = receiver
                .recv()
                .await
                .unwrap_or_else(|_| Err("Host disconnected".into()));
            let _ = this.update(cx, |sheet, cx| {
                sheet.error = answer
                    .and_then(|value| apply(sheet, value).map_err(|e| e.to_string()))
                    .err();
                sheet.pending = false;
                if then_list {
                    sheet.list(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn row(&self, leftover: &LeftoverWorktree, p: Palette, cx: &mut Context<Self>) -> Div {
        let selectable = LeftoverSelection::selectable(leftover);
        let id = leftover.ticket_id.clone().unwrap_or_default();
        let selected = self.selection.as_ref().is_some_and(|s| s.is_selected(&id));
        let title = if leftover.title.is_empty() {
            leftover.branch.clone()
        } else {
            format!(
                "{} · {} · {}",
                leftover.repository, leftover.title, leftover.state
            )
        };
        let head: String = leftover.head.chars().take(7).collect();
        div()
            .py_1()
            .flex()
            .items_start()
            .gap_2()
            .child(
                Checkbox::new(SharedString::from(format!(
                    "leftover-{}",
                    leftover.worktree
                )))
                .checked(selected)
                .disabled(!selectable || self.pending)
                .on_click(cx.listener(move |sheet, _: &bool, _, cx| {
                    if let Some(selection) = &mut sheet.selection {
                        selection.toggle(&id);
                    }
                    cx.notify();
                })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().text_ellipsis().child(title))
                    .child(mono(leftover.worktree.clone(), p))
                    .child(hint(format!("{} at {head}", leftover.branch), p)),
            )
            .child(pill(
                leftover.class.label(),
                if leftover.class.is_removable() {
                    p.subtle
                } else {
                    p.red
                },
            ))
    }
}

impl Render for LeftoverSheet {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx.theme().is_dark());
        let mut body = div().flex().flex_col().gap_3().child(hint(
            "Worktrees left by tickets finished before Wiffletree removed them. Nothing is removed until you select rows and confirm; branches are always kept. Protected rows cannot be selected.",
            p,
        ));
        for outcome in &self.outcomes {
            body = body.child(hint(
                match &outcome.reason {
                    None => format!("Removed the worktree of {}", outcome.ticket_id),
                    Some(reason) => format!("Kept {}: {reason}", outcome.ticket_id),
                },
                p,
            ));
        }
        let Some(selection) = &self.selection else {
            return body.child(hint("Listing leftover worktrees…", p));
        };
        let groups = selection.groups();
        if groups.is_empty() {
            body = body.child(hint("No leftover worktrees.", p));
        }
        for (project, rows) in groups {
            let mut group = div()
                .flex()
                .flex_col()
                .child(section(project.to_uppercase(), p));
            for leftover in rows {
                group = group.child(self.row(leftover, p, cx));
            }
            body = body.child(group);
        }
        let confirmation = selection.confirmation();
        body = body.child(match confirmation {
            None => div().flex().child(
                button("remove-leftovers")
                    .label("Remove selected…")
                    .disabled(self.pending || selection.selected.is_empty())
                    .on_click(cx.listener(|sheet, _, _, cx| {
                        if let Some(selection) = &mut sheet.selection {
                            selection.confirm();
                        }
                        cx.notify();
                    })),
            ),
            Some(question) => div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().flex_1().child(question))
                .child(
                    button("cancel-leftover-removal")
                        .ghost()
                        .small()
                        .label("Cancel")
                        .on_click(cx.listener(|sheet, _, _, cx| {
                            if let Some(selection) = &mut sheet.selection {
                                selection.cancel();
                            }
                            cx.notify();
                        })),
                )
                .child(
                    button("confirm-leftover-removal")
                        .danger()
                        .small()
                        .label("Remove")
                        .disabled(self.pending)
                        .on_click(cx.listener(|sheet, _, _, cx| sheet.remove(cx))),
                ),
        });
        body.children(
            self.error
                .clone()
                .map(|error| hint(error, p).text_color(p.red)),
        )
    }
}

#[cfg(test)]
mod tests {
    // Only what the tests need: the parent's glob would bring GPUI's own `test` attribute.
    use super::LeftoverSelection;
    use std::path::Path;
    use workspace_core::*;
    use workspace_host::Host;

    fn git(path: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(path)
            .args(["-c", "user.name=T", "-c", "user.email=t@example.invalid"])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    /// A temporary store whose closed "Kept clean" and "Has notes" tickets left their worktrees.
    fn store() -> (tempfile::TempDir, Host, Ticket, Ticket) {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("web");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "Base"]);
        let mut host = Host::open(directory.path().join("home")).unwrap();
        host.set_workspaces_dir(directory.path().join("workspaces").to_str().unwrap())
            .unwrap();
        let project = host.create_project("Old work").unwrap();
        let root = host.sessions().unwrap().remove(0);
        let repository = host
            .attach_repository(&project.id, repo.to_str().unwrap(), "HEAD")
            .unwrap();
        let clean = host
            .create_ticket(&root.id, &repository.id, "Kept clean", "Done")
            .unwrap();
        let dirty = host
            .create_ticket(&root.id, &repository.id, "Has notes", "Done")
            .unwrap();
        std::fs::write(Path::new(&dirty.worktree).join("notes.md"), "draft").unwrap();
        // Closed by a release that did not remove worktrees.
        rusqlite::Connection::open(directory.path().join("home/workspace.sqlite3"))
            .unwrap()
            .execute_batch("UPDATE tickets SET data=json_set(data,'$.state','closed')")
            .unwrap();
        (directory, host, clean, dirty)
    }

    #[test]
    fn nothing_starts_selected_and_protected_rows_cannot_be_selected() {
        let (_directory, host, clean, dirty) = store();
        let mut selection = LeftoverSelection::new(host.leftover_worktrees().unwrap());

        let groups = selection.groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "Old work");
        assert_eq!(groups[0].1.len(), 2);
        assert!(!selection.is_selected(&clean.id) && !selection.is_selected(&dirty.id));
        assert_eq!(selection.confirm(), None, "nothing to confirm");
        assert!(selection.command().is_none());

        selection.toggle(&dirty.id);
        assert!(!selection.is_selected(&dirty.id), "dirty work is protected");
        selection.toggle(&clean.id);
        assert!(selection.is_selected(&clean.id));
        selection.toggle(&clean.id);
        assert!(!selection.is_selected(&clean.id));
    }

    #[test]
    fn removal_needs_a_confirmation_naming_the_count_and_removes_only_the_selection() {
        let (_directory, mut host, clean, dirty) = store();
        let mut selection = LeftoverSelection::new(host.leftover_worktrees().unwrap());
        selection.toggle(&clean.id);
        assert!(selection.command().is_none(), "not before confirmation");

        assert_eq!(
            selection.confirm().as_deref(),
            Some("Remove 1 leftover worktree? Their branches and commits are kept.")
        );
        let Some(Command::RemoveLeftoverWorktrees { ticket_ids }) = selection.command() else {
            panic!("removal command")
        };
        assert_eq!(ticket_ids, [clean.id.clone()]);
        selection.cancel();
        assert!(selection.command().is_none(), "cancelling withdraws it");

        let outcomes = host.remove_leftover_worktrees(&ticket_ids).unwrap();
        assert!(outcomes[0].removed);
        assert!(!Path::new(&clean.worktree).exists());
        assert!(Path::new(&dirty.worktree).join("notes.md").exists());
    }
}
