//! The training environment in `$ARDANA_HOME/finetune/` (Q2, Q3, Q25, Q29), which the first `ardana finetune` builds
//! and later runs reuse: uv 0.13.1 from its GitHub release and llama.cpp's converter from the source archive of the
//! commit `llama-cpp-sys-2` vendors, each checked against a sha256 compiled in here, and one venv on uv's CPython 3.12
//! holding the pinned packages, its torch the build the machine runs ([`TorchBuild`]). uv's interpreters, cache and venv
//! stay in the same directory, and so does the working directory of each run.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use ardana_api::human_size;
use sha2::{Digest, Sha256};
use tokio::process::Command;

use super::Interrupt;

/// The uv release the environment is built with.
pub const UV_VERSION: &str = "0.13.1";
/// The sha256 of uv's release archive for each target it runs on, from the release's `.sha256` files.
const UV_DIGESTS: [(&str, &str); 3] = [
    (
        "x86_64-unknown-linux-gnu",
        "eadd9d27c134ab5ac1ffd4473b97a741407f3ea238288f03121fc4deddf90220",
    ),
    (
        "aarch64-unknown-linux-gnu",
        "9041c73c99fdc5892cf128140beb354eca9231d502068cc36e6cb53a8e59ad14",
    ),
    (
        "aarch64-apple-darwin",
        "cfe454573412ff7380d15fc7dae5997ab53ffce72fce0a78e91ddbc84001acd2",
    ),
];
/// The interpreter of the venv.
pub const PYTHON: &str = "3.12";
/// The torch release [`PACKAGES`] pins, which publishes no Intel macOS wheel.
pub const TORCH: &str = "2.14.1";
/// The venv's packages (Q29), each pinned exactly, torch ([`TORCH`]) from the index its [`TorchBuild`] names.
pub const PACKAGES: [&str; 11] = [
    "torch==2.14.1",
    "transformers==5.18.0",
    "peft==0.21.2",
    "trl==1.16.0",
    "accelerate==1.15.0",
    "datasets==5.1.0",
    "safetensors==0.8.0",
    "tokenizers==0.23.2",
    "sentencepiece==0.2.2",
    "protobuf==7.36.2",
    "numpy==2.5.3",
];
/// QLoRA's 4-bit quantization (Q16, Q29), which a CUDA venv alone holds.
pub const BITSANDBYTES: &str = "bitsandbytes==0.50.2";
/// The file in the venv naming the [`TorchBuild`] it was made for.
const TORCH_BUILD: &str = "ardana-torch-build";
/// The llama.cpp commit `llama-cpp-sys-2` 0.1.159 vendors (b11074), whose converter writes the GGUFs (Q25).
pub const LLAMA_CPP_COMMIT: &str = "26394b4e6749a41c3633db040e0987500a5f7013";
/// The sha256 of GitHub's source archive of that commit.
const LLAMA_CPP_DIGEST: &str = "1d0fcd22eb0fb31b1f75de2cc9e585a3338b8937043662a0c222a1e9261fed29";
/// What the converter needs of the archive: the script, the `conversion/` package it imports, the `gguf` package in
/// `gguf-py/`, which the script puts on its own path, and the license.
const CONVERTER_PARTS: [&str; 4] = [
    "convert_hf_to_gguf.py",
    "conversion/",
    "gguf-py/",
    "LICENSE",
];

/// The torch build the venv carries (R5.1): CUDA's on Linux with an NVIDIA driver, PyPI's on macOS arm64 (which runs
/// MPS), PyTorch's CPU wheels on Linux without it (PyPI's Linux wheel carries CUDA). Intel macOS has no torch 2.14.1
/// wheel and is refused before ([`super::runs_here`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TorchBuild {
    /// PyTorch's CUDA wheels from the newest of its CUDA indexes the driver runs (uv's `--torch-backend auto`, which
    /// reads the driver's version), with bitsandbytes for QLoRA.
    Cuda,
    /// PyPI's wheels.
    Pypi,
    /// PyTorch's CPU wheels.
    Cpu,
}

impl TorchBuild {
    /// The build of a machine: its OS and architecture as `std::env::consts` names them, and whether `nvidia-smi`
    /// answers on it.
    pub fn of(os: &str, arch: &str, nvidia: bool) -> TorchBuild {
        match (os, arch) {
            ("linux", _) if nvidia => TorchBuild::Cuda,
            ("macos", "aarch64") => TorchBuild::Pypi,
            _ => TorchBuild::Cpu,
        }
    }

