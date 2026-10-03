//! Regression coverage for tinymist-flow's opt-in retention and detached drops.

use std::hash::Hash;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use comemo::{RetentionPolicy, Track, Tracked, evict, memoize, track};
use serial_test::serial;

fn policy(entries: usize, bytes: usize) -> RetentionPolicy {
    RetentionPolicy {
        min_compute_time: Duration::ZERO,
        max_age: 4,
        max_entries: entries,
        max_payload_bytes: bytes,
    }
}

#[test]
#[serial]
fn costly_nested_result_survives_outer_hits_and_manual_clear() {
    #[derive(Clone, Debug, PartialEq)]
    struct Output(Vec<u8>);
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    #[memoize]
    fn costly(id: u8) -> Output {
        CALLS.fetch_add(1, Ordering::Relaxed);
        Output(vec![id; 8])
    }
    #[memoize]
    fn outer(revision: u8) -> (u8, Output) {
        (revision, costly(7))
    }
    evict(0);
    assert!(comemo::retain_expensive::<Output>(policy(4, 32), |v| Some(v.0.len())));
    assert_eq!(outer(0).1.0, vec![7; 8]);
    for _ in 0..3 {
        evict(1);
        assert_eq!(outer(0).1.0, vec![7; 8]);
        assert!(comemo::testing::last_was_hit());
    }
    assert_eq!(outer(1).1.0, vec![7; 8]);
    assert_eq!(CALLS.load(Ordering::Relaxed), 1);
    assert!(comemo::retention_stats().hits > 0);
    for _ in 0..5 {
        evict(1);
    }
    costly(7);
    assert_eq!(CALLS.load(Ordering::Relaxed), 2, "extended age must expire");
    evict(0);
    assert_eq!(comemo::retention_stats().entries, 0);
    costly(7);
    assert_eq!(
        CALLS.load(Ordering::Relaxed),
        3,
        "manual clear must release protected results"
    );
    evict(0);
}

#[test]
#[serial]
fn payload_entry_limits_and_pressure_reduction_are_enforced() {
    #[derive(Clone)]
    struct Output(Vec<u8>);
    #[memoize]
    fn value(id: u8, len: usize) -> Output {
        Output(vec![id; len])
    }
    evict(0);
    assert!(comemo::retain_expensive::<Output>(policy(2, 5), |v| Some(v.0.len())));
    value(1, 3);
    value(2, 3);
    value(3, 2);
    value(4, 0);
    let stats = comemo::retention_stats();
    assert_eq!((stats.entries, stats.payload_bytes), (2, 5));
    evict(1);
    evict(1);
    value(1, 3);
    assert!(comemo::testing::last_was_hit());
    value(2, 3);
    assert!(!comemo::testing::last_was_hit(), "over-budget output uses normal age");
    value(4, 0);
    assert!(!comemo::testing::last_was_hit(), "entry cap also covers empty outputs");
    assert!(comemo::set_retention_limits::<Output>(1, 2));
    evict(1);
    let stats = comemo::retention_stats();
    assert!(stats.entries <= 1 && stats.payload_bytes <= 2);
    assert!(comemo::set_retention_limits::<Output>(0, 0));
    evict(1);
    assert_eq!(comemo::retention_stats().entries, 0);
    value(5, 1);
    assert_eq!(comemo::retention_stats().entries, 0);
    evict(0);
}

#[test]
#[serial]
fn keys_functions_and_tracked_constraints_remain_distinct() {
    #[derive(Clone, Debug, PartialEq)]
    struct Output(i32);
    struct Data(i32);
    #[track]
    impl Data {
        fn get(&self) -> i32 {
            self.0
        }
    }
    #[memoize]
    fn first(id: i32) -> Output {
        Output(id)
    }
    #[memoize]
    fn second(id: i32) -> Output {
        Output(-id)
    }
    #[memoize]
    fn dependent(data: Tracked<Data>) -> Output {
        Output(data.get())
    }
    evict(0);
    assert!(comemo::retain_expensive::<Output>(policy(16, 64), |_| Some(4)));
    assert_eq!(first(1), Output(1));
    assert_eq!(first(2), Output(2));
    assert_eq!(second(1), Output(-1));
    assert_eq!(dependent(Data(5).track()), Output(5));
    assert_eq!(
        comemo::retention_stats().entries,
        3,
        "tracked outputs cannot reserve retention"
    );
    evict(1);
    evict(1);
    assert_eq!(first(1), Output(1));
    assert!(comemo::testing::last_was_hit());
    assert_eq!(second(1), Output(-1));
    assert!(comemo::testing::last_was_hit());
    assert_eq!(dependent(Data(6).track()), Output(6));
    assert!(!comemo::testing::last_was_hit());
    assert_eq!(dependent(Data(5).track()), Output(5));
    assert!(!comemo::testing::last_was_hit());
    assert_eq!(dependent(Data(6).track()), Output(6));
    assert!(comemo::testing::last_was_hit());
    evict(0);
}

