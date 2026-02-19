//! # mwdg - Micro-Watchdog Library
//!
//! A `no_std` software multi-watchdog library for embedded RTOS systems.
//!
//! Each RTOS task registers a [`SoftwareWdg`] with a timeout interval.
//! The task periodically calls [`mwdg_feed`] to signal liveness.
//! A central [`mwdg_check`] function verifies all registered watchdogs
//! are healthy, enabling the main loop to gate hardware watchdog resets.
//!
//! ## C FFI
//!
//! All public functions use `#[unsafe(no_mangle)] extern "C"` and the struct uses
//! `#[repr(C)]`, so the library can be linked from C/C++ code. Use the
//! generated `include/mwdg.h` header.
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
    pub timeout_interval_ms: u32,

    /// Timestamp (ms) of the last feed. Updated by [`mwdg_feed`].
    pub last_touched_timestamp_ms: u32,

    /// Intrusive linked-list pointer to the next registered watchdog.
    /// Null if this is the tail of the list.
    pub next: *mut mwdg_node,
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
