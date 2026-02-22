//! # mwdg — Micro-Watchdog Library
//!
//! A `no_std` software multi-watchdog library for embedded RTOS systems.
//!
//! Each RTOS task registers a [`WatchdogNode`] with a timeout interval via
//! [`WatchdogRegistry::add`]. The task periodically calls
//! [`WatchdogRegistry::feed`] to signal liveness. A central
//! [`WatchdogRegistry::check`] method verifies all registered watchdogs are
//! healthy, enabling the main loop to gate hardware watchdog resets.
//!
//! ## Design
//!
//! The registry maintains an intrusive singly-linked list of watchdog nodes.
//! All raw-pointer manipulation is encapsulated behind a safe public API:
//!
//! - [`Pin<&mut WatchdogNode>`] prevents nodes from being moved while they
//!   are linked into the list.
//! - `&mut self` on registry methods ensures exclusive access, eliminating
//!   data races without requiring a critical section in the library itself.
//!
//! This crate contains **no** global state, **no** C FFI, and **no** unsafe
//! in its public interface. The companion `mwdg-ffi` crate provides the C
//! shim layer on top of this API.

#![no_std]

use core::marker::PhantomPinned;
use core::pin::Pin;
use core::ptr;

/// A single software watchdog node.
///
/// Each RTOS task owns one of these (typically as a `static` or a long-lived
/// stack variable). The struct is an intrusive linked-list node managed by
/// [`WatchdogRegistry`].
///
/// # Pinning
///
/// Once a node has been added to a registry it **must not** be moved in
/// memory, because the registry holds a raw pointer to it. The public API
/// enforces this by requiring [`Pin<&mut WatchdogNode>`] for all mutating
/// operations.
///
/// `WatchdogNode` deliberately implements `!Unpin` (via [`PhantomPinned`]) so
/// that [`Pin`] provides its full move-prevention guarantee.
///
/// ```compile_fail
/// fn assert_unpin<T: Unpin>() {}
/// assert_unpin::<mwdg::WatchdogNode>(); // must not compile
/// ```
#[repr(C)]
pub struct WatchdogNode {
    /// Timeout interval in milliseconds. Set during [`WatchdogRegistry::add`].
    timeout_interval_ms: u32,

    /// Timestamp (ms) of the last feed. Updated by [`WatchdogRegistry::feed`]
    /// and [`WatchdogRegistry::add`].
    last_touched_timestamp_ms: u32,

    /// User-assigned identifier for this watchdog node.
    /// Set via [`WatchdogRegistry::assign_id`]. Defaults to `0`.
    /// The library never modifies this field internally; it is purely for the
    /// caller's benefit when identifying expired nodes via
    /// [`WatchdogRegistry::next_expired`].
    id: u32,

    /// Intrusive linked-list pointer to the next registered watchdog.
    /// Null if this node is the tail of the list or is not registered.
    next: *mut WatchdogNode,

    /// Marker to make `WatchdogNode` `!Unpin`, so that [`Pin`] actually
    /// prevents moves in safe code.
    _pin: PhantomPinned,
}

impl Default for WatchdogNode {
    fn default() -> Self {
        Self {
            timeout_interval_ms: 0,
            last_touched_timestamp_ms: 0,
            id: 0,
            next: ptr::null_mut(),
            _pin: PhantomPinned,
        }
    }
}

impl WatchdogNode {
    /// Returns the user-assigned identifier of this watchdog node.
    ///
    /// The identifier is set via [`WatchdogRegistry::assign_id`] and defaults
    /// to `0`.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.id
    }
}

