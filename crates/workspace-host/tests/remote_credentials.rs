//! Credentials stay on the host: a scripted session of two paired devices against a host whose
//! environment and credential files hold planted secrets receives none of them. Its own test
//! binary, because it sets the process's home folder and environment.
mod remote_support;

use remote_support::*;
use std::path::Path;
use workspace_core::{pairing::KeyPair, *};

/// Planted where a host keeps credentials: each file under the home folder, and each variable.
const FILES: [(&str, &str); 7] = [
    (".config/gh/hosts.yml", "planted-gh-oauth-41"),
    (".config/git/credentials", "planted-git-store-42"),
    (".git-credentials", "planted-git-store-43"),
    (".ssh/id_ed25519", "planted-ssh-key-44"),
    (".claude/.credentials.json", "planted-claude-45"),
    (".codex/auth.json", "planted-codex-46"),
    (".netrc", "planted-netrc-47"),
];
const VARIABLES: [(&str, &str); 4] = [
    ("GH_TOKEN", "planted-env-51"),
    ("GITHUB_TOKEN", "planted-env-52"),
    ("ANTHROPIC_API_KEY", "planted-env-53"),
    ("OPENAI_API_KEY", "planted-env-54"),
];

#[test]
fn no_reply_or_event_carries_a_planted_secret_and_credential_paths_are_refused() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    for (file, secret) in FILES {
        let path = home.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, secret).unwrap();
    }
    // SAFETY: the only test in this binary, before its host starts.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("CODEX_HOME");
        for (name, secret) in VARIABLES {
            std::env::set_var(name, secret);
        }
    }
    let studio = Studio::start(0);
    studio
        .host(Command::SetWorkspacesDir {
            path: home.to_string_lossy().into(),
        })
        .unwrap();
    studio.on();
    let (control, view_only) = (KeyPair::generate().unwrap(), KeyPair::generate().unwrap());
    studio.pair(&control, "MacBook");
    studio.pair(&view_only, "Office iMac");
    studio.level(&view_only, AccessLevel::ViewOnly);
    let mut macbook = studio.connect(&control).unwrap();
    let mut office = studio.connect(&view_only).unwrap();

    macbook
        .call(Command::Subscribe {
            after: None,
            kinds: None,
        })
        .unwrap();
    let coordinator = studio.coordinator.clone();
    let session: Vec<Command> = vec![
        Command::Snapshot,
        Command::Archive,
        Command::Settings,
        Command::Usage {
            days: 7,
            project_id: None,
            provider: None,
            timezone: "UTC".into(),
        },
        Command::Quotas,
        Command::InspectProvider {
            provider: Provider::Claude,
        },
        Command::CoreInstructions,
        Command::LeftoverWorktrees,
        send("scripted", &coordinator),
        Command::Messages {
            session_id: coordinator.clone(),
            before: None,
            limit: 50,
        },
        Command::SetOwnerAllowed {
            entry: "Vyttle-LLC".into(),
            allowed: true,
        },
        Command::ListFolder { path: home.clone() },
        Command::ResolvePaths {
            references: FILES.iter().map(|(file, _)| format!("~/{file}")).collect(),
            bases: vec![home.clone()],
        },
        Command::Snapshot,
    ];
    for command in session {
        let _ = macbook.call(command);
    }
    for (file, _) in FILES {
        let _ = macbook.call(Command::ReadFile {
            path: home.join(file),
            max_bytes: 1024,
        });
    }
    let _ = office.call(Command::Snapshot);

    let received = [&macbook.received[..], &office.received[..]].concat();
    assert!(received.len() > 20, "the session ran: {}", received.len());
    for secret in FILES.iter().chain(&VARIABLES).map(|(_, secret)| secret) {
        let leaked: Vec<_> = received.iter().filter(|m| m.contains(secret)).collect();
        assert!(leaked.is_empty(), "{secret} reached a device: {leaked:?}");
    }

    let refused = |connection: &mut Connection, command: Command| {
        let answer = connection.call(command.clone());
        assert!(answer.is_err(), "{command:?} answered {answer:?}");
    };
    let read = |file: &str| Command::ReadFile {
        path: home.join(file),
        max_bytes: 1024,
    };
    refused(&mut office, read(".config/gh/hosts.yml"));
    refused(
        &mut macbook,
        Command::ListFolder {
            path: home.join(".ssh"),
        },
    );
    for connection in [&mut macbook, &mut office] {
        refused(connection, read(".config/git/credentials"));
    }
    assert!(Path::new(&home.join(".ssh/id_ed25519")).exists());
}
