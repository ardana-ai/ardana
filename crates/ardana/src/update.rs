//! `ardana update` (Q1) and the check behind it: the install receipt dist's installers write, the latest GitHub
//! Release and that release's installer, all through axoupdater.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use axoupdater::{AxoUpdater, AxoupdateError};

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
        Some(mut updater) => latest(&mut updater),
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
    if let Check::Newer(version) = latest(&mut updater)? {
        // The lookup's release is the one installed: `run` asks for no other.
        let updated = block_on(updater.run())?
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
}

/// An updater holding the receipt of this executable, or `None` when the installers did not place it: no receipt,
/// one that does not read, or one whose `install_prefix` is another directory.
fn installed() -> Result<Option<AxoUpdater>> {
    let mut updater = AxoUpdater::new_for(APP);
    match updater.load_receipt() {
        Ok(_) => {}
        Err(AxoupdateError::NoReceipt { .. } | AxoupdateError::ReceiptLoadFailed { .. }) => {
            return Ok(None);
        }
        Err(err) => return Err(err).context("reading the install receipt"),
    }
    if !updater
        .check_receipt_is_for_this_executable()
        .context("comparing the install receipt with this executable")?
    {
        return Ok(None);
    }
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .context("building the HTTP client")?;
    updater.set_client(client);
    Ok(Some(updater))
}

/// Looks the latest stable release up (one `releases/latest` GET, and the release list when that fails) and compares
/// it with the receipt's version.
fn latest(updater: &mut AxoUpdater) -> Result<Check> {
    let (version, newer) = block_on(async {
        let version = updater.query_new_version().await?.map(ToString::to_string);
        // The release is at hand now, so this sends nothing.
        let newer = updater.is_update_needed().await?;
        Ok::<_, AxoupdateError>((version, newer))
    })?
    .context("the update check failed")?;
    Ok(match version {
        Some(version) if newer => Check::Newer(version),
        _ => Check::UpToDate,
    })
}
