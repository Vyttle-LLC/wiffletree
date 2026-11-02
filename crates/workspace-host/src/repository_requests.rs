//! A coordinator asking for a repository its project does not use, and the human's answer.
//! Approving adds it to that one project: no Space's list changes.
use crate::repositories::{candidate, origin_identity};
use crate::*;
use std::collections::BTreeSet;

const MAX_REASON_BYTES: usize = 200;

fn in_workspace(repository: Repository, reason: &str) -> RepositoryRequest {
    RepositoryRequest {
        identity: repository.github_identity(),
        repository_id: Some(repository.id),
        name: repository.name,
        path: repository.path,
        reason: reason.to_owned(),
    }
}

fn in_folder(folder: RepositoryCandidate, reason: &str) -> RepositoryRequest {
    RepositoryRequest {
        repository_id: None,
        identity: origin_identity(Path::new(&folder.path)),
        name: folder.name,
        path: folder.path,
        reason: reason.to_owned(),
    }
}

/// A linked worktree has a Git directory of its own, apart from its main clone's. A submodule
/// does not count: its two are the same.
fn is_linked_worktree(path: &Path) -> Result<bool> {
    let dirs = runtime::git_output(path, &["rev-parse", "--git-dir", "--git-common-dir"])?;
    let dirs: Vec<&str> = dirs.lines().collect();
    Ok(dirs.first() != dirs.last())
}

impl Host {
    /// `request_repository`: raises the human's card, unless the project already uses the
    /// repository or the same request still waits.
    pub(crate) fn request_repository(&mut self, caller: &Session, args: &Value) -> Result<Value> {
        ensure!(
            caller.role == Role::ProjectOrchestrator,
            "Only the project coordinator requests repositories"
        );
        service::only_arguments("request_repository", args, &["repository", "reason"])?;
        let asked = args["repository"].as_str().context("Missing repository")?;
        let reason = args["reason"].as_str().context("Missing reason")?.trim();
        text(reason, MAX_REASON_BYTES)?;
        ensure!(!reason.contains('\n'), "Give reason as one short line");
        let project = self.project(&caller.project_id)?;
        let request = self.requested(&project, asked.trim(), reason)?;
        if self.uses_path(&project, &request.path)? {
            return Ok(json!({"status": "already_used", "repository": request.name,
                "repository_id": request.repository_id,
                "next": "This project already uses it; workspace_context lists it."}));
        }
        let waiting = |request: &RepositoryRequest| {
            json!({"status": "waiting_for_human", "repository": request.name,
                "next": "Finish your turn. Wiffletree messages you the human's answer."})
        };
        let open = self.open_attention(&project.id)?;
        let asked_before = (open.iter())
            .filter_map(|item| item.repository_request.as_ref())
            .find(|open| open.path == request.path);
        if let Some(open) = asked_before {
            return Ok(waiting(open));
        }
        ensure!(
            Path::new(&request.path).exists(),
            "{} is in the workspace, but its folder {} has gone from this Mac",
            request.name,
            request.path
        );
        self.raise_attention(
            &caller.id,
            "local",
            &format!("{REPOSITORY_REQUEST_PREFIX}{}", new_id()),
            &format!("{}\n{reason}", request.title()),
            &[],
            Some(request.clone()),
        )?;
        Ok(waiting(&request))
    }

