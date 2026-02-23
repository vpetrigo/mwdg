//! # mwdg-ffi — C FFI bindings for the mwdg micro-watchdog library
//!
//! This crate provides C-compatible bindings for the [`mwdg`] library,
//! enabling use from C/C++ code. Use the generated `include/mwdg.h` header
//! for the proper interface declarations.
//!
//! A user of the library must provide the following functions:
//!
//! ```c
//! extern uint32_t mwdg_get_time_milliseconds(void);
//! extern void mwdg_enter_critical(void);
//! extern void mwdg_exit_critical(void);
//! ```
#![no_std]

#[cfg(feature = "pack")]
use core::panic::PanicInfo;

use core::cell::UnsafeCell;
use core::pin::Pin;
use core::ptr;

use mwdg::{WatchdogNode, WatchdogRegistry};

unsafe extern "C" {
    /// User-provided function that returns the current time in milliseconds.
    fn mwdg_get_time_milliseconds() -> u32;
    /// User-provided function to enter a critical section.
    fn mwdg_enter_critical();
    /// User-provided function to exit a critical section.
    fn mwdg_exit_critical();
}

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

    /// User-assigned identifier for this watchdog node.
    /// Set via [`mwdg_assign_id`]. Defaults to `0`.
    /// The library never modifies this field; it is purely for the user's
    /// benefit when identifying expired nodes via [`mwdg_get_next_expired`].
    id: u32,

    /// Intrusive linked-list pointer to the next registered watchdog.
    /// Null if this is the tail of the list.
    next: *mut mwdg_node,
}

impl Default for mwdg_node {
    fn default() -> Self {
        Self {
            timeout_interval_ms: 0,
            last_touched_timestamp_ms: 0,
            id: 0,
            next: ptr::null_mut(),
        }
    }
}

// `WatchdogNode` is `#[repr(C)]` with fields (u32, u32, u32, *mut Self,
// PhantomPinned). `PhantomPinned` is a ZST with alignment 1, so it does not
// affect the `repr(C)` layout. The first four fields are identical in type and
// order to `mwdg_node`, therefore the two types share the same size and
// alignment. Casting `*mut mwdg_node` ↔ `*mut WatchdogNode` is sound.
const _: () = assert!(
    core::mem::size_of::<mwdg_node>() == core::mem::size_of::<WatchdogNode>(),
    "mwdg_node and WatchdogNode must have the same size"
);
const _: () = assert!(
    core::mem::align_of::<mwdg_node>() == core::mem::align_of::<WatchdogNode>(),
    "mwdg_node and WatchdogNode must have the same alignment"
);

/// Cast a `*mut mwdg_node` to `*mut WatchdogNode`.
///
/// # Safety
/// The caller must ensure the pointer is either null or points to a valid
/// `mwdg_node`. The layout of `mwdg_node` and `WatchdogNode` is verified
/// at compile time to be identical.
#[inline]
unsafe fn cast_node(ptr: *mut mwdg_node) -> *mut WatchdogNode {
    ptr.cast::<WatchdogNode>()
}

/// Create a `Pin<&mut WatchdogNode>` from a raw `*mut mwdg_node`.
///
/// Returns `None` if the pointer is null.
///
/// # Safety
/// The caller must ensure the pointer is valid, properly aligned, and
/// the pointed-to node will not be moved for the duration of the
/// returned reference's lifetime.
#[inline]
unsafe fn pin_node_mut<'a>(ptr: *mut mwdg_node) -> Option<Pin<&'a mut WatchdogNode>> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: ptr is non-null, cast is layout-compatible (verified at compile
    // time), and the caller guarantees validity. Pin is safe because FFI
    // callers must not move the node while registered.
    unsafe { Some(Pin::new_unchecked(&mut *cast_node(ptr))) }
}

/// Wrapper to allow `WatchdogRegistry` in a `static`.
///
/// # Safety
/// All access to the inner state is protected by the user-provided
/// critical section callbacks (enter/exit). `mwdg_init` must be called
/// once from a single context before any other function.
struct GlobalState(UnsafeCell<WatchdogRegistry>);

// SAFETY: All access is gated by user-provided critical section.
unsafe impl Sync for GlobalState {}

