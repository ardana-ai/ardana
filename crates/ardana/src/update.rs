//! `ardana update` (Q1) and the check behind it: the install receipt dist's installers write, the latest GitHub
//! Release and that release's installer, all through axoupdater. `run` and `serve` make that check once a day and
//! say when a newer release exists (Q4, Q5).

use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use ardana_registry::Registry;
use axoupdater::{AxoUpdater, AxoupdateError, Version};
use serde_json::{Value, json};

use crate::block_on;

/// The name the installers know ardana by: the receipt is `ardana-receipt.json`, the installer `ardana-installer.sh`.
const APP: &str = "ardana";

/// This binary's version.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How long one request may take. A lookup that fails sends two: axoupdater asks for the release list when
/// `releases/latest` gives it no release.
const TIMEOUT: Duration = Duration::from_secs(10);

/// What installs the latest release where no receipt can.
const INSTALL: &str = if cfg!(windows) {
    "irm https://ardana.ai/install.ps1 | iex"
} else {
    "curl -fsSL https://ardana.ai/install.sh | sh"
};

/// Set to `1`, it stops `run` and `serve` from checking for a newer release (Q3); `ardana update` ignores it.
pub const NO_CHECK_VAR: &str = "ARDANA_NO_UPDATE_CHECK";

/// The daily check's file in the Ardana home: `checked_at`, when the last lookup started (Unix seconds), and
/// `version`, the newest release seen, once a lookup answered.
const CHECK_FILE: &str = "update-check.json";

/// How long a lookup, answered or failed, stands before the next one.
const DAY: u64 = 24 * 60 * 60;

/// What the latest release is to this copy of ardana.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// The installers did not place this executable: there is no receipt, or it is for another copy.
    Unmanaged,
    /// The latest release is the receipt's version, or older.
    UpToDate,
    /// The latest release is above the receipt's version: its version, `MAJOR.MINOR.PATCH`.
    Newer(String),
}

/// Reads the install receipt and looks the latest release up once, blocking. It runs a runtime of its own, so the
/// caller is a plain thread, never one inside a tokio runtime. Without a receipt for this executable nothing is
/// sent.
pub fn check() -> Result<Check> {
    match installed()? {
        Some(mut updater) => block_on(latest(&mut updater))?,
        None => Ok(Check::Unmanaged),
    }
}

/// `ardana update`: the latest release installed over this copy by its own installer, when the receipt says an
/// installer placed it (Q2).
pub fn update() -> Result<()> {
    let Some(mut updater) = installed()? else {
        bail!(
            "this ardana was not installed by the installer, so it cannot update itself; \
             install the latest release with:\n  {INSTALL}"
        );
    };
    // One runtime under the lookup and the install: the client's connections do not outlive the runtime they
    // were opened on.
    block_on(async {
        if let Check::Newer(version) = latest(&mut updater).await? {
            // The lookup's release is the one installed: `run` asks for no other.
            let updated = updater
                .run()
                .await
                .with_context(|| format!("installing ardana {version}"))?
                .with_context(|| format!("ardana {version} was not installed"))?;
            let from = updated
                .old_version
                .map_or_else(|| VERSION.to_string(), |old| old.to_string());
            println!("updated ardana from {from} to {}", updated.new_version);
        } else {
            println!("ardana is up to date ({VERSION})");
        }
        Ok(())
    })?
}

/// An updater holding the receipt of this executable, or `None` when the installers did not place it: no receipt,
/// one that does not read (its versions included), or one whose `install_prefix` is another directory.
fn installed() -> Result<Option<AxoUpdater>> {
    let mut updater = AxoUpdater::new_for(APP);
    match updater.load_receipt() {
        Ok(_) => {}
        Err(
            AxoupdateError::NoReceipt { .. }
            | AxoupdateError::ReceiptLoadFailed { .. }
            | AxoupdateError::Version(_),
        ) => return Ok(None),
        Err(err) => return Err(err).context("reading the install receipt"),
    }
    if !updater
        .check_receipt_is_for_this_executable()
        .context("comparing the install receipt with this executable")?
    {
        return Ok(None);
    }
    Ok(Some(updater))
}

/// Looks the latest stable release up (one `releases/latest` GET, and the release list when that fails) and compares
/// it with the receipt's version.
async fn latest(updater: &mut AxoUpdater) -> Result<Check> {
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .context("building the HTTP client")?;
    updater.set_client(client);
    let (version, newer) = async {
        let version = updater.query_new_version().await?.map(ToString::to_string);
        // The release is at hand now, so this sends nothing.
        let newer = updater.is_update_needed().await?;
        Ok::<_, AxoupdateError>((version, newer))
    }
    .await
    .context("the update check failed")?;
    Ok(match version {
        Some(version) if newer => Check::Newer(version),
        _ => Check::UpToDate,
    })
}

