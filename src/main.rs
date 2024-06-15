use std::{path::PathBuf, process::ExitCode};

use structopt::StructOpt;

#[derive(StructOpt, Debug)]
struct Options {
    /// Verbose mode (-v, -vv, -vvv, etc)
    #[structopt(short = "v", long = "verbose", parse(from_occurrences))]
    verbose: usize,

    /// Command to run.
    #[structopt(subcommand)]
    command: Command,
}

#[derive(StructOpt, Debug)]
enum Command {
    /// Run the provided file.
    Run { file: PathBuf },
}

#[tokio::main]
async fn main() -> eyre::Result<ExitCode> {
    let opts = Options::from_args();

    stderrlog::new()
        .module(module_path!())
        .verbosity(match opts.verbose {
            0 => log::Level::Warn,
            1 => log::Level::Info,
            2 => log::Level::Debug,
            3 | _ => log::Level::Trace,
        })
        .init()?;

    match &opts.command {
        Command::Run { file } => {
            let status = flock::execute_at_path(&file).await?;
            Ok(ExitCode::from(status as u8))
        }
    }
}
