//! `ardana update` over dist's install receipt, against a loopback stand-in of GitHub's API; the real download and
//! install is the one `#[ignore]`d network test (Q7).

mod common;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use common::Ardana;
use serde_json::json;

/// The version of the `ardana` under test.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The one lookup an answered check sends.
const LATEST: &str = "/api/v3/repos/ardana-ai/ardana/releases/latest";

/// What a [`GitHub`] answers every request with.
#[derive(Clone)]
enum Answer {
    /// The latest release, `version`, with dist's installers among its assets, sent `after` the request arrived.
    Release { version: String, after: Duration },
    /// A 500.
    Broken,
    /// Nothing: the connection is accepted and held open for a minute.
    Silence,
}

impl Answer {
    fn release(version: &str) -> Answer {
        Answer::Release {
            version: version.to_string(),
            after: Duration::ZERO,
        }
    }
}

/// A loopback stand-in of GitHub's API, which `ARDANA_INSTALLER_GHE_BASE_URL` sends axoupdater to. Every request's
/// path is recorded.
struct GitHub {
    /// What `ARDANA_INSTALLER_GHE_BASE_URL` names.
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl GitHub {
    fn start(answer: Answer) -> Result<GitHub> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = format!("http://{}", listener.local_addr()?);
        let requests: Arc<Mutex<Vec<String>>> = Arc::default();
        let seen = requests.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let (answer, seen) = (answer.clone(), seen.clone());
                // One thread a connection: a held or late answer keeps no other request waiting.
                std::thread::spawn(move || {
                    let _ = reply(stream, &answer, &seen);
                });
            }
        });
        Ok(GitHub { url, requests })
    }

    /// The address of a port no one listens on.
    fn unreachable() -> Result<String> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        Ok(format!("http://{}", listener.local_addr()?))
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("the request log").clone()
    }
}

/// Answers the one request on `stream` with `answer`, recording its path in `seen`.
fn reply(mut stream: TcpStream, answer: &Answer, seen: &Mutex<Vec<String>>) -> Result<()> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte)? == 1 {
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let path = head
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .unwrap_or_default();
    seen.lock().expect("the request log").push(path.to_string());
    let (status, body) = match answer {
        Answer::Release { version, after } => {
            std::thread::sleep(*after);
            let download = format!(
                "https://github.com/ardana-ai/ardana/releases/download/v{version}/ardana-installer"
            );
            let assets: Vec<_> = ["sh", "ps1"]
                .iter()
                .map(|kind| {
                    json!({
                        "name": format!("ardana-installer.{kind}"),
                        "url": format!("{download}.{kind}"),
                        "browser_download_url": format!("{download}.{kind}"),
                    })
                })
                .collect();
            let release = json!({
                "tag_name": format!("v{version}"),
                "name": format!("v{version}"),
                "url": format!("https://api.github.com/repos/ardana-ai/ardana/releases/tags/v{version}"),
                "prerelease": false,
                "assets": assets,
            });
            ("200 OK", release.to_string())
        }
        Answer::Broken => ("500 Internal Server Error", "{}".to_string()),
        Answer::Silence => {
            std::thread::sleep(Duration::from_secs(60));
            return Ok(());
        }
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    Ok(())
}

/// The directory holding the `ardana` under test, which a receipt for it names as its `install_prefix`.
fn bin_dir() -> Result<PathBuf> {
    Ok(Path::new(env!("CARGO_BIN_EXE_ardana"))
        .parent()
        .context("the binary's directory")?
        .to_path_buf())
}

/// Writes `ardana-receipt.json` into `dir` as dist 0.32.0's installers do: `version`, installed in `prefix`.
fn write_receipt(dir: &Path, version: &str, prefix: &Path, modify_path: bool) -> Result<()> {
    let receipt = json!({
        "binaries": ["ardana"],
        "binary_aliases": {},
        "cdylibs": [],
        "cstaticlibs": [],
        "install_layout": "flat",
        "install_prefix": prefix,
        "modify_path": modify_path,
        "provider": {"source": "cargo-dist", "version": "0.32.0"},
        "source": {"app_name": "ardana", "name": "ardana", "owner": "ardana-ai", "release_type": "github"},
        "version": version,
    });
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("ardana-receipt.json"), receipt.to_string())?;
    Ok(())
}

/// A receipt for the `ardana` under test at its own version, in `<scratch>/receipt`; the directory.
fn own_receipt(ardana: &Ardana) -> Result<PathBuf> {
    let dir = ardana.home.with_file_name("receipt");
    write_receipt(&dir, VERSION, &bin_dir()?, true)?;
    Ok(dir)
}

/// `ardana <args>` reading its receipt from `receipts` and GitHub's API at `github`.
fn command(ardana: &Ardana, args: &[&str], receipts: &Path, github: &str) -> Command {
    let mut cmd = ardana.command(args);
    cmd.env("AXOUPDATER_CONFIG_PATH", receipts)
        .env("ARDANA_INSTALLER_GHE_BASE_URL", github);
    cmd
}

/// Runs `cmd`, printing what it wrote; its exit code, stdout and stderr.
fn run(cmd: &mut Command) -> Result<(Option<i32>, String, String)> {
    let output = cmd.output().with_context(|| format!("running {cmd:?}"))?;
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    println!("{cmd:?} ({}):\n{stdout}{stderr}", output.status);
    Ok((output.status.code(), stdout, stderr))
}

