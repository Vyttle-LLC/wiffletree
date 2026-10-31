-- Shared issues (`tracker`, `issues_sync`), keyed by storage repository as
-- `StorageRepository::key`. What this Mac last synced and last wrote of each tracker file is
-- sync's per-file record in `sync.sql`, which the repository's one engine keeps for every rule
-- set.
-- The index of each tracker's issues for the list and search, as `IssueListing` JSON, rebuilt
-- from the files when `issue_index_format` is missing or holds another format. `stamp` is the
-- issue folder's file names, sizes and times, so a change on disk is found without reading the
-- files; `version` is a digest of their content, for unread marks; `text` is the ID, title,
-- description and comments in lower case, for search; `people` is the logins the issue names as
-- author, assignee, commenter or in an event, a JSON array, for the assignee picker.
CREATE TABLE IF NOT EXISTS issue_index (storage TEXT NOT NULL, id TEXT NOT NULL, state TEXT, data TEXT NOT NULL, stamp TEXT NOT NULL, version TEXT NOT NULL, text TEXT NOT NULL, people TEXT NOT NULL, PRIMARY KEY(storage, id));
CREATE TABLE IF NOT EXISTS issue_index_format (format INTEGER NOT NULL);
-- The `version` of each issue as the human last opened it on this Mac, or as the host last
-- wrote it for the human. An issue whose version differs is unread. Never written to a tracker.
CREATE TABLE IF NOT EXISTS issue_read (storage TEXT NOT NULL, id TEXT NOT NULL, version TEXT NOT NULL, PRIMARY KEY(storage, id));
-- The Space of the last issue filed on this Mac, one row at most, where New issue starts.
CREATE TABLE IF NOT EXISTS issue_last_filed (space TEXT NOT NULL);
-- The temporary IDs this Mac gave issues it filed, and the number each was pushed as.
CREATE TABLE IF NOT EXISTS issue_temporary (storage TEXT NOT NULL, temporary TEXT NOT NULL, number TEXT, PRIMARY KEY(storage, temporary));
-- The number a temporary ID's issue would rather be pushed as, such as its number in Linear;
-- a pass gives it that number only when the tracker's history never had it.
CREATE TABLE IF NOT EXISTS issue_preferred (storage TEXT NOT NULL, temporary TEXT NOT NULL, number INTEGER NOT NULL, PRIMARY KEY(storage, temporary));
-- Links between issues and this Mac's projects, or one of a project's tickets (`ticket` is ''
-- for the whole project). Never written to a tracker.
CREATE TABLE IF NOT EXISTS issue_links (storage TEXT NOT NULL, id TEXT NOT NULL, project TEXT NOT NULL, ticket TEXT NOT NULL, PRIMARY KEY(storage, id, project, ticket));
-- The state each linked issue's work last derived on this Mac. The issue follows its work when
-- this changes, so a manual move holds until then.
CREATE TABLE IF NOT EXISTS issue_derived (storage TEXT NOT NULL, id TEXT NOT NULL, state TEXT NOT NULL, PRIMARY KEY(storage, id));
-- The `work` and `pr` events linked work gave an issue while one of its files was changed
-- outside Wiffletree, as a JSON array, and what they did for the commit message. They are
-- written once the file is settled.
CREATE TABLE IF NOT EXISTS issue_pending (storage TEXT NOT NULL, id TEXT NOT NULL, events TEXT NOT NULL, did TEXT NOT NULL, PRIMARY KEY(storage, id));
-- The parts of an `issue.md` paused by the per-part merge, a front matter field or the
-- description, keyed like `sync_paused` by the sync record's key and the file's path: the other
-- side's value as written, which they compare against, and who last changed the file there. A
-- whole file still pauses in `sync_paused`. The table before this merge was never written.
DROP TABLE IF EXISTS issue_paused_parts;
CREATE TABLE IF NOT EXISTS issue_parts (repository TEXT NOT NULL, path TEXT NOT NULL, part TEXT NOT NULL, theirs TEXT, by TEXT NOT NULL, PRIMARY KEY(repository, path, part));
-- The human's choice to follow an issue (1) or not (0) on this Mac. Without one, they follow
-- the issues they filed, are assigned or commented on.
CREATE TABLE IF NOT EXISTS issue_follows (storage TEXT NOT NULL, id TEXT NOT NULL, follow INTEGER NOT NULL, PRIMARY KEY(storage, id));
-- The Inbox's news of issues, as `IssueNote` JSON, each once by `key`, until the human opens
-- the issue.
CREATE TABLE IF NOT EXISTS issue_notes (storage TEXT NOT NULL, id TEXT NOT NULL, key TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(storage, id, key));
-- A GitHub tracker's collaborators where GitHub let this Mac read them, and when they were read.
CREATE TABLE IF NOT EXISTS issue_collaborators (storage TEXT NOT NULL, login TEXT NOT NULL, PRIMARY KEY(storage, login));
CREATE TABLE IF NOT EXISTS issue_collaborators_read (storage TEXT PRIMARY KEY, at INTEGER NOT NULL);
-- The name on each login's GitHub profile, `NULL` for none, and when it was read.
CREATE TABLE IF NOT EXISTS issue_profiles (login TEXT PRIMARY KEY, name TEXT, at INTEGER NOT NULL);
