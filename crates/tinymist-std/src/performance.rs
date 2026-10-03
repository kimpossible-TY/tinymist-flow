//! Platform scheduling hints and inexpensive process memory observations.
//!
//! QoS is an operating-system hint, not CPU affinity. Memory observations cover
//! the whole process and must not be treated as one compilation's allocations.

use std::marker::PhantomData;
use std::rc::Rc;

/// The kind of CPU work currently running on a worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkClass {
    /// Work whose result a user is waiting for.
    Interactive,
    /// Cache cleanup and other work without an immediate user-visible result.
    Maintenance,
}

/// Restores a worker's scheduling class when synchronous work finishes.
// The guard is deliberately !Send and !Sync: it must be dropped on its thread.
#[must_use]
pub struct QosGuard {
    #[cfg(target_os = "macos")]
    previous: Option<(u32, i32)>,
    _thread: PhantomData<Rc<()>>,
}

impl QosGuard {
    /// Classifies synchronous work on the current thread.
    ///
    /// On platforms without macOS QoS this is a no-op. If the current macOS
    /// thread has opted out of QoS, leave its existing scheduler policy alone.
    pub fn enter(class: WorkClass) -> Self {
        #[cfg(target_os = "macos")]
        let previous = macos::enter(class);
        #[cfg(not(target_os = "macos"))]
        let _ = class;
        Self {
            #[cfg(target_os = "macos")]
            previous,
            _thread: PhantomData,
        }
    }
}

impl Drop for QosGuard {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some((class, priority)) = self.previous {
            macos::set(class, priority);
        }
    }
}

/// Sets the baseline class of a newly created, application-owned worker.
///
/// Unlike a scoped guard this intentionally lasts for the worker's lifetime.
/// It should only be called from a thread pool's worker initialization hook.
pub fn initialize_worker(class: WorkClass) {
    #[cfg(target_os = "macos")]
    macos::set(macos::class_value(class), 0);
    #[cfg(not(target_os = "macos"))]
    let _ = class;
}

/// System-wide memory pressure as reported by the operating system.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    /// No pressure signal is available on this platform or invocation.
    #[default]
    Unknown,
    /// The system reports normal memory conditions.
    Normal,
    /// The system is reclaiming memory more aggressively.
    Warning,
    /// The system reports critical memory pressure.
    Critical,
}

/// A process observation used for diagnostics and bounded cache policy.
#[derive(Debug, Default, Clone, Copy)]
pub struct MemorySnapshot {
    /// Resident memory for the entire current process, when available.
    pub resident_bytes: Option<u64>,
    /// macOS physical footprint, including memory charged to this process.
    pub physical_footprint_bytes: Option<u64>,
    /// Current system-wide pressure; this is not inferred from swap occupancy.
    pub pressure: MemoryPressure,
}

impl MemorySnapshot {
    /// Captures a snapshot without spawning another process.
    pub fn capture() -> Self {
        #[cfg(target_os = "macos")]
        return macos::memory();
        #[cfg(not(target_os = "macos"))]
        Self::default()
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{MemoryPressure, MemorySnapshot, WorkClass};

    // Values and signatures from the macOS SDK's sys/qos.h and pthread/qos.h.
    const QOS_USER_INITIATED: u32 = 0x19;
    const QOS_UTILITY: u32 = 0x11;

    unsafe extern "C" {
        fn pthread_get_qos_class_np(
            thread: libc::pthread_t,
            class: *mut u32,
            relative_priority: *mut i32,
        ) -> i32;
        fn pthread_set_qos_class_self_np(class: u32, relative_priority: i32) -> i32;
    }

    pub(super) fn class_value(class: WorkClass) -> u32 {
        match class {
            WorkClass::Interactive => QOS_USER_INITIATED,
            WorkClass::Maintenance => QOS_UTILITY,
        }
    }

    fn current() -> Option<(u32, i32)> {
        let (mut class, mut priority) = (0, 0);
        // SAFETY: pthread_self is the current live thread; both output pointers
        // refer to initialized stack values of the SDK's expected types.
        let result =
            unsafe { pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut priority) };
        (result == 0 && class != 0).then_some((class, priority))
    }

