//! Local Network privacy probe for the remote listener (task 1.1 of
//! `openspec/changes/remote-hosts`).
//!
//! Listens on TCP on every interface and advertises `_wiffletree._tcp`, then reports what the
//! system allowed. Sign it, run the signed copy as a launchd agent and connect from another
//! device; `docs/DEVELOPMENT.md` lists the runs.
//!
//!   local_network_probe <port> <seconds> [<address:port to connect to>]

#[cfg(target_os = "macos")]
fn main() {
    use std::io::{Read, Write};
    let args: Vec<String> = std::env::args().skip(1).collect();
    let port: u16 = args.first().and_then(|p| p.parse().ok()).unwrap_or(47821);
    let seconds: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(120);
    let say = |line: String| {
        println!("{} {line}", chrono_like_now());
        let _ = std::io::stdout().flush();
    };
    let listener = match std::net::TcpListener::bind(("0.0.0.0", port)) {
        Ok(listener) => listener,
        Err(error) => return say(format!("listen: refused: {error}")),
    };
    say(format!("listen: bound 0.0.0.0:{port}"));
    let txt = workspace_host::remote::bonjour::txt_record(&[
        ("id", "0123456789abcdef"),
        ("name", "Local network probe"),
        ("version", "0"),
        ("pairing", "0"),
    ])
    .unwrap();
    let _advertisement =
        match workspace_host::remote::bonjour::Advertisement::register("Wiffletree probe", port, &txt) {
            Ok(advertisement) => {
                say(format!("advertise: registered as {:?}", advertisement.name));
                Some(advertisement)
            }
            Err(error) => {
                say(format!("advertise: refused: {error:#}"));
                None
            }
        };
    if let Some(target) = args.get(2) {
        let address = target.parse().expect("an address:port");
        match std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_secs(5)) {
            Ok(_) => say(format!("connect {target}: connected")),
            Err(error) => say(format!("connect {target}: refused: {error}")),
        }
    }
    listener.set_nonblocking(true).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    while std::time::Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, peer)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.write_all(b"wiffletree probe\n");
                let mut byte = [0];
                let _ = stream.read(&mut byte);
                say(format!("accept: {peer}"));
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
    say("done".into());
}

#[cfg(target_os = "macos")]
fn chrono_like_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("local_network_probe runs only on macOS");
    std::process::exit(1);
}
