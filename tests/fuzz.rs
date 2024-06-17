use std::{
    io::Write,
    time::{Duration, Instant},
};

use colored::Colorize;
use eyre::Context;

use crate::common::execute_program_with_seed;

mod common;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    eprintln!("");

    let fuzz = std::env::var("FUZZ");
    let mut fuzz_for = match fuzz.as_ref().map(|s| s.as_str()) {
        Ok("forever") => FuzzFor::Forever,
        Ok("never") => FuzzFor::Never,
        Ok(t) => {
            if let Some(seconds) = t.strip_suffix("s") {
                FuzzFor::Duration(Duration::from_secs(seconds.parse()?))
            } else {
                FuzzFor::Count(t.parse()?)
            }
        }
        Err(_) => FuzzFor::Never,
    };

    if let FuzzFor::Never = &fuzz_for {
        return Ok(());
    }

    let programs = common::programs()?;
    let mut programs = programs.iter().cycle();

    let start = Instant::now();
    let mut passed = 0;

    while fuzz_for.should_run(start) {
        let seed: u64 = rand::random();

        let (path, program) = programs.next().unwrap();
        let failed = match execute_program_with_seed(program.clone(), seed).await {
            Ok(0) => {
                passed += 1;
                continue;
            }
            r => r,
        };

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("tests/found_with_fuzzing.txt")?;
        writeln!(file, "{} {seed}", path.display())?;

        let err = match failed {
            Err(e) => e,
            Ok(c) => eyre::eyre!(format!("Program exited with code: {c}")),
        };
        return Err(err).context(format!("{} {seed}", path.display()));
    }

    eprintln!(
        "test result: {}, {passed} passed; 0 failed; finished in {:?}",
        "ok".green(),
        start.elapsed()
    );
    eprintln!("");

    Ok(())
}

enum FuzzFor {
    Forever,
    Duration(Duration),
    Count(usize),
    Never,
}

impl FuzzFor {
    fn should_run(&mut self, start: Instant) -> bool {
        match self {
            FuzzFor::Forever => true,
            FuzzFor::Duration(d) => start.elapsed() < *d,
            FuzzFor::Count(n) => {
                if *n == 0 {
                    return false;
                }
                *n -= 1;
                true
            }
            FuzzFor::Never => false,
        }
    }
}