/// R1.1: at the latest release, `ardana update` says so after one GET of `releases/latest`.
#[test]
fn up_to_date_sends_one_get() -> Result<()> {
    let ardana = Ardana::new("update-up-to-date")?;
    let receipts = own_receipt(&ardana)?;
    let github = GitHub::start(Answer::release(VERSION))?;
    let (code, stdout, _) = run(&mut command(&ardana, &["update"], &receipts, &github.url))?;
    assert_eq!(code, Some(0));
    assert_eq!(stdout, format!("ardana is up to date ({VERSION})\n"));
    assert_eq!(github.requests(), [LATEST]);
    Ok(())
}

/// R1.2: no receipt, a receipt for another copy and a receipt that does not parse all end in the install line, and
/// nothing is sent.
#[test]
fn a_source_build_is_unmanaged() -> Result<()> {
    let ardana = Ardana::new("update-unmanaged")?;
    let scratch = ardana.home.parent().context("the scratch directory")?;
    let github = GitHub::start(Answer::release("9.9.9"))?;

    let empty = scratch.join("empty");
    std::fs::create_dir_all(&empty)?;
    let elsewhere = scratch.join("elsewhere");
    write_receipt(&elsewhere, VERSION, &scratch.join("bin"), true)?;
    let garbled = scratch.join("garbled");
    std::fs::create_dir_all(&garbled)?;
    std::fs::write(garbled.join("ardana-receipt.json"), "{\"binaries\": [")?;

    for receipts in [&empty, &elsewhere, &garbled] {
        let (code, stdout, stderr) =
            run(&mut command(&ardana, &["update"], receipts, &github.url))?;
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{receipts:?}");
        assert!(
            stderr.contains("not installed by the installer")
                && stderr
                    .trim_end()
                    .ends_with("curl -fsSL https://ardana.ai/install.sh | sh"),
            "{receipts:?}: {stderr}"
        );
    }
    assert_eq!(github.requests(), [] as [&str; 0]);
    Ok(())
}

/// R1.3: a 500, a refused connection and a server that never answers each fail the command within 30 s.
#[test]
fn a_failed_lookup_is_an_error() -> Result<()> {
    let ardana = Ardana::new("update-failed-lookup")?;
    let receipts = own_receipt(&ardana)?;
    let broken = GitHub::start(Answer::Broken)?;
    let silent = GitHub::start(Answer::Silence)?;
    for github in [&broken.url, &GitHub::unreachable()?, &silent.url] {
        let started = Instant::now();
        let (code, stdout, stderr) = run(&mut command(&ardana, &["update"], &receipts, github))?;
        let took = started.elapsed();
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{github}");
        assert!(
            stderr.starts_with("error: the update check failed"),
            "{github}: {stderr}"
        );
        assert!(took < Duration::from_secs(30), "{github}: took {took:?}");
    }
    // A failed lookup is two requests: `releases/latest`, then the release list axoupdater falls back to.
    assert_eq!(
        broken.requests(),
        [LATEST, "/api/v3/repos/ardana-ai/ardana/releases"]
    );
    Ok(())
}

/// R1.4: a copy the installer placed at an old version becomes the real latest release, receipt included.
#[cfg(unix)]
#[test]
#[ignore = "e2e: network, GitHub Releases"]
fn latest_release_is_installed() -> Result<()> {
    let scratch = common::scratch("update-latest-release")?;
    let (bin, config) = (scratch.join("bin"), scratch.join("config"));
    std::fs::create_dir_all(&bin)?;
    let copy = bin.join("ardana");
    std::fs::copy(env!("CARGO_BIN_EXE_ardana"), &copy)?;
    let receipt = config.join("ardana/ardana-receipt.json");
    write_receipt(
        receipt.parent().context("the receipt's directory")?,
        "0.0.1",
        &bin,
        false,
    )?;

    let installed = |args: &[&str]| -> Command {
        let mut cmd = Command::new(&copy);
        cmd.args(args)
            .env("XDG_CONFIG_HOME", &config)
            .env("ARDANA_HOME", scratch.join("home"));
        cmd
    };
    let (code, stdout, _) = run(&mut installed(&["update"]))?;
    assert_eq!(code, Some(0));
    let latest = stdout
        .lines()
        .find_map(|line| line.strip_prefix("updated ardana from 0.0.1 to "))
        .with_context(|| format!("no `updated ardana from 0.0.1 to <latest>` in {stdout:?}"))?
        .to_string();

    assert_ne!(
        std::fs::read(&copy)?,
        std::fs::read(env!("CARGO_BIN_EXE_ardana"))?,
        "{} is still the copy",
        copy.display()
    );
    let (code, stdout, _) = run(&mut installed(&["-v"]))?;
    assert_eq!(
        (code, stdout.trim_end()),
        (Some(0), format!("ardana {latest}").as_str())
    );
    let receipt: serde_json::Value = serde_json::from_slice(&std::fs::read(&receipt)?)?;
    assert_eq!(receipt["version"], latest.as_str(), "{receipt}");
    Ok(())
}

/// R1.5: the command is listed, with its examples.
#[test]
fn help_names_the_command() -> Result<()> {
    let ardana = Ardana::new("update-help")?;
    let help = ardana.ok(["--help"])?;
    assert!(
        help.lines().any(|line| line.starts_with("  update ")),
        "{help}"
    );
    let help = ardana.ok(["update", "--help"])?;
    assert!(help.contains("Examples:\n  ardana update\n"), "{help}");
    Ok(())
}
