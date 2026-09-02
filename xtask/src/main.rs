//! Repository automation.
//!
//! Run through the alias in `.cargo/config.toml`: `cargo xtask <task>`.

mod graph;
mod metadata;
mod rules;

use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
xtask - repository automation

Usage:
    cargo xtask <task>

Tasks:
    check-deps    Assert the crate boundaries recorded in docs/ARCHITECTURE.md
    help          Show this message
";

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("check-deps") => check_deps(),
        Some("help" | "--help" | "-h") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        None => {
            eprint!("{USAGE}");
            ExitCode::FAILURE
        }
        Some(task) => {
            eprintln!("unknown task `{task}`\n");
            eprint!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn check_deps() -> ExitCode {
    // The workspace is located from this crate rather than the working
    // directory, so the check reports on the same workspace wherever it is run.
    let graph = match metadata::load(Path::new(env!("CARGO_MANIFEST_DIR"))) {
        Ok(graph) => graph,
        Err(error) => {
            eprintln!("xtask check-deps: {error}");
            return ExitCode::FAILURE;
        }
    };

    let violations = rules::check(&graph);
    if violations.is_empty() {
        println!(
            "check-deps: boundaries hold across {} workspace crates",
            graph.members().len()
        );
        return ExitCode::SUCCESS;
    }

    eprintln!(
        "check-deps: {} boundary violation(s), see docs/ARCHITECTURE.md\n",
        violations.len()
    );
    for violation in &violations {
        eprintln!("  {}", violation.rule);
        eprintln!("    {}", violation.detail);
        eprintln!("    fix: {}\n", violation.remedy);
    }

    ExitCode::FAILURE
}
