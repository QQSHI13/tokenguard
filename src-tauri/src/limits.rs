//! In-flight limit counters.
//!
//! SQLite is the source of truth for persisted usage, but it can't prevent a
//! burst of concurrent requests from overshooting a request cap between the
//! pre-check and the log insert. This module keeps atomic per-window counters
//! in memory so request limits can be enforced atomically.

use crate::config::{Limit, LimitPeriod};
use chrono::Utc;
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct LimitCounters {
    counters: DashMap<(i64, String), AtomicU64>,
}

impl LimitCounters {
    pub fn new() -> Self {
        Self {
            counters: DashMap::new(),
        }
    }

    /// Atomically add `delta` to the in-flight counter for `limit` and return
    /// the new counter value.
    pub fn increment(&self, limit: &Limit, delta: f64) -> f64 {
        let key = counter_key(limit);
        let entry = self
            .counters
            .entry(key)
            .or_insert_with(|| AtomicU64::new(0.0_f64.to_bits()));
        atomic_f64_add(entry.value(), delta)
    }

    /// Read the current in-flight counter for a limit without modifying it.
    #[cfg(test)]
    pub fn get(&self, limit: &Limit) -> f64 {
        let key = counter_key(limit);
        self.counters
            .get(&key)
            .map(|v| f64::from_bits(v.load(Ordering::Relaxed)))
            .unwrap_or(0.0)
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

fn counter_key(limit: &Limit) -> (i64, String) {
    if limit.metric.is_rate() {
        // Rate-based metrics (RPM/TPM) use a fixed 60-second rolling window.
        (limit.id, period_key_seconds(60))
    } else {
        (limit.id, period_key(limit.period))
    }
}

fn period_key_seconds(seconds: u64) -> String {
    let seconds = seconds.max(1) as i64;
    let now = Utc::now().timestamp();
    let bucket_start = (now / seconds) * seconds;
    bucket_start.to_string()
}

/// A key that identifies the current rolling-window bucket for a limit period.
/// It must match the `ts >= now - period_seconds` window used by the SQLite
/// usage query, so requests that fall in the same bucket share a counter.
fn period_key(period: LimitPeriod) -> String {
    match period {
        LimitPeriod::Once => "once".to_string(),
        _ => period_key_seconds(period.seconds().unwrap_or(0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Limit, LimitAction, LimitMetric, LimitPeriod, LimitScope};

    fn make_limit(metric: LimitMetric) -> Limit {
        Limit {
            id: 1,
            name: "test".into(),
            metric,
            period: LimitPeriod::Daily,
            cap: 10.0,
            warning_threshold: 0.8,
            scope: LimitScope::Global,
            scope_id: None,
            action: LimitAction::Warn,
            enabled: true,
        }
    }

    #[test]
    fn request_counter_increments_atomically() {
        let counters = LimitCounters::new();
        let limit = make_limit(LimitMetric::Requests);
        assert_eq!(counters.increment(&limit, 1.0), 1.0);
        assert_eq!(counters.increment(&limit, 1.0), 2.0);
        assert_eq!(counters.get(&limit), 2.0);
    }

    #[test]
    fn different_limits_have_separate_counters() {
        let counters = LimitCounters::new();
        let mut a = make_limit(LimitMetric::Requests);
        a.id = 1;
        let mut b = make_limit(LimitMetric::Requests);
        b.id = 2;
        counters.increment(&a, 3.0);
        counters.increment(&b, 5.0);
        assert_eq!(counters.get(&a), 3.0);
        assert_eq!(counters.get(&b), 5.0);
    }

    #[test]
    fn once_period_uses_single_key() {
        let mut limit = make_limit(LimitMetric::Requests);
        limit.period = LimitPeriod::Once;
        assert_eq!(period_key(limit.period), "once");
    }
}
