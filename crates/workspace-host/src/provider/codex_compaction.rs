//! A Codex compaction through `codex app-server`: the requests the host writes and how the
//! server's messages say it ended. The protocol is experimental, so only a turn that completed
//! is a success; everything else is a failure with a reason a person can read.
use super::*;
use workspace_core::{REASON_LIMIT, first_line};

/// The requests in the order they are written. A request's id is its place, from 1.
const REQUESTS: [&str; 3] = ["initialize", "thread/resume", "thread/compact/start"];
/// How long a Codex compaction may run. Its server never exits on its own, so nothing else
/// ends one whose turn does not complete.
const CODEX_COMPACTION_LIMIT: Duration = Duration::from_secs(10 * 60);
/// How long the server may take to exit by itself once its outcome is read and its input is
/// closed, before its process group is stopped. Codex 0.160 writes the last entries of the
/// thread's record before it reports the outcome, but no interface promises that order, and
/// it exits within 0.02 seconds of its input closing. A second is fifty times that and the
/// grace a stopped group already gets; less would trust one machine's timing.
pub(super) const EXIT_MARGIN: Duration = Duration::from_secs(1);
/// Shortens the limit, in milliseconds, so a test need not wait it out.
const LIMIT_OVERRIDE: &str = "WORKSPACE_CODEX_COMPACTION_LIMIT_MS";

pub(super) fn limit() -> Duration {
    let shortened = std::env::var(LIMIT_OVERRIDE).ok();
    let shortened = shortened.and_then(|millis| millis.parse().ok());
    shortened.map_or(CODEX_COMPACTION_LIMIT, Duration::from_millis)
}

pub(super) fn out_of_time() -> String {
    let minutes = CODEX_COMPACTION_LIMIT.as_secs() / 60;
    format!("Codex did not finish compacting within {minutes} minutes.")
}

/// The four lines the server reads, written at once; it answers them in order. The thread is
/// resumed with a turn's own model, sandbox and approval policy, because the server writes
/// them into the thread's record.
pub(super) fn input(turn: &Turn) -> Result<Vec<u8>> {
    let thread = turn.provider_session.as_deref();
    let thread = thread.context("No Codex thread to compact")?;
    let request =
        |id: usize, params: Value| json!({"id":id,"method":REQUESTS[id - 1],"params":params});
    let client =
        json!({"name":"wiffletree","title":"Wiffletree","version":env!("CARGO_PKG_VERSION")});
    let resume = json!({
        "threadId":thread,
        "cwd":turn.cwd.to_string_lossy(),
        "model":turn.profile.model,
        "sandbox":"danger-full-access",
        "approvalPolicy":"never",
        "excludeTurns":true,
    });
    let lines = [
        request(1, json!({"clientInfo":client})),
        json!({"method":"initialized"}),
        request(2, resume),
        request(3, json!({"threadId":thread})),
    ];
    let lines: String = lines.iter().map(|line| format!("{line}\n")).collect();
    Ok(lines.into_bytes())
}

/// Why a compaction failed when Codex refused `request` with `text`. Codex answers a method
/// it does not have by listing every method it has. It starts the same way when it rejects a
/// parameter's value, so the variant it names must be the request itself.
fn refusal(request: &str, text: &str) -> String {
    if text.starts_with(&format!("Invalid request: unknown variant `{request}`")) {
        format!("Codex refused {request}: this Codex has no such method")
    } else {
        first_line(&format!("Codex refused {request}: {text}"), REASON_LIMIT)
    }
}

/// How a compaction ended.
#[derive(Debug, PartialEq)]
pub(super) struct Ended {
    /// What the host settles it with: its status, and for a turn that ended, its id and figures.
    pub(super) records: Vec<CompactionRecord>,
    /// Codex's own words for a failure, whole; the status carries them cut to a reason.
    pub(super) reported: Option<String>,
}

impl Ended {
    fn failed(reason: &str, reported: Option<&str>) -> Self {
        Self {
            records: vec![CompactionRecord::Status(Err(first_line(
                reason,
                REASON_LIMIT,
            )))],
            reported: reported.map(str::to_owned),
        }
    }
}

/// What the server has said about the compaction so far.
pub(super) struct Messages {
    /// The context total in the thread's record as the compaction started.
    before: Option<u64>,
    turn_started: bool,
    /// Codex's total during the compaction's turn. Resuming a thread reports its last turn's
    /// total first, so only a figure that follows `turn/started` counts.
    after: Option<u64>,
}

impl Messages {
    pub(super) fn new(before: Option<u64>) -> Self {
        Self {
            before,
            turn_started: false,
            after: None,
        }
    }

