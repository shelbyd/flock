use std::{ffi::OsStr, time::Instant};

use colored::Colorize;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    eprintln!("");

    let start = Instant::now();

    let ok = "ok".green();
    let failed = "FAILED".red();

    let files = walkdir::WalkDir::new("tests")
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|f| f.file_type().is_file())
        .filter(|f| f.path().extension() == Some(OsStr::new("flasm")))
        .map(|f| f.path().to_owned())
        .collect::<Vec<_>>();

    let mut results = std::collections::BTreeMap::new();

    for file in files {
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