/// Owns the head of the intrusive linked list of registered watchdog nodes
/// and tracks expiration state.
///
/// # Usage
///
/// ```rust
/// use mwdg::{WatchdogRegistry, WatchdogNode};
/// use core::pin::Pin;
///
/// let mut registry = WatchdogRegistry::new();
///
/// let mut node = WatchdogNode::default();
/// // SAFETY: we will not move `node` after pinning it.
/// let pinned = unsafe { Pin::new_unchecked(&mut node) };
/// registry.add(pinned, 200, 0);
/// ```
pub struct WatchdogRegistry {
    /// Head of the intrusive linked list of registered watchdogs.
    head: *mut WatchdogNode,
    /// Whether any registered watchdog has expired. Once set, this flag is
    /// never cleared (latching behaviour).
    expired: bool,
    /// Timestamp (ms) captured by [`check`](Self::check) at the moment it
    /// first detected an expiration. [`next_expired`](Self::next_expired)
    /// uses this snapshot instead of requiring the caller to pass `now`
    /// again, so the two methods evaluate against the same point in time.
    expired_at_ms: u32,
}

// SAFETY: `WatchdogRegistry` owns an intrusive linked list of `WatchdogNode`
// pointers. Sending the registry to another thread is safe as long as the
// nodes it points to remain valid and are not concurrently accessed — which
// is the caller's responsibility (e.g. via `Mutex<WatchdogRegistry>`).
// The raw pointers are an implementation detail; they do not alias mutable
// references in other threads.
unsafe impl Send for WatchdogRegistry {}

