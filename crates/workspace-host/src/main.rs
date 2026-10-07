use anyhow::{Context, Result, ensure};
use std::io::{self, BufRead, Read, Write};
use workspace_core::*;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("agent-mcp") {
        return workspace_host::mcp::serve_stdio();
    }
    if args.first().map(String::as_str) == Some("probe-quota") {
        println!(
            "{}",
            serde_json::to_string_pretty(&workspace_host::runtime::codex_quota()?)?
        );
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("probe-codex") {
        println!(
            "{}",
            serde_json::to_string_pretty(&workspace_host::runtime::codex_models()?)?
        );
        return Ok(());
    }
    if let [command, provider, role, model, session, directory] = args.as_slice()
        && command == "probe-context"
    {
        let profile = ModelProfile {
            provider: serde_json::from_value(serde_json::json!(provider))?,
            model: model.clone(),
            effort: String::new(),
        };
        let usage = workspace_host::context::usage(&workspace_host::context::Probe {
            role: serde_json::from_value(serde_json::json!(role))?,
            profile: &profile,
            provider_session: session,
            directory: std::path::Path::new(directory),
        })?;
        println!("{usage:#?}");
        return Ok(());
    }
    if args.len() != 2 || args[0] != "--data-dir" {
        eprintln!(
            "Usage: workspace-host --data-dir <directory>\nReads version-1 JSON-line commands on stdin; writes correlated responses on stdout.\n       workspace-host probe-codex  (model discovery, no inference)\n       workspace-host probe-quota  (Codex account limits, no inference)\n       workspace-host probe-context <claude|codex> <role> <model> <provider-session> <directory>  (context window use, no inference)"
        );
        return Ok(());
    }
    let service =
        workspace_host::service::Service::start(args[1].clone().into(), std::env::current_exe()?)?;
    let mut reader = io::stdin().lock();
    let mut output = io::stdout().lock();
    loop {
        let mut line = Vec::new();
        let count = reader
            .by_ref()
            .take((256 * 1024 + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        ensure!(line.len() <= 256 * 1024, "Command exceeds 256 KiB bound");
        let response = match serde_json::from_slice::<Request>(&line) {
            Ok(request) => {
                let result = if request.version != PROTOCOL_VERSION {
                    Err("Unsupported protocol version".to_string())
                } else {
                    service
                        .request(request.command)
                        .and_then(|r| r.recv_blocking().map_err(|e| e.to_string())?)
                };
                match result {
                    Ok(value) => Response {
                        version: PROTOCOL_VERSION,
                        id: request.id,
                        result: Some(value),
                        error: None,
                    },
                    Err(error) => Response {
                        version: PROTOCOL_VERSION,
                        id: request.id,
                        result: None,
                        error: Some(error),
                    },
                }
            }
            Err(error) => Response {
                version: PROTOCOL_VERSION,
                id: "invalid".into(),
                result: None,
                error: Some(format!("Invalid command: {error}")),
            },
        };
        serde_json::to_writer(&mut output, &response).context("Write response")?;
        writeln!(output)?;
        output.flush()?;
    }
    Ok(())
}
