//! R5.8 on the binary: `ardana serve` binds 127.0.0.1 and takes its key from `ARDANA_API_KEY`.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use common::Ardana;

/// `GET path` on 127.0.0.1:`port` with `headers`; returns the status and body.
fn get(port: u16, path: &str, headers: &str) -> Result<(u16, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n{headers}\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let status = response
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .with_context(|| format!("no status in {response:?}"))?;
    let body = response.split("\r\n\r\n").nth(1).unwrap_or_default();
    Ok((status, body.to_string()))
}

/// Kills the server when the test ends, however it ends.
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn serve_binds_localhost_and_reads_the_key_from_the_environment() -> Result<()> {
    let ardana = Ardana::new("serve-api-key-env")?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let mut server = Server(
        ardana
            .command(["serve", "--port", &port.to_string()])
            .env("ARDANA_API_KEY", "serve-test-key")
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let stderr = server.0.stderr.take().context("stderr")?;
    let mut banner = String::new();
    BufReader::new(stderr).read_line(&mut banner)?;
    assert!(
        banner.contains(&format!("listening on http://127.0.0.1:{port} "))
            && banner.contains("API key required"),
        "{banner}"
    );

    let started = Instant::now();
    while get(port, "/health", "").is_err() {
        if started.elapsed() > Duration::from_secs(10) {
            bail!("ardana serve did not answer on port {port}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(get(port, "/health", "")?.0, 200);
    let (status, body) = get(port, "/v1/models", "")?;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("authentication_error"), "{body}");
    assert_eq!(
        get(port, "/v1/models", "Authorization: Bearer wrong\r\n")?.0,
        401
    );
    assert_eq!(
        get(
            port,
            "/v1/models",
            "Authorization: Bearer serve-test-key\r\n"
        )?,
        (200, r#"{"models":[]}"#.to_string())
    );
    drop(server);
    Ok(())
}
