//! The hosts this app drives (design 7, 9 and 11 of `remote-hosts`): This Mac, and each host
//! paired in this device's client settings, with how the app is connected to each and how many
//! things need the human there. The sidebar's switcher lists them; the window shows the selected
//! one, labels what is remote and freezes while a remote host does not answer.
use crate::{
    bridge::{Bridge, RemoteName},
    browse,
};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use workspace_core::{
    AccessLevel,
    pairing::{KeyPair, short_id},
    remote_launch::{RemoteLaunch, Turnaway},
};

/// A host paired with this device, kept in its client settings, never synced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PairedHost {
    /// The SHA-256 of its public key, in hex.
    pub id: String,
    /// Its public key from pairing, in X9.63 form as base64; a host with another is refused.
    pub public_key: String,
    pub name: String,
    /// Where it was last reached, each a name or address with an optional `:port`.
    #[serde(default)]
    pub addresses: Vec<String>,
    /// When it last answered, in Unix milliseconds.
    #[serde(default)]
    pub last_seen: Option<i64>,
}

/// How the app is connected to a paired host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Link {
    Connected(AccessLevel),
    /// It stopped answering; the app keeps trying.
    Reconnecting,
    /// Not reached since the app started.
    Offline,
    /// It answered but turned this device away; only pairing again helps.
    TurnedAway(Turnaway),
}
impl Link {
    /// How the app is connected to the host `bridge` reaches.
    pub(crate) fn of(bridge: &Bridge) -> Self {
        let reached = bridge.reached();
        match (bridge.connected(), reached.level, reached.turnaway) {
            (true, Some(level), _) => Self::Connected(level),
            (_, _, Some(turnaway)) => Self::TurnedAway(turnaway),
            _ if bridge.heard_at().is_some() => Self::Reconnecting,
            _ => Self::Offline,
        }
    }
}

/// Which host the window shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum HostChoice {
    #[default]
    ThisMac,
    Remote(String),
}

/// A paired host as the switcher sees it.
#[derive(Clone, Debug)]
pub(crate) struct RemoteHost {
    pub paired: PairedHost,
    pub link: Link,
    /// Its count of things that need the human, once it has sent one.
    pub needs_you: Option<usize>,
}

/// One row of the host switcher.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SwitcherRow {
    pub choice: HostChoice,
    pub name: String,
    pub detail: String,
    pub needs_you: usize,
    pub selected: bool,
}

/// This Mac, then each paired host, as the switcher lists them.
pub(crate) fn switcher_rows(
    local_projects: usize,
    local_needs_you: usize,
    remotes: &[RemoteHost],
    selected: &HostChoice,
    now: i64,
) -> Vec<SwitcherRow> {
    let projects = match local_projects {
        1 => "1 project".to_owned(),
        n => format!("{n} projects"),
    };
    let this_mac = SwitcherRow {
        choice: HostChoice::ThisMac,
        name: "This Mac".into(),
        detail: format!("Local workspace · {projects}"),
        needs_you: local_needs_you,
        selected: *selected == HostChoice::ThisMac,
    };
    let remotes = remotes.iter().map(|host| {
        let choice = HostChoice::Remote(host.paired.id.clone());
        SwitcherRow {
            selected: *selected == choice,
            choice,
            name: host.paired.name.clone(),
            detail: link_detail(host.link, host.paired.last_seen, now),
            needs_you: host.needs_you.unwrap_or(0),
        }
    });
    std::iter::once(this_mac).chain(remotes).collect()
}

/// A paired host's state in its switcher row.
fn link_detail(link: Link, last_seen: Option<i64>, now: i64) -> String {
    match (link, last_seen) {
        (Link::Connected(AccessLevel::Control), _) => "Connected · you can control it".into(),
        (Link::Connected(AccessLevel::ViewOnly), _) => "Connected · view only".into(),
        (Link::Reconnecting, _) => "Reconnecting…".into(),
        (Link::Offline, Some(seen)) => format!("Offline · last seen {}", spelled_age(seen, now)),
        (Link::Offline, None) => "Offline".into(),
        (Link::TurnedAway(Turnaway::KeyChanged), _) => "Its key has changed · pair again".into(),
        (Link::TurnedAway(Turnaway::Unpaired), _) => "No longer accepts this Mac".into(),
    }
}