    /// This machine's build.
    async fn here() -> TorchBuild {
        let nvidia = std::env::consts::OS == "linux"
            && Command::new("nvidia-smi")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .is_ok_and(|status| status.success());
        TorchBuild::of(std::env::consts::OS, std::env::consts::ARCH, nvidia)
    }

    /// `uv pip install --torch-backend`, none for PyPI's wheels.
    pub fn backend(self) -> Option<&'static str> {
        match self {
            TorchBuild::Cuda => Some("auto"),
            TorchBuild::Pypi => None,
            TorchBuild::Cpu => Some("cpu"),
        }
    }

    /// The pinned packages of its venv: [`PACKAGES`], and [`BITSANDBYTES`] for CUDA alone.
    pub fn packages(self) -> Vec<&'static str> {
        let mut packages = PACKAGES.to_vec();
        if self == TorchBuild::Cuda {
            packages.push(BITSANDBYTES);
        }
        packages
    }

    /// The name [`TORCH_BUILD`] holds.
    fn name(self) -> &'static str {
        match self {
            TorchBuild::Cuda => "cuda",
            TorchBuild::Pypi => "pypi",
            TorchBuild::Cpu => "cpu",
        }
    }
}

/// `$ARDANA_HOME/finetune/`, built.
#[derive(Debug, Clone)]
pub struct Env {
    root: PathBuf,
}

impl Env {
    /// The venv's interpreter.
    pub fn python(&self) -> PathBuf {
        self.root.join("venv/bin/python")
    }

    /// llama.cpp's `convert_hf_to_gguf.py`.
    pub fn converter(&self) -> PathBuf {
        self.root
            .join(format!("llama.cpp-{LLAMA_CPP_COMMIT}"))
            .join(CONVERTER_PARTS[0])
    }

    /// The working directory of this process's run that builds `name`, `work/<name>-<pid>`: two runs building the same
    /// name each have their own, and neither removes the other's.
    pub fn work(&self, name: &str) -> PathBuf {
        self.root
            .join("work")
            .join(format!("{name}-{}", std::process::id()))
    }
}

/// The environment in `<home>/finetune/`: what is missing of it downloaded and built, what is there reused (a uv
/// binary that says another version, and a venv on another interpreter or made for another torch build, as when a
/// driver came after it, are replaced), and the pinned packages installed into the venv, which uv leaves as they are
/// when the venv holds them.
pub async fn prepare(home: &Path, interrupt: &mut Interrupt) -> Result<Env> {
    let env = Env {
        root: home.join("finetune"),
    };
    let root = &env.root;
    std::fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
    let uv = root.join("bin/uv");
    if !uv_is_pinned(&uv).await {
        let target = uv_target()?;
        let digest = UV_DIGESTS
            .iter()
            .find(|(t, _)| *t == target)
            .map(|&(_, digest)| digest)
            .with_context(|| format!("no uv {UV_VERSION} digest for {target}"))?;
        let url = format!(
            "https://github.com/astral-sh/uv/releases/download/{UV_VERSION}/uv-{target}.tar.gz"
        );
        let archive = root.join(format!("uv-{target}.tar.gz"));
        interrupt
            .or(download(
                &url,
                digest,
                &archive,
                &format!("uv {UV_VERSION}"),
            ))
            .await?;
        let member = format!("uv-{target}/uv");
        unpack(&archive, root, |path| {
            (path == Path::new(&member)).then(|| PathBuf::from("bin/uv"))
        })?;
        std::fs::remove_file(&archive)?;
        ensure!(
            uv_is_pinned(&uv).await,
            "{} does not say it is uv {UV_VERSION}",
            uv.display()
        );
    }

    let venv = root.join("venv");
    let build = TorchBuild::here().await;
    let made_for = std::fs::read_to_string(venv.join(TORCH_BUILD)).unwrap_or_default();
    if !venv_is_pinned(root, &venv) || made_for != build.name() {
        eprintln!(
            "creating the training environment in {} (torch for {})",
            venv.display(),
            build.name()
        );
        let mut create = uv_command(root, &uv);
        create
            .args(["venv", "--quiet", "--clear", "--python", PYTHON])
            .arg(&venv);
        run(&mut create, interrupt, "uv venv").await?;
        std::fs::write(venv.join(TORCH_BUILD), build.name())?;
    }
    // Compiled at install, so a run reads every module's bytecode and writes none: a venv that holds the pins is
    // left as it is, which uv only checks.
    let mut install = uv_command(root, &uv);
    install
        .args(["pip", "install", "--compile-bytecode", "--python"])
        .arg(env.python());
    if let Some(backend) = build.backend() {
        install.args(["--torch-backend", backend]);
    }
    install.args(build.packages());
    run(&mut install, interrupt, "uv pip install").await?;

    let converter = env.converter();
    let dir = converter
        .parent()
        .expect("the converter sits in a directory");
    if !dir.is_dir() {
        let url =
            format!("https://github.com/ggml-org/llama.cpp/archive/{LLAMA_CPP_COMMIT}.tar.gz");
        let archive = root.join(format!("llama.cpp-{LLAMA_CPP_COMMIT}.tar.gz"));
        interrupt
            .or(download(
                &url,
                LLAMA_CPP_DIGEST,
                &archive,
                &format!("llama.cpp {}'s converter", &LLAMA_CPP_COMMIT[..7]),
            ))
            .await?;
        let staged = dir.with_extension("partial");
        if staged.exists() {
            std::fs::remove_dir_all(&staged)?;
        }
        let top = Path::new(
            dir.file_name()
                .expect("the converter's directory has a name"),
        );
        unpack(&archive, &staged, |path| {
            let rel = path.strip_prefix(top).ok()?;
            let text = rel.to_str()?;
            CONVERTER_PARTS
                .iter()
                .any(|part| {
                    text == part.trim_end_matches('/')
                        || (part.ends_with('/') && text.starts_with(part))
                })
                .then(|| rel.to_path_buf())
        })?;
        std::fs::remove_file(&archive)?;
        ensure!(
            staged.join(CONVERTER_PARTS[0]).is_file(),
            "{url} holds no {}",
            CONVERTER_PARTS[0]
        );
        std::fs::rename(&staged, dir)?;
    }
    Ok(env)
}

