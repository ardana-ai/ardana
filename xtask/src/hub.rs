//! The sandbox's Hub cache `tmp/hf/hub` under the end-to-end suites. `cargo xtask fetch` installs no weights, so a
//! suite downloads the ones it reads at their `xtask/fetch.toml` pins, runs, and removes every path the cache gained
//! since ([`with_weights`]); a suite that pulls through `ardana` itself lists the cache and removes what was added the
//! same way ([`paths`], [`remove_added`]).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::fetch::{Hf, Manifest};
use crate::sandbox::Sandbox;

/// `tmp/hf/hub`.
pub fn dir(sandbox: &Sandbox) -> PathBuf {
    sandbox.tmp().join("hf/hub")
}

/// The `[[hf]]` entries of `xtask/fetch.toml` for `repos` (`org/name`), in that order; fails naming one it lacks.
pub fn pins(sandbox: &Sandbox, repos: &[&str]) -> Result<Vec<Hf>> {
    let manifest = Manifest::load(sandbox.repo_root())?;
    repos
        .iter()
        .map(|repo| {
            let pin = manifest.hf.iter().find(|hf| hf.repo == *repo);
            pin.cloned()
                .with_context(|| format!("xtask/fetch.toml has no [[hf]] entry for {repo}"))
        })
        .collect()
}

/// Runs `run` with every file of `repos`' `[[hf]]` entries, weights included, in the cache in its usual layout, then
/// removes every path the cache gained meanwhile, whatever `run` returned. `what` names the caller in the output.
pub fn with_weights<T>(
    sandbox: &Sandbox,
    what: &str,
    repos: &[&str],
    run: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let hub = dir(sandbox);
    let before = paths(&hub)?;
    let ran = pins(sandbox, repos)
        .and_then(|pins| pins.iter().try_for_each(|pin| pin.ensure(sandbox)))
        .and_then(|()| run());
    let removed = remove_added(&hub, &before)
        .with_context(|| format!("removing what {what} added to {}", hub.display()));
    match &removed {
        Ok(bytes) => println!(
            "{what}: removed {:.2} GB from {}",
            *bytes as f64 / 1e9,
            hub.display()
        ),
        Err(err) => println!("{what}: {err:#}"),
    }
    let value = ran?;
    removed.map(|_| value)
}

/// Every path under `dir`, links not followed; none when `dir` is missing.
pub(crate) fn paths(dir: &Path) -> Result<BTreeSet<PathBuf>> {
    fn walk(dir: &Path, into: &mut BTreeSet<PathBuf>) -> Result<()> {
        for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                walk(&entry.path(), into)?;
            }
            into.insert(entry.path());
        }
        Ok(())
    }
    let mut paths = BTreeSet::new();
    if dir.is_dir() {
        walk(dir, &mut paths)?;
    }
    Ok(paths)
}

/// R3.5: removes every path under `dir` that is not in `before`, so a repository the cache held in part keeps the files it
/// had and loses the ones added since; returns the bytes of the files removed.
pub(crate) fn remove_added(dir: &Path, before: &BTreeSet<PathBuf>) -> Result<u64> {
    let mut bytes = 0;
    if !dir.is_dir() {
        return Ok(bytes);
    }
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let added = !before.contains(&path);
        if entry.file_type()?.is_dir() {
            bytes += remove_added(&path, before)?;
            if added {
                std::fs::remove_dir(&path)
                    .with_context(|| format!("removing {}", path.display()))?;
            }
        } else if added {
            bytes += entry.metadata()?.len();
            std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use anyhow::bail;

    use super::*;

    /// Whatever the run returned, the cache is as it was: what a failed run added goes and its error comes back, and
    /// a repository `fetch.toml` does not pin fails before the run, naming it.
    #[test]
    fn with_weights_leaves_the_cache_as_it_was() -> Result<()> {
        let base = std::env::temp_dir().join(format!("hub-with-weights-{}", std::process::id()));
        let root = base.join("repo");
        std::fs::create_dir_all(root.join("xtask"))?;
        std::fs::write(root.join("xtask/fetch.toml"), "")?;
        let sandbox = Sandbox::new(&root, &base)?;
        let hub = dir(&sandbox);
        std::fs::create_dir_all(hub.join("models--a--old"))?;
        let before = paths(&hub)?;

        let failed: Result<()> = with_weights(&sandbox, "test", &[], || {
            std::fs::write(hub.join("models--a--old/weights"), "123")?;
            bail!("the run failed")
        });
        assert_eq!(failed.unwrap_err().to_string(), "the run failed");
        assert_eq!(paths(&hub)?, before);

        let mut ran = false;
        let unpinned = with_weights(&sandbox, "test", &["o/none"], || {
            ran = true;
            Ok(())
        });
        assert!(
            unpinned
                .unwrap_err()
                .to_string()
                .contains("no [[hf]] entry for o/none")
        );
        assert!(!ran);
        assert_eq!(with_weights(&sandbox, "test", &[], || Ok(7))?, 7);
        std::fs::remove_dir_all(&base)?;
        Ok(())
    }

    /// R3.5: what a suite added goes, in a directory that is new and in one that was there, links included; what was there
    /// stays.
    #[test]
    fn remove_added_keeps_what_was_there() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("hub-remove-added-{}", std::process::id()));
        let old = dir.join("models--a--old");
        std::fs::create_dir_all(old.join("blobs"))?;
        std::fs::create_dir_all(old.join("snapshots/c1"))?;
        std::fs::write(old.join("blobs/tokenizer"), "12345")?;
        std::os::unix::fs::symlink(
            "../../blobs/tokenizer",
            old.join("snapshots/c1/tokenizer.json"),
        )?;
        let before = paths(&dir)?;
        assert_eq!(remove_added(&dir, &before)?, 0);

        std::fs::write(old.join("blobs/weights"), "1234567")?;
        std::os::unix::fs::symlink(
            "../../blobs/weights",
            old.join("snapshots/c1/model.safetensors"),
        )?;
        let new = dir.join("models--a--new/snapshots/c2");
        std::fs::create_dir_all(&new)?;
        std::fs::write(new.join("model.gguf"), "123")?;
        std::fs::create_dir_all(dir.join(".locks/models--a--new"))?;
        let link = "../../blobs/weights".len() as u64;
        assert_eq!(remove_added(&dir, &before)?, 7 + link + 3);
        assert_eq!(paths(&dir)?, before);
        assert_eq!(
            std::fs::read_to_string(old.join("snapshots/c1/tokenizer.json"))?,
            "12345"
        );
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
