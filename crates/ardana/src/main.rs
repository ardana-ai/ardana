//! The `ardana` command line: wires the runtimes into the registry and the server.

use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches};

fn main() -> ExitCode {
    // The matches stay around: `ardana run` asks its `--noul`, `--choice` and `--score` questions in the order given.
    let matches = ardana::Cli::command().get_matches();
    let cli = ardana::Cli::from_arg_matches(&matches).unwrap_or_else(|err| err.exit());
    match ardana::run(cli, &matches) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}
