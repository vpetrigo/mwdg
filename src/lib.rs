//! # mwdg - Micro-Watchdog Library
//!
//! A `no_std` software multi-watchdog library for embedded RTOS systems.
//!
//! Each RTOS task registers a [`mwdg_node`] with a timeout interval.
//! The task periodically calls [`mwdg_feed`] to signal liveness.
//! A central [`mwdg_check`] function verifies all registered watchdogs
//! are healthy, enabling the main loop to gate hardware watchdog resets.
//!
//! ## C FFI
//!
//! All public functions declared to be exposed without mangling, so the library can be
//! linked from C/C++ code. Use the generated `include/mwdg.h` header for having proper
//! interface declaration.
//!
//! A user of the library must provide the following functions that the library uses
//! to get system timestamp in milliseconds, enter/exit critical sections.
//!
//! ```c++
//! extern uint32_t mwdg_get_time_milliseconds(void);
//! extern void mwdg_enter_critical(void);
//! extern void mwdg_exit_critical(void);
//! ```
#![no_std]

use core::cell::UnsafeCell;
#[cfg(feature = "pack")]
use core::panic::PanicInfo;
use core::ptr;

mod api;
mod critical;
mod external;

pub use api::*;

#[cfg(feature = "pack")]
#[inline(never)]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// A single software watchdog node.
///
/// Each RTOS task owns one of these (typically as a static or stack variable
/// in a long-lived task). The struct is an intrusive linked-list node.
///
/// # C Usage
/// ```c
/// static struct mwdg_node my_wdg;
/// mwdg_add(&my_wdg, 200); // 200 ms timeout
/// ```
#[repr(C)]
pub struct mwdg_node {
    /// Timeout interval in milliseconds. Set during [`mwdg_add`].
    /// Treat as read-only after registration.
    timeout_interval_ms: u32,

    /// Timestamp (ms) of the last feed. Updated by [`mwdg_feed`].
    last_touched_timestamp_ms: u32,

    /// Intrusive linked-list pointer to the next registered watchdog.
    /// Null if this is the tail of the list.
    next: *mut mwdg_node,
}

impl Default for mwdg_node {
    fn default() -> Self {
        Self {
            timeout_interval_ms: 0,
            last_touched_timestamp_ms: 0,
            next: ptr::null_mut(),
        }
    }
}

/// All mutable global state for the library, collected in one place.
struct MwdgState {
    /// Head of the intrusive linked list of registered watchdogs.
    head: *mut mwdg_node,
    /// Whether any of registered WDGs is expired
    expired: bool,
}

/// Wrapper to allow `MwdgState` in a `static`.
///
/// # Safety
/// All access to the inner state is protected by the user-provided
/// critical section callbacks (enter/exit). `mwdg_init` must be called
/// once from a single context before any other function.
struct GlobalState(UnsafeCell<MwdgState>);

// SAFETY: All access is gated by user-provided critical section.
unsafe impl Sync for GlobalState {}

static STATE: GlobalState = GlobalState(UnsafeCell::new(MwdgState {
    head: ptr::null_mut(),
    expired: false,
}));

impl GlobalState {
    #[allow(clippy::mut_from_ref)]
    fn as_mut(&self) -> &mut MwdgState {
        unsafe { &mut *self.0.get() }
    }

    fn as_ref(&self) -> &MwdgState {
        self.as_mut()
    }
}

#[cfg(test)]
mod tests {
    use core::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    static MOCK_TIME: AtomicU32 = AtomicU32::new(0);

    fn mock_get_time_ms() -> u32 {
        MOCK_TIME.load(Ordering::Relaxed)
    }

    fn mock_enter_critical() {
        // no-op for single-threaded tests
    }

    fn mock_exit_critical() {
        // no-op for single-threaded tests
    }

