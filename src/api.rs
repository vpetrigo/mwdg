use core::ptr;

use crate::critical::with_critical_section;
use crate::external::mwdg_get_time_milliseconds;
use crate::{STATE, mwdg_node};

/// Convert a raw `*mut mwdg_node` to an optional mutable reference.
///
/// # Safety
/// The function performs an unchecked conversion from a raw pointer to a
/// mutable reference inside an `unsafe` block after verifying the pointer is
/// not null. The caller must ensure that:
/// - `wdg` points to a valid, properly initialized `mwdg_node` for the
///   requested lifetime `'a`.
/// - The pointer is properly aligned for `mwdg_node`.
/// - The pointer is unique for the duration of the returned reference (no
///   other mutable or immutable references alias the same memory).
/// - The memory is not freed or moved while the returned reference is in use.
///
/// This function only checks for null; it does not perform any other runtime
/// validation.
fn get_wdg_node_ref<'a>(wdg: *mut mwdg_node) -> Option<&'a mut mwdg_node> {
    if wdg.is_null() {
        return None;
    }
    // SAFETY: raw pointer checked for being non-null; caller must uphold
    // the additional safety requirements documented above.
    unsafe { Some(&mut *wdg) }
}

fn touch_wdg_node(wdg: &mut mwdg_node) {
    let now = unsafe { mwdg_get_time_milliseconds() };

    wdg.last_touched_timestamp_ms = now;
}

/// Initialize the multi-watchdog subsystem.
///
/// Must be called exactly once before any other `mwdg_*` function,
/// from a single execution context (e.g., main or init task).
///
/// # Safety
/// - All three function pointers must be non-null and valid for the lifetime of the program.
/// - Must be called before any other `mwdg_*` function.
#[unsafe(no_mangle)]
pub extern "C" fn mwdg_init() {
    let state = STATE.as_mut();
    state.head = ptr::null_mut();
    state.expired = false;
}

/// Register a software watchdog with the given timeout.
///
/// Initializes the watchdog fields and prepends it to the global list.
/// The watchdog's `last_touched_timestamp_ms` is set to the current time.
///
/// # Parameters
/// - `wdg`: pointer to a caller-owned [`mwdg_node`]. Must remain valid
///   (not dropped/freed) for as long as it is registered.
/// - `timeout_ms`: the timeout interval in milliseconds. If the watchdog
///   is not fed within this interval, [`mwdg_check`] will report a failure.
///
/// # Safety
/// - `wdg` must be a valid, non-null pointer to a `SoftwareWdg`.
/// - The pointed-to `SoftwareWdg` must not already be registered.
/// - `mwdg_init` must have been called.
#[unsafe(no_mangle)]
pub extern "C" fn mwdg_add(wdg: *mut mwdg_node, timeout_ms: u32) {
    if wdg.is_null() {
        return;
    }

    let is_present = with_critical_section(|state| {
        let mut current = state.head;
        let mut present = false;

        while !current.is_null() {
            if wdg == current {
                present = true;
                break;
            }

            let current_ref = unsafe { &mut *current };
            current = current_ref.next;
        }

        present
    });

    if is_present {
        with_critical_section(|_| {
            if let Some(node) = get_wdg_node_ref(wdg) {
                touch_wdg_node(node);
                node.timeout_interval_ms = timeout_ms;
            }
        });

        return;
    }

    with_critical_section(|state| {
        if let Some(node) = get_wdg_node_ref(wdg) {
            touch_wdg_node(node);
            node.timeout_interval_ms = timeout_ms;
            node.next = state.head;
            state.head = wdg;
        }
    });
}

/// Remove a previously registered watchdog from the global list.
///
/// This function takes a caller-owned pointer to an `mwdg_node` and removes
/// the corresponding node from the internal singly-linked list of registered
/// watchdogs. The operation is performed inside a critical section to avoid
/// concurrent list mutations. After successful removal the node's `next`
/// pointer is cleared (`ptr::null_mut()`), which marks the node as not
/// registered.
///
/// If `wdg` is null or the node is not found in the list, the function
/// returns silently and no state is modified.
///
/// # Parameters
/// - `wdg`: pointer to a caller-owned `mwdg_node` that was previously added
///   via `mwdg_add`.
///
/// # Safety
/// - `wdg` must be either a null pointer or a valid pointer to an
///   `mwdg_node` that is not concurrently accessed elsewhere while this
///   function runs.
/// - The pointed-to `mwdg_node` must remain valid (not freed or moved) for
///   the duration of the call.
/// - `mwdg_init` must have been called prior to invoking this function.
///
/// The function only checks for `null` and membership in the internal list;
/// it does not otherwise validate the memory referenced by `wdg`.
#[unsafe(no_mangle)]
pub extern "C" fn mwdg_remove(wdg: *mut mwdg_node) {
    with_critical_section(|state| {
        if wdg.is_null() {
            return;
        }

        let mut prev = ptr::null_mut::<mwdg_node>();
        let mut current = unsafe { state.head.as_mut() };

        while let Some(current_node) = current {
            let current_ptr = core::ptr::from_mut(current_node);

            if current_ptr == wdg {
                if prev.is_null() {
                    state.head = current_node.next;
                } else {
                    // SAFETY: `prev` was set to a pointer to a previous `current`
                    // node which was checked to be non-null in an earlier loop
                    // iteration.
                    let prev_ref = unsafe { &mut *prev };

                    prev_ref.next = current_node.next;
                }

                // SAFETY: `wdg` is non-null (checked above) and matched an
                // entry in the list, so `get_wdg_node_ref(wdg)` is expected
                // to return `Some`; unwrap_unchecked is used for minimal
                // runtime overhead in this hotpath.
                let node = unsafe { get_wdg_node_ref(wdg).unwrap_unchecked() };

                node.next = ptr::null_mut();
                break;
            }

            prev = current_ptr;
            current = unsafe { current_node.next.as_mut() };
        }
    });
}

/// Feed (touch) a watchdog, resetting its timestamp to the current time.
///
/// Must be called periodically by the owning task to signal liveness.
///
/// # Safety
/// - `wdg` must be a valid, non-null pointer to a registered `struct mwdg_node`.
/// - `mwdg_init` must have been called.
#[unsafe(no_mangle)]
pub extern "C" fn mwdg_feed(wdg: *mut mwdg_node) {
    with_critical_section(|_| {
        if let Some(node) = get_wdg_node_ref(wdg) {
            touch_wdg_node(node);
        }
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
/// - All registered `struct mwdg_node` pointers must still be valid.
#[unsafe(no_mangle)]
pub extern "C" fn mwdg_check() -> i32 {
    let state = STATE.as_ref();

    if state.expired {
        return 1;
    }

    with_critical_section(|state| {
        let now = unsafe { mwdg_get_time_milliseconds() };
        let mut current = state.head;

        while let Some(node) = unsafe { current.as_mut() } {
            let elapsed = now.wrapping_sub(node.last_touched_timestamp_ms);

            if elapsed > node.timeout_interval_ms {
                state.expired = true;
                return 1;
            }

            current = node.next;
        }

        0
    })
}
