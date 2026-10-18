//! The workspace as views read it: the live snapshot and the archive joined once, when either
//! arrives, so that looking something up never searches a list.
use crate::{Archive, Project, Session, SessionRuntime, Snapshot, Status, Ticket};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

/// Which list holds a session, ticket or runtime, and where.
#[derive(Clone, Copy)]
enum At {
    Live(usize),
    Archived(usize),
}

pub struct WorkspaceIndex {
    snapshot: Snapshot,
    archive: Rc<Archive>,
    /// Every session, and every ticket, in the order they were created.
    sessions: Vec<At>,
    tickets: Vec<At>,
    /// By id, the position in `sessions` or `tickets`.
    session_at: HashMap<String, usize>,
    ticket_at: HashMap<String, usize>,
    runtime_at: HashMap<String, At>,
    /// Positions in `sessions` by parent, by the project of those with none, and by the ticket
    /// their runtime names.
    children: HashMap<String, Vec<usize>>,
    roots: HashMap<String, Vec<usize>>,
    agents: HashMap<String, Vec<usize>>,
    /// Positions in `tickets` by the coordinator that owns them.
    owned: HashMap<String, Vec<usize>>,
    /// Sessions with an unarchived session working anywhere below them.
    team_working: HashSet<String>,
}

impl WorkspaceIndex {
    pub fn new(snapshot: Snapshot, archive: Rc<Archive>) -> Self {
        let sessions = joined(
            &archive.session_order,
            &snapshot.sessions,
            &archive.sessions,
            |s| &s.id,
        );
        let tickets = joined(
            &archive.ticket_order,
            &snapshot.tickets,
            &archive.tickets,
            |t| &t.id,
        );
        let mut index = Self {
            snapshot,
            archive,
            sessions,
            tickets,
            session_at: HashMap::new(),
            ticket_at: HashMap::new(),
            runtime_at: HashMap::new(),
            children: HashMap::new(),
            roots: HashMap::new(),
            agents: HashMap::new(),
            owned: HashMap::new(),
            team_working: HashSet::new(),
        };
        let runtimes = (index.archive.runtimes.iter().enumerate())
            .map(|(i, r)| (r.session_id.clone(), At::Archived(i)));
        // A session restored since the archive was read has its live runtime.
        let live = (index.snapshot.runtimes.iter().enumerate())
            .map(|(i, r)| (r.session_id.clone(), At::Live(i)));
        index.runtime_at = runtimes.chain(live).collect();
        let mut session_at = HashMap::new();
        let (mut children, mut roots, mut agents) =
            (HashMap::new(), HashMap::new(), HashMap::new());
        for (position, &at) in index.sessions.iter().enumerate() {
            let session = index.at_session(at);
            session_at.insert(session.id.clone(), position);
            let (siblings, key): (&mut HashMap<String, Vec<usize>>, _) = match &session.parent_id {
                Some(parent) => (&mut children, parent),
                None => (&mut roots, &session.project_id),
            };
            siblings.entry(key.clone()).or_default().push(position);
            if let Some(ticket) = index
                .runtime(&session.id)
                .and_then(|r| r.ticket_id.as_ref())
            {
                let crew: &mut Vec<usize> = agents.entry(ticket.clone()).or_default();
                crew.push(position);
            }
        }
        let (mut ticket_at, mut owned) = (HashMap::new(), HashMap::new());
        for (position, &at) in index.tickets.iter().enumerate() {
            let ticket = index.at_ticket(at);
            ticket_at.insert(ticket.id.clone(), position);
            let tickets: &mut Vec<usize> = owned.entry(ticket.coordinator_id.clone()).or_default();
            tickets.push(position);
        }
        (index.session_at, index.children, index.agents) = (session_at, children, agents);
        index.roots = roots;
        (index.ticket_at, index.owned) = (ticket_at, owned);
        index.team_working = index.working_teams();
        index
    }

    /// The live snapshot the index was built from.
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    pub fn archive(&self) -> &Rc<Archive> {
        &self.archive
    }
    /// The archive was read before the latest change to what it holds.
    pub fn archive_is_stale(&self) -> bool {
        self.archive.stamp != self.snapshot.archive_stamp
    }

