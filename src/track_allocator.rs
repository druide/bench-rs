#[cfg(feature = "track-allocator")]
use stats_alloc::{StatsAlloc, INSTRUMENTED_SYSTEM};
#[cfg(feature = "track-allocator")]
use std::alloc::System;

#[cfg(feature = "track-allocator")]
#[global_allocator]
pub static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;