/// The switcher's line in the sidebar footer: "Local workspace", or the remote host and its state.
pub(crate) fn footer_line(host: &HostView) -> String {
    match &host.remote {
        None => "Local workspace".into(),
        Some(remote) => {
            let state = match remote.link {
                Link::Connected(_) => "connected",
                Link::Reconnecting => "reconnecting…",
                Link::Offline => "offline",
                Link::TurnedAway(_) => "not connected",
            };
            format!("{} · {state}", remote.name)
        }
    }
}

/// A path on `host` as it reads here: "Studio Mac: ~/dev/tidepool", or as it is on This Mac.
pub(crate) fn labelled_path(host: Option<&str>, path: &str) -> String {
    match host {
        Some(host) => format!("{host}: {path}"),
        None => path.to_owned(),
    }
}

/// How long ago, spelled out as the disconnected banner and the switcher say it: "40 seconds
/// ago", "1 minute ago", "2 hours ago".
pub(crate) fn spelled_age(at: i64, now: i64) -> String {
    let seconds = (now - at).max(0) / 1000;
    let (count, unit) = match seconds {
        0..60 => (seconds, "second"),
        60..3600 => (seconds / 60, "minute"),
        3600..86_400 => (seconds / 3600, "hour"),
        _ => (seconds / 86_400, "day"),
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{plural} ago")
}

/// The remote host the window shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RemoteView {
    pub name: String,
    pub link: Link,
    /// When it last sent anything, in Unix milliseconds.
    pub heard_at: Option<i64>,
}

/// The host the window shows, as views label and gate what they draw. On This Mac nothing gains
/// a label, as before remote hosts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct HostView {
    pub remote: Option<RemoteView>,
}

/// The banner over a remote host's frozen view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Frozen {
    pub title: String,
    pub detail: String,
    /// Under the composer, which keeps working.
    pub composer: String,
}

impl HostView {
    pub(crate) fn remote(name: &str, link: Link, heard_at: Option<i64>) -> Self {
        let name = name.to_owned();
        Self {
            remote: Some(RemoteView {
                name,
                link,
                heard_at,
            }),
        }
    }
    /// The remote host's name; `None` on This Mac.
    pub(crate) fn name(&self) -> Option<&str> {
        self.remote.as_ref().map(|r| r.name.as_str())
    }
    /// The end of the composer's model line on a remote host: "on Studio Mac".
    pub(crate) fn on_host(&self) -> Option<String> {
        self.name().map(|host| format!("on {host}"))
    }
    /// The sidebar heading over the host's projects, in capitals as every sidebar heading.
    pub(crate) fn projects_heading(&self, projects: usize) -> String {
        match self.name() {
            Some(host) => format!("Projects · {projects} · on {host}").to_uppercase(),
            None => format!("PROJECTS · {projects}"),
        }
    }
    /// The sidebar heading over the Workspace pages, which are the host's.
    pub(crate) fn workspace_heading(&self) -> String {
        match self.name() {
            Some(host) => format!("Workspace · {host}").to_uppercase(),
            None => "WORKSPACE".into(),
        }
    }
    /// The parts of a breadcrumb, starting with the host on a remote one.
    pub(crate) fn breadcrumb<'a>(&'a self, parts: impl IntoIterator<Item = &'a str>) -> String {
        (self.name().into_iter().chain(parts))
            .collect::<Vec<_>>()
            .join("  ›  ")
    }
    /// The banner, while a remote host does not answer and the view shows what it last sent.
    pub(crate) fn frozen(&self, now: i64) -> Option<Frozen> {
        let remote = self.remote.as_ref()?;
        if matches!(remote.link, Link::Connected(_)) {
            return None;
        }
        let host = &remote.name;
        let detail = match remote.heard_at {
            Some(at) => format!(
                "Showing what it sent {}. Its agents keep working.",
                spelled_age(at, now)
            ),
            None => "Nothing has arrived from it yet. Its agents keep working.".into(),
        };
        let title = match remote.link {
            Link::TurnedAway(Turnaway::KeyChanged) => {
                format!("{host}'s key has changed. Wiffletree won't connect until you pair again.")
            }
            Link::TurnedAway(Turnaway::Unpaired) => {
                format!("{host} no longer accepts this Mac. Pair again from {host}.")
            }
            _ => format!("Reconnecting to {host}…"),
        };
        Some(Frozen {
            title,
            detail,
            composer: format!("You can write; it sends when {host} is back."),
        })
    }
    /// Why changes cannot be made on this host now, or `None` when they can: a View only device
    /// sees the host's settings with saving off, and a host that does not answer takes nothing
    /// but messages, which wait.
    pub(crate) fn changes_refused(&self) -> Option<String> {
        let remote = self.remote.as_ref()?;
        let host = &remote.name;
        match remote.link {
            Link::Connected(AccessLevel::Control) => None,
            Link::Connected(AccessLevel::ViewOnly) => Some(format!(
                "View only: {host} refuses changes from this device"
            )),
            Link::Reconnecting | Link::Offline | Link::TurnedAway(_) => {
                Some(format!("Waiting for {host} to answer"))
            }
        }
    }
    /// The Models page's subtitle and note on whose table it is.
    pub(crate) fn models_owner(&self) -> Option<(String, String)> {
        let host = self.name()?;
        Some((
            format!("each job's model on {host}"),
            format!(
                "Saving changes {host}'s agents only. Your Mac keeps its own table for its own projects."
            ),
        ))
    }
}

