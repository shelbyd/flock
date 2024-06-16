use std::time::Instant;

use colored::Colorize;

mod common;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    eprintln!("");

    let start = Instant::now();

    let ok = "ok".green();
    let failed = "FAILED".red();

    let mut results = std::collections::BTreeMap::new();

    for file in common::files()? {
        eprint!("test {} ... ", file.display());

        let result = flock::execute_at_path(&file).await;

        let result = match result {
            Ok(0) => {
                eprintln!("{ok}");
                Ok(())
            }
            Ok(e) => {
                eprintln!("{failed}");
                Err(eyre::eyre!("Program exited with code: {e}"))
            }
            Err(e) => {
                eprintln!("{failed}");
                Err(e)
            }
        };
        results.insert(file, result);
    }

    eprintln!("");

    for (file, result) in &results {
        let Err(e) = result else {
            continue;
        };

        eprintln!("test {} {failed}", file.display());
        eprintln!("");
        eprintln!("{e:?}");
        eprintln!("");
    }

    let success = results.values().filter(|r| r.is_ok()).count();
    let failed_count = results.values().filter(|r| r.is_err()).count();

    let result = if failed_count > 0 { failed } else { ok };
    eprintln!(
        "test result: {result}. {success} passed; {failed_count} failed; finished in {:?}",
        start.elapsed()
    );

    eprintln!("");

    Ok(())
}
