//! Hugging Face repositories through `hf-hub`, in the standard hub cache (`$HF_HOME/hub`).
//!
//! `hf-hub` 1.0.0 does not read `HF_HUB_OFFLINE`, so [`offline`] reads it and every download passes
//! `local_files_only`. A repository is read at the commit a library entry pins (Q4), else at `main` (an `hf.co/`
//! reference typed by the user). Offline, a repository's files are those of its cached snapshot of that commit (of
//! `main`, the commit `refs/main` names) and no metadata endpoint is called; [`Hub::cache_only`] reads a repository
//! that way whatever the variable says (a browser variant is looked up in the cache first, and taken from there only
//! when the cache holds all of it). A pull that asks for progress prints each download's bytes to stderr
//! ([`FileReport`]).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use hf_hub::progress::{DownloadEvent, FileStatus, Progress, ProgressEvent, ProgressHandler};
use hf_hub::{HFClient, HFError};

use crate::RegistryError;

/// The variable that keeps every Hub lookup in the local cache.
pub const OFFLINE_VAR: &str = "HF_HUB_OFFLINE";

/// Whether `HF_HUB_OFFLINE` is set to a true value (`1`, `ON`, `YES`, `TRUE`, case-insensitively), as the Python
/// client reads it.
pub fn offline() -> bool {
    std::env::var(OFFLINE_VAR).is_ok_and(|v| {
        matches!(
            v.trim().to_ascii_uppercase().as_str(),
            "1" | "ON" | "YES" | "TRUE"
        )
    })
}

/// The variables hf-hub reads, in its order, before it falls back to `$HOME/.cache/huggingface/hub`.
const CACHE_VARS: [&str; 5] = [
    "HF_HUB_CACHE",
    "HUGGINGFACE_HUB_CACHE",
    "HF_HOME",
    "XDG_CACHE_HOME",
    "HOME",
];

/// `<home_dir>/.cache/huggingface/hub` when none of [`CACHE_VARS`] is set (as on Windows), where hf-hub would put the
/// cache under `/tmp`; otherwise hf-hub's own resolution.
fn home_cache_dir() -> Option<PathBuf> {
    if CACHE_VARS.iter().any(|var| std::env::var(var).is_ok()) {
        return None;
    }
    std::env::home_dir().map(|home| home.join(".cache").join("huggingface").join("hub"))
}

/// One Hub client for a pull.
pub(crate) struct Hub {
    client: HFClient,
    offline: bool,
    /// Print download progress to stderr.
    progress: bool,
}

/// The files of one repository commit.
pub(crate) struct Snapshot {
    org: String,
    repo: String,
    commit: String,
    /// Repository paths, sorted.
    pub files: Vec<String>,
}

impl Snapshot {
    /// `hf.co/<org>/<repo>`, for messages.
    pub fn id(&self) -> String {
        format!("{}{}/{}", crate::refs::HF_PREFIX, self.org, self.repo)
    }

    pub fn has(&self, file: &str) -> bool {
        self.files.iter().any(|f| f == file)
    }

    pub fn commit(&self) -> &str {
        &self.commit
    }
}

impl Hub {
    pub fn from_env(progress: bool) -> Result<Hub, RegistryError> {
        let mut builder = HFClient::builder();
        if let Some(dir) = home_cache_dir() {
            builder = builder.cache_dir(dir);
        }
        let client = builder.build().map_err(|err| RegistryError::Hub {
            repo: "the Hugging Face client".into(),
            err,
        })?;
        Ok(Hub::new(client, offline(), progress))
    }

    /// A pull through `client`, every lookup in its cache alone when `offline`.
    pub fn new(client: HFClient, offline: bool, progress: bool) -> Hub {
        Hub {
            client,
            offline,
            progress,
        }
    }

    /// The same client reading the cache alone, as under `HF_HUB_OFFLINE`.
    pub fn cache_only(&self) -> Hub {
        Hub {
            client: self.client.clone(),
            offline: true,
            progress: self.progress,
        }
    }

    /// Whether every lookup stays in the cache.
    pub fn is_offline(&self) -> bool {
        self.offline
    }