/// The daily check of `run` and `serve`: where the notice of a newer release arrives, when there is one. Within a
/// day of the last lookup the notice comes from the check's file and is there at once (Q5); otherwise a thread of
/// its own looks the release up, so the caller never waits on the network, and nothing arrives when the lookup
/// fails, when the installers did not place this copy or under [`NO_CHECK_VAR`].
pub fn notice() -> mpsc::Receiver<String> {
    let (tell, told) = mpsc::channel();
    if std::env::var_os(NO_CHECK_VAR).is_some_and(|value| value == "1") {
        return told;
    }
    let Ok(home) = Registry::home() else {
        return told;
    };
    let path = home.join(CHECK_FILE);
    let seen = std::fs::read_to_string(&path)
        .map(|text| Seen::parse(&text))
        .unwrap_or_default();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    if !seen.due(now) {
        if let Some(notice) = seen.news() {
            let _ = tell.send(notice);
        }
        return told;
    }
    std::thread::spawn(move || {
        // A check that cannot be made or recorded says nothing.
        if let Ok(Some(notice)) = look(&path, now) {
            let _ = tell.send(notice);
        }
    });
    told
}

/// One lookup for the daily check: `path` records `now` before the lookup starts and the version once it answers.
/// The notice, when that version is news.
fn look(path: &Path, now: u64) -> Result<Option<String>> {
    // A copy the installers did not place checks nothing and leaves no file.
    if installed()?.is_none() {
        return Ok(None);
    }
    let (mut seen, ours) = claim(path, now)?;
    if !ours {
        return Ok(seen.news());
    }
    let version = match check() {
        Ok(Check::Newer(version)) => version,
        Ok(Check::UpToDate) => VERSION.to_string(),
        Ok(Check::Unmanaged) => return Ok(None),
        // The lookup failed: the file keeps the version an earlier one saw, and that is still the news (Q5).
        Err(_) => return Ok(seen.news()),
    };
    seen.version = Some(version);
    seen.put(&mut locked(path)?)?;
    Ok(seen.news())
}

/// Dates a lookup at `now` in the check's file, keeping the version seen so far, unless the file holds a check that
/// still stands: what the file holds now, and whether the lookup is this caller's to make. The file's lock is held
/// from the read to the write, so of the ardanas that find a lookup due together one makes it.
fn claim(path: &Path, now: u64) -> Result<(Seen, bool)> {
    let mut file = locked(path)?;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .with_context(|| format!("reading {}", path.display()))?;
    let mut seen = Seen::parse(&text);
    if !seen.due(now) {
        return Ok((seen, false));
    }
    seen.checked_at = now;
    seen.put(&mut file)?;
    Ok((seen, true))
}

/// The check's file, created when missing, under its exclusive lock, which closing it releases.
fn locked(path: &Path) -> Result<File> {
    if let Some(home) = path.parent() {
        std::fs::create_dir_all(home).with_context(|| format!("creating {}", home.display()))?;
    }
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    file.lock()
        .with_context(|| format!("locking {}", path.display()))?;
    Ok(file)
}

/// What [`CHECK_FILE`] holds; a file that is missing or does not read is no check yet.
#[derive(Default)]
struct Seen {
    checked_at: u64,
    version: Option<String>,
}

impl Seen {
    fn parse(text: &str) -> Seen {
        let value: Value = serde_json::from_str(text).unwrap_or_default();
        Seen {
            checked_at: value["checked_at"].as_u64().unwrap_or_default(),
            version: value["version"].as_str().map(str::to_string),
        }
    }

    /// Whether a day has passed since the lookup this dates.
    fn due(&self, now: u64) -> bool {
        now.checked_sub(self.checked_at)
            .is_none_or(|age| age >= DAY)
    }

    /// The notice for the version seen, when it is above this binary's.
    fn news(&self) -> Option<String> {
        let version = self.version.as_deref()?;
        let newer = Version::parse(version).ok()? > Version::parse(VERSION).ok()?;
        newer.then(|| {
            format!(
                "a newer ardana is available: {version} (this is {VERSION}); run `ardana update`"
            )
        })
    }

    /// Replaces what `file` holds.
    fn put(&self, file: &mut File) -> Result<()> {
        let mut value = json!({"checked_at": self.checked_at});
        if let Some(version) = &self.version {
            value["version"] = json!(version);
        }
        file.rewind()?;
        file.set_len(0)?;
        file.write_all(value.to_string().as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Of the checks that find a lookup due at the same moment, one makes it, and it stands for a day.
    #[test]
    fn one_lookup_is_claimed_a_day() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("ardana-update-claim-{}", std::process::id()));
        let path = dir.join(CHECK_FILE);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            &path,
            json!({"checked_at": 1, "version": "9.9.9"}).to_string(),
        )?;

        let now = 10 * DAY;
        let start = std::sync::Barrier::new(16);
        let claims: Vec<bool> = std::thread::scope(|scope| {
            let racers: Vec<_> = (0..16)
                .map(|_| {
                    scope.spawn(|| {
                        start.wait();
                        claim(&path, now).map(|(_, ours)| ours)
                    })
                })
                .collect();
            racers
                .into_iter()
                .map(|racer| racer.join().expect("a claim does not panic"))
                .collect::<Result<_>>()
        })?;
        assert_eq!(claims.iter().filter(|ours| **ours).count(), 1, "{claims:?}");

        // The claim kept the version seen, and no one claims again until a day has passed.
        let (seen, ours) = claim(&path, now + DAY - 1)?;
        assert!(!ours);
        assert_eq!(
            (seen.checked_at, seen.version.as_deref()),
            (now, Some("9.9.9"))
        );
        assert!(claim(&path, now + DAY)?.1);
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
