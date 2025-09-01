use crate::{timing_future::TimingFuture, Stats, Step};
use human_bytes::human_bytes;
use lazy_static::lazy_static;
use stats_alloc::{Region, StatsAlloc};
use std::future::Future;
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

lazy_static! {
    static ref FIRST: AtomicBool = AtomicBool::new(true);
}

const MAX_NAME_LEN: usize = 30;
const DEFAULT_DEADLINE_MS: u64 = 300;
const MAX_ITERATIONS: usize = 2000;

pub struct Bencher<A: std::alloc::GlobalAlloc + 'static> {
    pub name: String,
    pub count: usize,
    pub steps: Vec<Step>,
    pub bytes: usize,
    pub n: usize,
    pub poll: usize,
    /// Number of performed iterations.
    pub passed: usize,
    /// Whenever to display throughput stats. `bytes` value should be assigned
    /// from the bench.
    pub display_bytes: bool,
    pub format_fn: fn(&Stats, &Bencher<A>),
    allocator: &'static StatsAlloc<A>,

    // time, mem, allocations, leaked
    pub mem_track: (AtomicUsize, AtomicUsize, AtomicUsize, AtomicUsize),
}

impl<A: std::alloc::GlobalAlloc> Bencher<A> {
    pub fn new(
        name: impl AsRef<str>,
        count: usize,
        bytes: usize,
        display_bytes: bool,
        allocator: &'static StatsAlloc<A>,
    ) -> Self {
        Bencher {
            name: name.as_ref().to_owned(),
            count,
            steps: Vec::with_capacity(count),
            bytes,
            n: 0,
            poll: 0,
            passed: 0,
            display_bytes,
            format_fn: |s, b| Self::default_format(s, b),
            allocator,

            mem_track: (
                AtomicUsize::new(0),
                AtomicUsize::new(0),
                AtomicUsize::new(0),
                AtomicUsize::new(0),
            ),
        }
    }

    // (time, memory_usage, passed)
    #[inline]
    pub fn bench_once<T>(
        &self,
        f: &mut impl FnMut() -> T,
        n: usize,
        deadline: u128,
        started: &Instant,
    ) -> (u128, usize, usize, usize, usize) {
        self.reset_mem();

        let mut allocations = 0;
        let mut allocated_bytes = 0;
        let mut leaked_bytes = 0;
        let now = started.elapsed().as_nanos();
        let mut passed = 0;
        let mut reg = Region::new(self.allocator);

        for _ in 0..n {
            // Iterations are checked by the total count. Slow iterations
            // (n < 10) are checked by timeout.
            if self.passed + passed >= MAX_ITERATIONS
                || (n < 10 && self.passed + passed > 3 && started.elapsed().as_nanos() >= deadline)
            {
                break;
            }

            reg.reset();
            let _output = black_box(f());
            let changes = reg.change();
            allocations = allocations.max(changes.allocations);
            allocated_bytes = allocated_bytes.max(changes.bytes_allocated);
            leaked_bytes = leaked_bytes.max(
                changes
                    .bytes_allocated
                    .saturating_sub(changes.bytes_deallocated),
            );
            passed += 1;
        }

        (
            started.elapsed().as_nanos() - now,
            allocated_bytes,
            allocations,
            leaked_bytes,
            passed,
        )
    }

    pub fn iter<T>(&mut self, mut f: impl FnMut() -> T) {
        let now = Instant::now();
        let deadline = Duration::from_millis(DEFAULT_DEADLINE_MS).as_nanos();

        let single = self.bench_once(&mut f, 1, deadline, &now).0;
        // 1_000_000ns : 1ms
        self.n = (1_000_000 / single.max(1)).max(1) as usize;

        for _ in 0..self.count {
            if self.passed >= MAX_ITERATIONS
                || self.passed > 3 && now.elapsed().as_nanos() >= deadline
            {
                break;
            }
            let res = self.bench_once(&mut f, self.n, deadline, &now);

            if res.4 != 0 {
                self.steps.push(Step {
                    time: res.0 / res.4 as u128,
                    mem: res.1,
                    allocations: res.2,
                    leaked_bytes: res.3,
                });
                self.passed += res.4;
            }
        }
    }

