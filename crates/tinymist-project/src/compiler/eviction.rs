//! Automatic maintenance of the process-wide compiler caches.

use std::sync::Once;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use typst::diag::StrResult;
use typst::foundations::Bytes;

use tinymist_std::performance::{MemoryPressure, MemorySnapshot, QosGuard, WorkClass};
use tinymist_std::time::Instant;

// Keep the working set touched since the preceding sweep. In comemo, a hit
// resets age to zero and a sweep increments age before retaining age <= this
// limit. One unused interval therefore survives; the next sweep releases it.
// This bounds retained generations, not bytes: one document can itself be large.
pub(super) const MAX_UNUSED_AGE: usize = 1;

static EVICTION: EvictionScheduler = EvictionScheduler::new();

const PROTECTED_ENTRIES: usize = 64;
const PROTECTED_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

/// Keep costly byte results alive across outer memoization hits. The usual
/// compiler graph still uses the short age above. No plugin instances or
/// argument buffers are retained by this policy, and errors are excluded.
/// The budget counts output payload bytes, not allocator usage or process RSS.
pub(super) fn initialize_retention() {
    static INITIALIZED: Once = Once::new();
    INITIALIZED.call_once(|| {
        comemo::retain_expensive::<StrResult<Bytes>>(
            comemo::RetentionPolicy {
                min_compute_time: Duration::from_millis(50),
                max_age: 16,
                max_entries: PROTECTED_ENTRIES,
                max_payload_bytes: PROTECTED_PAYLOAD_BYTES,
            },
            result_payload_bytes,
        );
    });
}

/// Count successful byte payloads only. In particular, this cannot retain
/// plugin modules, WASM stores, or plugin errors through the protected budget.
fn result_payload_bytes(output: &StrResult<Bytes>) -> Option<usize> {
    output.as_ref().ok().map(|bytes| bytes.len())
}

fn retention_payload_budget(pressure: MemoryPressure) -> usize {
    match pressure {
        MemoryPressure::Warning => 16 * 1024 * 1024,
        MemoryPressure::Critical => 0,
        MemoryPressure::Normal | MemoryPressure::Unknown => PROTECTED_PAYLOAD_BYTES,
    }
}

fn maintenance_work_class(pressure: MemoryPressure) -> WorkClass {
    match pressure {
        // Reclaiming memory becomes interactive work when the system is under
        // pressure. The scoped guard still restores the worker after each sweep.
        MemoryPressure::Warning | MemoryPressure::Critical => WorkClass::Interactive,
        MemoryPressure::Normal | MemoryPressure::Unknown => WorkClass::Maintenance,
    }
}

/// Request a sweep without spawning one worker per completed compilation.
pub(super) fn schedule() {
    if EVICTION.request() {
        let queued_at = Instant::now();
        super::spawn_cpu(move || {
            log::debug!(
                "ProjectCompiler: automatic cache sweep queued for {:?}",
                queued_at.elapsed()
            );
            EVICTION.run(|| {
                let start = Instant::now();
                let pressure = MemorySnapshot::capture().pressure;
                let work_class = maintenance_work_class(pressure);
                let _qos = QosGuard::enter(work_class);
                let payload_budget = retention_payload_budget(pressure);
                comemo::set_retention_limits::<StrResult<Bytes>>(
                    PROTECTED_ENTRIES,
                    payload_budget,
                );
                comemo::evict(MAX_UNUSED_AGE);
                let retained = comemo::retention_stats();
                log::debug!(
                    "ProjectCompiler: evict comemo cache in {:?} (max_age={MAX_UNUSED_AGE}, protected_entries={}, protected_payload_bytes={}, protected_hits={}, expensive_compute_count={}, expensive_compute_time={:?}, payload_budget={payload_budget}, memory_pressure={pressure:?}, work_class={work_class:?})",
                    start.elapsed(), retained.entries, retained.payload_bytes, retained.hits,
                    retained.expensive_compute_count, retained.expensive_compute_time,
                );
            });
        });
    }
}

