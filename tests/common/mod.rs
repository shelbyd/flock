#![allow(unused)]

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    path::PathBuf,
};

use eyre::Context;
use flock::{rand::Rand, spawn_host, Eal, Program, Word};

pub fn files() -> eyre::Result<BTreeSet<PathBuf>> {
    Ok(walkdir::WalkDir::new("tests")
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|f| f.file_type().is_file())
        .filter(|f| f.path().extension() == Some(OsStr::new("flasm")))
        .map(|f| f.path().to_owned())
        .collect::<BTreeSet<_>>())
}

pub fn programs() -> eyre::Result<BTreeMap<PathBuf, Program>> {
    files()?
        .into_iter()
        .map(|path| {
            let contents =
                std::fs::read_to_string(&path).context(format!("Reading {}", path.display()))?;
            Ok((path, Program::parse(&contents)?))
        })
        .collect()
}

pub struct RandomVm {
    rand: Rand,
}

#[async_trait::async_trait]
impl Eal for RandomVm {
    fn rand(&self) -> Rand {
        self.rand.get("for_eal")
    }
}

pub async fn execute_program_with_seed(program: Program, seed: u64) -> eyre::Result<Word> {
    let rand = Rand::new(seed);
    let node_count = (rand.get("host_processes").poisson(3.0) as usize).max(1);

    let nodes = futures::future::try_join_all((0..node_count).map(|i| {
        spawn_host(RandomVm {
            rand: rand.get(i.to_string()),
        })
    }))
    .await?;

    let node = rand.get("root_node").select(&nodes).unwrap();
    Ok(node.execute(program).await?)
}
