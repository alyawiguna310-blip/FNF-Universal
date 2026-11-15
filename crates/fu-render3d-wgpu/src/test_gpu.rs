//! Serializes GPU device creation in tests.
//!
//! Several tests in this crate create a headless `wgpu` device, and Rust runs
//! test functions in parallel threads by default. Creating those devices
//! concurrently corrupts the heap inside the platform graphics stack on some
//! drivers (observed on Windows as `STATUS_HEAP_CORRUPTION`), which aborts the
//! whole test binary and hides unrelated failures.
//!
//! Tests that need a device take [`gpu_lock`] first, so device creation is
//! serialized while the remaining tests stay parallel. This is test scaffolding
//! only: the runtime backend is unchanged.

use std::sync::{Mutex, MutexGuard, OnceLock};

/// Blocks until this process is the only one creating or using a test device.
///
/// A test that panics while holding the lock poisons it; the poisoning is
/// deliberately ignored so one failure does not cascade into the others.
pub(crate) fn gpu_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
