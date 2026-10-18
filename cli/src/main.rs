use std::process::ExitCode;

fn main() -> ExitCode {
    // Before anything can replace the file: drivers are started from this path.
    cpd_core::host::exe::record_start();
    cpd::main_with(std::env::args_os(), &mut std::io::stdout().lock())
}