    /// Reads one message. `Some` once it says how the compaction ended; nothing after it counts.
    pub(super) fn read(&mut self, message: &Value) -> Option<Ended> {
        let params = &message["params"];
        match (message.get("id"), message["method"].as_str()) {
            // The host writes its input once and a compaction has nothing to approve, so a
            // request from the server is never answered.
            (Some(_), Some(asked)) => {
                let reason = format!("Codex asked for {asked}, which a compaction cannot answer");
                Some(Ended::failed(&reason, None))
            }
            (Some(id), None) => {
                // A result says nothing about the compaction; the first refusal decides it.
                let error = message.get("error")?;
                let place = id.as_u64().and_then(|id| id.checked_sub(1));
                let request = place.and_then(|place| REQUESTS.get(place as usize));
                let text = error["message"].as_str().map(str::to_owned);
                let text = text.unwrap_or_else(|| error.to_string());
                let reason = refusal(request.unwrap_or(&"a request"), &text);
                Some(Ended::failed(&reason, Some(&text)))
            }
            (None, Some("turn/started")) => {
                self.turn_started = true;
                None
            }
            (None, Some("thread/tokenUsage/updated")) if self.turn_started => {
                let total = params["tokenUsage"]["last"]["totalTokens"].as_u64();
                self.after = total.or(self.after);
                None
            }
            (None, Some("turn/completed")) => Some(self.completed(&params["turn"])),
            // Every other notification, `error` included: the completion that follows it
            // carries the same text.
            _ => None,
        }
    }