    pub fn async_iter<'a, T, Fut: Future<Output = T>>(
        &'a mut self,
        mut f: impl FnMut() -> Fut + 'a,
    ) -> impl Future + 'a {
        async move {
            let now = Instant::now();
            let deadline = Duration::from_millis(DEFAULT_DEADLINE_MS).as_nanos();

            let single = TimingFuture::new(f()).await.elapsed_time.as_nanos();
            // 1_000_000ns : 1ms
            self.n = (1_000_000 / single.max(1)).max(1) as usize;

            let mut polls = Vec::with_capacity(self.count);

            for _ in 0..self.count {
                if self.passed >= MAX_ITERATIONS
                    || self.passed > 3 && now.elapsed().as_nanos() >= deadline
                {
                    break;
                }
                let mut mtime = 0u128;
                self.reset_mem();
                let mut passed = 0;
                let mut allocations = 0;
                let mut allocated_bytes = 0;
                let mut leaked_bytes = 0;

                for _ in 0..self.n {
                    if self.passed + passed >= MAX_ITERATIONS
                        || self.passed > 3 && now.elapsed().as_nanos() >= deadline
                    {
                        break;
                    }
                    let reg = Region::new(self.allocator);
                    let (tf_time, tf_poll) = {
                        let tf = TimingFuture::new(f()).await;
                        (tf.elapsed_time.as_nanos(), tf.poll)
                    };
                    let changes = reg.change();
                    mtime += tf_time;
                    passed += 1;
                    polls.push(tf_poll);
                    self.passed += 1;

                    allocations = allocations.max(changes.allocations);
                    allocated_bytes = allocated_bytes.max(changes.bytes_allocated);
                    leaked_bytes = leaked_bytes.max(
                        changes
                            .bytes_allocated
                            .saturating_sub(changes.bytes_deallocated),
                    );
                }

                if passed != 0 {
                    self.steps.push(Step {
                        time: mtime / passed as u128,
                        mem: allocated_bytes,
                        allocations,
                        leaked_bytes,
                    });
                }
            }

            if !polls.is_empty() {
                self.poll = polls.iter().sum::<usize>() / polls.len();
            }
        }
    }

    pub fn finish(&self) {
        let stats = Stats::from(self.steps.as_slice());
        (self.format_fn)(&stats, self)
    }

    pub fn reset_mem(&self) {
        self.mem_track.0.store(0, Ordering::SeqCst);
        self.mem_track.1.store(0, Ordering::SeqCst);
        self.mem_track.2.store(0, Ordering::SeqCst);
        self.mem_track.3.store(0, Ordering::SeqCst);
    }

    pub fn get_mem(&self) -> (usize, usize, usize) {
        (
            self.mem_track.1.load(Ordering::SeqCst),
            self.mem_track.2.load(Ordering::SeqCst),
            self.mem_track.3.load(Ordering::SeqCst),
        )
    }

    fn default_format(stats: &Stats, bencher: &Bencher<A>) {
        let first = if FIRST.swap(false, Ordering::SeqCst) {
            "."
        } else {
            ""
        };

        bunt::print!(
            "{[green]}{[white+bold]:>30} ... {[green]:>9} {[white+dimmed]:12} {$cyan}{:>5} op/s{/$}",
            first,
            format_name(&bencher.name),
            format_duration(
                stats.times_average,
                stats.times_average,
                false
            ),
            &["(+/-", &format_duration(
                    stats.times_max.saturating_sub(stats.times_average).max(stats.times_average.saturating_sub(stats.times_min)),
                    stats.times_average,
                    true
                ), "),"].concat(),
            format_ops(1_000_000_000 / stats.times_average.max(1), true)
        );
        if bencher.display_bytes {
            if bencher.bytes != 0 {
                let bytes_str = human_bytes(
                    bencher.bytes as f64 * (1_000_000_000f64 / stats.times_average.max(1) as f64),
                );
                bunt::print!(", {$cyan}{:>8}/s{/$}", bytes_str);
            } else {
                bunt::print!(", {$cyan+dimmed}     0 B/s{/$}");
            }
        }

        bunt::print!(", 🐏 ");
        let memory_str = human_bytes(stats.mem_max as f64);
        if stats.mem_max == 0 {
            bunt::print!("{$white+dimmed}{:>8}{/$}", memory_str);
        } else if stats.mem_max < 1000 {
            bunt::print!("{$yellow}{:>8}{/$}", memory_str);
        } else {
            bunt::print!("{$red}{:>8}{/$}", memory_str);
        }

        let leaked_str = human_bytes(stats.leaked_bytes as f64);
        if stats.leaked_bytes == 0 {
            // bunt::print!(", ✅ {}", leaked_str);
        } else if stats.leaked_bytes < 1000 {
            bunt::print!(", ⚠️ +{$yellow}{}{/$}", leaked_str);
        } else {
            bunt::print!(", ⚠️ +{$red}{}{/$}", leaked_str);
        }

        bunt::print!(", alloc ");
        let allocations_str = stats.allocations.to_string();
        if stats.allocations == 0 {
            bunt::print!("{$white+dimmed}{:7}{/$}", [&allocations_str, ","].concat());
        } else if stats.allocations < 10 {
            bunt::print!("{$yellow}{:7}{/$}", [&allocations_str, ","].concat());
        } else {
            bunt::print!("{$red}{:7}{/$}", [&allocations_str, ","].concat());
        }

        if bencher.poll > 0 {
            bunt::println!(
                " ▶ {[magenta]}, 🗳 {[magenta+bold]}",
                format_ops(bencher.passed, true),
                format_ops(bencher.poll, true)
            );
        } else {
            bunt::println!(" ▶ {[magenta]}", format_ops(bencher.passed, true));
        }
    }
}

