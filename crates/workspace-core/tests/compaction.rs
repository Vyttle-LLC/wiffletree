use serde_json::json;
use workspace_core::*;

#[test]
fn token_counts_read_like_the_provider_prints_them() {
    assert_eq!(compact_tokens(90_000), "90k");
    assert_eq!(compact_tokens(4_200), "4.2k");
    assert_eq!(compact_tokens(1_000_000), "1M");
    assert_eq!(compact_tokens(601), "601");
    assert_eq!(compact_tokens(987_000), "987k");
}

#[test]
fn a_runtime_stored_before_compaction_still_decodes_and_stays_as_it_was() {
    let stored = json!({"session_id":"s","provider_session_id":null,"profile":null,"ticket_id":null,"workdir":null,"last_error":null,"last_started_at":null,"last_finished_at":null});
    let runtime: SessionRuntime = serde_json::from_value(stored).unwrap();
    assert_eq!(runtime.compaction, None);
    assert_eq!(runtime.last_compaction, None);
    let encoded = serde_json::to_value(&runtime).unwrap();
    assert!(encoded.get("compaction").is_none() && encoded.get("last_compaction").is_none());
}

#[test]
fn a_request_and_its_outcome_round_trip_in_snake_case() {
    let runtime = SessionRuntime {
        session_id: "s".into(),
        compaction: Some(Compaction {
            instructions: Some("the failing test names".into()),
            requested_at: 7,
            state: CompactionState::Queued,
        }),
        last_compaction: Some(CompactionOutcome {
            result: CompactionResult::NothingToCompact,
            reason: Some("Not enough messages to compact.".into()),
            at: 9,
        }),
        ..Default::default()
    };
    let encoded = serde_json::to_value(&runtime).unwrap();
    assert_eq!(
        encoded["compaction"],
        json!({"instructions":"the failing test names","requested_at":7,"state":"queued"})
    );
    assert_eq!(
        encoded["last_compaction"],
        json!({"result":"nothing_to_compact","reason":"Not enough messages to compact.","at":9})
    );
    let decoded: SessionRuntime = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded.compaction, runtime.compaction);
    assert_eq!(decoded.last_compaction, runtime.last_compaction);
}
