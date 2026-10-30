use super::*;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write};

fn safe_log_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= 128
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "Repository log name must use letters, numbers, hyphens or underscores"
    );
    Ok(())
}
fn hash(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}
fn line(s: &str) -> String {
    s.lines()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("<!--", "&lt;!--")
        .replace("-->", "--&gt;")
}

fn insert_entries(content: &str, additions: &str, repository: &str) -> String {
    if content.is_empty() {
        return format!("# {repository} — working log\n\n{additions}");
    }
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        if line.starts_with("# ")
            || line.trim().is_empty()
            || line.trim_start().starts_with("<!-- compiled-through:")
        {
            offset += line.len();
        } else {
            break;
        }
    }
    format!("{}{additions}{}", &content[..offset], &content[offset..])
}
/// Releases the brain lock explicitly. Closing the file is not enough: a child
/// process forked by another thread before it execs shares the open file
/// description and would keep the flock held after this writer is done.
struct BrainLock(File);

impl Drop for BrainLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

pub(crate) fn render_log(log: &WorkLog) -> String {
    let date = DateTime::<Utc>::from_timestamp_millis(log.created_at)
        .unwrap_or_default()
        .format("%Y-%m-%d");
    let kind = match log.input.kind {
        LogKind::Decision => "decision",
        LogKind::Observation => "observation",
    };
    format!(
        "<!-- workspace-entry:{} -->\n## {date} — {}\n- type: {kind}\n- ref: {}\n- changed: {}\n- why: {}\n- state: {}\n- entry-id: {}\n- project: {}\n- task: {}\n- session: {}\n- milestone: {}\n- evidence: agent report; reference not independently verified\n\n",
        log.id,
        line(&log.input.title),
        line(&log.input.reference),
        line(&log.input.changed),
        line(&log.input.why),
        line(&log.input.state),
        log.id,
        log.project_id,
        log.task_id.as_deref().unwrap_or("—"),
        log.input.session_id,
        line(&log.input.milestone)
    )
}
impl Host {
    pub fn append_log(&mut self, input: LogInput) -> Result<WorkLog> {
        for field in [
            &input.title,
            &input.milestone,
            &input.reference,
            &input.changed,
            &input.why,
            &input.state,
        ] {
            text(field, 4096)?;
        }
        let session = self.session(&input.session_id)?;
        if let Some(existing) = self
            .db
            .query_row(
                "SELECT sequence,data FROM work_logs WHERE session_id=?1 AND milestone=?2",
                params![input.session_id, input.milestone],
                |r| {
                    let mut log: WorkLog = decode(r, 1)?;
                    log.sequence = r.get(0)?;
                    Ok(log)
                },
            )
            .optional()?
        {
            ensure!(
                existing.input == input,
                "Milestone identity reused with different content"
            );
            return Ok(existing);
        }
        let repo = session
            .repository_id
            .as_deref()
            .map(|id| self.repository(id))
            .transpose()?
            .map(|r| r.name)
            .unwrap_or_else(|| "project".into());
        safe_log_name(&repo)?;
        // A worker's task is its ticket.
        let task_id = if session.role.is_worker() {
            self.session_runtime(&session.id)?.ticket_id
        } else {
            None
        };
        let mut log = WorkLog {
            sequence: 0,
            id: new_id(),
            project_id: session.project_id.clone(),
            task_id,
            repository: repo,
            created_at: now(),
            input,
        };
        // The entry belongs to the Space its project is in now, wherever the project moves later.
        let space = self.project(&log.project_id)?.space;
        let tx = self.db.transaction()?;
        tx.execute("INSERT INTO work_logs(id,project_id,session_id,milestone,repository,data,space_id) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![log.id, log.project_id, log.input.session_id, log.input.milestone, log.repository, encode(&log)?, space.as_str()])?;
        log.sequence = tx.last_insert_rowid();
        Self::event(
            &tx,
            &log.project_id,
            Some(&log.input.session_id),
            "work_log",
            &log.input.title,
        )?;
        tx.commit()?;
        Ok(log)
    }
    pub fn logs(&self, project: &str, before: Option<i64>, limit: usize) -> Result<Vec<WorkLog>> {
        self.project(project)?;
        Ok(self.db.prepare("SELECT sequence,data FROM work_logs WHERE project_id=?1 AND sequence<?2 ORDER BY sequence DESC LIMIT ?3")?.query_map(params![project, before.unwrap_or(i64::MAX), page_limit(limit)?], |r| { let mut log: WorkLog = decode(r, 1)?; log.sequence = r.get(0)?; Ok(log) })?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn runs(&self, project: &str) -> Result<Vec<MaintenanceRun>> {
        self.project(project)?;
        self.list_data("SELECT data FROM maintenance_runs WHERE project_id=?1 ORDER BY sequence DESC LIMIT 100", [project])
    }
    pub fn export_logs(&mut self, project_id: &str) -> Result<MaintenanceRun> {
        let project = self.project(project_id)?;
        let (exported, failure) = match self.export_batch(&project) {
            Ok((exported, failures)) => (
                exported,
                (!failures.is_empty()).then(|| failures.join("; ")),
            ),
            Err(error) => (0, Some(format!("{error:#}"))),
        };
        let run = MaintenanceRun {
            id: new_id(),
            project_id: project_id.into(),
            outcome: match (&failure, exported) {
                (Some(failure), _) => format!("failed: {failure}"),
                (None, 0) => "skipped: no pending input".into(),
                (None, _) => {
                    "completed: source logs exported; semantic compilation unavailable".into()
                }
            },
            exported,
            created_at: now(),
        };
        self.db.execute(
            "INSERT INTO maintenance_runs(project_id,data) VALUES (?1,?2)",
            params![project_id, encode(&run)?],
        )?;
        if let Some(failure) = failure {
            bail!(failure);
        }
        Ok(run)
    }
    /// Exports each entry to the brain of the Space the project was in when it was logged, so a
    /// moved project's later entries go to its new Space and its earlier ones stay put. Each
    /// Space is exported on its own: one with nothing pending is never touched, and one that
    /// fails leaves the others' entries delivered. Answers how many entries were exported and
    /// each failure, naming its Space.
    fn export_batch(&mut self, project: &Project) -> Result<(usize, Vec<String>)> {
        let spaces: Vec<String> = (self.db.prepare(
            "SELECT space_id FROM work_logs WHERE project_id=?1 AND space_id IS NOT NULL GROUP BY space_id ORDER BY MIN(sequence)",
        )?)
        .query_map([&project.id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
        let (mut exported, mut failures) = (0, vec![]);
        for space in spaces {
            // A removed Space's entries have nowhere to go.
            let Some(SpaceRecord { space, local }) = self.find_space(&SpaceId(space))? else {
                continue;
            };
            let path = space.brain.as_ref().and(local.brain_path.as_deref());
            if self.pending_entries(project, &space.id, path)? == 0 {
                continue;
            }
            let result = match (&space.brain, path) {
                (None, _) if space.id == project.space => Err(anyhow::anyhow!(
                    "This project's Space has no brain; attach one before exporting logs"
                )),
                // An earlier Space without a brain keeps its entries.
                (None, _) => continue,
                (Some(_), None) => Err(anyhow::anyhow!("its brain is not on this Mac")),
                (Some(brain), Some(path)) => {
                    self.export_to_brain(project, &space.id, path, &brain.folder)
                }
            };
            match result {
                Ok(count) => exported += count,
                Err(error) => failures.push(format!("{}: {error:#}", space.name)),
            }
        }
        Ok((exported, failures))
    }
    /// How many of the project's entries logged in `space` have not reached `brain`, or no brain
    /// at all when there is none on this Mac.
    fn pending_entries(
        &self,
        project: &Project,
        space: &SpaceId,
        brain: Option<&str>,
    ) -> Result<i64> {
        Ok(self.db.query_row(
            "SELECT count(*) FROM work_logs l WHERE project_id=?1 AND space_id=?2 AND NOT EXISTS(SELECT 1 FROM log_exports e WHERE e.log_id=l.id AND (?3 IS NULL OR e.brain=?3))",
            params![project.id, space.as_str(), brain],
            |r| r.get(0),
        )?)
    }
    /// Exports the project's entries logged in `space` that `brain` has not received to
    /// `<brain>/<folder>/logs/`.
    fn export_to_brain(
        &mut self,
        project: &Project,
        space: &SpaceId,
        brain: &str,
        folder: &str,
    ) -> Result<usize> {
        let root = fs::canonicalize(brain)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".workspace-brain.lock"))?;
        lock.try_lock().context("Another writer owns this brain")?;
        let _lock = BrainLock(lock);
        let logs: Vec<WorkLog> = self.list_data("SELECT data FROM work_logs l WHERE project_id=?1 AND space_id=?3 AND NOT EXISTS(SELECT 1 FROM log_exports e WHERE e.log_id=l.id AND e.brain=?2) ORDER BY sequence ASC LIMIT 100", params![project.id, brain, space.as_str()])?;
        if logs.is_empty() {
            return Ok(0);
        }
        let brain_directory = root.join(folder);
        if brain_directory.exists() {
            ensure!(
                fs::canonicalize(&brain_directory)?.starts_with(&root),
                "Brain directory escapes attachment"
            );
        }
        let directory = brain_directory.join("logs");
        if directory.exists() {
            ensure!(
                fs::canonicalize(&directory)?.starts_with(&root),
                "Brain log directory escapes attachment"
            );
        }
        fs::create_dir_all(&directory)?;
        let canonical_directory = fs::canonicalize(&directory)?;
        ensure!(
            canonical_directory.starts_with(&root),
            "Brain log directory escapes attachment"
        );
        let mut grouped: BTreeMap<String, Vec<WorkLog>> = BTreeMap::new();
        for log in logs {
            safe_log_name(&log.repository)?;
            grouped.entry(log.repository.clone()).or_default().push(log);
        }
        let mut exported = 0;
        for (repository, logs) in grouped {
            let path = canonical_directory.join(format!("{repository}.md"));
            if let Ok(meta) = fs::symlink_metadata(&path) {
                ensure!(
                    !meta.file_type().is_symlink() && meta.is_file(),
                    "Refusing symlink or non-file log destination"
                );
                ensure!(
                    meta.len() <= 16 * 1024 * 1024,
                    "Log exceeds 16 MiB export bound; archive explicitly"
                );
            }
            let old = if path.exists() {
                fs::read(&path)?
            } else {
                Vec::new()
            };
            let content = std::str::from_utf8(&old).context("Existing Markdown is not UTF-8")?;
            let additions = logs
                .iter()
                .rev()
                .filter(|log| !content.contains(&format!("<!-- workspace-entry:{} -->", log.id)))
                .map(render_log)
                .collect::<String>();
            if !additions.is_empty() {
                ensure!(
                    additions.len() + old.len() <= 16 * 1024 * 1024,
                    "Export would exceed 16 MiB log bound; archive explicitly"
                );
                let mut temp = tempfile::NamedTempFile::new_in(&canonical_directory)?;
                if path.exists() {
                    temp.as_file()
                        .set_permissions(fs::metadata(&path)?.permissions())?;
                }
                temp.write_all(insert_entries(content, &additions, &repository).as_bytes())?;
                temp.as_file().sync_all()?;
                let current = if path.exists() {
                    fs::read(&path)?
                } else {
                    Vec::new()
                };
                ensure!(
                    hash(&old) == hash(&current),
                    "External brain edit detected; export held"
                );
                temp.persist(&path).map_err(|e| e.error)?;
                File::open(&canonical_directory)?.sync_all()?;
            }
            let tx = self.db.transaction()?;
            for log in &logs {
                tx.execute(
                    "INSERT OR IGNORE INTO log_exports VALUES (?1,?2)",
                    params![log.id, brain],
                )?;
            }
            tx.commit()?;
            exported += logs.len();
        }
        Ok(exported)
    }
}