    /// Every session, archived or not, in the order they were created.
    pub fn sessions(&self) -> impl Iterator<Item = &Session> {
        self.sessions.iter().map(|&at| self.at_session(at))
    }
    pub fn session(&self, id: &str) -> Option<&Session> {
        Some(self.at_session(self.sessions[*self.session_at.get(id)?]))
    }
    /// The sessions whose parent is `parent_id`, in the order they were created.
    pub fn children(&self, parent_id: &str) -> impl Iterator<Item = &Session> {
        self.positions(&self.children, parent_id)
            .map(|&p| self.at_session(self.sessions[p]))
    }
    /// The project's sessions without a parent, its coordinator first.
    pub fn roots(&self, project_id: &str) -> impl Iterator<Item = &Session> {
        self.positions(&self.roots, project_id)
            .map(|&p| self.at_session(self.sessions[p]))
    }
    /// The sessions working on the ticket, in the order they were created.
    pub fn agents(&self, ticket_id: &str) -> impl Iterator<Item = &Session> {
        self.positions(&self.agents, ticket_id)
            .map(|&p| self.at_session(self.sessions[p]))
    }
    pub fn runtime(&self, session_id: &str) -> Option<&SessionRuntime> {
        Some(match *self.runtime_at.get(session_id)? {
            At::Live(i) => &self.snapshot.runtimes[i],
            At::Archived(i) => &self.archive.runtimes[i],
        })
    }
    /// An unarchived session below this one, at any depth, is working.
    pub fn team_working(&self, session_id: &str) -> bool {
        self.team_working.contains(session_id)
    }

    /// Every ticket, open or not, in the order they were created.
    pub fn tickets(&self) -> impl Iterator<Item = &Ticket> {
        self.tickets.iter().map(|&at| self.at_ticket(at))
    }
    pub fn ticket(&self, id: &str) -> Option<&Ticket> {
        Some(self.at_ticket(self.tickets[*self.ticket_at.get(id)?]))
    }
    /// The ticket the session's runtime names.
    pub fn ticket_of(&self, session_id: &str) -> Option<&Ticket> {
        self.ticket(self.runtime(session_id)?.ticket_id.as_deref()?)
    }
    /// The tickets the coordinator owns, in the order they were created.
    pub fn tickets_owned_by(&self, coordinator_id: &str) -> impl Iterator<Item = &Ticket> {
        self.positions(&self.owned, coordinator_id)
            .map(|&p| self.at_ticket(self.tickets[p]))
    }

    pub fn project(&self, id: &str) -> Option<&Project> {
        self.snapshot.projects.iter().find(|p| p.id == id)
    }

    fn at_session(&self, at: At) -> &Session {
        match at {
            At::Live(i) => &self.snapshot.sessions[i],
            At::Archived(i) => &self.archive.sessions[i],
        }
    }
    fn at_ticket(&self, at: At) -> &Ticket {
        match at {
            At::Live(i) => &self.snapshot.tickets[i],
            At::Archived(i) => &self.archive.tickets[i],
        }
    }
    fn positions<'a>(
        &self,
        map: &'a HashMap<String, Vec<usize>>,
        key: &str,
    ) -> std::slice::Iter<'a, usize> {
        map.get(key).map_or([].iter(), |positions| positions.iter())
    }
    fn working_teams(&self) -> HashSet<String> {
        let mut teams = HashSet::new();
        let working = self
            .sessions()
            .filter(|s| !s.archived && s.status == Status::Working);
        for session in working {
            let mut parent = session.parent_id.as_deref();
            // A parent already counted has had its own parents counted too.
            while let Some(id) = parent.filter(|id| teams.insert(id.to_string())) {
                parent = self.session(id).and_then(|s| s.parent_id.as_deref());
            }
        }
        teams
    }
}

/// The live and archived items in the order they were created: `order` places everything the
/// archive was read with, and live items created since follow in the snapshot's order. Live
/// wins over archived, for a session restored since.
fn joined<T>(order: &[String], live: &[T], archived: &[T], id: impl Fn(&T) -> &String) -> Vec<At> {
    let mut at: HashMap<&str, At> = (archived.iter().enumerate())
        .map(|(i, item)| (id(item).as_str(), At::Archived(i)))
        .collect();
    at.extend((live.iter().enumerate()).map(|(i, item)| (id(item).as_str(), At::Live(i))));
    let mut joined: Vec<At> = order
        .iter()
        .filter_map(|id| at.remove(id.as_str()))
        .collect();
    joined.extend(
        (live.iter().enumerate())
            .filter(|(_, item)| at.contains_key(id(item).as_str()))
            .map(|(i, _)| At::Live(i)),
    );
    joined
}
