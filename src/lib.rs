use serde::{Deserialize, Serialize};

pub use bencher::Bencher;
pub use bencher_macro::*;

mod bencher;
mod timing_future;
mod track_allocator;

#[derive(Debug, Serialize, Deserialize)]
pub struct Stats {
    pub times_average: usize,
    pub times_min: usize,
    pub times_max: usize,

    pub mem_average: usize,
    pub mem_min: usize,
    pub mem_max: usize,

    pub allocations: usize,
    pub leaked_bytes: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Step {
    time: u128,
    mem: usize,
    allocations: usize,
    leaked_bytes: usize,
}

impl From<&Vec<Step>> for Stats {
    fn from(steps: &Vec<Step>) -> Self {
        let count = steps.len();

        let times = steps.iter().map(|step| step.time).collect::<Vec<u128>>();
        let times_iter = times.iter();

        let mem = steps.iter().map(|step| step.mem).collect::<Vec<usize>>();
        let mem_iter = mem.iter();

        let a = steps
            .iter()
            .map(|step| step.allocations)
            .collect::<Vec<usize>>();
        let allocations_iter = a.iter();

        let l_bytes = steps
            .iter()
            .map(|step| step.leaked_bytes)
            .collect::<Vec<usize>>();
        let leaked_bytes_iter = l_bytes.iter();

        Stats {
            times_average: (times_iter.clone().sum::<u128>() / count as u128) as usize,
            times_min: times_iter.clone().copied().min().unwrap_or_default() as usize,
            times_max: times_iter.clone().copied().max().unwrap_or_default() as usize,
            mem_average: mem_iter.clone().sum::<usize>() / count,
            mem_min: mem_iter.clone().copied().min().unwrap_or_default(),
            mem_max: mem_iter.clone().copied().max().unwrap_or_default(),
            allocations: allocations_iter.clone().copied().max().unwrap_or_default(),
            leaked_bytes: leaked_bytes_iter.clone().copied().max().unwrap_or_default(),
        }
    }
}
