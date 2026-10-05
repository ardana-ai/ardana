//! A running `ardana serve` for the end-to-end suites: the release binary serving decider:2b, pulled offline from
//! `tmp/hf` into its own `ARDANA_HOME` under `tmp/e2e/<suite>` (or with that home empty, so the first request pulls
//! it, over an empty Hub cache of its own when asked), on a free port of 127.0.0.1. Dropping it stops the server.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::sandbox::{Sandbox, cargo};

/// The model every suite serves: the library name `ardana pull` resolves to
/// `hf.co/Mapika/decider-2b-GGUF:decider-2b-v11-Q4_K_M.gguf`.
pub const MODEL: &str = "decider:2b";

/// How long the server may take to answer `/health`.
const START_TIMEOUT: Duration = Duration::from_secs(60);

/// Builds the release `ardana` (llama.cpp at full speed) and returns its path.
pub fn build_ardana(sandbox: &Sandbox) -> Result<PathBuf> {
    let status = sandbox
        .command(cargo())
        .current_dir(sandbox.repo_root())
        .args(["build", "--release", "--locked", "--package", "ardana"])
        .status()
        .context("running cargo build --release")?;
    if !status.success() {
        bail!("cargo build --release --package ardana failed ({status})");
    }
    Ok(target_dir(sandbox).join("release/ardana"))
}

/// The workspace's cargo target directory.
pub fn target_dir(sandbox: &Sandbox) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| sandbox.repo_root().join("target"))
}

pub struct Server {
    child: Child,
    /// `http://127.0.0.1:<port>`.
    pub url: String,
    pub log: PathBuf,
    /// Its `ARDANA_HOME`.
    pub home: PathBuf,
}

/// How [`Server::launch`] starts `ardana serve`.
enum Mode<'a> {
    /// decider:2b pulled first, and the API key when given.
    Pulled(Option<&'a str>),
    /// On an empty registry: its first request pulls decider:2b. Its Hub cache is `<dir>/hf` when `own_cache` (empty:
    /// every pull fails, offline), else `tmp/hf`.
    Empty { own_cache: bool },
}

impl Server {
    /// Pulls decider:2b into `tmp/e2e/<name>/home` and starts `ardana serve` there (from the binary's own
    /// directory), with `ARDANA_API_KEY` set to `api_key` when given; returns once `/health` answers.
    pub fn start(
        sandbox: &Sandbox,
        ardana: &Path,
        name: &str,
        api_key: Option<&str>,
    ) -> Result<Server> {
        Server::launch(sandbox, ardana, name, Mode::Pulled(api_key))
    }

    /// Starts `ardana serve` on the empty home `tmp/e2e/<name>/home`: its first request pulls decider:2b, offline from
    /// `tmp/hf`.
    pub fn start_empty(sandbox: &Sandbox, ardana: &Path, name: &str) -> Result<Server> {
        Server::launch(sandbox, ardana, name, Mode::Empty { own_cache: false })
    }

    /// Starts `ardana serve` on the empty home `tmp/e2e/<name>/home` over the empty Hub cache `tmp/e2e/<name>/hf`: it
    /// holds no model and no browser variant, and every pull fails offline.
    pub fn start_uncached(sandbox: &Sandbox, ardana: &Path, name: &str) -> Result<Server> {
        Server::launch(sandbox, ardana, name, Mode::Empty { own_cache: true })
    }

    fn launch(sandbox: &Sandbox, ardana: &Path, name: &str, mode: Mode) -> Result<Server> {
        let dir = sandbox.tmp().join("e2e").join(name);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).with_context(|| format!("emptying {}", dir.display()))?;
        }
        let home = dir.join("home");
        std::fs::create_dir_all(&home)?;
        let ardana_command = || {
            let mut cmd = sandbox.command(ardana);
            if let Some(dir) = ardana.parent() {
                cmd.current_dir(dir);
            }
            cmd.env("ARDANA_HOME", &home)
                .env("HF_HUB_OFFLINE", "1")
                .env_remove("ARDANA_API_KEY");
            cmd
        };
        if let Mode::Pulled(_) = mode {
            let pulled = ardana_command()
                .args(["pull", MODEL])
                .status()
                .context("running ardana pull")?;
            if !pulled.success() {
                bail!("ardana pull {MODEL} failed ({pulled}); run `cargo xtask fetch`");
            }
        }

        let port = std::net::TcpListener::bind("127.0.0.1:0")?
            .local_addr()?
            .port();
        let log = dir.join("serve.log");
        let out = File::create(&log)?;
        let mut serve = ardana_command();
        serve
            .args(["serve", "--port", &port.to_string()])
            .stdout(out.try_clone()?)
            .stderr(out);
        let own_cache = match mode {
            Mode::Pulled(key) => {
                if let Some(key) = key {
                    serve.env("ARDANA_API_KEY", key);
                }
                false
            }
            Mode::Empty { own_cache } => own_cache,
        };
        if own_cache {
            let cache = dir.join("hf");
            std::fs::create_dir_all(&cache)?;
            serve.env("HF_HOME", cache);
        }
        let child = serve.spawn().context("starting ardana serve")?;
        let mut server = Server {
            child,
            url: format!("http://127.0.0.1:{port}"),
            log,
            home,
        };
        server.wait_ready(sandbox)?;
        println!(
            "e2e: ardana serve on {} (log {})",
            server.url,
            server.log.display()
        );
        Ok(server)
    }

    fn wait_ready(&mut self, sandbox: &Sandbox) -> Result<()> {
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait()? {
                bail!("ardana serve exited ({status}):\n{}", self.log_text());
            }
            let health = sandbox
                .command("curl")
                .args(["-fsS", "--max-time", "2"])
                .arg(format!("{}/health", self.url))
                .output()
                .context("running curl")?;
            if health.status.success() {
                return Ok(());
            }
            if started.elapsed() > START_TIMEOUT {
                bail!(
                    "ardana serve did not answer /health within {START_TIMEOUT:?}:\n{}",
                    self.log_text()
                );
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    pub fn log_text(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// Stops the server with SIGTERM (graceful shutdown), killing it when it does not exit within 10 s.
    pub fn stop(mut self) -> Result<()> {
        self.terminate()
    }

    fn terminate(&mut self) -> Result<()> {
        if self.child.try_wait()?.is_some() {
            return Ok(());
        }
        Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status()
            .context("running kill")?;
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(10) {
            if let Some(status) = self.child.try_wait()? {
                if !status.success() {
                    bail!("ardana serve exited with {status}:\n{}", self.log_text());
                }
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        self.child.kill()?;
        self.child.wait()?;
        bail!("ardana serve did not stop within 10 s of SIGTERM")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
