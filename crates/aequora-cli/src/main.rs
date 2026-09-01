//! Thin process boundary for the Aequora CLI.

use std::process::ExitCode;

fn main() -> ExitCode {
    aequora_cli::run_main()
}
