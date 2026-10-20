//! The host's sequenced event stream: what clients should fetch again, with a cursor to resume
//! from. Nothing is persisted; each start is a new epoch.
use async_channel::{Receiver, Sender, TrySendError};
use std::{
    collections::{BTreeSet, VecDeque},
    sync::{Arc, Mutex},
};
use workspace_core::{Cursor, Event, EventKind};

/// How many of the latest events a subscription can resume after.
pub const LOG_EVENTS: usize = 1024;
/// How many events wait for one subscriber before it is sent `resync` instead.
pub const QUEUE_EVENTS: usize = 256;

/// What one actor event changed, emitted as at most one event of each kind.
#[derive(Debug, Default)]
pub(crate) struct Changes {
    pub state: bool,
    pub steps: BTreeSet<String>,
    pub progress: bool,
    pub usage: bool,
}
impl Changes {
    fn kinds(self) -> Vec<EventKind> {
        let mut kinds = vec![];
        if self.state {
            kinds.push(EventKind::State);
        }
        if !self.steps.is_empty() {
            kinds.push(EventKind::Steps {
                sessions: self.steps.into_iter().collect(),
            });
        }
        if self.progress {
            kinds.push(EventKind::Progress);
        }
        if self.usage {
            kinds.push(EventKind::Usage);
        }
        kinds
    }
}

/// A subscription: the events missed since its cursor, then the live ones.
pub struct Subscription {
    /// The latest event's cursor when it began.
    pub cursor: Cursor,
    pub missed: Vec<Event>,
    pub events: Receiver<Event>,
}

/// The in-process form of a subscription: one bounded wake per kind, each coalescing any
/// number of events until it is read.
#[derive(Clone)]
pub struct Pings {
    pub state: Receiver<()>,
    pub steps: Receiver<()>,
    pub progress: Receiver<()>,
    pub usage: Receiver<()>,
}
struct PingSenders {
    state: Sender<()>,
    steps: Sender<()>,
    progress: Sender<()>,
    usage: Sender<()>,
}
impl PingSenders {
    fn ping(&self, kind: &EventKind) {
        let senders = match kind {
            EventKind::State => vec![&self.state],
            EventKind::Steps { .. } => vec![&self.steps],
            EventKind::Progress => vec![&self.progress],
            EventKind::Usage => vec![&self.usage],
            EventKind::Attention { .. } => vec![],
            EventKind::Resync => vec![&self.state, &self.steps, &self.progress, &self.usage],
        };
        for sender in senders {
            let _ = sender.try_send(());
        }
    }
}

/// A subscriber's queue. The log keeps a receiver too, so it can clear a full queue.
struct Queue {
    sender: Sender<Event>,
    receiver: Receiver<Event>,
}

