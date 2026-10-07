use super::*;
use gpui_component::input::SelectAll;

pub(super) struct NameEdit {
    pub project_id: String,
    original: String,
    pub saving: bool,
    pub error: String,
}

impl Workspace {
    pub(super) fn new_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.creating_project || self.project_edit.as_ref().is_some_and(|e| e.saving) {
            return;
        }
        if self.project_edit.is_some() {
            self.project_name
                .update(cx, |input, cx| input.focus(window, cx));
            return;
        }
        let name = next_name(self.snapshot.as_ref());
        self.creating_project = true;
        self.request(Command::CreateProject { name }, window, cx);
        cx.notify();
    }

    pub(super) fn begin_project_rename(
        &mut self,
        project_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project_edit.as_ref().is_some_and(|e| e.saving) {
            return;
        }
        let Some(project) = self
            .snapshot
            .as_ref()
            .and_then(|s| s.projects.iter().find(|p| p.id == project_id))
        else {
            return;
        };
        let original = project.name.clone();
        self.project_name.update(cx, |input, cx| {
            input.set_value(original.clone(), window, cx)
        });
        self.project_edit = Some(NameEdit {
            project_id: project_id.clone(),
            original,
            saving: false,
            error: String::new(),
        });
        // Focus after the sidebar input exists in the next frame's dispatch tree.
        cx.on_next_frame(window, move |view, window, cx| {
            if view
                .project_edit
                .as_ref()
                .is_some_and(|e| e.project_id == project_id && !e.saving)
            {
                view.project_name
                    .update(cx, |input, cx| input.focus(window, cx));
                // Actions follow the drawn focus, so select once the focused input has rendered.
                cx.on_next_frame(window, |_, window, cx| {
                    window.dispatch_action(Box::new(SelectAll), cx);
                });
            }
        });
        cx.notify();
    }

    pub(super) fn save_project_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.project_edit else {
            return;
        };
        if edit.saving {
            return;
        }
        let name = self.project_name.read(cx).value().trim().to_string();
        if name.is_empty() || name == edit.original {
            self.project_edit = None;
            cx.notify();
            return;
        }
        if name.len() > 128 {
            edit.error = "Use a name of at most 128 bytes.".into();
            cx.notify();
            return;
        }
        edit.saving = true;
        edit.error.clear();
        let project_id = edit.project_id.clone();
        self.request(Command::RenameProject { project_id, name }, window, cx);
        cx.notify();
    }

    pub(super) fn cancel_project_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project_edit.as_ref().is_some_and(|e| !e.saving) {
            self.project_edit = None;
            window.blur(cx);
            cx.notify();
        }
    }
}

fn next_name(snapshot: Option<&Snapshot>) -> String {
    let names: std::collections::HashSet<_> = snapshot
        .into_iter()
        .flat_map(|s| &s.projects)
        .map(|p| p.name.as_str())
        .collect();
    if !names.contains("New project") {
        return "New project".into();
    }
    (2..)
        .map(|i| format!("New project {i}"))
        .find(|name| !names.contains(name.as_str()))
        .unwrap()
}
