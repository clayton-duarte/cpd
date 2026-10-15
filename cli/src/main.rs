use std::process::ExitCode;

fn main() -> ExitCode {
    cpd::main_with(std::env::args_os(), &mut std::io::stdout().lock())
}