    /// User-provided function that returns the current time in milliseconds.
    #[unsafe(no_mangle)]
    pub extern "C" fn mwdg_get_time_milliseconds() -> u32 {
        mock_get_time_ms()
    }
    /// User-provided function to enter a critical section.
    #[unsafe(no_mangle)]
    pub extern "C" fn mwdg_enter_critical() {
        mock_enter_critical();
    }
    /// User-provided function to exit a critical section.
    #[unsafe(no_mangle)]
    pub extern "C" fn mwdg_exit_critical() {
        mock_exit_critical();
    }

    fn set_time(ms: u32) {
        MOCK_TIME.store(ms, Ordering::Relaxed);
    }

    /// Reset global state between tests (since tests share the static).
    fn reset() {
        set_time(0);
        mwdg_init();
    }

    fn count_nodes_in_list(head: *const mwdg_node) -> u32 {
        let mut counter = 0u32;
        let mut current = head;

        while !current.is_null() {
            counter += 1;
            current = unsafe { current.as_ref() }.unwrap().next;
        }

        counter
    }

    #[test]
    fn test_internal_state_single_node_add() {
        reset();
        let mut wdg = mwdg_node::default();
        mwdg_add(&mut wdg, 1);
        let counter = count_nodes_in_list(STATE.as_ref().head);
        assert_eq!(1, counter, "Invalid number of nodes");
    }

    #[test]
    fn test_internal_state_multiple_nodes_add() {
        reset();

        let mut wdg1 = mwdg_node::default();
        let mut wdg2 = mwdg_node::default();
        let mut wdg3 = mwdg_node::default();

        mwdg_add(&mut wdg1, 1);
        mwdg_add(&mut wdg2, 2);
        mwdg_add(&mut wdg3, 3);

        let counter = count_nodes_in_list(STATE.as_ref().head);
        assert_eq!(3, counter, "Invalid number of nodes");
    }

    #[test]
    fn test_internal_state_multiple_nodes_add_multiple_remove() {
        reset();

        let mut wdg1 = mwdg_node::default();
        let mut wdg2 = mwdg_node::default();
        let mut wdg3 = mwdg_node::default();

        mwdg_add(&mut wdg1, 1);
        mwdg_add(&mut wdg1, 1);
        mwdg_add(&mut wdg1, 1);
        mwdg_add(&mut wdg2, 2);
        mwdg_add(&mut wdg2, 2);
        mwdg_add(&mut wdg2, 2);
        mwdg_add(&mut wdg3, 3);
        mwdg_add(&mut wdg3, 3);
        mwdg_add(&mut wdg3, 3);

        let counter = count_nodes_in_list(STATE.as_ref().head);
        assert_eq!(3, counter, "Invalid number of nodes");

        mwdg_remove(&mut wdg3);
        mwdg_remove(&mut wdg3);
        mwdg_remove(&mut wdg3);

        let counter = count_nodes_in_list(STATE.as_ref().head);
        assert_eq!(2, counter, "Invalid number of nodes");

        mwdg_remove(&mut wdg1);
        mwdg_remove(&mut wdg1);
        mwdg_remove(&mut wdg1);

        let counter = count_nodes_in_list(STATE.as_ref().head);
        assert_eq!(1, counter, "Invalid number of nodes");

        mwdg_remove(&mut wdg2);
        mwdg_remove(&mut wdg2);
        mwdg_remove(&mut wdg2);

        let counter = count_nodes_in_list(STATE.as_ref().head);
        assert_eq!(0, counter, "Invalid number of nodes");
    }

    #[test]
    fn test_register_sets_fields() {
        reset();
        set_time(42);
        let mut wdg = mwdg_node::default();

        mwdg_add(&mut wdg, 250);
        assert_eq!(wdg.timeout_interval_ms, 250);
        assert_eq!(wdg.last_touched_timestamp_ms, 42);
    }

    #[test]
    fn test_feed_updates_timestamp() {
        reset();
        set_time(100);
        let mut wdg = mwdg_node::default();

        mwdg_add(&mut wdg, 500);
        assert_eq!(wdg.last_touched_timestamp_ms, 100);

        set_time(350);

        mwdg_feed(&mut wdg);
        assert_eq!(wdg.last_touched_timestamp_ms, 350);
    }
}