    fn completed(&self, turn: &Value) -> Ended {
        let said = turn["error"]["message"].as_str();
        let said = said.filter(|said| !said.trim().is_empty());
        // A message whose first line is blank gives no reason to show.
        let message = said.filter(|said| !first_line(said, REASON_LIMIT).trim().is_empty());
        let mut ended = match (turn["status"].as_str(), message) {
            (Some("completed"), _) => Ended {
                records: vec![CompactionRecord::Status(Ok(()))],
                reported: None,
            },
            (Some("failed"), Some(error)) => Ended::failed(error, Some(error)),
            (Some(status), _) => {
                Ended::failed(&format!("Codex ended the compaction as {status}"), said)
            }
            (None, _) => Ended::failed("Codex ended the compaction with no status", None),
        };
        if let Some(id) = turn["id"].as_str() {
            ended.records.push(CompactionRecord::Turn(id.into()));
        }
        if let (Some(before), Some(after)) = (self.before, self.after) {
            ended
                .records
                .push(CompactionRecord::Boundary { before, after });
        }
        ended
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CompactionRecord::{Boundary, Status, Turn as TurnId};

    /// The server's side of a recorded exchange.
    fn server(recording: &str) -> Vec<Value> {
        let streams = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/streams");
        let lines = fs::read_to_string(streams.join(format!("codex-compact-{recording}.jsonl")));
        let exchange = lines.unwrap();
        let exchange = exchange.lines().map(|l| serde_json::from_str(l).unwrap());
        let said = exchange.filter(|entry: &Value| entry["from"] == "server");
        said.map(|entry| entry["message"].clone()).collect()
    }

    /// How the compaction ended, reading the messages in order.
    fn ended(before: Option<u64>, messages: &[Value]) -> Ended {
        let mut read = Messages::new(before);
        let ended = messages.iter().find_map(|message| read.read(message));
        ended.expect("the exchange ends the compaction")
    }

    fn failure(reason: &str, reported: Option<&str>) -> Ended {
        Ended {
            records: vec![Status(Err(reason.into()))],
            reported: reported.map(str::to_owned),
        }
    }

    fn completed(turn: Value) -> Value {
        json!({"method":"turn/completed","params":{"turn":turn}})
    }

    #[test]
    fn a_completed_turn_is_a_success_with_the_figure_its_own_turn_reported() {
        let messages = server("success");
        let turn = "01a1363f-4691-7c72-860a-23022c15c8b9";
        let figures = Boundary {
            before: 29_532,
            after: 6_550,
        };
        assert_eq!(
            ended(Some(29_532), &messages),
            Ended {
                records: vec![Status(Ok(())), TurnId(turn.into()), figures],
                reported: None
            }
        );
        // Resuming reports the last turn's total, 29,532, before the compaction's turn starts:
        // that is not the figure after, so without the turn's own there are no figures.
        let own = |message: &&Value| {
            message["method"] != "thread/tokenUsage/updated" || message["params"]["turnId"] != turn
        };
        let without_its_own: Vec<_> = messages.iter().filter(own).cloned().collect();
        assert_eq!(messages.len() - without_its_own.len(), 1);
        let no_figures = vec![Status(Ok(())), TurnId(turn.into())];
        assert_eq!(ended(Some(29_532), &without_its_own).records, no_figures);
        // Nor are there any when the thread's record gave no figure before.
        assert_eq!(ended(None, &messages).records, no_figures);
        // Written at once or one at a time, the exchange reads the same way.
        let at_once = ended(Some(29_532), &server("all-written-at-once"));
        assert_eq!(at_once.records[0], Status(Ok(())));
    }

    #[test]
    fn only_a_turn_that_completed_is_a_success() {
        let interrupted = ended(None, &server("second-start-interrupts-first"));
        assert_eq!(
            interrupted.records[0],
            Status(Err("Codex ended the compaction as interrupted".into()))
        );
        assert_eq!(interrupted.reported, None);
        let mut read = Messages::new(None);
        let mut end = |turn: Value| read.read(&completed(turn)).unwrap();
        assert_eq!(
            end(json!({"status":"superseded"})),
            failure("Codex ended the compaction as superseded", None)
        );
        // A failure that gives no message is named by its status, like any other ending.
        assert_eq!(
            end(json!({"status":"failed","error":null})),
            failure("Codex ended the compaction as failed", None)
        );
        // So is one whose message has no first line to show; the run keeps what Codex said.
        assert_eq!(
            end(json!({"status":"failed","error":{"message":""}})),
            failure("Codex ended the compaction as failed", None)
        );
        assert_eq!(
            end(json!({"status":"failed","error":{"message":"\nThe model is overloaded"}})),
            failure(
                "Codex ended the compaction as failed",
                Some("\nThe model is overloaded")
            )
        );
        assert_eq!(
            end(json!({"state":"completed"})),
            failure("Codex ended the compaction with no status", None)
        );
        assert_eq!(
            end(
                json!({"status":"failed","error":{"message":"The model is overloaded\nRetry later"}})
            ),
            failure(
                "The model is overloaded",
                Some("The model is overloaded\nRetry later")
            )
        );
    }

    #[test]
    fn the_first_refusal_decides_and_names_its_request() {
        // `thread/resume` is refused, then `thread/compact/start`: only the first is kept.
        let said = "no rollout found for thread id 3d05f392-e4f9-416c-918f-f39cd045a145";
        assert_eq!(
            ended(None, &server("unknown-thread")),
            failure(&format!("Codex refused thread/resume: {said}"), Some(said))
        );
        // A refusal the host cannot place still ends the compaction with Codex's words.
        let mut read = Messages::new(None);
        let unplaced = json!({"id":null,"error":{"code":-32700,"message":"Parse error"}});
        assert_eq!(
            read.read(&unplaced).unwrap(),
            failure("Codex refused a request: Parse error", Some("Parse error"))
        );
        // A result says nothing, whichever request it answers.
        assert_eq!(read.read(&json!({"id":3,"result":{}})), None);
    }

    #[test]
    fn a_refusal_is_a_missing_method_only_when_the_variant_it_names_is_the_request() {
        let listed = &server("refused-requests")[0]["error"]["message"];
        let listed = listed.as_str().unwrap();
        assert!(listed.starts_with("Invalid request: unknown variant `thread/compact/bogus`"));
        assert_eq!(
            refusal("thread/compact/bogus", listed),
            "Codex refused thread/compact/bogus: this Codex has no such method"
        );
        // The same opening about another name is Codex rejecting something else: its own
        // words, cut to one bounded line.
        let about_another = refusal("thread/compact/start", listed);
        assert!(about_another.starts_with(
            "Codex refused thread/compact/start: Invalid request: unknown variant `thread/compact/bogus`"
        ));
        assert_eq!(about_another.chars().count(), REASON_LIMIT + 1);
        assert!(about_another.ends_with('…'));
        // A value Codex 0.160 does not have for a parameter of a method it has.
        let value = "Invalid request: unknown variant `no-such-sandbox`, expected one of `read-only`, `workspace-write`, `danger-full-access`";
        assert_eq!(
            refusal("thread/resume", value),
            format!("Codex refused thread/resume: {value}")
        );
    }

    #[test]
    fn a_request_from_the_server_ends_the_compaction_unanswered() {
        assert_eq!(
            ended(None, &server("synthetic-server-request")),
            failure(
                "Codex asked for item/commandExecution/requestApproval, which a compaction cannot answer",
                None
            )
        );
    }

    #[test]
    fn the_limit_is_ten_minutes_and_says_so() {
        assert_eq!(CODEX_COMPACTION_LIMIT, Duration::from_secs(600));
        assert_eq!(
            out_of_time(),
            "Codex did not finish compacting within 10 minutes."
        );
    }
}
