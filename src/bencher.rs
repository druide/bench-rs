use crate::timing_future::TimingFuture;
#[cfg(feature = "track-allocator")]
use crate::track_allocator::GLOBAL;
use crate::{Stats, Step};
use human_bytes::human_bytes;
use lazy_static::lazy_static;
use stats_alloc::Region;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

lazy_static! {
    /// This is an example for using doc comment attributes
    static ref FIRST: AtomicBool = AtomicBool::new(true);
}

pub struct Bencher {
    pub name: String,
    pub count: usize,
    pub steps: Vec<Step>,
    pub bytes: usize,
    pub n: usize,
    pub poll: usize,
    pub format_fn: fn(&Stats, &Bencher),

    // time, mem, allocations, leaked
    pub mem_track: (AtomicUsize, AtomicUsize, AtomicUsize, AtomicUsize),
}

impl Bencher {
    #[cfg(feature = "track-allocator")]
    pub fn new(name: impl AsRef<str>, count: usize, bytes: usize) -> Self {
        Bencher {
            name: name.as_ref().to_owned(),
            count,
            steps: Vec::with_capacity(count),
            bytes,
            n: 0,
            poll: 0,
            format_fn: |s, b| Self::default_format(s, b),

            mem_track: (
                AtomicUsize::new(0),
                AtomicUsize::new(0),
                AtomicUsize::new(0),
                AtomicUsize::new(0),
            ),
        }
    }

    #[cfg(not(feature = "track-allocator"))]
    pub fn new(
        name: impl AsRef<str>,
        count: usize,
        bytes: usize,
        counter: &'static AtomicUsize,
        peak: &'static AtomicUsize,
    ) -> Self {
        Bencher {
            name: name.as_ref().to_owned(),
            count,
            steps: Vec::with_capacity(count),
            bytes,
            n: 0,
            poll: 0,
            format_fn: |s, b| Self::default_format(s, b),

            mem_track: (counter, peak),
        }
    }

    // (time, memory_usage)
    pub fn bench_once<T>(
        &self,
        f: &mut impl FnMut() -> T,
        n: usize,
    ) -> (u128, usize, usize, usize) {
        self.reset_mem();

        let mut allocations = 0;
        let mut allocated_bytes = 0;
        let mut leaked_bytes = 0;
        let now = Instant::now();

        for _ in 0..n {
            let reg = Region::new(GLOBAL);
            let _output = f();
            let changes = reg.change();
            allocations = allocations.max(changes.allocations);
            allocated_bytes = allocated_bytes.max(changes.bytes_allocated);
            leaked_bytes = leaked_bytes.max(
                changes
                    .bytes_allocated
                    .saturating_sub(changes.bytes_deallocated),
            );
        }

        (
            now.elapsed().as_nanos(),
            allocated_bytes,
            allocations,
            leaked_bytes,
        )
    }

    pub fn iter<T>(&mut self, mut f: impl FnMut() -> T) {
        let single = self.bench_once(&mut f, 1).0;
        // 1_000_000ns : 1ms
        self.n = (1_000_000 / single.max(1)).max(1) as usize;
        (0..self.count).for_each(|_| {
            let res = self.bench_once(&mut f, self.n);
            self.steps.push(Step {
                time: res.0 / self.n as u128,
                mem: res.1,
                allocations: res.2,
                leaked_bytes: res.3,
            })
        });
    }

    pub fn async_iter<'a, T, Fut: Future<Output = T>>(
        &'a mut self,
        mut f: impl FnMut() -> Fut + 'a,
    ) -> impl Future + 'a {
        async move {
            let single = TimingFuture::new(f()).await.elapsed_time.as_nanos();
            // 1_000_000ns : 1ms
            self.n = (1_000_000 / single.max(1)).max(1) as usize;

            let mut polls = Vec::with_capacity(self.count);

            for _ in 0..self.count {
                let mut mtime = 0u128;
                self.reset_mem();

                for _ in 0..self.n {
                    let tf = TimingFuture::new(f()).await;
                    mtime += tf.elapsed_time.as_nanos();
                    polls.push(tf.poll);
                }

                let info = self.get_mem();
                self.steps.push(Step {
                    time: mtime / self.n as u128,
                    mem: info.0,
                    allocations: info.1,
                    leaked_bytes: info.2,
                });
            }

            self.poll = polls.iter().sum::<usize>() / polls.len();
        }
    }

    pub fn finish(&self) {
        let stats = Stats::from(&self.steps);
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

    fn default_format(stats: &Stats, bencher: &Bencher) {
        bunt::println!(
            "{}{[bg:white+blue+bold]}\t... {$green+underline}{}/it{/$} ({}-{}), {$cyan+underline}{}/s{/$}, \
            max memory: {$yellow+underline}{}{/$}, memory leak: {$red+underline}{}{/$}, allocations: {[red]}, \
            samples: {[magenta]}{[bold]}",
             if (*FIRST).load(Ordering::SeqCst) {
                (*FIRST).store(false, Ordering::SeqCst);
                "."
             } else {""},
             &bencher.name,
             format_duration(stats.times_average, stats.times_min, stats.times_average, stats.times_max),
             format_duration(stats.times_min, stats.times_min, stats.times_average, stats.times_max),
             format_duration(stats.times_max, stats.times_min, stats.times_average, stats.times_max),
             human_bytes(bencher.bytes as f64 * (1_000_000_000f64 / stats.times_average as f64)),

             human_bytes(stats.mem_max as f64),
             human_bytes(stats.leaked_bytes as f64),
             stats.allocations,

             bencher.count * bencher.n,

             if bencher.poll > 0 {
                format!(
                    ", {} polls",
                    bencher.poll
                 )
             } else {
                String::new()
             },
        );
    }
}

fn format_duration(value: usize, min: usize, mean: usize, max: usize) -> String {
    if min < 1_000 && mean < 1_000 && max < 1_000 {
        format!("{value} ns")
    } else if min < 1_000_000 && mean < 1_000_000 && max < 1_000_000 {
        format!("{:.2} µs", value as f64 / 1_000_f64)
    } else if min < 1_000_000_000 && mean < 1_000_000_000 && max < 1_000_000_000 {
        format!("{:.2} ms", value as f64 / 1_000_000_f64)
    } else {
        format!("{:.2} s", value as f64 / 1_000_000_000_f64)
    }
}