    /// Finds what `asked` names, refusing anything no card could offer. Among several clones of
    /// a name, one this project uses comes first, so the request reports it as used.
    fn requested(&self, project: &Project, asked: &str, reason: &str) -> Result<RepositoryRequest> {
        let repositories = self.repositories()?;
        if asked.starts_with('/') {
            let path = fs::canonicalize(asked)
                .with_context(|| format!("Folder {asked} does not exist on this Mac"))?;
            let known = (repositories.into_iter()).find(|r| Path::new(&r.path) == path);
            let Some(repository) = known else {
                let folder = candidate(&path, &BTreeSet::new())?;
                ensure!(
                    !is_linked_worktree(&path)?,
                    "{asked} is a linked Git worktree. Ask for its main clone instead"
                );
                return Ok(in_folder(folder, reason));
            };
            return Ok(in_workspace(repository, reason));
        }
        ensure!(
            validate_repository_identity(asked).is_ok(),
            "Give repository as owner/name or an absolute folder path"
        );
        let mut matches: Vec<Repository> = (repositories.into_iter())
            .filter(|r| r.is_github(asked))
            .collect();
        let used = self.project_repositories(&project.id)?;
        matches.sort_by_key(|r| !used.iter().any(|u| u.id == r.id));
        ensure!(
            !matches.is_empty(),
            "{asked} is not in the workspace. It must be cloned first: clone it on this Mac, then ask again with its absolute folder path"
        );
        ensure!(
            matches.len() == 1 || used.iter().any(|u| u.id == matches[0].id),
            "{asked} has several clones in the workspace, so ask again with one's folder path: {}",
            (matches.iter().map(|r| r.path.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        );
        Ok(in_workspace(matches.remove(0), reason))
    }

    /// Whether `project` uses the workspace repository in the folder `path`.
    fn uses_path(&self, project: &Project, path: &str) -> Result<bool> {
        let used = self.project_repositories(&project.id)?;
        Ok(used.iter().any(|r| r.path == path))
    }

    /// Answers an Inbox item, with a repository request's change and result in the same
    /// transaction: a crash leaves it waiting, so it can neither apply twice nor go unanswered.
    pub(crate) fn answer_attention(&mut self, id: &str, answer: &str) -> Result<Attention> {
        let item: Attention =
            self.db
                .query_row("SELECT data FROM attention WHERE id=?1", [id], |r| {
                    decode(r, 0)
                })?;
        if !item.is_repository_request() {
            return self.resolve_attention(id, answer);
        }
        self.atomically(|host| {
            let attention = host.resolve_attention(id, answer)?;
            let request = (attention.repository_request.as_ref())
                .context("The request is missing from its Inbox item")?;
            let name = &request.name;
            let result = if answer != "approved" {
                format!(
                    "The human denied your request for {name}. Nothing changed. Do not add it another way."
                )
            } else {
                match host.atomically(|host| host.add_requested(&attention, request)) {
                    Ok(repository) => format!(
                        "The human approved your request: {name} is in this project now, as repository_id {}, and you can use it at once. No Space's list changed.",
                        repository.id
                    ),
                    Err(why) => format!(
                        "{name} was not added although the human approved it: {why:#}. Nothing changed."
                    ),
                }
            };
            host.send_repository_result(&attention.operation_id, &attention.session_id, &result)?;
            Ok(attention)
        })
    }

    /// Adds what the card showed to the project that asked, or says why it no longer can.
    fn add_requested(
        &mut self,
        attention: &Attention,
        request: &RepositoryRequest,
    ) -> Result<Repository> {
        ensure!(
            !self.session(&attention.session_id)?.archived,
            "the project's coordinator was archived"
        );
        let project = &attention.project_id;
        if let Some(id) = &request.repository_id {
            let repository = (self.repository(id).ok())
                .context("it was removed from the workspace after you asked")?;
            ensure!(
                Path::new(&repository.path).exists(),
                "its folder {} has gone from this Mac",
                repository.path
            );
            self.include_repositories(project, std::slice::from_ref(id))?;
            return Ok(repository);
        }
        let path = fs::canonicalize(&request.path)
            .ok()
            .with_context(|| format!("the folder {} has gone from this Mac", request.path))?;
        ensure!(
            path == Path::new(&request.path),
            "the folder {} now leads to {}, which the human did not see",
            request.path,
            path.display()
        );
        ensure!(
            origin_identity(&path) == request.identity,
            "the folder {}'s origin changed after you asked",
            request.path
        );
        let found = (self.repositories()?.into_iter()).find(|r| r.path == request.path);
        if let Some(repository) = found {
            // A root check added it while the card waited; its base stays as it is.
            self.include_repositories(project, std::slice::from_ref(&repository.id))?;
            return Ok(repository);
        }
        let folder = candidate(&path, &BTreeSet::new())?;
        self.attach_repository(project, &folder.path, &folder.base)
    }
}
