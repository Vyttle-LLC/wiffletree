//! Self-update from GitHub Releases.
//!
//! Only release builds update themselves: CI embeds `WIFFLETREE_VERSION`, while local builds
//! leave it unset so a development bundle is never replaced. A newer release is downloaded,
//! checked and staged in the background; the bundle swap happens after the app exits, either
//! from "Restart to update" or on an ordinary quit.
use anyhow::{Context as _, Result, bail, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub const VERSION: Option<&str> = option_env!("WIFFLETREE_VERSION");
const LATEST_RELEASE: &str = "https://api.github.com/repos/Vyttle-LLC/wiffletree/releases/latest";

/// A verified bundle waiting to replace the running one.
#[derive(Clone)]
pub struct Staged {
    pub version: String,
    /// The release notes' `## Summary` section.
    pub summary: Option<String>,
    /// The release page, which holds the full changelog.
    pub page: String,
    app: PathBuf,
    installed: PathBuf,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    /// `sha256:<hex>`, published by GitHub for every uploaded asset.
    digest: Option<String>,
}

/// The running bundle, when this build may update itself in place.
pub fn installed_bundle() -> Option<PathBuf> {
    VERSION?;
    // Releases are the stable app; installing one over the beta would replace it.
    if crate::data_dir::is_beta() {
        return None;
    }
    let bundle = std::env::current_exe().ok()?.ancestors().nth(3)?.to_owned();
    (bundle.extension()? == "app" && writable(bundle.parent()?)).then_some(bundle)
}

/// Downloads and verifies the latest release when it is newer than `current`.
pub fn check(current: &str, installed: &Path) -> Result<Option<Staged>> {
    let release = latest_release()?;
    let version = release.tag_name.trim_start_matches('v');
    if !newer(version, current)? {
        return Ok(None);
    }
    let name = asset_name(version);
    // A release published before CI attaches its build is not an update yet.
    let Some(asset) = release.assets.iter().find(|asset| asset.name == name) else {
        return Ok(None);
    };
    let digest = asset
        .digest
        .as_deref()
        .with_context(|| format!("{name} has no published digest"))?;

    let staging = cache_directory()?.join("update");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    let archive = staging.join(&name);
    curl(&[
        "--fail",
        "--max-time",
        "900",
        "--output",
        path_str(&archive)?,
        &asset.browser_download_url,
    ])
    .with_context(|| format!("Couldn't download {name}"))?;
    ensure!(
        matches_digest(&fs::read(&archive)?, digest),
        "{name} does not match its published digest"
    );
    run(Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(&archive)
        .arg(&staging))?;
    let app = staging.join("Wiffletree.app");
    run(Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&app))?;
    // A Developer ID build only accepts updates signed by the same team.
    if let Some(team) = team_identifier(installed)? {
        ensure!(
            team_identifier(&app)?.as_deref() == Some(team.as_str()),
            "{name} is not signed by team {team}"
        );
    }
    Ok(Some(Staged {
        version: version.to_owned(),
        summary: release.body.as_deref().and_then(summary),
        page: release.html_url.clone(),
        app,
        installed: installed.to_owned(),
    }))
}

impl Staged {
    /// Swaps the bundle once this process exits, then optionally reopens it.
    pub fn install_after_exit(&self, relaunch: bool) -> Result<()> {
        Command::new("/bin/sh")
            .args(["-c", SWAP, "swap"])
            .arg(std::process::id().to_string())
            .arg(&self.installed)
            .arg(&self.app)
            .arg(if relaunch { "1" } else { "0" })
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // Its own group, so stopping the app's children cannot stop the swap.
            .process_group(0)
            .spawn()
            .context("Could not start the updater")?;
        Ok(())
    }
}

/// Waits for the app to exit, swaps bundles and restores the old one if the move fails.
const SWAP: &str = r#"
while kill -0 "$1" 2>/dev/null; do sleep 0.2; done
previous="$3.previous"
rm -rf "$previous"
if mv "$2" "$previous"; then
    if mv "$3" "$2"; then rm -rf "$previous"; else mv "$previous" "$2"; fi
fi
if [ "$4" = 1 ]; then open "$2"; fi
"#;

/// The `## Summary` section, up to the next heading such as the generated `## What's Changed`.
/// Lines stay separate, as GitHub shows them on the release page.
fn summary(notes: &str) -> Option<String> {
    let summary = notes
        .lines()
        .map(str::trim)
        .skip_while(|line| *line != "## Summary")
        .skip(1)
        .take_while(|line| !line.starts_with('#'))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!summary.is_empty()).then_some(summary)
}

fn asset_name(version: &str) -> String {
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    };
    format!("Wiffletree-{version}-macos-{arch}.zip")
}

fn newer(candidate: &str, current: &str) -> Result<bool> {
    Ok(parse_version(candidate)? > parse_version(current)?)
}

fn parse_version(version: &str) -> Result<Vec<u64>> {
    version
        .split('.')
        .map(|part| {
            part.parse()
                .with_context(|| format!("Invalid version {version}"))
        })
        .collect()
}

fn matches_digest(bytes: &[u8], digest: &str) -> bool {
    let actual = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    digest.strip_prefix("sha256:") == Some(actual.as_str())
}