    /// `org/repo` at `commit` (a 40-hex commit), else at its `main` commit, and its files: from the Hub, or offline
    /// from the cached snapshot of that commit.
    pub async fn snapshot(
        &self,
        org: &str,
        repo: &str,
        commit: Option<&str>,
    ) -> Result<Snapshot, RegistryError> {
        let id = format!("{}{org}/{repo}", crate::refs::HF_PREFIX);
        let hub_err = |err| RegistryError::Hub {
            repo: id.clone(),
            err,
        };
        let model = self.client.model(org, repo);
        let (commit, mut files) = if self.offline {
            let dir = match model
                .snapshot_download()
                .maybe_revision(commit)
                .local_files_only(true)
                .send()
                .await
            {
                Ok(dir) => dir,
                Err(HFError::LocalEntryNotFound { .. }) => {
                    return Err(RegistryError::NotCached {
                        what: match commit {
                            Some(commit) => format!("{id} at commit {commit}"),
                            None => id,
                        },
                        cache: self.client.cache_dir().to_path_buf(),
                    });
                }
                Err(err) => return Err(hub_err(err)),
            };
            let commit = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut files = Vec::new();
            list_files(&dir, &dir, &mut files)?;
            (commit, files)
        } else {
            let info = model
                .info()
                .maybe_revision(commit)
                .send()
                .await
                .map_err(hub_err)?;
            let commit = info.sha.ok_or_else(|| RegistryError::Invalid {
                what: id.clone(),
                msg: format!(
                    "the Hub reported no commit for {}",
                    commit.unwrap_or("main")
                ),
            })?;
            let files = info
                .siblings
                .unwrap_or_default()
                .into_iter()
                .map(|s| s.rfilename)
                .collect();
            (commit, files)
        };
        files.sort();
        Ok(Snapshot {
            org: org.to_string(),
            repo: repo.to_string(),
            commit,
            files,
        })
    }

    /// The cache path (`snapshots/<commit>/<file>`) of `file`, downloaded unless offline.
    pub async fn file(&self, snapshot: &Snapshot, file: &str) -> Result<PathBuf, RegistryError> {
        let result = self
            .client
            .model(&snapshot.org, &snapshot.repo)
            .download_file()
            .filename(file)
            .revision(snapshot.commit.clone())
            .local_files_only(self.offline)
            .maybe_progress(
                self.progress
                    .then(|| Progress::new(FileReport::new(format!("downloading {file}")))),
            )
            .send()
            .await;
        match result {
            Ok(path) => Ok(path),
            Err(HFError::LocalEntryNotFound { .. }) => Err(RegistryError::NotCached {
                what: format!("{} file {file}", snapshot.id()),
                cache: self.client.cache_dir().to_path_buf(),
            }),
            Err(err) => Err(RegistryError::Hub {
                repo: snapshot.id(),
                err,
            }),
        }
    }
}

/// How often [`FileReport`] prints while bytes move.
const REPORT_EVERY: Duration = Duration::from_secs(1);

/// Prints one file's download to stderr, `<label>: <bytes so far> / <size>`, at most once a [`REPORT_EVERY`] when the
/// count moved and once when it completes. A file already in the cache moves no bytes and prints nothing.
struct FileReport {
    label: String,
    state: Mutex<ReportState>,
}

#[derive(Default)]
struct ReportState {
    /// The file's size, from the download's start (0 until known).
    total: u64,
    /// When the last line was printed and the count it printed; `None` until bytes move.
    printed: Option<(Instant, u64)>,
}

impl FileReport {
    fn new(label: String) -> FileReport {
        FileReport {
            label,
            state: Mutex::new(ReportState::default()),
        }
    }

    fn moved(&self, state: &mut ReportState, done: u64, total: u64) {
        if total > 0 {
            state.total = total;
        }
        let due = state
            .printed
            .is_none_or(|(at, printed)| printed != done && at.elapsed() >= REPORT_EVERY);
        if due {
            state.printed = Some((Instant::now(), done));
            eprintln!(
                "{}: {} / {}",
                self.label,
                ardana_core::human_size(done),
                ardana_core::human_size(state.total)
            );
        }
    }
}

impl ProgressHandler for FileReport {
    fn on_progress(&self, event: &ProgressEvent) {
        let ProgressEvent::Download(event) = event else {
            return;
        };
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match event {
            DownloadEvent::Start { total_bytes, .. } => state.total = *total_bytes,
            DownloadEvent::Progress { files } => {
                for file in files.iter().filter(|f| f.status == FileStatus::InProgress) {
                    self.moved(&mut state, file.bytes_completed, file.total_bytes);
                }
            }
            DownloadEvent::AggregateProgress {
                bytes_completed,
                total_bytes,
                ..
            } => self.moved(&mut state, *bytes_completed, *total_bytes),
            DownloadEvent::Complete => {
                if state.printed.is_some() {
                    eprintln!(
                        "{}: {} done",
                        self.label,
                        ardana_core::human_size(state.total)
                    );
                }
            }
        }
    }
}

/// Every file under `dir`, as a `/`-separated path relative to `root`.
fn list_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), RegistryError> {
    let io = |err| RegistryError::Io {
        path: dir.to_path_buf(),
        err,
    };
    for entry in std::fs::read_dir(dir).map_err(io)? {
        let path = entry.map_err(io)?.path();
        if path.is_dir() {
            list_files(root, &path, out)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            let parts: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            out.push(parts.join("/"));
        }
    }
    Ok(())
}