static STATE: GlobalState = GlobalState(UnsafeCell::new(WatchdogRegistry::new()));

impl GlobalState {
    #[allow(clippy::mut_from_ref)]
    fn as_mut(&self) -> &mut WatchdogRegistry {
        unsafe { &mut *self.0.get() }
    }

    fn as_ref(&self) -> &WatchdogRegistry {
        unsafe { &*self.0.get() }
    }
}

/// Execute `f` inside the user-provided critical section.
#[inline]
fn with_critical_section<R>(f: impl FnOnce(&mut WatchdogRegistry) -> R) -> R {
    let state = STATE.as_mut();
    unsafe { mwdg_enter_critical() };
    let result = f(state);
    unsafe { mwdg_exit_critical() };
    result
}

/// Initialize the multi-watchdog subsystem.
///
/// Must be called exactly once before any other `mwdg_*` function,
/// from a single execution context (e.g., main or init task).
///
/// # Safety
/// - Must be called before any other `mwdg_*` function.
/// - Must not be called from multiple threads concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mwdg_init() {
    STATE.as_mut().init();
}

/// Register a software watchdog with the given timeout.
///
/// Initializes the watchdog fields and prepends it to the global list.
/// The watchdog's `last_touched_timestamp_ms` is set to the current time.
///
/// If the node is already in the list (detected by pointer comparison), the
/// call acts as a combined feed + timeout update — the node is **not** added
/// a second time.
///
/// # Parameters
/// - `wdg`: pointer to a caller-owned [`mwdg_node`]. Must remain valid
///   (not dropped/freed) for as long as it is registered.
/// - `timeout_ms`: the timeout interval in milliseconds.
///
/// # Safety
/// - `wdg` must be a valid, non-null pointer to a `mwdg_node`.
/// - `mwdg_init` must have been called.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mwdg_add(wdg: *mut mwdg_node, timeout_ms: u32) {
    let Some(pinned) = (unsafe { pin_node_mut(wdg) }) else {
        return;
    };

    with_critical_section(|registry| {
        let now = unsafe { mwdg_get_time_milliseconds() };
        registry.add(pinned, timeout_ms, now);
    });
}

/// Remove a previously registered watchdog from the global list.
///
/// If `wdg` is null or the node is not found in the list, the function
/// returns silently.
///
/// # Safety
/// - `wdg` must be either null or a valid pointer to an `mwdg_node`.
/// - `mwdg_init` must have been called.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mwdg_remove(wdg: *mut mwdg_node) {
    let Some(pinned) = (unsafe { pin_node_mut(wdg) }) else {
        return;
    };

    with_critical_section(|registry| {
        registry.remove(pinned);
    });
}

/// Feed (touch) a watchdog, resetting its timestamp to the current time.
///
/// Must be called periodically by the owning task to signal liveness.
///
/// # Safety
/// - `wdg` must be a valid, non-null pointer to a registered `mwdg_node`.
/// - `mwdg_init` must have been called.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mwdg_feed(wdg: *mut mwdg_node) {
    let Some(pinned) = (unsafe { pin_node_mut(wdg) }) else {
        return;
    };

    with_critical_section(|_| {
        let now = unsafe { mwdg_get_time_milliseconds() };
        WatchdogRegistry::feed(pinned, now);
    });
}

/// Assign a user-chosen identifier to a watchdog node.
///
/// The identifier is stored in the node and can be retrieved later via
/// [`mwdg_get_next_expired`] to determine which watchdog(s) have expired.
/// The library never modifies this field internally; it is purely for the
/// caller's use.
///
/// This function may be called at any time — before or after [`mwdg_add`].
///
/// # Parameters
/// - `wdg`: pointer to a caller-owned [`mwdg_node`].
/// - `id`: the identifier to assign.
///
/// # Safety
/// - `wdg` must be either null or a valid pointer to an `mwdg_node`.
/// - `mwdg_init` must have been called.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mwdg_assign_id(wdg: *mut mwdg_node, id: u32) {
    let Some(pinned) = (unsafe { pin_node_mut(wdg) }) else {
        return;
    };

    with_critical_section(|_| {
        WatchdogRegistry::assign_id(pinned, id);
    });
}