#[test]
#[serial]
fn protected_results_support_parallel_lookup() {
    #[derive(Clone, Debug, PartialEq)]
    struct Output(Vec<u8>);
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    #[memoize]
    fn value(id: u8) -> Output {
        CALLS.fetch_add(1, Ordering::Relaxed);
        Output(vec![id; 1024])
    }
    evict(0);
    assert!(comemo::retain_expensive::<Output>(policy(2, 2048), |v| { Some(v.0.len()) }));
    value(3);
    evict(1);
    evict(1);
    thread::scope(|scope| {
        for _ in 0..16 {
            scope.spawn(|| {
                for _ in 0..32 {
                    assert_eq!(value(3).0, vec![3; 1024]);
                }
            });
        }
    });
    assert_eq!(CALLS.load(Ordering::Relaxed), 1);
    assert_eq!(comemo::retention_stats().entries, 1);
    evict(0);
}

#[test]
#[serial]
fn slow_and_reentrant_destructors_run_after_cache_and_registry_unlock() {
    #[derive(Clone)]
    struct Output(Arc<DropAction>);
    struct DropAction {
        entered: mpsc::Sender<()>,
        released: AtomicBool,
        release: std::sync::Mutex<mpsc::Receiver<()>>,
    }
    impl Drop for Output {
        fn drop(&mut self) {
            // Only the cached final clone performs the action.
            if Arc::strong_count(&self.0) != 1
                || self.0.released.swap(true, Ordering::Relaxed)
            {
                return;
            }
            // This initializes a new memoized cache and registers its evictor.
            // Holding EVICTORS.read() across callbacks would deadlock here.
            assert_eq!(initialized_in_drop(9), 9);
            self.0.entered.send(()).unwrap();
            self.0
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }
    }
    #[memoize]
    fn initialized_in_drop(id: u8) -> u8 {
        id
    }
    // The hash key deliberately excludes the channels; the test gives each
    // unique key a single deterministic output, as required by comemo.
    #[derive(Clone)]
    struct Input(u8, Arc<DropAction>);
    impl std::hash::Hash for Input {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.0.hash(state);
        }
    }
    #[memoize]
    fn value(input: Input) -> Output {
        Output(input.1)
    }
    evict(0);
    for max_age in [1, 0] {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let action = Arc::new(DropAction {
            entered: entered_tx,
            released: AtomicBool::new(false),
            release: std::sync::Mutex::new(release_rx),
        });
        drop(value(Input(1, action)));
        if max_age != 0 {
            evict(max_age);
        }
        thread::scope(|scope| {
            let worker = scope.spawn(|| evict(max_age));
            entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            let (retrieved_tx, retrieved_rx) = mpsc::channel();
            let reader = scope.spawn(move || {
                let (dummy_tx, _) = mpsc::channel();
                let (_, dummy_rx) = mpsc::channel();
                let output = value(Input(
                    2,
                    Arc::new(DropAction {
                        entered: dummy_tx,
                        released: AtomicBool::new(true),
                        release: std::sync::Mutex::new(dummy_rx),
                    }),
                ));
                retrieved_tx.send(()).unwrap();
                drop(output);
            });
            let result = retrieved_rx.recv_timeout(Duration::from_secs(5));
            release_tx.send(()).unwrap();
            worker.join().unwrap();
            reader.join().unwrap();
            assert!(
                result.is_ok(),
                "another call must acquire the cache while Drop is blocked"
            );
        });
        evict(0);
    }
}