/// A paired host and the app's connection to it.
pub(crate) struct Remote {
    pub paired: PairedHost,
    pub bridge: Bridge,
}

/// The hosts the app knows: This Mac's, and each paired one with a connection kept while the
/// app runs. The selected one is the window's; the others are followed only for their counts.
pub(crate) struct Hosts {
    pub this_mac: Bridge,
    /// This Mac's projects and things that need the human, as its last snapshot had them.
    pub this_mac_counts: (usize, usize),
    pub remotes: Vec<Remote>,
    pub selected: HostChoice,
    /// The hosts browsing finds on this network.
    pub browsed: browse::Hosts,
    /// This device's key, once read or created.
    device: Option<KeyPair>,
}
impl gpui::Global for Hosts {}
impl Hosts {
    pub(crate) fn new(this_mac: Bridge, paired: &[PairedHost], browsed: browse::Hosts) -> Self {
        let mut hosts = Self {
            this_mac,
            this_mac_counts: (0, 0),
            remotes: vec![],
            selected: HostChoice::ThisMac,
            browsed,
            device: None,
        };
        for host in paired {
            hosts.add(host.clone());
        }
        hosts
    }
    /// Keeps a connection to `paired`, in place of any to the same host.
    pub(crate) fn add(&mut self, paired: PairedHost) {
        let bridge = self.connect(&paired);
        bridge.follow(self.selected == HostChoice::Remote(paired.id.clone()));
        let remote = Remote { paired, bridge };
        match (self.remotes.iter_mut()).find(|r| r.paired.id == remote.paired.id) {
            Some(held) => {
                held.bridge.shutdown();
                *held = remote;
            }
            None => self.remotes.push(remote),
        }
    }
    /// Forgets a paired host and closes its connection; the window leaves it if it shows it.
    pub(crate) fn remove(&mut self, id: &str) {
        for remote in self.remotes.iter().filter(|r| r.paired.id == id) {
            remote.bridge.shutdown();
        }
        self.remotes.retain(|r| r.paired.id != id);
        if self.selected == HostChoice::Remote(id.into()) {
            self.selected = HostChoice::ThisMac;
        }
    }
    /// This device's key from the login Keychain, created there the first time it is needed.
    pub(crate) fn device(&mut self) -> Result<KeyPair, String> {
        if let Some(device) = &self.device {
            return Ok(device.clone());
        }
        let device = workspace_core::device_key::device_key()
            .map_err(|e| format!("This Mac's device key could not be read: {e}"))?;
        Ok(self.device.insert(device).clone())
    }
    fn connect(&mut self, paired: &PairedHost) -> Bridge {
        let name = RemoteName {
            id: paired.id.clone(),
            name: paired.name.clone(),
        };
        let key = base64::engine::general_purpose::STANDARD.decode(&paired.public_key);
        let (Ok(device), Ok(key)) = (self.device(), key) else {
            return Bridge::unreachable(name);
        };
        let (browsed, short) = (self.browsed.clone(), short_id(&paired.id).to_owned());
        let discover = Box::new(move || {
            (browsed.found().into_iter())
                .filter(|found| found.short_id == short)
                .map(|found| found.address.clone())
                .collect()
        });
        let launcher = RemoteLaunch::new(key, device, paired.addresses.clone(), discover);
        Bridge::remote_host(name, launcher)
    }
    /// Shows `choice`'s host: it gets every event, and the other paired hosts only their counts.
    pub(crate) fn select(&mut self, choice: HostChoice) {
        for remote in &self.remotes {
            remote
                .bridge
                .follow(choice == HostChoice::Remote(remote.paired.id.clone()));
        }
        self.selected = choice;
    }
    fn selected_remote(&self) -> Option<&Remote> {
        let HostChoice::Remote(id) = &self.selected else {
            return None;
        };
        self.remotes.iter().find(|r| &r.paired.id == id)
    }
    /// The selected host's bridge.
    pub(crate) fn bridge(&self) -> Bridge {
        match self.selected_remote() {
            Some(remote) => remote.bridge.clone(),
            None => self.this_mac.clone(),
        }
    }
    /// What the window labels and gates for the selected host.
    pub(crate) fn view(&self) -> HostView {
        match self.selected_remote() {
            Some(remote) => HostView::remote(
                &remote.paired.name,
                Link::of(&remote.bridge),
                remote.bridge.heard_at(),
            ),
            None => HostView::default(),
        }
    }
    /// What a workspace on `bridge`'s host labels and gates, whichever host is selected.
    pub(crate) fn view_of(&self, bridge: &Bridge) -> HostView {
        let id = bridge.remote().map(|r| r.id.as_str());
        match self
            .remotes
            .iter()
            .find(|r| Some(r.paired.id.as_str()) == id)
        {
            Some(remote) => HostView::remote(
                &remote.paired.name,
                Link::of(&remote.bridge),
                remote.bridge.heard_at(),
            ),
            None => HostView::default(),
        }
    }
    pub(crate) fn remote_hosts(&self) -> Vec<RemoteHost> {
        (self.remotes.iter())
            .map(|remote| RemoteHost {
                paired: remote.paired.clone(),
                link: Link::of(&remote.bridge),
                needs_you: remote.bridge.needs_you(),
            })
            .collect()
    }
    /// Tries every paired host that is not connected at once, as a change of network does.
    pub(crate) fn try_now(&self) {
        for remote in self.remotes.iter().filter(|r| !r.bridge.connected()) {
            remote.bridge.try_now();
        }
    }
}

