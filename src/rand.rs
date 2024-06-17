use crate::Word;

#[derive(Debug, PartialEq, Eq)]
pub struct Rand(u64);

impl Rand {
    pub fn new(seed: u64) -> Rand {
        Rand(seed)
    }

    pub fn get(&self, name: impl AsRef<str>) -> Rand {
        let mut to_hash = self.0.to_be_bytes().to_vec();
        to_hash.extend(name.as_ref().as_bytes());
        Rand::new(seahash::hash(&to_hash))
    }

    pub fn poisson(self, median: f64) -> f64 {
        let x = self.0 as f64 / u64::MAX as f64;
        poisson(x, median)
    }

    pub fn word(self) -> Word {
        self.0 as Word
    }

    pub fn select<'t, T>(&self, nodes: &'t [T]) -> Option<&'t T> {
        if nodes.len() == 0 {
            return None;
        }

        assert!((u64::MAX - self.0) as usize > nodes.len());

        Some(&nodes[(self.0 as usize) % nodes.len()])
    }
}

fn poisson(x: f64, median: f64) -> f64 {
    // The median of a Poisson distribution is approximately lambda - 1/3
    let lambda = median + 1.0 / 3.0;

    let mut k = 0.0;
    let mut p = (-lambda).exp();
    let mut sum = p;

    // Use inverse transform sampling to find the Poisson random variable
    while sum < x {
        k += 1.0;
        p *= lambda / k;
        sum += p;
    }

    k
}
