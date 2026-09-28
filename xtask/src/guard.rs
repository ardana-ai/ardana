//! The home guard behind [`crate::sandbox::Sandbox::guarded`]: a metadata-only
//! snapshot (kind, size, mtime; no hashing) of the home paths tools write to
//! by default, compared before and after a step.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::Metadata;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};

/// Paths under the real home that no xtask step may change (R1.8).
const GUARDED: &[&str] = &[
    ".cache/huggingface",
    ".ardana",
    ".ollama/models",
    ".cache/uv",
    ".local/share/uv",
    ".local/bin",
    "Library/Caches/pip",
    ".npm",
    "Library/Caches/ms-playwright",
    ".cache/puppeteer",
    "Library/Caches/dev.trunkrs.trunk",
    ".impeccable",
    ".cargo/bin",
    ".cargo/.crates.toml",
    ".cargo/.crates2.json",
];

/// Subtrees of guarded paths that the user's own tools rewrite on every run.
const EXCLUDED_DIRS: &[&str] = &[".npm/_npx", ".npm/_cacache", ".npm/_logs"];

fn excluded(rel: &Path) -> bool {
    EXCLUDED_DIRS.iter().any(|dir| rel.starts_with(dir))
        || (rel.starts_with(".impeccable")
            && rel
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("-check.json")))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    File,
    Dir,
    Symlink,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Entry {
    kind: Kind,
    size: u64,
    mtime: Option<SystemTime>,
}

impl Entry {
    fn of(meta: &Metadata) -> Entry {
        let file_type = meta.file_type();
        let kind = if file_type.is_symlink() {
            Kind::Symlink
        } else if file_type.is_dir() {
            Kind::Dir
        } else if file_type.is_file() {
            Kind::File
        } else {
            Kind::Other
        };
        Entry {
            kind,
            size: meta.len(),
            mtime: meta.modified().ok(),
        }
    }

    /// Directories count as changed only when they appear, vanish or change
    /// kind; the entries inside them report their own changes.
    fn differs(&self, other: &Entry) -> bool {
        self.kind != other.kind
            || (self.kind != Kind::Dir && (self.size != other.size || self.mtime != other.mtime))
    }
}

pub(crate) struct HomeSnapshot {
    entries: BTreeMap<PathBuf, Entry>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Change {
    Added(PathBuf),
    Removed(PathBuf),
    Modified(PathBuf),
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Change::Added(path) => write!(f, "added    {}", path.display()),
            Change::Removed(path) => write!(f, "removed  {}", path.display()),
            Change::Modified(path) => write!(f, "modified {}", path.display()),
        }
    }
}

impl HomeSnapshot {
    pub(crate) fn take(home: &Path) -> Result<HomeSnapshot> {
        let mut entries = BTreeMap::new();
        for rel in GUARDED {
            walk(home, Path::new(rel), &mut entries)?;
        }
        Ok(HomeSnapshot { entries })
    }

    pub(crate) fn changes(&self, after: &HomeSnapshot) -> Vec<Change> {
        let mut changes = Vec::new();
        for (path, before) in &self.entries {
            match after.entries.get(path) {
                None => changes.push(Change::Removed(path.clone())),
                Some(now) if before.differs(now) => changes.push(Change::Modified(path.clone())),
                Some(_) => {}
            }
        }
        changes.extend(
            after
                .entries
                .keys()
                .filter(|path| !self.entries.contains_key(*path))
                .map(|path| Change::Added(path.clone())),
        );
        changes
    }
}

/// Records `home/rel` and, for a directory, everything below it without
/// following symlinks. A missing path records nothing, so creating it shows
/// up as added.
fn walk(home: &Path, rel: &Path, entries: &mut BTreeMap<PathBuf, Entry>) -> Result<()> {
    let mut stack = vec![rel.to_path_buf()];
    while let Some(rel) = stack.pop() {
        if excluded(&rel) {
            continue;
        }
        let path = home.join(&rel);
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(err) if err.kind() == ErrorKind::NotFound => continue,
            Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
        };
        if meta.is_dir() {
            let dir =
                std::fs::read_dir(&path).with_context(|| format!("listing {}", path.display()))?;
            for child in dir {
                let child = child.with_context(|| format!("listing {}", path.display()))?;
                stack.push(rel.join(child.file_name()));
            }
        }
        entries.insert(path, Entry::of(&meta));
    }
    Ok(())
}