fn format_name(s: &str) -> String {
    let mut s = s.strip_prefix("bench_").unwrap_or(s);
    s = s.strip_prefix("test_").unwrap_or(s);
    if s.len() > MAX_NAME_LEN {
        [
            &s[..MAX_NAME_LEN / 2 - 1],
            "...",
            &s[s.len() - MAX_NAME_LEN / 2 + 2..],
        ]
        .concat()
    } else {
        s.to_string()
    }
}

fn format_ops(value: usize, with_unit: bool) -> String {
    if value < 1_000 {
        let unit = "";
        format!("{value}{unit}")
    } else if value < 1_000_000 {
        let unit = if with_unit { " K" } else { "" };
        format!("{:.0}{}", value as f64 / 1_000_f64, unit)
    } else if value < 1_000_000_000 {
        let unit = if with_unit { " M" } else { "" };
        format!("{:.0}{}", value as f64 / 1_000_000_f64, unit)
    } else {
        let unit = if with_unit { " B" } else { "" };
        format!("{:.0}{}", value as f64 / 1_000_000_000_f64, unit)
    }
}

fn format_duration(value: usize, mean: usize, short: bool) -> String {
    if mean < 1_000 {
        format!("{value} ns")
    } else if mean < 1_000_000 {
        if short {
            format!("{} µs", value / 1_000)
        } else {
            format!("{:.2} µs", value as f64 / 1_000_f64)
        }
    } else if mean < 1_000_000_000 {
        if short {
            format!("{} ms", value / 1_000_000)
        } else {
            format!("{:.2} ms", value as f64 / 1_000_000_f64)
        }
    } else if short {
        format!("{}  s", value / 1_000_000_000)
    } else {
        format!("{:.2}  s", value as f64 / 1_000_000_000_f64)
    }
}
