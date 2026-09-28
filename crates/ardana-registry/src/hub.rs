//! Hugging Face repositories through `hf-hub`, in the standard hub cache (`$HF_HOME/hub`).
//!
//! `hf-hub` 1.0.0 does not read `HF_HUB_OFFLINE`, so [`offline`] reads it and every download passes
//! `local_files_only`. Offline, a repository's files are those of its cached `main` snapshot and no metadata endpoint
//! is called.

use std::path::{Path, PathBuf};

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

/// One Hub client for a pull.
pub(crate) struct Hub {
    client: HFClient,
    offline: bool,
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
}

impl Hub {
    pub fn from_env() -> Result<Hub, RegistryError> {
        let client = HFClient::new().map_err(|err| RegistryError::Hub {
            repo: "the Hugging Face client".into(),
            err,
        })?;
        Ok(Hub {
            client,
            offline: offline(),
        })
    }

    /// The `main` commit of `org/repo` and its files: from the Hub, or offline from the cached snapshot.
    pub async fn snapshot(&self, org: &str, repo: &str) -> Result<Snapshot, RegistryError> {
        let id = format!("{}{org}/{repo}", crate::refs::HF_PREFIX);
        let hub_err = |err| RegistryError::Hub {
            repo: id.clone(),
            err,
        };
        let model = self.client.model(org, repo);
        let (commit, mut files) = if self.offline {
            let dir = match model
                .snapshot_download()
                .local_files_only(true)
                .send()
                .await
            {
                Ok(dir) => dir,
                Err(HFError::LocalEntryNotFound { .. }) => {
                    return Err(RegistryError::NotCached {
                        what: id,
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
            let info = model.info().send().await.map_err(hub_err)?;
            let commit = info.sha.ok_or_else(|| RegistryError::Invalid {
                what: id.clone(),
                msg: "the Hub reported no commit for main".into(),
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
