use std::{env, path::Path, process::ExitCode};

mod merge;
mod migrate;

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let command = args.next();
    if command.as_deref().is_some_and(|command| command == "merge") {
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
    if command
        .as_deref()
        .is_some_and(|command| command == "migrate")
    {
        let args: Vec<_> = args.collect();
        let [project, output] = args.as_slice() else {
            eprintln!("usage: engine-cli migrate <project-directory> <output-directory>");
            return ExitCode::from(2);
        };
        return match migrate::run(Path::new(project), Path::new(output)) {
            Ok(0) => {
                println!("Project is up to date.");
                ExitCode::SUCCESS
            }
            Ok(count) => {
                println!(
                    "Migrated {count} scene or prefab files to {}.",
                    Path::new(output).display()
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
    }
    println!("rgine engine-cli 0.1.0 - bootstrap");
    println!("Commands: merge, migrate");
    println!("Commands planned: new, run, build, package, doctor");
    ExitCode::SUCCESS
}