const IDLE: u8 = 0;
const RUNNING: u8 = 1;
const PENDING: u8 = 2;

/// Owns one sweep worker and at most one pending follow-up sweep.
struct EvictionScheduler {
    state: AtomicU8,
}

impl EvictionScheduler {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(IDLE),
        }
    }

    /// Returns whether the caller must start the sole worker.
    fn request(&self) -> bool {
        self.state.swap(PENDING, Ordering::AcqRel) == IDLE
    }

    /// Drains coalesced requests, including ones arriving during a sweep.
    /// The caller must have reserved this worker through `request`.
    fn run(&self, mut evict: impl FnMut()) {
        loop {
            // Requests already pending are satisfied by this sweep. Requests
            // arriving after this swap reserve a follow-up instead.
            self.state.swap(RUNNING, Ordering::AcqRel);
            evict();

            // Release ownership atomically with checking for more work. A
            // racing request either keeps this worker alive or starts a new
            // worker after we have finished; it cannot be lost.
            if self
                .state
                .compare_exchange(RUNNING, IDLE, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn maintenance_priority_follows_memory_pressure() {
        for pressure in [MemoryPressure::Normal, MemoryPressure::Unknown] {
            assert_eq!(maintenance_work_class(pressure), WorkClass::Maintenance);
        }
        for pressure in [MemoryPressure::Warning, MemoryPressure::Critical] {
            assert_eq!(maintenance_work_class(pressure), WorkClass::Interactive);
        }
    }

    #[test]
    fn pressure_reduces_only_the_extended_payload_budget() {
        assert_eq!(
            retention_payload_budget(MemoryPressure::Normal),
            64 * 1024 * 1024
        );
        assert_eq!(
            retention_payload_budget(MemoryPressure::Warning),
            16 * 1024 * 1024
        );
        assert_eq!(retention_payload_budget(MemoryPressure::Critical), 0);
        assert_eq!(
            retention_payload_budget(MemoryPressure::Unknown),
            64 * 1024 * 1024
        );
    }

    #[test]
    fn retention_counts_actual_byte_results_and_excludes_errors() {
        let bytes = Bytes::new(vec![7; 1024 * 1024]);
        assert_eq!(result_payload_bytes(&Ok(bytes)), Some(1024 * 1024));
        assert_eq!(result_payload_bytes(&Err("plugin failed".into())), None);
    }

    #[test]
    fn requests_before_worker_starts_share_one_sweep() {
        let scheduler = EvictionScheduler::new();
        assert!(scheduler.request());
        for _ in 0..32 {
            assert!(!scheduler.request());
        }
        let mut sweeps = 0;
        scheduler.run(|| sweeps += 1);
        assert_eq!(sweeps, 1);

        assert!(
            scheduler.request(),
            "the drained worker must release ownership"
        );
        scheduler.run(|| sweeps += 1);
        assert_eq!(sweeps, 2);
    }

    #[test]
    fn concurrent_requests_during_sweep_share_one_follow_up() {
        let scheduler = EvictionScheduler::new();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        assert!(scheduler.request());

        thread::scope(|scope| {
            let worker_scheduler = &scheduler;
            let worker = scope.spawn(move || {
                let mut sweeps = 0;
                worker_scheduler.run(|| {
                    sweeps += 1;
                    if sweeps == 1 {
                        started_tx.send(()).unwrap();
                        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                    }
                });
                sweeps
            });
            started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            let requesters: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| (0..32).filter(|_| scheduler.request()).count()))
                .collect();
            let extra_workers: usize = requesters
                .into_iter()
                .map(|requester| requester.join().unwrap())
                .sum();
            release_tx.send(()).unwrap();
            let sweeps = worker.join().unwrap();

            assert_eq!(extra_workers, 0, "a sweep must never start a second worker");
            assert_eq!(sweeps, 2, "a burst must produce just one follow-up sweep");
        });

        assert!(scheduler.request());
        scheduler.run(|| {});
    }
}
