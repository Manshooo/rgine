use std::{env, path::Path, process::ExitCode};

mod merge;

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    if args.next().is_some_and(|command| command == "merge") {
        let args: Vec<_> = args.collect();
        let [base, current, other, path] = args.as_slice() else {
            eprintln!("usage: engine-cli merge <base> <current> <other> <scene-path>");
            return ExitCode::from(2);
        };
        if let Err(error) = merge::run(Path::new(base), Path::new(current), Path::new(other)) {
            eprintln!("{}: {error}", Path::new(path).display());
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }
    println!("rgine engine-cli 0.1.0 - bootstrap");
    println!("Commands: merge");
    println!("Commands planned: new, run, build, package, doctor");
    ExitCode::SUCCESS
}
