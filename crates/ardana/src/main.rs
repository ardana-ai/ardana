//! The `ardana` command line: wires the runtimes into the registry and the server.

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let cli = ardana::Cli::parse();
    match ardana::run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}