#[derive(Clone)]
pub struct EventLog(Arc<Mutex<Log>>);
struct Log {
    epoch: String,
    seq: u64,
    recent: VecDeque<Event>,
    queues: Vec<Queue>,
    pings: Vec<PingSenders>,
}
impl Default for EventLog {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Log {
            epoch: workspace_core::new_id(),
            seq: 0,
            recent: VecDeque::with_capacity(LOG_EVENTS),
            queues: vec![],
            pings: vec![],
        })))
    }
}
impl EventLog {
    fn log(&self) -> std::sync::MutexGuard<'_, Log> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn epoch(&self) -> String {
        self.log().epoch.clone()
    }
    /// Sequences and sends what changed. A full queue is cleared and sent `resync`, so a
    /// subscriber that stops reading never holds the caller.
    pub(crate) fn emit(&self, changes: Changes) {
        let mut log = self.log();
        for kind in changes.kinds() {
            log.seq += 1;
            let event = Event {
                cursor: Cursor {
                    epoch: log.epoch.clone(),
                    seq: log.seq,
                },
                kind,
            };
            if log.recent.len() == LOG_EVENTS {
                log.recent.pop_front();
            }
            log.recent.push_back(event.clone());
            for pings in &log.pings {
                pings.ping(&event.kind);
            }
            let resync = log.resync();
            // The log's own receiver is the last one once the subscriber has gone.
            log.queues.retain(|queue| queue.sender.receiver_count() > 1);
            for queue in &log.queues {
                if let Err(TrySendError::Full(_)) = queue.sender.try_send(event.clone()) {
                    while queue.receiver.try_recv().is_ok() {}
                    let _ = queue.sender.try_send(resync.clone());
                }
            }
        }
    }
    /// Subscribes after `after`: a cursor of this epoch still in the log gets every later
    /// event, any other cursor `resync` first, and none only what comes next.
    pub fn subscribe(&self, after: Option<&Cursor>) -> Subscription {
        let mut log = self.log();
        let cursor = log.cursor();
        let missed = match after {
            None => vec![],
            Some(after) => log.after(after).unwrap_or_else(|| vec![log.resync()]),
        };
        let (sender, receiver) = async_channel::bounded(QUEUE_EVENTS);
        log.queues.push(Queue {
            sender,
            receiver: receiver.clone(),
        });
        Subscription {
            cursor,
            missed,
            events: receiver,
        }
    }
    pub fn pings(&self) -> Pings {
        let (state, state_pings) = async_channel::bounded(1);
        let (steps, step_pings) = async_channel::bounded(1);
        let (progress, progress_pings) = async_channel::bounded(1);
        let (usage, usage_pings) = async_channel::bounded(1);
        self.log().pings.push(PingSenders {
            state,
            steps,
            progress,
            usage,
        });
        Pings {
            state: state_pings,
            steps: step_pings,
            progress: progress_pings,
            usage: usage_pings,
        }
    }
}
impl Log {
    fn cursor(&self) -> Cursor {
        Cursor {
            epoch: self.epoch.clone(),
            seq: self.seq,
        }
    }
    /// Carries the latest cursor, which the snapshot taken after it reflects.
    fn resync(&self) -> Event {
        Event {
            cursor: self.cursor(),
            kind: EventKind::Resync,
        }
    }
    /// Every event after `after`, or `None` when the log cannot say which those are.
    fn after(&self, after: &Cursor) -> Option<Vec<Event>> {
        let oldest_kept = self.recent.front().map_or(self.seq, |e| e.cursor.seq - 1);
        let resumable = after.epoch == self.epoch && (oldest_kept..=self.seq).contains(&after.seq);
        resumable.then(|| {
            let skip = (after.seq - oldest_kept) as usize;
            self.recent.iter().skip(skip).cloned().collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Changes {
        Changes {
            state: true,
            ..Changes::default()
        }
    }
    fn seqs(events: &[Event]) -> Vec<u64> {
        events.iter().map(|e| e.cursor.seq).collect()
    }
    fn drain(receiver: &Receiver<Event>) -> Vec<Event> {
        std::iter::from_fn(|| receiver.try_recv().ok()).collect()
    }

    #[test]
    fn resuming_replays_exactly_the_missed_events_then_live_ones() {
        let log = EventLog::default();
        log.emit(state());
        let read = log.subscribe(None).cursor;
        for _ in 0..3 {
            log.emit(state());
        }
        let resumed = log.subscribe(Some(&read));
        assert_eq!(seqs(&resumed.missed), [2, 3, 4]);
        assert_eq!(resumed.cursor.seq, 4);
        log.emit(state());
        let live = drain(&resumed.events);
        assert_eq!(seqs(&live), [5]);
        // Up to date: nothing missed.
        assert!(log.subscribe(Some(&live[0].cursor)).missed.is_empty());
    }

    #[test]
    fn another_epoch_or_a_cursor_out_of_the_log_gets_resync() {
        let log = EventLog::default();
        for _ in 0..LOG_EVENTS + 5 {
            log.emit(state());
        }
        let earlier_epoch = Cursor {
            epoch: "before the restart".into(),
            seq: 3,
        };
        let out_of_the_log = Cursor {
            epoch: log.epoch(),
            seq: 3,
        };
        let ahead = Cursor {
            epoch: log.epoch(),
            seq: 10_000,
        };
        for cursor in [earlier_epoch, out_of_the_log, ahead] {
            let missed = log.subscribe(Some(&cursor)).missed;
            assert_eq!(missed.len(), 1);
            assert_eq!(missed[0].kind, EventKind::Resync);
            assert_eq!(missed[0].cursor.seq, (LOG_EVENTS + 5) as u64);
        }
        // The oldest event kept can still be resumed after the one before it.
        let oldest = Cursor {
            epoch: log.epoch(),
            seq: 5,
        };
        assert_eq!(log.subscribe(Some(&oldest)).missed.len(), LOG_EVENTS);
    }

    #[test]
    fn a_subscriber_that_never_reads_gets_resync_and_holds_no_one_up() {
        let log = EventLog::default();
        let stalled = log.subscribe(None);
        let reading = log.subscribe(None);
        let mut seen = 0;
        for _ in 0..QUEUE_EVENTS * 4 {
            log.emit(state());
            seen += drain(&reading.events).len();
        }
        assert_eq!(seen, QUEUE_EVENTS * 4, "the reader saw every event");
        let queued = drain(&stalled.events);
        assert_eq!(queued[0].kind, EventKind::Resync, "{queued:?}");
        assert!(queued.len() <= QUEUE_EVENTS);
        assert!(queued[1..].iter().all(|e| e.kind == EventKind::State));
    }

    #[test]
    fn one_change_emits_one_event_of_each_kind() {
        let log = EventLog::default();
        let subscription = log.subscribe(None);
        let pings = log.pings();
        log.emit(Changes {
            state: true,
            steps: ["b".to_string(), "a".to_string()].into(),
            progress: true,
            usage: false,
        });
        let kinds: Vec<_> = drain(&subscription.events)
            .into_iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(
            kinds,
            [
                EventKind::State,
                EventKind::Steps {
                    sessions: vec!["a".into(), "b".into()]
                },
                EventKind::Progress
            ]
        );
        assert_eq!(
            (pings.state.len(), pings.steps.len(), pings.progress.len()),
            (1, 1, 1)
        );
        assert!(pings.usage.is_empty());
    }

    #[test]
    fn a_dropped_subscription_is_forgotten() {
        let log = EventLog::default();
        drop(log.subscribe(None));
        log.emit(state());
        assert!(log.log().queues.is_empty());
    }
}