/// Follows each paired host's connection and count while the app runs: the switcher redraws, and
/// the host's client settings remember where and when it last answered.
pub(crate) fn watch(cx: &mut gpui::App) {
    let bridges: Vec<_> = (cx.global::<Hosts>().remotes.iter())
        .map(|r| (r.paired.id.clone(), r.bridge.clone()))
        .collect();
    for (id, bridge) in bridges {
        follow(id, bridge, cx);
    }
    watch_network(cx);
}

/// Redraws and remembers `bridge`'s host as its connection or count changes.
pub(crate) fn follow(id: String, bridge: Bridge, cx: &mut gpui::App) {
    let wakes = [bridge.link_changes(), bridge.attention_changes()];
    for changes in wakes.into_iter().flatten() {
        let (id, bridge) = (id.clone(), bridge.clone());
        cx.spawn(async move |cx| {
            while changes.recv().await.is_ok() {
                cx.update(|cx| {
                    if bridge.connected() {
                        remember(&id, bridge.reached().address, cx);
                    }
                    cx.refresh_windows();
                });
            }
        })
        .detach();
    }
}

/// Notes that the paired host `id` answered now, at `address`, in this device's client settings.
fn remember(id: &str, address: Option<String>, cx: &mut gpui::App) {
    let now = crate::ui::now();
    for remote in (cx.global_mut::<Hosts>().remotes.iter_mut()).filter(|r| r.paired.id == id) {
        seen(&mut remote.paired, address.as_deref(), now);
    }
    let preferences = cx.global_mut::<crate::Preferences>();
    let mut changed = false;
    for host in preferences.paired_hosts.iter_mut().filter(|h| h.id == id) {
        changed |= seen(host, address.as_deref(), now);
    }
    if changed && let Err(error) = preferences.save() {
        eprintln!("Saving preferences: {error}");
    }
}