    pub(super) fn set(class: u32, priority: i32) -> bool {
        // SAFETY: this changes only the calling thread. Class is a valid SDK
        // constant or one returned by pthread_get_qos_class_np.
        let result = unsafe { pthread_set_qos_class_self_np(class, priority) };
        if result != 0 {
            log::debug!("CPU QoS request failed: class={class} error={result}");
        }
        result == 0
    }

    pub(super) fn enter(class: WorkClass) -> Option<(u32, i32)> {
        let previous = current()?;
        let requested = (class_value(class), 0);
        if previous == requested {
            return None;
        }
        set(requested.0, requested.1).then_some(previous)
    }

    fn pressure(value: i32) -> MemoryPressure {
        // macOS memory pressure notifications use NORMAL=1, WARN=2, CRITICAL=4.
        match value {
            1 => MemoryPressure::Normal,
            2 => MemoryPressure::Warning,
            4 => MemoryPressure::Critical,
            _ => MemoryPressure::Unknown,
        }
    }

    pub(super) fn memory() -> MemorySnapshot {
        let mut snapshot = MemorySnapshot::default();
        let mut value = 0i32;
        let mut size = std::mem::size_of_val(&value);
        // SAFETY: the name is NUL-terminated, the output buffer has the stated
        // size, and null newp with zero newlen performs a read-only query.
        let result = unsafe {
            libc::sysctlbyname(
                c"kern.memorystatus_vm_pressure_level".as_ptr(),
                (&mut value as *mut i32).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if result == 0 && size == std::mem::size_of_val(&value) {
            snapshot.pressure = pressure(value);
        }
        let mut usage = std::mem::MaybeUninit::<libc::rusage_info_v0>::zeroed();
        // SAFETY: RUSAGE_INFO_V0 writes exactly rusage_info_v0 to a properly
        // aligned buffer. Reading it is guarded by a successful return code.
        let result = unsafe {
            libc::proc_pid_rusage(
                libc::getpid(),
                libc::RUSAGE_INFO_V0,
                usage.as_mut_ptr().cast(),
            )
        };
        if result == 0 {
            // SAFETY: proc_pid_rusage successfully initialized the buffer above.
            let usage = unsafe { usage.assume_init() };
            snapshot.resident_bytes = Some(usage.ri_resident_size);
            snapshot.physical_footprint_bytes = Some(usage.ri_phys_footprint);
        }
        snapshot
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::performance::QosGuard;

        #[test]
        fn nested_work_restores_worker_qos() {
            std::thread::spawn(|| {
                assert!(set(QOS_USER_INITIATED, 0));
                let original = current().unwrap();
                {
                    let _maintenance = QosGuard::enter(WorkClass::Maintenance);
                    assert_eq!(current(), Some((QOS_UTILITY, 0)));
                    {
                        let _interactive = QosGuard::enter(WorkClass::Interactive);
                        assert_eq!(current(), Some(original));
                    }
                    assert_eq!(current(), Some((QOS_UTILITY, 0)));
                }
                assert_eq!(current(), Some(original));
            })
            .join()
            .unwrap();
        }

        #[test]
        fn pressure_values_do_not_guess_unknown_states() {
            assert_eq!(pressure(1), MemoryPressure::Normal);
            assert_eq!(pressure(2), MemoryPressure::Warning);
            assert_eq!(pressure(4), MemoryPressure::Critical);
            assert_eq!(pressure(0), MemoryPressure::Unknown);
            assert_eq!(pressure(3), MemoryPressure::Unknown);
        }

        #[test]
        fn process_memory_is_observable() {
            let snapshot = memory();
            assert!(snapshot.resident_bytes.is_some_and(|bytes| bytes > 0));
            assert!(
                snapshot
                    .physical_footprint_bytes
                    .is_some_and(|bytes| bytes > 0)
            );
        }
    }
}
