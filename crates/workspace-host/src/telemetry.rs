//! Decode provider metadata; request counters and subscription limits have different scopes.
use crate::provider::ProviderEvent;
use crate::*;
use std::collections::BTreeMap;

pub fn codex_quota(value: &Value, at: i64) -> Result<QuotaReading> {
    let mut windows = Vec::new();
    let mut account = None;
    let buckets: Vec<(String, &Value)> = if let Some(map) = value["rateLimitsByLimitId"]
        .as_object()
        .filter(|m| !m.is_empty())
    {
        map.iter().map(|(id, v)| (id.clone(), v)).collect()
    } else if value["rateLimits"].is_object() {
        vec![("codex".into(), &value["rateLimits"])]
    } else {
        vec![]
    };
    for (bucket, limit) in buckets {
        account = account.or_else(|| limit["accountId"].as_str().map(str::to_owned));
        for slot in ["primary", "secondary"] {
            let window = &limit[slot];
            if !window.is_object() {
                continue;
            }
            let duration = window["windowDurationMins"].as_u64();
            let label = match duration {
                Some(10080) => "Weekly".to_owned(),
                Some(300) => "5-hour".to_owned(),
                Some(mins) if mins % 60 == 0 => format!("{}-hour", mins / 60),
                Some(mins) => format!("{mins}-minute"),
                None => slot.into(),
            };
            windows.push(QuotaWindow {
                key: format!("{bucket}:{slot}"),
                label: if bucket == "codex" {
                    label
                } else {
                    format!("{bucket} · {label}")
                },
                used_percent: percentage(&window["usedPercent"], 1.),
                observed_at: at,
                duration_mins: duration,
                resets_at: millis(&window["resetsAt"]),
                status: None,
            });
        }
    }
    Ok(QuotaReading {
        provider: Provider::Codex,
        account,
        observed_at: at,
        source: "Codex app-server".into(),
        error: if windows.is_empty() {
            Some("No quota windows were reported for this account.".into())
        } else {
            None
        },
        windows,
    })
}
fn percentage(value: &Value, scale: f64) -> Option<f64> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && *v >= 0.)
        .map(|v| v * scale)
        .filter(|v| v.is_finite())
}
fn millis(value: &Value) -> Option<i64> {
    value.as_i64()?.checked_mul(1000)
}
fn claude_window(key: &str, value: &Value, at: i64) -> QuotaWindow {
    QuotaWindow {
        key: key.into(),
        label: match key {
            "five_hour" => "5-hour",
            "seven_day" => "Weekly",
            _ => key,
        }
        .into(),
        used_percent: percentage(&value["utilization"], 100.),
        observed_at: at,
        duration_mins: match key {
            "five_hour" => Some(300),
            "seven_day" => Some(10080),
            _ => None,
        },
        resets_at: millis(&value["resetsAt"]),
        status: value["status"].as_str().map(str::to_owned),
    }
}
pub fn claude_quota(value: &Value, at: i64) -> Option<QuotaReading> {
    let info = &value["rate_limit_info"];
    if !info.is_object() {
        return None;
    }
    let mut windows = Vec::new();
    if let Some(map) = info["unifiedWindows"].as_object() {
        for (key, window) in map {
            if window.is_object() {
                windows.push(claude_window(key, window, at));
            }
        }
    }
    if let Some(key) = info["rateLimitType"]
        .as_str()
        .filter(|key| !windows.iter().any(|w| w.key == *key))
    {
        windows.push(claude_window(key, info, at));
    }
    Some(QuotaReading {
        provider: Provider::Claude,
        account: None,
        observed_at: at,
        source: "Claude stream".into(),
        windows,
        error: None,
    })
}
#[derive(Default)]
pub struct ClaudeRequests {
    active: BTreeMap<String, Request>,
}
struct Request {
    id: String,
    model: Option<String>,
    usage: Value,
    output_reported: bool,
}
impl ClaudeRequests {
    pub fn event(&mut self, value: &Value) -> Option<ProviderEvent> {
        let event = &value["event"];
        let key = value["parent_tool_use_id"]
            .as_str()
            .unwrap_or("main")
            .to_owned();
        match event["type"].as_str()? {
            "message_start" => {
                let message = &event["message"];
                let id = message["id"].as_str()?.to_owned();
                let model = message["model"].as_str().map(str::to_owned);
                let mut usage = message["usage"].clone();
                // Initial output counts are placeholders; only the usage delta reports output.
                usage
                    .as_object_mut()?
                    .insert("output_tokens".into(), json!(0));
                self.active.insert(
                    key,
                    Request {
                        id: id.clone(),
                        model: model.clone(),
                        usage: usage.clone(),
                        output_reported: false,
                    },
                );
                Some(ProviderEvent::Usage {
                    request_id: id,
                    model,
                    usage,
                    complete: false,
                })
            }
            "message_delta" => {
                let request = self.active.get_mut(&key)?;
                if let Some(update) = event["usage"].as_object() {
                    let current = request.usage.as_object_mut()?;
                    request.output_reported |= update
                        .get("output_tokens")
                        .is_some_and(|v| v.as_u64().is_some());
                    for (field, value) in update {
                        if value.is_number() {
                            current.insert(field.clone(), value.clone());
                        }
                    }
                }
                Some(ProviderEvent::Usage {
                    request_id: request.id.clone(),
                    model: request.model.clone(),
                    usage: request.usage.clone(),
                    complete: false,
                })
            }
            "message_stop" => {
                let request = self.active.remove(&key)?;
                Some(ProviderEvent::Usage {
                    request_id: request.id,
                    model: request.model,
                    usage: request.usage,
                    complete: request.output_reported,
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_uses_canonical_buckets_and_preserves_unknown_values() {
        let reading=codex_quota(&json!({"rateLimits":{"primary":{"usedPercent":99}},"rateLimitsByLimitId":{
            "codex":{"accountId":"test-account","primary":{"usedPercent":36,"windowDurationMins":10080,"resetsAt":100},"secondary":null},
            "review":{"primary":{"usedPercent":null,"windowDurationMins":300}}
        }}),60_000).unwrap();
        assert_eq!(reading.windows.len(), 2);
        assert_eq!(reading.windows[0].label, "Weekly");
        assert_eq!(reading.windows[0].used_percent, Some(36.));
        assert_eq!(reading.windows[0].resets_at, Some(100_000));
        assert_eq!(reading.windows[1].used_percent, None);
        assert_eq!(reading.account.as_deref(), Some("test-account"));
    }
    #[test]
    fn claude_status_is_not_a_percentage_and_optional_windows_remain_separate() {
        let status=claude_quota(&json!({"rate_limit_info":{"status":"rejected","rateLimitType":"five_hour","resetsAt":100}}),1).unwrap();
        assert_eq!(status.windows[0].used_percent, None);
        let reading=claude_quota(&json!({"rate_limit_info":{"unifiedWindows":{
            "five_hour":{"utilization":0.25,"resetsAt":100},"seven_day":{"utilization":0.75,"resetsAt":200}
        }}}),2).unwrap();
        assert_eq!(reading.windows.len(), 2);
        assert_eq!(reading.windows[0].used_percent, Some(25.));
        assert_eq!(reading.windows[1].used_percent, Some(75.));
    }
    #[test]
    fn initial_output_placeholder_and_missing_delta_remain_partial() {
        let mut stream = ClaudeRequests::default();
        let event=stream.event(&json!({"event":{"type":"message_start","message":{"id":"request","usage":{"input_tokens":10,"output_tokens":1}}}})).unwrap();
        assert!(
            matches!(event,ProviderEvent::Usage {usage,complete:false,..} if usage["output_tokens"]==0)
        );
        assert!(matches!(
            stream.event(&json!({"event":{"type":"message_stop"}})),
            Some(ProviderEvent::Usage {
                complete: false,
                ..
            })
        ));
    }
    #[test]
    fn request_stream_keeps_parallel_subagents_separate_and_ignores_cumulative_results() {
        let mut stream = ClaudeRequests::default();
        let start = |id: &str, parent: Value| json!({"type":"stream_event","parent_tool_use_id":parent,"event":{"type":"message_start","message":{"id":id,"model":"opus","usage":{"input_tokens":100,"output_tokens":0,"cache_read_input_tokens":20}}}});
        assert!(matches!(
            stream.event(&start("main", Value::Null)),
            Some(ProviderEvent::Usage {
                complete: false,
                ..
            })
        ));
        stream.event(&start("child", json!("tool-1")));
        let update = stream
            .event(&json!({"event":{"type":"message_delta","usage":{"output_tokens":37}}}))
            .unwrap();
        if let ProviderEvent::Usage {
            request_id, usage, ..
        } = update
        {
            assert_eq!(request_id, "main");
            assert_eq!(usage["input_tokens"], 100);
            assert_eq!(usage["output_tokens"], 37);
        } else {
            panic!("Usage expected");
        }
        assert!(
            matches!(stream.event(&json!({"event":{"type":"message_stop"}})),Some(ProviderEvent::Usage{request_id,complete:true,..}) if request_id=="main")
        );
        assert!(
            matches!(stream.event(&json!({"parent_tool_use_id":"tool-1","event":{"type":"message_stop"}})),Some(ProviderEvent::Usage{request_id,..}) if request_id=="child")
        );
        assert!(
            stream
                .event(
                    &json!({"type":"result","usage":{"input_tokens":10000,"output_tokens":10000}})
                )
                .is_none()
        );
    }
}
