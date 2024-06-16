#![allow(unused)]

mod rand;

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    path::PathBuf,
};

use eyre::Context;
use flock::{Eal, Program};

use self::rand::ConsistentRand;

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

pub fn random_eal(seed: u64) -> RandomVm {
    let rand = ConsistentRand::new(seed);
    let host_processes = (rand.get("host_processes").poisson(3.0) as usize).max(1);

    RandomVm {}
}

pub struct RandomVm {}

#[async_trait::async_trait]
impl Eal for RandomVm {}