/// How finely when a host was last seen is kept: the switcher says it in minutes at most.
const SEEN_STEP: i64 = 60_000;

/// Notes that `host` answered at `now`, at `address`, first among its addresses. Whether that
/// changes what is kept: the address order, or the time by a minute or more.
fn seen(host: &mut PairedHost, address: Option<&str>, now: i64) -> bool {
    let mut changed = false;
    if host.last_seen.is_none_or(|seen| now - seen >= SEEN_STEP) {
        host.last_seen = Some(now);
        changed = true;
    }
    if let Some(address) = address
        && host.addresses.first().map(String::as_str) != Some(address)
    {
        host.addresses.retain(|a| a != address);
        host.addresses.insert(0, address.to_owned());
        changed = true;
    }
    changed
}

/// This Mac's name as other devices see it: its computer name.
pub(crate) fn this_mac_name() -> String {
    let named = std::process::Command::new("/usr/sbin/scutil")
        .args(["--get", "ComputerName"])
        .output();
    (named.ok())
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Mac".into())
}

/// How often the network is checked for a change, which tries every paired host at once.
const NETWORK_CHECK: std::time::Duration = std::time::Duration::from_secs(3);

/// Tries the paired hosts at once whenever this Mac's network addresses change.
fn watch_network(cx: &mut gpui::App) {
    cx.spawn(async move |cx| {
        let mut seen = addresses();
        loop {
            cx.background_executor().timer(NETWORK_CHECK).await;
            let now = addresses();
            if now != seen {
                seen = now;
                cx.update(|cx| cx.global::<Hosts>().try_now());
            }
        }
    })
    .detach();
}

/// This Mac's interface addresses, as `getifaddrs` lists them.
fn addresses() -> std::collections::BTreeSet<String> {
    let mut found = std::collections::BTreeSet::new();
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: `list` receives a list freed below, and each entry is read only while it lives.
    unsafe {
        if libc::getifaddrs(&mut list) != 0 {
            return found;
        }
        let mut entry = list;
        while !entry.is_null() {
            let address = (*entry).ifa_addr;
            if !address.is_null() {
                let family = (*address).sa_family as i32;
                let bytes: &[u8] = match family {
                    libc::AF_INET => &(*(address as *const libc::sockaddr_in))
                        .sin_addr
                        .s_addr
                        .to_ne_bytes(),
                    libc::AF_INET6 => &(*(address as *const libc::sockaddr_in6)).sin6_addr.s6_addr,
                    _ => &[],
                };
                if !bytes.is_empty() {
                    found.insert(format!("{bytes:?}"));
                }
            }
            entry = (*entry).ifa_next;
        }
        libc::freeifaddrs(list);
    }
    found
}