#[test]
#[serial]
fn cheap_outputs_and_rejected_results_keep_standard_eviction() {
    #[derive(Clone)]
    struct Cheap(Vec<u8>);
    #[derive(Clone)]
    struct Selective(Result<Vec<u8>, ()>);
    #[memoize]
    fn cheap(id: u8) -> Cheap {
        Cheap(vec![id])
    }
    #[memoize]
    fn selective(success: bool) -> Selective {
        Selective(if success { Ok(vec![1]) } else { Err(()) })
    }
    evict(0);
    let mut slow_only = policy(4, 32);
    slow_only.min_compute_time = Duration::from_secs(24 * 60 * 60);
    assert!(comemo::retain_expensive::<Cheap>(slow_only, |v| Some(v.0.len())));
    assert!(comemo::retain_expensive::<Selective>(policy(4, 32), |v| v
        .0
        .as_ref()
        .ok()
        .map(Vec::len)));
    cheap(1);
    selective(false);
    selective(true);
    assert_eq!(comemo::retention_stats().entries, 1);
    evict(1);
    evict(1);
    cheap(1);
    assert!(!comemo::testing::last_was_hit());
    selective(false);
    assert!(!comemo::testing::last_was_hit());
    selective(true);
    assert!(comemo::testing::last_was_hit());
    evict(0);
}

#[test]
#[serial]
fn expensive_computation_counters_include_over_budget_misses_only() {
    #[derive(Clone)]
    struct Output(Result<Vec<u8>, ()>);
    #[memoize]
    fn value(id: u8) -> Output {
        Output(if id == 0 { Err(()) } else { Ok(vec![id; 2]) })
    }
    struct Data(u8);
    #[track]
    impl Data {
        fn get(&self) -> u8 {
            self.0
        }
    }
    // comemo tracks immutable and mutable surfaces on separate types.
    struct MutableData(u8);
    #[track]
    impl MutableData {
        fn put(&mut self, value: u8) {
            self.0 = value;
        }
    }
    #[memoize]
    fn dependent(data: Tracked<Data>) -> Output {
        Output(Ok(vec![data.get(); 2]))
    }
    #[memoize]
    fn mutating(mut data: comemo::TrackedMut<MutableData>) -> Output {
        data.put(9);
        Output(Ok(vec![9; 2]))
    }
    evict(0);
    assert!(comemo::retain_expensive::<Output>(policy(1, 2), |v| v
        .0
        .as_ref()
        .ok()
        .map(Vec::len)));
    let before = comemo::retention_stats();
    value(1);
    value(1); // A cache hit must not count as another computation.
    value(2); // Eligible computation, but the payload/entry budget is full.
    value(0); // Rejected output.
    dependent(Data(3).track()); // Tracked dependencies disqualify the computation.
    mutating(MutableData(4).track_mut()); // Mutable tracked calls also disqualify it.
    let after = comemo::retention_stats();
    assert_eq!(after.expensive_compute_count - before.expensive_compute_count, 2);
    assert!(after.expensive_compute_time >= before.expensive_compute_time);
    assert_eq!(after.entries, 1);
    evict(0);
    let cleared = comemo::retention_stats();
    assert_eq!(cleared.entries, 0);
    assert_eq!(cleared.expensive_compute_count, after.expensive_compute_count);
    assert_eq!(cleared.expensive_compute_time, after.expensive_compute_time);
}

#[test]
#[serial]
fn unconfigured_functions_observe_late_output_policy_registration() {
    #[derive(Clone, Debug, PartialEq)]
    struct Output(u8);
    #[memoize]
    fn first(id: u8) -> Output {
        Output(id)
    }
    #[memoize]
    fn second(id: u8) -> Output {
        Output(id + 10)
    }
    evict(0);
    for id in 0..4 {
        assert_eq!(first(id), Output(id));
        assert!(!comemo::testing::last_was_hit());
        assert_eq!(second(id), Output(id + 10));
        assert!(!comemo::testing::last_was_hit());
    }
    assert_eq!(comemo::retention_stats().entries, 0);
    assert!(comemo::retain_expensive::<Output>(policy(4, 4), |_| Some(1)));
    assert_eq!(first(4), Output(4));
    assert_eq!(second(4), Output(14));
    assert_eq!(comemo::retention_stats().entries, 2);
    evict(1);
    evict(1);
    assert_eq!(first(4), Output(4));
    assert!(comemo::testing::last_was_hit());
    assert_eq!(second(4), Output(14));
    assert!(comemo::testing::last_was_hit());
    // Registering a policy does not retroactively promote existing outputs.
    assert_eq!(first(0), Output(0));
    assert!(!comemo::testing::last_was_hit());
    evict(0);
    assert_eq!(comemo::retention_stats().entries, 0);
}