impl Default for WatchdogRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl WatchdogRegistry {
    /// Create a new, empty watchdog registry.
    ///
    /// No watchdogs are registered and the expiration state is clear.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            head: ptr::null_mut(),
            expired: false,
            expired_at_ms: 0,
        }
    }

    /// Re-initialize the registry, resetting it to the same state as
    /// [`new`](Self::new).
    ///
    /// Any previously registered nodes are effectively unlinked from the
    /// registry's perspective (their individual `next` pointers are **not**
    /// cleared — the caller is responsible for dropping or re-initializing
    /// them).
    pub fn init(&mut self) {
        self.head = ptr::null_mut();
        self.expired = false;
        self.expired_at_ms = 0;
    }

    /// Returns `true` if the registry has latched into the expired state.
    ///
    /// This is a cheap field read — no list traversal is performed.
    /// The companion `mwdg-ffi` crate uses this for an early-return
    /// optimisation in `mwdg_check` that avoids entering the critical
    /// section when the registry is already known to be expired.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.expired
    }

    /// Register a watchdog node with the given timeout.
    ///
    /// The node is prepended to the registry's internal linked list. Its
    /// `last_touched_timestamp_ms` is set to `now` and its timeout is set to
    /// `timeout_ms`.
    ///
    /// If the node is already present in the list (detected by raw pointer
    /// comparison), the call acts as a combined
    /// [`feed`](Self::feed) + timeout update — the node is **not** added a
    /// second time.
    ///
    /// # Parameters
    /// - `node`: a pinned mutable reference to the watchdog node.
    /// - `timeout_ms`: timeout interval in milliseconds.
    /// - `now`: the current timestamp in milliseconds.
    pub fn add(&mut self, node: Pin<&mut WatchdogNode>, timeout_ms: u32, now: u32) {
        // Obtain a raw pointer to the node. We need this for list operations.
        // SAFETY: We are not moving the node — only reading its address and
        // writing to its fields through the raw pointer. The Pin guarantee
        // ensures the caller will not move the node after this call.
        let node_ptr: *mut WatchdogNode = unsafe { &raw mut *node.get_unchecked_mut() };

        // Check if the node is already in the list.
        let mut current = self.head;
        while !current.is_null() {
            if current == node_ptr {
                // Node is already registered — update timestamp and timeout.
                // SAFETY: `node_ptr` points to a valid `WatchdogNode` that
                // is pinned and alive (the caller holds a Pin<&mut> to it).
                unsafe {
                    (*node_ptr).last_touched_timestamp_ms = now;
                    (*node_ptr).timeout_interval_ms = timeout_ms;
                }
                return;
            }
            // SAFETY: `current` is non-null and points to a valid node in
            // the list (all nodes are pinned and alive by API contract).
            current = unsafe { (*current).next };
        }

        // Node is not in the list — initialize fields and prepend.
        // SAFETY: `node_ptr` points to a valid, pinned `WatchdogNode`.
        unsafe {
            (*node_ptr).last_touched_timestamp_ms = now;
            (*node_ptr).timeout_interval_ms = timeout_ms;
            (*node_ptr).next = self.head;
        }
        self.head = node_ptr;
    }

    /// Remove a previously registered watchdog from the registry.
    ///
    /// Walks the linked list, finds the node by raw pointer address, unlinks
    /// it, and clears its `next` pointer. If the node is not found the call
    /// is a no-op.
    ///
    /// # Parameters
    /// - `node`: a pinned mutable reference to the watchdog node to remove.
    pub fn remove(&mut self, node: Pin<&mut WatchdogNode>) {
        // SAFETY: We only read the address; we do not move the node.
        let node_ptr: *mut WatchdogNode = unsafe { &raw mut *node.get_unchecked_mut() };

        let mut prev: *mut WatchdogNode = ptr::null_mut();
        let mut current = self.head;

        while !current.is_null() {
            if current == node_ptr {
                if prev.is_null() {
                    // Removing the head of the list.
                    // SAFETY: `current` (== `node_ptr`) is valid and in the
                    // list. Reading its `next` field is safe.
                    self.head = unsafe { (*current).next };
                } else {
                    // Removing from the middle or tail.
                    // SAFETY: `prev` is non-null and was set to a valid node
                    // pointer in a previous iteration. `current` is valid.
                    unsafe {
                        (*prev).next = (*current).next;
                    }
                }
                // Clear the removed node's next pointer.
                // SAFETY: `node_ptr` is valid (pinned and alive).
                unsafe {
                    (*node_ptr).next = ptr::null_mut();
                }
                return;
            }
            prev = current;
            // SAFETY: `current` is non-null, valid, and in the list.
            current = unsafe { (*current).next };
        }
    }

    /// Feed (touch) a watchdog, resetting its timestamp to `now`.
    ///
    /// Must be called periodically by the owning task to signal liveness.
    /// This is a static method — it does not require `&mut self` because it
    /// only writes to the node itself, not to the registry.
    ///
    /// # Parameters
    /// - `node`: a pinned mutable reference to the watchdog node to feed.
    /// - `now`: the current timestamp in milliseconds.
    pub fn feed(node: Pin<&mut WatchdogNode>, now: u32) {
        // SAFETY: We are writing to a field of the pinned node. We do not
        // move the node. The caller guarantees the node is alive.
        unsafe {
            node.get_unchecked_mut().last_touched_timestamp_ms = now;
        }
    }

    /// Assign a user-defined identifier to a watchdog node.
    ///
    /// The identifier can be set at any time — before or after adding the
    /// node to a registry. It is never modified by the library; it is purely
    /// for the caller to identify expired nodes via
    /// [`next_expired`](Self::next_expired).
    ///
    /// # Parameters
    /// - `node`: a pinned mutable reference to the watchdog node.
    /// - `id`: the identifier to assign.
    pub fn assign_id(node: Pin<&mut WatchdogNode>, id: u32) {
        // SAFETY: Writing to a field; not moving the node.
        unsafe {
            node.get_unchecked_mut().id = id;
        }
    }

    /// Check all registered watchdogs for expiration.
    ///
    /// Iterates the linked list of registered watchdogs. For each one,
    /// computes elapsed time using wrapping arithmetic (safe across `u32`
    /// overflow) and compares against the timeout interval.
    ///
    /// Once an expiration is detected the registry latches into the expired
    /// state: all subsequent calls return `true` without re-scanning the
    /// list, and `expired_at_ms` is frozen at the timestamp of first
    /// detection.
    ///
    /// # Parameters
    /// - `now`: the current timestamp in milliseconds.
    ///
    /// # Returns
    /// `true` if any watchdog has expired, `false` if all are healthy.
    pub fn check(&mut self, now: u32) -> bool {
        if self.expired {
            return true;
        }

        let mut current = self.head;
        while !current.is_null() {
            // SAFETY: `current` is non-null and points to a valid, pinned
            // node in the list. We only read fields — no mutation, no move.
            let node = unsafe { &*current };
            let elapsed = now.wrapping_sub(node.last_touched_timestamp_ms);

            if elapsed > node.timeout_interval_ms {
                self.expired = true;
                self.expired_at_ms = now;
                return true;
            }

            current = node.next;
        }

        false
    }

    /// Get the next expired watchdog node in the iteration.
    ///
    /// This method implements a cursor-based iterator over the linked list.
    /// On each call it resumes from the position stored in `*cursor` and
    /// scans forward for the next node whose elapsed time exceeds its
    /// timeout interval.
    ///
    /// The evaluation uses the `expired_at_ms` timestamp snapshot captured by
    /// [`check`](Self::check), so nodes are compared against the same point
    /// in time that triggered the expiration — even if a task calls
    /// [`feed`](Self::feed) between `check` and this method.
    ///
    /// # Parameters
    /// - `cursor`: a mutable reference to a raw pointer that tracks iteration
    ///   state. The caller must initialize it to [`core::ptr::null()`] before
    ///   the first call. The method advances the cursor to the found node on
    ///   success.
    ///
    /// # Returns
    /// - `Some(id)` if an expired node was found.
    /// - `None` when no more expired nodes remain, or if [`check`](Self::check)
    ///   has not yet detected an expiration.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use mwdg::WatchdogRegistry;
    /// # let mut registry = WatchdogRegistry::new();
    /// # let now = 0u32;
    /// if registry.check(now) {
    ///     let mut cursor = core::ptr::null();
    ///     while let Some(id) = registry.next_expired(&mut cursor) {
    ///         // handle expired watchdog `id`
    ///     }
    /// }
    /// ```
    pub fn next_expired(&self, cursor: &mut *const WatchdogNode) -> Option<u32> {
        if !self.expired {
            return None;
        }

        let now = self.expired_at_ms;

        // Determine start position: if cursor is null we start from the head
        // of the list; otherwise from the node after the cursor.
        let start = if (*cursor).is_null() {
            self.head.cast_const()
        } else {
            // SAFETY: `*cursor` is non-null and was previously set by this
            // method to point to a valid registered node.
            unsafe { (*(*cursor)).next.cast_const() }
        };

        let mut current = start;
        while !current.is_null() {
            // SAFETY: `current` is non-null and points to a valid, pinned
            // node in the list. We only read fields.
            let node = unsafe { &*current };
            let elapsed = now.wrapping_sub(node.last_touched_timestamp_ms);

            if elapsed > node.timeout_interval_ms {
                *cursor = current;
                return Some(node.id);
            }

            current = node.next.cast_const();
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    /// Helper: create a pinned mutable reference from a mutable reference.
    ///
    /// # Safety
    /// The caller must not move the referenced value after calling this.
    /// In tests we own the nodes on the stack and never move them, so this
    /// is safe.
    unsafe fn pin_mut(node: &mut WatchdogNode) -> Pin<&mut WatchdogNode> {
        unsafe { Pin::new_unchecked(node) }
    }

    /// Helper: count nodes reachable from `head`.
    fn count_nodes(head: *const WatchdogNode) -> u32 {
        let mut count = 0u32;
        let mut current = head;
        while !current.is_null() {
            count += 1;
            // SAFETY: `current` is non-null and points to a valid node.
            current = unsafe { (*current).next as *const WatchdogNode };
        }
        count
    }

    // ---------------------------------------------------------------
    // add
    // ---------------------------------------------------------------

    #[test]
    fn test_add_single_node() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        unsafe { reg.add(pin_mut(&mut n), 100, 0) };

        assert_eq!(count_nodes(reg.head), 1);
        assert_eq!(n.timeout_interval_ms, 100);
        assert_eq!(n.last_touched_timestamp_ms, 0);
    }

    #[test]
    fn test_add_multiple_nodes() {
        let mut reg = WatchdogRegistry::new();
        let mut n1 = WatchdogNode::default();
        let mut n2 = WatchdogNode::default();
        let mut n3 = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n1), 100, 0);
            reg.add(pin_mut(&mut n2), 200, 0);
            reg.add(pin_mut(&mut n3), 300, 0);
        }

        assert_eq!(count_nodes(reg.head), 3);
        // Prepend order: head -> n3 -> n2 -> n1
        assert_eq!(reg.head, &mut n3 as *mut WatchdogNode);
    }

    #[test]
    fn test_add_duplicate_acts_as_feed() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n), 100, 10);
        }
        assert_eq!(n.last_touched_timestamp_ms, 10);
        assert_eq!(n.timeout_interval_ms, 100);

        // Re-add with new timeout and timestamp
        unsafe {
            reg.add(pin_mut(&mut n), 250, 50);
        }
        assert_eq!(n.last_touched_timestamp_ms, 50);
        assert_eq!(n.timeout_interval_ms, 250);
        // Should still be just one node
        assert_eq!(count_nodes(reg.head), 1);
    }

    #[test]
    fn test_add_preserves_user_id() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        unsafe {
            WatchdogRegistry::assign_id(pin_mut(&mut n), 42);
            reg.add(pin_mut(&mut n), 100, 0);
        }
        assert_eq!(n.id, 42, "add must not overwrite a pre-set id");
    }

    #[test]
    fn test_readd_preserves_user_id() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        unsafe {
            WatchdogRegistry::assign_id(pin_mut(&mut n), 7);
            reg.add(pin_mut(&mut n), 100, 0);
            reg.add(pin_mut(&mut n), 200, 50);
        }
        assert_eq!(n.id, 7, "re-add must not overwrite the id field");
    }

    // ---------------------------------------------------------------
    // remove
    // ---------------------------------------------------------------

    #[test]
    fn test_remove_single_node() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n), 100, 0);
        }
        assert_eq!(count_nodes(reg.head), 1);

        unsafe {
            reg.remove(pin_mut(&mut n));
        }
        assert_eq!(count_nodes(reg.head), 0);
        assert!(n.next.is_null());
    }

    #[test]
    fn test_remove_head() {
        let mut reg = WatchdogRegistry::new();
        let mut n1 = WatchdogNode::default();
        let mut n2 = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n1), 100, 0);
            reg.add(pin_mut(&mut n2), 200, 0);
        }
        // head -> n2 -> n1
        assert_eq!(count_nodes(reg.head), 2);

        unsafe {
            reg.remove(pin_mut(&mut n2));
        }
        assert_eq!(count_nodes(reg.head), 1);
        assert_eq!(reg.head, &mut n1 as *mut WatchdogNode);
    }

    #[test]
    fn test_remove_from_middle() {
        let mut reg = WatchdogRegistry::new();
        let mut n1 = WatchdogNode::default();
        let mut n2 = WatchdogNode::default();
        let mut n3 = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n1), 100, 0);
            reg.add(pin_mut(&mut n2), 200, 0);
            reg.add(pin_mut(&mut n3), 300, 0);
        }
        // head -> n3 -> n2 -> n1
        assert_eq!(count_nodes(reg.head), 3);

        unsafe {
            reg.remove(pin_mut(&mut n2));
        }
        assert_eq!(count_nodes(reg.head), 2);
        assert!(n2.next.is_null());
        // n3 -> n1
        assert_eq!(reg.head, &mut n3 as *mut WatchdogNode);
        assert_eq!(n3.next, &mut n1 as *mut WatchdogNode);
    }

    #[test]
    fn test_remove_not_found_is_noop() {
        let mut reg = WatchdogRegistry::new();
        let mut n1 = WatchdogNode::default();
        let mut n2 = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n1), 100, 0);
        }
        // Try removing a node that was never added
        unsafe {
            reg.remove(pin_mut(&mut n2));
        }
        assert_eq!(count_nodes(reg.head), 1);
    }

    #[test]
    fn test_remove_idempotent() {
        let mut reg = WatchdogRegistry::new();
        let mut n1 = WatchdogNode::default();
        let mut n2 = WatchdogNode::default();
        let mut n3 = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n1), 100, 0);
            reg.add(pin_mut(&mut n2), 200, 0);
            reg.add(pin_mut(&mut n3), 300, 0);
        }

        // Remove n3 three times — should not corrupt the list
        unsafe {
            reg.remove(pin_mut(&mut n3));
            reg.remove(pin_mut(&mut n3));
            reg.remove(pin_mut(&mut n3));
        }
        assert_eq!(count_nodes(reg.head), 2);

        // Remove n1 three times
        unsafe {
            reg.remove(pin_mut(&mut n1));
            reg.remove(pin_mut(&mut n1));
            reg.remove(pin_mut(&mut n1));
        }
        assert_eq!(count_nodes(reg.head), 1);

        // Remove n2 three times
        unsafe {
            reg.remove(pin_mut(&mut n2));
            reg.remove(pin_mut(&mut n2));
            reg.remove(pin_mut(&mut n2));
        }
        assert_eq!(count_nodes(reg.head), 0);
    }

    // ---------------------------------------------------------------
    // feed
    // ---------------------------------------------------------------

    #[test]
    fn test_feed_updates_timestamp() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n), 500, 100);
        }
        assert_eq!(n.last_touched_timestamp_ms, 100);

        unsafe {
            WatchdogRegistry::feed(pin_mut(&mut n), 350);
        }
        assert_eq!(n.last_touched_timestamp_ms, 350);
    }

    #[test]
    fn test_feed_preserves_user_id() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        unsafe {
            WatchdogRegistry::assign_id(pin_mut(&mut n), 13);
            reg.add(pin_mut(&mut n), 100, 0);
            WatchdogRegistry::feed(pin_mut(&mut n), 50);
        }
        assert_eq!(n.id, 13, "feed must not overwrite the id field");
    }

    // ---------------------------------------------------------------
    // assign_id
    // ---------------------------------------------------------------

    #[test]
    fn test_assign_id() {
        let mut n = WatchdogNode::default();
        assert_eq!(n.id(), 0);

        unsafe {
            WatchdogRegistry::assign_id(pin_mut(&mut n), 42);
        }
        assert_eq!(n.id(), 42);
    }

    // ---------------------------------------------------------------
    // check — healthy
    // ---------------------------------------------------------------

    #[test]
    fn test_check_healthy() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n), 200, 0);
        }

        // 100 ms elapsed, timeout is 200 — still healthy
        assert!(!reg.check(100));
    }

    #[test]
    fn test_check_healthy_at_boundary() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n), 200, 0);
        }

        // Exactly at the timeout boundary — not expired (> required, not >=)
        assert!(!reg.check(200));
    }

    // ---------------------------------------------------------------
    // check — expired
    // ---------------------------------------------------------------

    #[test]
    fn test_check_expired() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n), 100, 0);
        }

        // 200 ms elapsed, timeout is 100 — expired
        assert!(reg.check(200));
    }

    #[test]
    fn test_check_expired_at_ms_set() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n), 100, 0);
        }
        assert_eq!(reg.expired_at_ms, 0);

        assert!(reg.check(200));
        assert_eq!(reg.expired_at_ms, 200);
    }

    // ---------------------------------------------------------------
    // check — latching
    // ---------------------------------------------------------------

    #[test]
    fn test_check_latching() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n), 100, 0);
        }

        assert!(reg.check(200));
        assert_eq!(reg.expired_at_ms, 200);

        // Feed the node so it would be healthy again...
        unsafe {
            WatchdogRegistry::feed(pin_mut(&mut n), 300);
        }

        // ...but the registry latches — still expired
        assert!(reg.check(350));
        // expired_at_ms should NOT change
        assert_eq!(reg.expired_at_ms, 200);
    }

    // ---------------------------------------------------------------
    // check — wrapping time arithmetic
    // ---------------------------------------------------------------

    #[test]
    fn test_check_wrapping_time_healthy() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        // Feed near u32::MAX
        unsafe {
            reg.add(pin_mut(&mut n), 200, u32::MAX - 50);
        }

        // Time wraps around: now = 100 → elapsed = 100 - (MAX-50) wrapping = 151
        // 151 <= 200 → healthy
        assert!(!reg.check(100));
    }

    #[test]
    fn test_check_wrapping_time_expired() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        // Feed near u32::MAX
        unsafe {
            reg.add(pin_mut(&mut n), 200, u32::MAX - 50);
        }

        // Time wraps around: now = 300 → elapsed = 300 - (MAX-50) wrapping = 351
        // 351 > 200 → expired
        assert!(reg.check(300));
    }

    // ---------------------------------------------------------------
    // next_expired
    // ---------------------------------------------------------------

    #[test]
    fn test_next_expired_iteration() {
        let mut reg = WatchdogRegistry::new();
        let mut n1 = WatchdogNode::default();
        let mut n2 = WatchdogNode::default();
        let mut n3 = WatchdogNode::default();

        unsafe {
            WatchdogRegistry::assign_id(pin_mut(&mut n1), 1);
            WatchdogRegistry::assign_id(pin_mut(&mut n2), 2);
            WatchdogRegistry::assign_id(pin_mut(&mut n3), 3);

            reg.add(pin_mut(&mut n1), 100, 0);
            reg.add(pin_mut(&mut n2), 500, 0); // long timeout — healthy
            reg.add(pin_mut(&mut n3), 100, 0);
        }
        // list: n3 -> n2 -> n1

        // Trigger expiration at t=200
        assert!(reg.check(200));

        let mut cursor: *const WatchdogNode = ptr::null();
        let mut expired_ids = [0u32; 4];
        let mut count = 0;

        while let Some(id) = reg.next_expired(&mut cursor) {
            expired_ids[count] = id;
            count += 1;
            if count >= expired_ids.len() {
                break;
            }
        }

        // n3 (id=3) and n1 (id=1) should be expired; n2 (id=2) is healthy
        assert_eq!(count, 2);
        assert_eq!(expired_ids[0], 3); // head is n3
        assert_eq!(expired_ids[1], 1); // tail is n1
    }

    #[test]
    fn test_next_expired_without_check_returns_none() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n), 100, 0);
        }

        // Don't call check — next_expired should return None
        let mut cursor: *const WatchdogNode = ptr::null();
        assert_eq!(reg.next_expired(&mut cursor), None);
    }

    #[test]
    fn test_next_expired_all_healthy() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n), 500, 0);
        }

        // Check at t=100 — all healthy
        assert!(!reg.check(100));

        let mut cursor: *const WatchdogNode = ptr::null();
        assert_eq!(reg.next_expired(&mut cursor), None);
    }

    // ---------------------------------------------------------------
    // init
    // ---------------------------------------------------------------

    #[test]
    fn test_init_resets_state() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n), 100, 0);
        }
        assert!(reg.check(200));
        assert!(reg.expired);
        assert_eq!(reg.expired_at_ms, 200);

        reg.init();

        assert!(reg.head.is_null());
        assert!(!reg.expired);
        assert_eq!(reg.expired_at_ms, 0);
    }

    // ---------------------------------------------------------------
    // WatchdogNode default
    // ---------------------------------------------------------------

    #[test]
    fn test_node_default() {
        let n = WatchdogNode::default();
        assert_eq!(n.timeout_interval_ms, 0);
        assert_eq!(n.last_touched_timestamp_ms, 0);
        assert_eq!(n.id, 0);
        assert!(n.next.is_null());
    }

    // ---------------------------------------------------------------
    // check — empty registry
    // ---------------------------------------------------------------

    #[test]
    fn test_check_empty_registry() {
        let mut reg = WatchdogRegistry::new();
        assert!(!reg.check(1000));
    }

    // ---------------------------------------------------------------
    // multiple add/remove cycles
    // ---------------------------------------------------------------

    #[test]
    fn test_add_remove_add_cycle() {
        let mut reg = WatchdogRegistry::new();
        let mut n = WatchdogNode::default();

        unsafe {
            reg.add(pin_mut(&mut n), 100, 0);
        }
        assert_eq!(count_nodes(reg.head), 1);

        unsafe {
            reg.remove(pin_mut(&mut n));
        }
        assert_eq!(count_nodes(reg.head), 0);

        // Re-add after removal
        unsafe {
            reg.add(pin_mut(&mut n), 200, 50);
        }
        assert_eq!(count_nodes(reg.head), 1);
        assert_eq!(n.timeout_interval_ms, 200);
        assert_eq!(n.last_touched_timestamp_ms, 50);
    }
}