/// A host's projects, as the sidebar counts them, and its things that need the human: every
/// attention item and workspace note, as the Inbox rail counts them for each project.
pub(crate) fn counts(snapshot: &workspace_core::Snapshot) -> (usize, usize) {
    let projects = (snapshot.sessions.iter())
        .filter(|s| s.parent_id.is_none() && !s.archived)
        .count();
    let needs_you = snapshot.attention.len() + snapshot.workspace_notes.len();
    (projects, needs_you)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: i64 = 60_000;

    fn paired(id: &str, name: &str, last_seen: Option<i64>) -> PairedHost {
        PairedHost {
            id: id.into(),
            public_key: "BA==".into(),
            name: name.into(),
            addresses: vec![format!("{id}.local")],
            last_seen,
        }
    }

    /// A paired host as a connection to it would report it.
    fn connection(id: &str, name: &str, link: Link, needs_you: Option<usize>) -> RemoteHost {
        RemoteHost {
            paired: paired(id, name, Some(-120 * MINUTE)),
            link,
            needs_you,
        }
    }

    #[test]
    fn the_switcher_lists_this_mac_then_each_host_with_its_state_and_count() {
        let remotes = [
            connection(
                "studio",
                "Studio Mac",
                Link::Connected(AccessLevel::Control),
                Some(1),
            ),
            connection(
                "office",
                "Office iMac",
                Link::Connected(AccessLevel::ViewOnly),
                None,
            ),
            connection("mini", "Mac mini", Link::Offline, Some(4)),
            connection("air", "MacBook Air", Link::Reconnecting, Some(2)),
        ];
        let selected = HostChoice::Remote("studio".into());
        let rows = switcher_rows(3, 2, &remotes, &selected, 0);
        let shown: Vec<_> = (rows.iter())
            .map(|r| (r.name.as_str(), r.detail.as_str(), r.needs_you, r.selected))
            .collect();
        assert_eq!(
            shown,
            [
                ("This Mac", "Local workspace · 3 projects", 2, false),
                ("Studio Mac", "Connected · you can control it", 1, true),
                ("Office iMac", "Connected · view only", 0, false),
                ("Mac mini", "Offline · last seen 2 hours ago", 4, false),
                ("MacBook Air", "Reconnecting…", 2, false),
            ]
        );
        assert_eq!(rows[1].choice, selected);
    }

    #[test]
    fn a_question_waiting_elsewhere_shows_without_selecting_that_host() {
        let quiet = [connection(
            "studio",
            "Studio Mac",
            Link::Connected(AccessLevel::Control),
            Some(0),
        )];
        let asked = [connection(
            "studio",
            "Studio Mac",
            Link::Connected(AccessLevel::Control),
            Some(1),
        )];
        let row = |remotes: &[RemoteHost]| {
            switcher_rows(1, 0, remotes, &HostChoice::ThisMac, 0)[1].clone()
        };
        assert_eq!(row(&quiet).needs_you, 0);
        assert_eq!(row(&asked).needs_you, 1);
        assert!(!row(&asked).selected);
        let local = &switcher_rows(1, 0, &asked, &HostChoice::ThisMac, 0)[0];
        assert_eq!(
            (local.detail.as_str(), local.selected),
            ("Local workspace · 1 project", true)
        );
    }

    #[test]
    fn a_host_that_turned_this_mac_away_says_why_and_how_to_fix_it() {
        let changed = Link::TurnedAway(Turnaway::KeyChanged);
        let removed = Link::TurnedAway(Turnaway::Unpaired);
        let rows = switcher_rows(
            0,
            0,
            &[
                connection("studio", "Studio Mac", changed, None),
                connection("mini", "Mac mini", removed, None),
            ],
            &HostChoice::ThisMac,
            0,
        );
        assert_eq!(rows[1].detail, "Its key has changed · pair again");
        assert_eq!(rows[2].detail, "No longer accepts this Mac");
        let title = |link| {
            HostView::remote("Studio Mac", link, None)
                .frozen(0)
                .unwrap()
                .title
        };
        assert_eq!(
            title(changed),
            "Studio Mac's key has changed. Wiffletree won't connect until you pair again."
        );
        assert_eq!(
            title(removed),
            "Studio Mac no longer accepts this Mac. Pair again from Studio Mac."
        );
    }

    #[test]
    fn a_host_answering_again_is_kept_only_when_its_address_or_minute_changes() {
        let mut host = paired("studio", "Studio Mac", None);
        assert!(seen(&mut host, Some("studio.local"), 0), "first seen");
        assert!(
            !seen(&mut host, Some("studio.local"), 30_000),
            "the same minute"
        );
        assert!(
            seen(&mut host, Some("10.0.0.2:47821"), 40_000),
            "a new address"
        );
        assert_eq!(host.addresses[0], "10.0.0.2:47821");
        assert!(!seen(&mut host, None, 50_000));
        assert!(seen(&mut host, None, 70_000), "a minute on");
        assert_eq!(host.last_seen, Some(70_000));
    }

    #[test]
    fn an_offline_host_never_seen_says_only_offline() {
        let mut host = connection("mini", "Mac mini", Link::Offline, None);
        host.paired.last_seen = None;
        let rows = switcher_rows(0, 0, &[host], &HostChoice::ThisMac, 0);
        assert_eq!(rows[1].detail, "Offline");
    }

    #[test]
    fn the_footer_line_names_the_remote_host_and_its_state() {
        assert_eq!(footer_line(&HostView::default()), "Local workspace");
        let line = |link| footer_line(&HostView::remote("Studio Mac", link, None));
        assert_eq!(
            line(Link::Connected(AccessLevel::Control)),
            "Studio Mac · connected"
        );
        assert_eq!(line(Link::Reconnecting), "Studio Mac · reconnecting…");
        assert_eq!(line(Link::Offline), "Studio Mac · offline");
    }

    #[test]
    fn ages_are_spelled_out() {
        assert_eq!(spelled_age(0, 40_000), "40 seconds ago");
        assert_eq!(spelled_age(0, 1_000), "1 second ago");
        assert_eq!(spelled_age(0, MINUTE), "1 minute ago");
        assert_eq!(spelled_age(0, 150 * MINUTE), "2 hours ago");
        assert_eq!(spelled_age(0, 3 * 1440 * MINUTE), "3 days ago");
        assert_eq!(
            spelled_age(10, 0),
            "0 seconds ago",
            "a clock that went back"
        );
    }

    #[test]
    fn a_remote_host_that_stops_answering_freezes_with_its_age_counting_up() {
        let heard = 1_000_000;
        let host = HostView::remote("Studio Mac", Link::Reconnecting, Some(heard));
        let frozen = host.frozen(heard + 40_000).unwrap();
        assert_eq!(frozen.title, "Reconnecting to Studio Mac…");
        assert_eq!(
            frozen.detail,
            "Showing what it sent 40 seconds ago. Its agents keep working."
        );
        assert_eq!(
            frozen.composer,
            "You can write; it sends when Studio Mac is back."
        );
        let later = host.frozen(heard + 2 * MINUTE).unwrap();
        assert!(later.detail.contains("2 minutes ago"), "{}", later.detail);

        let connected = HostView::remote("Studio Mac", Link::Connected(AccessLevel::Control), None);
        assert_eq!(connected.frozen(heard), None);
        assert_eq!(HostView::default().frozen(heard), None, "This Mac");
    }

    #[test]
    fn changes_wait_for_control_and_an_answering_host() {
        assert_eq!(HostView::default().changes_refused(), None);
        let host = |link| HostView::remote("Studio Mac", link, None).changes_refused();
        assert_eq!(host(Link::Connected(AccessLevel::Control)), None);
        assert_eq!(
            host(Link::Connected(AccessLevel::ViewOnly)).as_deref(),
            Some("View only: Studio Mac refuses changes from this device")
        );
        assert!(host(Link::Reconnecting).is_some());
        assert!(host(Link::Offline).is_some());
    }

    #[test]
    fn remote_things_say_where_they_are_and_this_macs_do_not() {
        let remote = HostView::remote("Studio Mac", Link::Connected(AccessLevel::Control), None);
        assert_eq!(
            labelled_path(remote.name(), "~/dev/tidepool"),
            "Studio Mac: ~/dev/tidepool"
        );
        assert_eq!(remote.on_host().as_deref(), Some("on Studio Mac"));
        assert_eq!(remote.projects_heading(2), "PROJECTS · 2 · ON STUDIO MAC");
        assert_eq!(remote.workspace_heading(), "WORKSPACE · STUDIO MAC");
        assert_eq!(
            remote.breadcrumb(["Tide alerts", "Coordinator"]),
            "Studio Mac  ›  Tide alerts  ›  Coordinator"
        );
        let (subtitle, note) = remote.models_owner().unwrap();
        assert_eq!(subtitle, "each job's model on Studio Mac");
        assert!(note.starts_with("Saving changes Studio Mac's agents only."));

        let here = HostView::default();
        assert_eq!(
            labelled_path(here.name(), "~/dev/tidepool"),
            "~/dev/tidepool"
        );
        assert_eq!(here.on_host(), None);
        assert_eq!(here.projects_heading(2), "PROJECTS · 2");
        assert_eq!(here.workspace_heading(), "WORKSPACE");
        assert_eq!(
            here.breadcrumb(["Tide alerts", "Coordinator"]),
            "Tide alerts  ›  Coordinator"
        );
        assert_eq!(here.models_owner(), None);
    }
}
