#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ConsistentRand(u64);

impl ConsistentRand {
    pub(crate) fn new(seed: u64) -> ConsistentRand {
        ConsistentRand(seed)
    }

    pub(crate) fn get(&self, name: &str) -> ConsistentRand {
        let mut to_hash = self.0.to_be_bytes().to_vec();
        to_hash.extend(name.as_bytes());
        ConsistentRand::new(seahash::hash(&to_hash))
    }

    pub(crate) fn poisson(self, median: f64) -> f64 {
        let x = self.0 as f64 / u64::MAX as f64;
        poisson(x, median)
    }
}

fn poisson(x: f64, median: f64) -> f64 {
    // The median of a Poisson distribution is approximately lambda - 1/3
    let lambda = (median + 1.0 / 3.0);

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