fn team_identifier(app: &Path) -> Result<Option<String>> {
    let output = Command::new("/usr/bin/codesign")
        .args(["-d", "--verbose=2"])
        .arg(app)
        .output()?;
    ensure!(
        output.status.success(),
        "Could not read the signature of {}",
        app.display()
    );
    Ok(parse_team(&String::from_utf8_lossy(&output.stderr)))
}

fn parse_team(details: &str) -> Option<String> {
    details
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier="))
        .filter(|team| *team != "not set")
        .map(str::to_owned)
}

fn cache_directory() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join("Library/Caches/com.vyttle.wiffletree"))
}

fn writable(directory: &Path) -> bool {
    let probe = directory.join(".wiffletree-update-probe");
    let created = fs::File::create(&probe).is_ok();
    let _ = fs::remove_file(probe);
    created
}

fn latest_release() -> Result<Release> {
    let response = curl(&[
        "--max-time",
        "30",
        "--write-out",
        "\n%{http_code}",
        LATEST_RELEASE,
    ])
    .context("Couldn't reach GitHub")?;
    parse_feed(&String::from_utf8_lossy(&response))
}

/// Reads the feed response, whose last line is the HTTP status from `--write-out`.
fn parse_feed(response: &str) -> Result<Release> {
    let (body, status) = response
        .rsplit_once('\n')
        .context("Unreadable release feed")?;
    match status {
        "200" => serde_json::from_str(body).context("Unreadable release feed"),
        // Also what GitHub answers for a private repository.
        "404" => bail!("No published release was found on GitHub."),
        other => bail!("GitHub answered with HTTP {other}."),
    }
}

fn curl(args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("/usr/bin/curl")
        .args(["--silent", "--show-error", "--location"])
        .args(["--header", "Accept: application/vnd.github+json"])
        .args(args)
        .output()?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(output.stdout)
}

fn run(command: &mut Command) -> Result<()> {
    let output = command.output()?;
    ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str().context("Cache path is not UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert!(newer("0.1.10", "0.1.9").unwrap());
        assert!(newer("1.0.0", "0.9.99").unwrap());
        assert!(!newer("0.1.9", "0.1.9").unwrap());
        assert!(!newer("0.1.8", "0.1.9").unwrap());
        assert!(newer("0.1.x", "0.1.9").is_err());
    }

    #[test]
    fn rejects_a_mismatched_digest() {
        let empty = "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert!(matches_digest(b"", empty));
        assert!(!matches_digest(b"tampered", empty));
        assert!(!matches_digest(b"", empty.trim_start_matches("sha256:")));
    }

    #[test]
    fn summarizes_the_summary_section() {
        let notes = "## Summary\r\nClearer messages\r\n\r\n* Newer dependencies\r\n\r\n## What's Changed\r\n* Fix messages by @someone";
        assert_eq!(
            summary(notes).as_deref(),
            Some("Clearer messages\n* Newer dependencies")
        );
        assert_eq!(summary("Intro\n## What's Changed\n* Add tasks"), None);
        assert_eq!(summary("## Summary\n## What's Changed\n* Add tasks"), None);
        assert_eq!(summary(""), None);
    }

    #[test]
    fn explains_feed_failures_in_plain_words() {
        let missing = parse_feed("{\"message\":\"Not Found\"}\n404")
            .err()
            .unwrap();
        assert_eq!(
            missing.to_string(),
            "No published release was found on GitHub."
        );
        let limited = parse_feed("{}\n403").err().unwrap();
        assert_eq!(limited.to_string(), "GitHub answered with HTTP 403.");
        let release = parse_feed(
            "{\"tag_name\":\"v0.1.3\",\"html_url\":\"https://example.com\",\"body\":null,\"assets\":[]}\n200",
        )
        .unwrap();
        assert_eq!(release.tag_name, "v0.1.3");
    }

    #[test]
    fn reads_the_signing_team() {
        assert_eq!(
            parse_team("Identifier=com.vyttle.wiffletree\nTeamIdentifier=ABCDE12345\n"),
            Some("ABCDE12345".into())
        );
        assert_eq!(
            parse_team("Signature=adhoc\nTeamIdentifier=not set\n"),
            None
        );
    }

    #[test]
    fn swaps_the_bundle_after_the_app_exits() {
        let root = tempfile::tempdir().unwrap();
        let installed = root.path().join("Wiffletree.app");
        let app = root.path().join("update/Wiffletree.app");
        fs::create_dir_all(&installed).unwrap();
        fs::create_dir_all(&app).unwrap();
        fs::write(installed.join("version"), "old").unwrap();
        fs::write(app.join("version"), "new").unwrap();
        let exited = Command::new("/usr/bin/true").spawn().unwrap();
        let pid = exited.id();
        exited.wait_with_output().unwrap();

        let status = Command::new("/bin/sh")
            .args(["-c", SWAP, "swap", &pid.to_string()])
            .arg(&installed)
            .arg(&app)
            .arg("0")
            .status()
            .unwrap();

        assert!(status.success());
        assert_eq!(
            fs::read_to_string(installed.join("version")).unwrap(),
            "new"
        );
        assert!(!app.exists());
        assert!(!root.path().join("update/Wiffletree.app.previous").exists());
    }
}
