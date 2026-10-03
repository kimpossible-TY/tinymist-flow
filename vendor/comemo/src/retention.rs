//! Opt-in retention of costly outputs with known payload sizes.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use parking_lot::{Mutex, RwLock};

static POLICIES: LazyLock<RwLock<HashMap<TypeId, Arc<RegisteredPolicy>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Limits for retaining expensive results beyond the ordinary eviction age.
///
/// Only calls without tracked dependencies or mutations qualify. The payload
/// count is supplied by the caller: it does not include allocator overhead,
/// shared backing allocations, ordinary cache entries, or total process memory.
#[derive(Debug, Clone, Copy)]
pub struct RetentionPolicy {
    /// Minimum elapsed computation time before an output qualifies.
    pub min_compute_time: Duration,
    /// Maximum unused sweep age for qualifying outputs.
    pub max_age: usize,
    /// Maximum number of protected outputs across all caches of this type.
    pub max_entries: usize,
    /// Maximum sum of the qualifying outputs' reported payload sizes.
    pub max_payload_bytes: usize,
}

/// Protected-output accounting and cumulative computation measurements across
/// all configured types.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetentionStats {
    /// Number of outputs holding a retention reservation.
    pub entries: usize,
    /// Sum of the outputs' reported payload sizes, not allocated memory or RSS.
    pub payload_bytes: usize,
    /// Cache hits on outputs holding a retention reservation.
    pub hits: usize,
    /// Completed costly computations with eligible outputs and no tracked
    /// dependencies or mutations, including those rejected by the budget.
    /// Cumulative since policy registration; this is not a total miss count.
    pub expensive_compute_count: usize,
    /// Sum of those computations' elapsed body times, measured before cache
    /// insertion. Nested calls can overlap, so this is not wall-clock savings.
    pub expensive_compute_time: Duration,
}

/// Enable bounded retention for one output type.
///
/// `payload_bytes` returns `None` for outputs that should not be protected (for
/// example errors). Register before computation begins. Returns false if this
/// type already has a policy; registration never silently replaces a policy.
///
/// On WebAssembly targets elapsed-time measurement is disabled, so no new
/// outputs qualify. Ordinary memoization remains unchanged on every target.
pub fn retain_expensive<T: 'static>(
    policy: RetentionPolicy,
    payload_bytes: fn(&T) -> Option<usize>,
) -> bool {
    let mut policies = POLICIES.write();
    let id = TypeId::of::<T>();
    if policies.contains_key(&id) {
        return false;
    }
    policies.insert(
        id,
        Arc::new(RegisteredPolicy {
            min_compute_time: policy.min_compute_time,
            max_age: policy.max_age,
            measure: Box::new(move |output| {
                output.downcast_ref::<T>().and_then(payload_bytes)
            }),
            budget: Mutex::new(Budget {
                max_entries: policy.max_entries,
                max_payload_bytes: policy.max_payload_bytes,
                entries: 0,
                payload_bytes: 0,
                expensive_compute_count: 0,
                expensive_compute_time: Duration::ZERO,
            }),
            hits: AtomicUsize::new(0),
        }),
    );
    true
}

/// Change a registered type's entry and payload limits.
///
/// The next sweep releases excess reservations. Fresh outputs can still remain
/// under the ordinary age policy. A zero limit prevents new reservations and
/// removes extended retention at the next sweep. In-flight computations also
/// observe the updated limits.
/// Returns false if the output type has no policy.
pub fn set_retention_limits<T: 'static>(
    max_entries: usize,
    max_payload_bytes: usize,
) -> bool {
    let policies = POLICIES.read();
    let Some(policy) = policies.get(&TypeId::of::<T>()) else {
        return false;
    };
    let mut budget = policy.budget.lock();
    budget.max_entries = max_entries;
    budget.max_payload_bytes = max_payload_bytes;
    true
}

