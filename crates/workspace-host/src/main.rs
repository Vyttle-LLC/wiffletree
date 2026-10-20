use anyhow::{Context, Result, ensure};
use std::io::{self, BufRead, Read, Write};
use workspace_core::*;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("serve") {
        return serve(&args[1..]);
    }
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
            serde_json::to_string_pretty(&workspace_host::runtime::models(
                workspace_core::Provider::Codex
            )?)?
        );
        return Ok(());
    }
    if let [
        flag,
        data_dir,
        command,
        provider,
        role,
        model,
        session,
        directory,
    ] = args.as_slice()
        && flag == "--data-dir"
        && command == "probe-context"
    {
        let profile = ModelProfile {
            provider: serde_json::from_value(serde_json::json!(provider))?,
            model: model.clone(),
            effort: String::new(),
        };
        let role = serde_json::from_value(serde_json::json!(role))?;
        let usage = workspace_host::context::usage(&workspace_host::context::Probe {
            profile: &profile,
            provider_session: session,
            directory: std::path::Path::new(directory),
            instructions: &workspace_host::skills::store(data_dir.as_ref()).instructions(role),
        })?;
        println!("{usage:#?}");
        return Ok(());
    }
    if args.len() != 2 || args[0] != "--data-dir" {
        eprintln!(
            "Usage: workspace-host --data-dir <directory>\nReads version-1 JSON-line commands on stdin; writes correlated responses on stdout.\n       workspace-host serve --data-dir <directory> [--child | --app <Wiffletree.app>]  (clients on <directory>/host.sock; see docs/host-protocol.md)\n       workspace-host probe-codex  (model discovery, no inference)\n       workspace-host probe-quota  (Codex account limits, no inference)\n       workspace-host --data-dir <directory> probe-context <claude|codex> <role> <model> <provider-session> <directory>  (context window use, no inference)"
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
                let result = if request.version != STDIN_PROTOCOL {
                    Err("Unsupported protocol version".to_string())
                } else {
                    service
                        .request(request.command)
                        .and_then(|r| r.recv_blocking().map_err(|e| e.to_string())?)
                };
                match result {
                    Ok(value) => Response {
                        version: STDIN_PROTOCOL,
                        id: request.id,
                        result: Some(value),
                        error: None,
                        host_ms: None,
                    },
                    Err(error) => Response {
                        version: STDIN_PROTOCOL,
                        id: request.id,
                        result: None,
                        error: Some(error),
                        host_ms: None,
                    },
                }
            }
            Err(error) => Response {
                version: STDIN_PROTOCOL,
                id: "invalid".into(),
                result: None,
                error: Some(format!("Invalid command: {error}")),
                host_ms: None,
            },
        };
        serde_json::to_writer(&mut output, &response).context("Write response")?;
        writeln!(output)?;
        output.flush()?;
    }
    Ok(())
}

/// `serve --data-dir <dir> [--child | --app <bundle>]`: the host for clients on its socket. A
/// child host reads its credential from the first line of standard input and stops when that
/// closes; otherwise the credential is the data folder's Keychain item, readable by the host and
/// the app at `--app`. Either stops on SIGTERM, SIGINT or SIGHUP, and exits successfully once a
/// client's `StopAgentsAndQuit` or `RestartHost` has stopped it. Run from a versioned copy, it
/// removes the copies no host uses.
fn serve(args: &[String]) -> Result<()> {
    let mut stops = catch_stop_signals()?;
    // SAFETY: no other thread has started yet.
    unsafe { workspace_host::login_env::adopt() };
    let mut data = None;
    let mut child = false;
    let mut app = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => data = args.next().map(std::path::PathBuf::from),
            "--child" => child = true,
            "--app" => app = args.next().map(std::path::PathBuf::from),
            other => anyhow::bail!("Unknown serve argument {other}"),
        }
    }
    let data = data.context("serve needs --data-dir <directory>")?;
    ensure!(!(child && app.is_some()), "A child host takes no --app");
    std::fs::create_dir_all(&data)?;
    let data = std::fs::canonicalize(&data)?;
    workspace_host::socket::socket_path(&data)?;
    let binary = std::env::current_exe()?;
    let credential = if child {
        workspace_host::socket::read_credential(&mut io::stdin().lock())?
    } else {
        let mut trusted = vec![binary.as_path()];
        trusted.extend(app.as_deref());
        workspace_host::keychain::credential(&data, &trusted)?
    };
    let remote = {
        let (data, mut trusted) = (data.clone(), vec![binary.clone()]);
        trusted.extend(app.clone());
        workspace_host::remote::Options {
            key: Box::new(move || {
                let trusted: Vec<_> = trusted.iter().map(|p| p.as_path()).collect();
                workspace_host::keychain::host_key(&data, &trusted)
            }),
            advertiser: Box::new(workspace_host::remote::Bonjour::default()),
            binary: binary.clone(),
            ..Default::default()
        }
    };
    let service =
        workspace_host::service::Service::start_with(data.clone(), binary.clone(), remote)?;
    // Only now that this host owns the store is no other copy in use.
    workspace_core::host_copy::remove_unused(&data, &binary);
    let options = workspace_host::socket::Options {
        credential,
        // SAFETY: getuid cannot fail.
        owner: unsafe { libc::getuid() },
        binary,
    };
    let listener = workspace_host::socket::listen(service.clone(), &data, options)?;
    let (stop, stopped) = std::sync::mpsc::channel();
    if child {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let _ = io::copy(&mut io::stdin().lock(), &mut io::sink());
            let _ = stop.send(());
        });
    }
    let ended = service.ended();
    {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let _ = ended.recv_blocking();
            let _ = stop.send(());
        });
    }
    std::thread::spawn(move || {
        let _ = stops.read(&mut [0]);
        let _ = stop.send(());
    });
    let _ = stopped.recv();
    // The host stops first, so the listener's wait sees every reply answered.
    service.shutdown();
    drop(listener);
    Ok(())
}

/// The write end of the pipe `catch_stop_signals` returns the read end of.
static STOP_PIPE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

extern "C" fn on_stop_signal(_: libc::c_int) {
    let pipe = STOP_PIPE.load(std::sync::atomic::Ordering::Relaxed);
    // SAFETY: write is async-signal-safe; the pipe stays open for the life of the process.
    unsafe { libc::write(pipe, [1u8].as_ptr().cast(), 1) };
}

/// Makes SIGTERM, SIGINT and SIGHUP write to a pipe whose read end it returns, so a thread can
/// shut the host down cleanly. Caught signals return to their default action on exec and no
/// signal is blocked, so the programs the host starts stop on them as usual.
fn catch_stop_signals() -> Result<std::fs::File> {
    use std::os::fd::FromRawFd as _;
    let mut ends = [0; 2];
    // SAFETY: `ends` has room for both descriptors; the action is zeroed, then filled in.
    unsafe {
        ensure!(
            libc::pipe(ends.as_mut_ptr()) == 0,
            io::Error::last_os_error()
        );
        for end in ends {
            libc::fcntl(end, libc::F_SETFD, libc::FD_CLOEXEC);
        }
        STOP_PIPE.store(ends[1], std::sync::atomic::Ordering::Relaxed);
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = on_stop_signal as extern "C" fn(libc::c_int) as usize;
        libc::sigemptyset(&mut action.sa_mask);
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            ensure!(
                libc::sigaction(signal, &action, std::ptr::null_mut()) == 0,
                io::Error::last_os_error()
            );
        }
        Ok(std::fs::File::from_raw_fd(ends[0]))
    }
}
