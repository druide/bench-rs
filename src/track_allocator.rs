use crate::bencher::Bencher as B;
#[cfg(feature = "track-allocator")]
use stats_alloc::{StatsAlloc, INSTRUMENTED_SYSTEM};
#[cfg(feature = "track-allocator")]
use std::alloc::System;

#[cfg(feature = "track-allocator")]
#[global_allocator]
pub static GLOBAL_ALLOC: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;
#[cfg(feature = "track-allocator")]
pub type Bencher = B<std::alloc::System>;