/// Check all registered watchdogs for expiration.
///
/// Iterates the linked list of registered watchdogs. For each one,
/// computes elapsed time using wrapping arithmetic (safe across `u32` overflow)
/// and compares against the timeout interval.
///
/// # Returns
/// - `0` if all watchdogs are healthy (fed within their timeout).
/// - `1` if any watchdog has expired.
///
/// # Safety
/// - `mwdg_init` must have been called.
/// - All registered `mwdg_node` pointers must still be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mwdg_check() -> i32 {
    // Fast path: if already expired, skip the critical section entirely.
    // This is safe because `expired` is only ever set from false to true
    // (monotonic / latching) inside the critical section, so a stale read
    // of `true` is always correct.
    if STATE.as_ref().is_expired() {
        return 1;
    }

    with_critical_section(|registry| {
        let now = unsafe { mwdg_get_time_milliseconds() };
        i32::from(registry.check(now))
    })
}

/// Iterate over registered watchdogs and find the next expired one.
///
/// This function implements a cursor-based iterator over the linked list of
/// registered watchdogs.  On each call it resumes from the position stored in
/// `*cursor` and scans forward for the next node whose elapsed time exceeds
/// its timeout interval.
///
/// # Precondition
/// [`mwdg_check`] must have been called **and returned `1`** before using
/// this function.  Internally the iterator uses the timestamp snapshot
/// captured by `mwdg_check` (`expired_at_ms`) to evaluate each node, so
/// nodes are compared against the same point in time that triggered the
/// expiration — even if a frozen task calls [`mwdg_feed`] between
/// `mwdg_check` and this function.  If `mwdg_check` has not yet detected
/// an expiration the function returns `0` immediately.
///
/// # Usage (C)
/// ```c
/// if (mwdg_check() != 0) {
///     struct mwdg_node *cursor = NULL;
///     uint32_t id;
///     while (mwdg_get_next_expired(&cursor, &id)) {
///         printf("expired watchdog id: %u\n", id);
///     }
/// }
/// ```
///
/// # Parameters
/// - `cursor`: pointer to a `*mut mwdg_node` that tracks iteration state.
///   The caller must initialise `*cursor` to `NULL` before the first call.
///   The function advances `*cursor` to the found node on success.
/// - `out_id`: pointer to a `u32` where the expired node's identifier
///   (set via [`mwdg_assign_id`]) will be written on success.
///
/// # Returns
/// - `1` if an expired node was found (`*out_id` is written, `*cursor` is
///   advanced).
/// - `0` when no more expired nodes remain (iteration complete), when
///   [`mwdg_check`] has not detected an expiration, or if `cursor` or
///   `out_id` is null.
///
/// # Note
/// Each call enters and exits the critical section independently. If the
/// list is modified between calls the iterator may skip or revisit nodes.
/// In typical RTOS usage the check loop runs from a single supervisory task,
/// so this is not a concern.
///
/// # Safety
/// - `cursor` must be either null or a valid pointer to a `*mut mwdg_node`.
/// - `out_id` must be either null or a valid pointer to a `u32`.
/// - `mwdg_init` must have been called.
/// - All registered `mwdg_node` pointers must still be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mwdg_get_next_expired(
    cursor: *mut *mut mwdg_node,
    out_id: *mut u32,
) -> i32 {
    if cursor.is_null() || out_id.is_null() {
        return 0;
    }

    with_critical_section(|registry| {
        // Convert the C cursor (*mut *mut mwdg_node) to our internal cursor
        // (*const WatchdogNode).
        let mut internal_cursor: *const WatchdogNode = if unsafe { (*cursor).is_null() } {
            ptr::null()
        } else {
            unsafe { cast_node(*cursor).cast_const() }
        };

        match registry.next_expired(&mut internal_cursor) {
            Some(id) => {
                unsafe {
                    *out_id = id;
                    // Cast back: internal_cursor points to a WatchdogNode
                    // which is layout-compatible with mwdg_node.
                    *cursor = internal_cursor.cast_mut().cast::<mwdg_node>();
                }
                1
            }
            None => 0,
        }
    })
}