/// Read protection and computation counters without traversing memoization trees.
pub fn retention_stats() -> RetentionStats {
    let mut stats = RetentionStats::default();
    for policy in POLICIES.read().values() {
        let budget = policy.budget.lock();
        stats.entries += budget.entries;
        stats.payload_bytes += budget.payload_bytes;
        stats.hits += policy.hits.load(Ordering::Relaxed);
        stats.expensive_compute_count = stats
            .expensive_compute_count
            .saturating_add(budget.expensive_compute_count);
        stats.expensive_compute_time = stats
            .expensive_compute_time
            .saturating_add(budget.expensive_compute_time);
    }
    stats
}

type PayloadMeasure = dyn Fn(&dyn Any) -> Option<usize> + Send + Sync;

struct RegisteredPolicy {
    min_compute_time: Duration,
    max_age: usize,
    measure: Box<PayloadMeasure>,
    budget: Mutex<Budget>,
    hits: AtomicUsize,
}

struct Budget {
    max_entries: usize,
    max_payload_bytes: usize,
    entries: usize,
    payload_bytes: usize,
    expensive_compute_count: usize,
    expensive_compute_time: Duration,
}

/// A reservation does not own or duplicate the memoized output.
pub(crate) struct Reservation {
    policy: Arc<RegisteredPolicy>,
    payload_bytes: usize,
}

impl Reservation {
    pub(crate) fn keeps(&self, age: usize) -> bool {
        let budget = self.policy.budget.lock();
        age <= self.policy.max_age
            && budget.entries <= budget.max_entries
            && budget.payload_bytes <= budget.max_payload_bytes
            && budget.max_entries != 0
            && budget.max_payload_bytes != 0
    }

    pub(crate) fn hit(&self) {
        self.policy.hits.fetch_add(1, Ordering::Relaxed);
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        let mut budget = self.policy.budget.lock();
        budget.entries -= 1;
        budget.payload_bytes -= self.payload_bytes;
    }
}

pub(crate) struct Measurement {
    policy: Arc<RegisteredPolicy>,
    #[cfg(not(target_arch = "wasm32"))]
    start: std::time::Instant,
}

pub(crate) fn start<T: 'static>() -> Option<Measurement> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let policy = POLICIES.read().get(&TypeId::of::<T>())?.clone();
        Some(Measurement { policy, start: std::time::Instant::now() })
    }
    #[cfg(target_arch = "wasm32")]
    None
}

impl Measurement {
    pub(crate) fn finish(self, output: &dyn Any) -> Option<MeasuredOutput> {
        #[cfg(not(target_arch = "wasm32"))]
        let elapsed = self.start.elapsed();
        #[cfg(target_arch = "wasm32")]
        let elapsed = Duration::ZERO;
        if elapsed < self.policy.min_compute_time {
            return None;
        }
        let payload_bytes = (self.policy.measure)(output)?;
        Some(MeasuredOutput { policy: self.policy, payload_bytes, elapsed })
    }
}

/// A costly eligible output, not yet checked for tracked inputs or budget space.
pub(crate) struct MeasuredOutput {
    policy: Arc<RegisteredPolicy>,
    payload_bytes: usize,
    elapsed: Duration,
}

impl MeasuredOutput {
    /// Called only after the memoizer excludes tracked dependencies/mutations.
    pub(crate) fn reserve(self) -> Option<Reservation> {
        let mut budget = self.policy.budget.lock();
        budget.expensive_compute_count = budget.expensive_compute_count.saturating_add(1);
        budget.expensive_compute_time =
            budget.expensive_compute_time.saturating_add(self.elapsed);
        let next_bytes = budget.payload_bytes.checked_add(self.payload_bytes)?;
        if budget.entries >= budget.max_entries
            || next_bytes > budget.max_payload_bytes
            || budget.max_payload_bytes == 0
        {
            return None;
        }
        budget.entries += 1;
        budget.payload_bytes = next_bytes;
        drop(budget);
        Some(Reservation {
            policy: self.policy,
            payload_bytes: self.payload_bytes,
        })
    }
}