/// uv's target triple for this machine (Q21: Linux, and macOS on Apple Silicon).
fn uv_target() -> Result<&'static str> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        (os, arch) => bail!("uv {UV_VERSION} is not built for {arch} {os}"),
    })
}

/// Whether `uv` runs and says it is [`UV_VERSION`].
async fn uv_is_pinned(uv: &Path) -> bool {
    let Ok(output) = Command::new(uv).arg("--version").output().await else {
        return false;
    };
    String::from_utf8_lossy(&output.stdout).starts_with(&format!("uv {UV_VERSION} "))
}

/// Whether `venv` runs uv's [`PYTHON`] from `<root>/python`, never another interpreter.
fn venv_is_pinned(root: &Path, venv: &Path) -> bool {
    let Ok(cfg) = std::fs::read_to_string(venv.join("pyvenv.cfg")) else {
        return false;
    };
    let value = |key: &str| {
        cfg.lines()
            .filter_map(|line| line.split_once('='))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim().to_string())
    };
    let python = root.join("python");
    value("home").is_some_and(|home| Path::new(&home).starts_with(&python))
        && value("version_info")
            .is_some_and(|version| version == PYTHON || version.starts_with(&format!("{PYTHON}.")))
}

/// `uv` with its interpreters, cache and links in `root`, uv-managed interpreters only, no configuration file read and
/// torch's index the [`TorchBuild`]'s alone.
fn uv_command(root: &Path, uv: &Path) -> Command {
    let mut cmd = Command::new(uv);
    cmd.env("UV_CACHE_DIR", root.join("cache"))
        .env("UV_PYTHON_INSTALL_DIR", root.join("python"))
        .env("UV_PYTHON_BIN_DIR", root.join("bin"))
        .env("UV_MANAGED_PYTHON", "1")
        .env("UV_NO_CONFIG", "1")
        .env_remove("UV_TORCH_BACKEND")
        .env_remove("VIRTUAL_ENV");
    cmd
}

/// Runs `cmd` with its output on stderr, until it exits or `interrupt` stops it; fails unless it exits 0.
pub async fn run(cmd: &mut Command, interrupt: &mut Interrupt, what: &str) -> Result<()> {
    cmd.stdout(std::io::stderr()).kill_on_drop(true);
    let mut child = cmd.spawn().with_context(|| format!("starting {what}"))?;
    let status = tokio::select! {
        status = child.wait() => status.with_context(|| format!("waiting for {what}"))?,
        () = interrupt.recv() => {
            child.kill().await.with_context(|| format!("stopping {what}"))?;
            bail!(super::INTERRUPTED);
        }
    };
    ensure!(status.success(), "{what} failed ({status})");
    Ok(())
}

