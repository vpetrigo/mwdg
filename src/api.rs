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

/// Convert a raw `*mut *mut mwdg_node` to an optional mutable reference
/// to the inner `*mut mwdg_node`.
///
/// # Safety
/// Same guarantees as [`get_wdg_node_ref`] but for a double-pointer.
fn get_cursor_ref<'a>(cursor: *mut *mut mwdg_node) -> Option<&'a mut *mut mwdg_node> {
    if cursor.is_null() {
        return None;
    }
    unsafe { Some(&mut *cursor) }
}

/// Convert a raw `*mut u32` to an optional mutable reference.
///
/// # Safety
/// Same guarantees as [`get_wdg_node_ref`] but for a `u32`.
fn get_u32_ref<'a>(ptr: *mut u32) -> Option<&'a mut u32> {
    if ptr.is_null() {
        return None;
    }
    unsafe { Some(&mut *ptr) }
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
/// - Must be called before any other `mwdg_*` function.
/// - Must not be called from multiple threads
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mwdg_init() {
    let state = STATE.as_mut();
    state.head = ptr::null_mut();
    state.expired = false;
    state.expired_at_ms = 0;
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
pub unsafe extern "C" fn mwdg_add(wdg: *mut mwdg_node, timeout_ms: u32) {
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
pub unsafe extern "C" fn mwdg_remove(wdg: *mut mwdg_node) {
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
pub unsafe extern "C" fn mwdg_feed(wdg: *mut mwdg_node) {
    with_critical_section(|_| {
        if let Some(node) = get_wdg_node_ref(wdg) {
            touch_wdg_node(node);
        }
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
    with_critical_section(|_| {
        if let Some(node) = get_wdg_node_ref(wdg) {
            node.id = id;
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
pub unsafe extern "C" fn mwdg_check() -> i32 {
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
                state.expired_at_ms = now;
                return 1;
            }

            current = node.next;
        }

        0
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
/// - All registered `struct mwdg_node` pointers must still be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mwdg_get_next_expired(
    cursor: *mut *mut mwdg_node,
    out_id: *mut u32,
) -> i32 {
    let (Some(cursor_ref), Some(out_id_ref)) = (get_cursor_ref(cursor), get_u32_ref(out_id)) else {
        return 0;
    };

    with_critical_section(|state| {
        // Nothing to report if mwdg_check has not detected an expiration.
        if !state.expired {
            return 0;
        }

        // Use the timestamp captured by mwdg_check rather than reading the
        // clock again. This ensures nodes are evaluated against the same
        // point in time that triggered the expiration, even if a task has
        // called mwdg_feed in the meantime.
        let now = state.expired_at_ms;

        // Determine start position: if *cursor is NULL we start from
        // the head of the list; otherwise from the node after *cursor.
        let start = if (*cursor_ref).is_null() {
            state.head
        } else {
            // SAFETY: *cursor_ref is non-null and was previously set by this
            // function to point to a valid registered node.
            unsafe { (**cursor_ref).next }
        };

        let mut current = start;

        while let Some(node) = unsafe { current.as_mut() } {
            let elapsed = now.wrapping_sub(node.last_touched_timestamp_ms);

            if elapsed > node.timeout_interval_ms {
                *out_id_ref = node.id;
                *cursor_ref = current;
                return 1;
            }

            current = node.next;
        }

        0
    })
}
