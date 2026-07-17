//! In-flight limit counters.
//!
//! SQLite is the source of truth for persisted usage, but it can't prevent a
//! burst of concurrent requests from overshooting a request cap between the
//! pre-check and the log insert. This module keeps atomic per-limit counters
//! in memory so request limits can be enforced atomically.
//!
//! Counters are keyed by limit id only: a reservation lives just for the
//! duration of one request and is released exactly once per terminal outcome,
//! while the persisted DB count handles the time window. That also keeps the
//! map bounded by the number of configured limits.

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct LimitCounters {
    counters: DashMap<i64, AtomicU64>,
}

impl LimitCounters {
    pub fn new() -> Self {
        Self {
            counters: DashMap::new(),
        }
    }

    /// Atomically add `delta` to the in-flight counter for a limit and return
    /// the new counter value.
    pub fn increment(&self, limit_id: i64, delta: f64) -> f64 {
        let entry = self
            .counters
            .entry(limit_id)
            .or_insert_with(|| AtomicU64::new(0.0_f64.to_bits()));
        atomic_f64_add(entry.value(), delta)
    }

    /// Release one previously reserved unit, clamping at zero so a double
    /// release can never drive the counter negative. Returns the new value.
    pub fn release(&self, limit_id: i64) -> f64 {
        let Some(entry) = self.counters.get(&limit_id) else {
            return 0.0;
        };
        atomic_f64_sub_clamped(entry.value(), 1.0)
    }

    /// Read the current in-flight counter for a limit without modifying it.
    #[cfg(test)]
    pub fn get(&self, limit_id: i64) -> f64 {
        self.counters
            .get(&limit_id)
            .map(|v| f64::from_bits(v.load(Ordering::Relaxed)))
            .unwrap_or(0.0)
    }

    /// Number of counter entries; bounded by the number of distinct limits.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.counters.len()
    }
}

fn atomic_f64_add(atomic: &AtomicU64, delta: f64) -> f64 {
    let mut current = f64::from_bits(atomic.load(Ordering::Relaxed));
    loop {
        let new = current + delta;
        match atomic.compare_exchange_weak(
            current.to_bits(),
            new.to_bits(),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return new,
            Err(actual_bits) => current = f64::from_bits(actual_bits),
        }
    }
}

fn atomic_f64_sub_clamped(atomic: &AtomicU64, delta: f64) -> f64 {
    let mut current = f64::from_bits(atomic.load(Ordering::Relaxed));
    loop {
        let new = (current - delta).max(0.0);
        match atomic.compare_exchange_weak(
            current.to_bits(),
            new.to_bits(),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return new,
            Err(actual_bits) => current = f64::from_bits(actual_bits),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_counter_increments_atomically() {
        let counters = LimitCounters::new();
        assert_eq!(counters.increment(1, 1.0), 1.0);
        assert_eq!(counters.increment(1, 1.0), 2.0);
        assert_eq!(counters.get(1), 2.0);
    }

    #[test]
    fn different_limits_have_separate_counters() {
        let counters = LimitCounters::new();
        counters.increment(1, 3.0);
        counters.increment(2, 5.0);
        assert_eq!(counters.get(1), 3.0);
        assert_eq!(counters.get(2), 5.0);
    }

    #[test]
    fn success_path_reservation_roundtrips_to_zero() {
        // Success path: reserve +1 at check time, release after the request is
        // logged. The counter returns to zero so the request is counted once
        // (by the DB), never twice.
        let counters = LimitCounters::new();
        assert_eq!(counters.increment(1, 1.0), 1.0);
        assert_eq!(counters.release(1), 0.0);
        assert_eq!(counters.get(1), 0.0);
    }

    #[test]
    fn block_path_does_not_leak_reservations() {
        // Block path: reserve +1 at check time, release once on the terminal
        // block outcome. Repeated blocked requests must not accumulate.
        let counters = LimitCounters::new();
        for _ in 0..10 {
            counters.increment(1, 1.0);
            counters.release(1);
        }
        assert_eq!(counters.get(1), 0.0);
    }

    #[test]
    fn release_clamps_at_zero() {
        let counters = LimitCounters::new();
        // Release with no reservation at all.
        assert_eq!(counters.release(1), 0.0);
        // Double release after a single reservation.
        counters.increment(1, 1.0);
        counters.release(1);
        assert_eq!(counters.release(1), 0.0);
        assert_eq!(counters.get(1), 0.0);
    }

    #[test]
    fn entries_are_bounded_by_limit_count() {
        let counters = LimitCounters::new();
        for id in 0..5 {
            counters.increment(id, 1.0);
        }
        assert_eq!(counters.len(), 5);
    }
}