/// Downloads `url` to `to` through `<to>.part`, which it becomes only once its sha256 is `digest`.
async fn download(url: &str, digest: &str, to: &Path, what: &str) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent(ardana_registry::library::USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .build()
        .context("building the HTTP client")?;
    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .with_context(|| format!("downloading {url}"))?;
    let size = response
        .content_length()
        .map_or_else(String::new, |bytes| format!(" ({})", human_size(bytes)));
    eprintln!("downloading {what}{size} from {url}");
    let mut part = to.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let mut file =
        std::fs::File::create(&part).with_context(|| format!("creating {}", part.display()))?;
    let mut hasher = Sha256::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .with_context(|| format!("downloading {url}"))?
    {
        hasher.update(&chunk);
        file.write_all(&chunk)
            .with_context(|| format!("writing {}", part.display()))?;
    }
    let got: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if got != digest {
        std::fs::remove_file(&part)?;
        bail!("{url} has the sha256 {got}, not the {digest} this ardana was built to trust");
    }
    std::fs::rename(&part, to).with_context(|| format!("writing {}", to.display()))
}

/// Unpacks into `into` every file and directory of the `.tar.gz` `archive` that `keep` gives a path (relative to
/// `into`, made of plain components only) for. A kept entry of another kind (a symbolic or hard link, a device) is
/// refused: neither archive holds one, and a link could reach outside `into`.
fn unpack(archive: &Path, into: &Path, keep: impl Fn(&Path) -> Option<PathBuf>) -> Result<()> {
    let file =
        std::fs::File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    for entry in tar
        .entries()
        .with_context(|| format!("reading {}", archive.display()))?
    {
        let mut entry = entry.with_context(|| format!("reading {}", archive.display()))?;
        let path = entry.path()?.into_owned();
        let Some(rel) = keep(&path) else {
            continue;
        };
        ensure!(
            rel.components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
            "{} holds the path {}",
            archive.display(),
            path.display()
        );
        let kind = entry.header().entry_type();
        ensure!(
            kind.is_file() || kind.is_dir(),
            "{} holds {} as a {kind:?} entry; only files and directories are unpacked",
            archive.display(),
            path.display()
        );
        let dest = into.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        entry
            .unpack(&dest)
            .with_context(|| format!("unpacking {} into {}", path.display(), dest.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `unpack` writes the files and directories it keeps and refuses a kept link, which it never creates: a symbolic
    /// link to a path outside the directory, and a hard link.
    #[test]
    fn unpack_refuses_links() {
        let dir = std::env::temp_dir().join(format!("ardana-unpack-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let archive = |name: &str, link: Option<tar::EntryType>| {
            let path = dir.join(name);
            let gz = flate2::write::GzEncoder::new(
                std::fs::File::create(&path).unwrap(),
                flate2::Compression::fast(),
            );
            let mut tar = tar::Builder::new(gz);
            let mut header = tar::Header::new_gnu();
            header.set_size(3);
            header.set_mode(0o644);
            tar.append_data(&mut header, "top/kept.txt", &b"yes"[..])
                .unwrap();
            if let Some(kind) = link {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(kind);
                header.set_size(0);
                header.set_mode(0o644);
                tar.append_link(&mut header, "top/link", "/etc/passwd")
                    .unwrap();
            }
            tar.into_inner().unwrap().finish().unwrap();
            path
        };
        let keep = |path: &Path| path.strip_prefix("top").ok().map(Path::to_path_buf);

        let into = dir.join("plain");
        unpack(&archive("plain.tar.gz", None), &into, keep).unwrap();
        assert_eq!(std::fs::read(into.join("kept.txt")).unwrap(), b"yes");
        for kind in [tar::EntryType::Symlink, tar::EntryType::Link] {
            let into = dir.join(format!("{kind:?}"));
            let err = unpack(&archive("link.tar.gz", Some(kind)), &into, keep)
                .expect_err("a link unpacked")
                .to_string();
            assert!(
                err.ends_with(&format!(
                    "holds top/link as a {kind:?} entry; only files and directories are unpacked"
                )),
                "{err}"
            );
            assert!(into.join("kept.txt").is_file());
            assert!(std::fs::symlink_metadata(into.join("link")).is_err());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
