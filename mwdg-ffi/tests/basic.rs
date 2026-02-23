use mwdg_ffi::*;

use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

// Safe wrapper helpers that call the unsafe crate functions.
fn safe_mwdg_init() {
    unsafe {
        mwdg_init();
    }
}

fn safe_mwdg_add(wdg: *mut mwdg_node, timeout_ms: u32) {
    unsafe {
        mwdg_add(wdg, timeout_ms);
    }
}

static MOCK_TIME: AtomicU32 = AtomicU32::new(0);

extern "C" fn mock_get_time_ms() -> u32 {
    MOCK_TIME.load(Ordering::Relaxed)
}

extern "C" fn mock_enter_critical() {
    // no-op for single-threaded tests
}

extern "C" fn mock_exit_critical() {
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
    safe_mwdg_init();
}

/// Helper to create a zeroed SoftwareWdg.
fn new_wdg() -> mwdg_node {
    Default::default()
}

#[test]
fn test_check_no_watchdogs() {
    reset();
    assert_eq!(unsafe { mwdg_check() }, 0, "Empty list should be healthy");
}

#[test]
fn test_check_add_null() {
    reset();

    safe_mwdg_add(ptr::null_mut(), 100);
    safe_mwdg_add(ptr::null_mut(), 200);
    safe_mwdg_add(ptr::null_mut(), 300);

    assert_eq!(unsafe { mwdg_check() }, 0, "Empty list should be healthy");
}

#[test]
fn test_check_add_with_remove() {
    reset();

    let mut wdg = new_wdg();

    safe_mwdg_add(&mut wdg, 100);
    set_time(200);
    unsafe {
        mwdg_remove(&mut wdg);
    }
    assert_eq!(
        unsafe { mwdg_check() },
        0,
        "Removed expired WDG should not trigger failure"
    );
}

#[test]
fn test_check_add_multiple_with_remove() {
    reset();

    let mut wdg1 = new_wdg();
    let mut wdg2 = new_wdg();
    let mut wdg3 = new_wdg();

    safe_mwdg_add(&mut wdg1, 100);
    safe_mwdg_add(&mut wdg2, 300);
    safe_mwdg_add(&mut wdg3, 199);
    set_time(200);
    unsafe {
        mwdg_remove(&mut wdg1);
        mwdg_remove(&mut wdg3);
    }

    assert_eq!(
        unsafe { mwdg_check() },
        0,
        "Removed expired WDG should not trigger failure"
    );
}

#[test]
fn test_check_add_with_remove_and_add_again() {
    reset();

    let mut wdg1 = new_wdg();
    let mut wdg2 = new_wdg();
    let mut wdg3 = new_wdg();
    let mut wdg4 = new_wdg();

    safe_mwdg_init();
    safe_mwdg_add(&mut wdg1, 100);
    safe_mwdg_add(&mut wdg2, 300);
    safe_mwdg_add(&mut wdg3, 199);
    set_time(200);
    unsafe {
        mwdg_remove(&mut wdg1);
        mwdg_remove(&mut wdg3);
    }

    assert_eq!(
        unsafe { mwdg_check() },
        0,
        "Removed expired WDG should not trigger failure"
    );

    unsafe {
        mwdg_add(&mut wdg4, 400);
        mwdg_remove(&mut wdg2);
    }
    set_time(350);

    assert_eq!(
        unsafe { mwdg_check() },
        0,
        "Removed expired WDG should not trigger failure"
    );
}

#[test]
fn test_check_remove_null() {
    reset();
    unsafe {
        mwdg_remove(ptr::null_mut());
    }
    assert_eq!(unsafe { mwdg_check() }, 0, "Empty list should be healthy");
}

#[test]
fn test_register_single_and_check_ok() {
    reset();
    set_time(1000);
    let mut wdg = new_wdg();
    safe_mwdg_add(&mut wdg, 100);
    // Still at time 1000, no time has elapsed
    assert_eq!(unsafe { mwdg_check() }, 0);
}

#[test]
fn test_single_expired() {
    reset();
    set_time(1000);
    let mut wdg = new_wdg();
    safe_mwdg_add(&mut wdg, 100);
    set_time(1150);
    assert_eq!(unsafe { mwdg_check() }, 1, "Should detect expired watchdog");
}

#[test]
fn test_feed_resets_timer() {
    reset();
    set_time(1000);
    let mut wdg = new_wdg();
    safe_mwdg_add(&mut wdg, 100);
    // Advance 80ms and feed
    set_time(1080);
    unsafe {
        mwdg_feed(&mut wdg);
    }
    // Advance another 80ms (total 160ms from register, but only 80ms from last feed)
    set_time(1160);
    assert_eq!(
        unsafe { mwdg_check() },
        0,
        "Should be OK because we fed at 1080"
    );
}

#[test]
fn test_multiple_all_ok() {
    reset();
    set_time(500);
    let mut wdg1 = new_wdg();
    let mut wdg2 = new_wdg();
    let mut wdg3 = new_wdg();

    safe_mwdg_add(&mut wdg1, 100);
    safe_mwdg_add(&mut wdg2, 200);
    safe_mwdg_add(&mut wdg3, 300);

    assert_eq!(unsafe { mwdg_check() }, 0);
}

#[test]
fn test_multiple_one_expired() {
    reset();
    set_time(500);
    let mut wdg1 = new_wdg();
    let mut wdg2 = new_wdg();
    let mut wdg3 = new_wdg();

    safe_mwdg_add(&mut wdg1, 100);
    safe_mwdg_add(&mut wdg2, 200);
    safe_mwdg_add(&mut wdg3, 300);

    set_time(650);
    assert_eq!(unsafe { mwdg_check() }, 1, "wdg1 should be expired");
}

#[test]
fn test_wrapping_no_expire() {
    reset();
    // Set time near u32::MAX
    let near_max = u32::MAX - 50;
    set_time(near_max);
    let mut wdg = new_wdg();

    safe_mwdg_add(&mut wdg, 100);

    // Wrap around: 30ms past u32::MAX (i.e., elapsed = 80ms < 100ms)
    set_time(near_max.wrapping_add(80));
    assert_eq!(
        unsafe { mwdg_check() },
        0,
        "80ms elapsed < 100ms timeout, should be OK across wrap"
    );
}

#[test]
fn test_wrapping_expired() {
    reset();
    // Set time near u32::MAX
    let near_max = u32::MAX - 50;
    set_time(near_max);
    let mut wdg = new_wdg();

    safe_mwdg_add(&mut wdg, 100);

    // Wrap around: 150ms elapsed (past 100ms timeout)
    set_time(near_max.wrapping_add(150));
    assert_eq!(
        unsafe { mwdg_check() },
        1,
        "150ms elapsed > 100ms timeout, should be expired across wrap"
    );
}

#[test]
fn test_once_expired_always_expired() {
    reset();

    let mut wdg1 = new_wdg();
    let mut wdg2 = new_wdg();

    set_time(0);
    unsafe {
        mwdg_add(&mut wdg1, 1);
        mwdg_add(&mut wdg2, 5);
    }
    set_time(2);

    assert_eq!(1, unsafe { mwdg_check() }, "WDG1 should be already expired");
    unsafe {
        mwdg_remove(&mut wdg1);
    }
    assert_eq!(
        1,
        unsafe { mwdg_check() },
        "Once expired should be always expired"
    );
}

#[test]
fn test_multiple_add_of_the_same_node() {
    reset();

    let mut wdg = new_wdg();

    set_time(0);
    unsafe {
        mwdg_add(&mut wdg, 1);
        set_time(2);
        mwdg_add(&mut wdg, 1);
        set_time(4);
        mwdg_add(&mut wdg, 1);
    }

    assert_eq!(0, unsafe { mwdg_check() }, "Multiple add works as a feed");
}

#[test]
fn test_assign_id_before_add() {
    reset();
    set_time(0);
    let mut wdg = new_wdg();
    unsafe {
        mwdg_assign_id(&mut wdg, 42);
        mwdg_add(&mut wdg, 100);
    }
    // The node should be healthy and the id should survive add
    assert_eq!(unsafe { mwdg_check() }, 0);
}

#[test]
fn test_assign_id_after_add() {
    reset();
    set_time(0);
    let mut wdg = new_wdg();
    unsafe {
        mwdg_add(&mut wdg, 100);
        mwdg_assign_id(&mut wdg, 55);
    }
    assert_eq!(unsafe { mwdg_check() }, 0);
}

#[test]
fn test_assign_id_null_safe() {
    reset();
    unsafe {
        mwdg_assign_id(ptr::null_mut(), 99);
    }
    // No crash is the assertion
    assert_eq!(unsafe { mwdg_check() }, 0);
}

/// Helper: collect all expired IDs by iterating with mwdg_get_next_expired.
fn collect_expired_ids() -> Vec<u32> {
    let mut ids = Vec::new();
    let mut cursor: *mut mwdg_node = ptr::null_mut();
    let mut id: u32 = 0;
    while unsafe { mwdg_get_next_expired(&mut cursor, &mut id) } == 1 {
        ids.push(id);
    }
    ids
}

#[test]
fn test_get_next_expired_empty_list() {
    reset();
    let ids = collect_expired_ids();
    assert!(ids.is_empty(), "No expired nodes when list is empty");
}

#[test]
fn test_get_next_expired_none_expired() {
    reset();
    set_time(0);
    let mut wdg1 = new_wdg();
    let mut wdg2 = new_wdg();
    unsafe {
        mwdg_assign_id(&mut wdg1, 1);
        mwdg_assign_id(&mut wdg2, 2);
        mwdg_add(&mut wdg1, 100);
        mwdg_add(&mut wdg2, 200);
    }

    // No time elapsed, nothing expired
    let ids = collect_expired_ids();
    assert!(ids.is_empty(), "No expired nodes when all are healthy");
}

#[test]
fn test_get_next_expired_one_expired() {
    reset();
    set_time(0);
    let mut wdg1 = new_wdg();
    let mut wdg2 = new_wdg();
    let mut wdg3 = new_wdg();

    unsafe {
        mwdg_assign_id(&mut wdg1, 1);
        mwdg_assign_id(&mut wdg2, 2);
        mwdg_assign_id(&mut wdg3, 3);
        mwdg_add(&mut wdg1, 100);
        mwdg_add(&mut wdg2, 200);
        mwdg_add(&mut wdg3, 300);
    }

    set_time(150);
    // wdg1 (100ms) expired, wdg2 (200ms) and wdg3 (300ms) ok
    assert_eq!(
        unsafe { mwdg_check() },
        1,
        "mwdg_check must detect expiration"
    );
    let ids = collect_expired_ids();
    assert_eq!(ids.len(), 1, "Exactly one node should be expired");
    assert_eq!(ids[0], 1, "The expired node should be wdg1 (id=1)");
}

#[test]
fn test_get_next_expired_multiple_expired() {
    reset();
    set_time(0);
    let mut wdg1 = new_wdg();
    let mut wdg2 = new_wdg();
    let mut wdg3 = new_wdg();
    unsafe {
        mwdg_assign_id(&mut wdg1, 10);
        mwdg_assign_id(&mut wdg2, 20);
        mwdg_assign_id(&mut wdg3, 30);
        mwdg_add(&mut wdg1, 100);
        mwdg_add(&mut wdg2, 200);
        mwdg_add(&mut wdg3, 300);
    }

    set_time(250); // wdg1 (100ms) and wdg2 (200ms) expired, wdg3 (300ms) ok
    assert_eq!(
        unsafe { mwdg_check() },
        1,
        "mwdg_check must detect expiration"
    );
    let ids = collect_expired_ids();
    assert_eq!(ids.len(), 2, "Two nodes should be expired");
    // Order depends on list order (head-prepend: wdg3 -> wdg2 -> wdg1)
    assert!(ids.contains(&10), "wdg1 (id=10) should be expired");
    assert!(ids.contains(&20), "wdg2 (id=20) should be expired");
}

#[test]
fn test_get_next_expired_all_expired() {
    reset();
    set_time(0);
    let mut wdg1 = new_wdg();
    let mut wdg2 = new_wdg();
    let mut wdg3 = new_wdg();

    unsafe {
        mwdg_assign_id(&mut wdg1, 100);
        mwdg_assign_id(&mut wdg2, 200);
        mwdg_assign_id(&mut wdg3, 300);
        mwdg_add(&mut wdg1, 50);
        mwdg_add(&mut wdg2, 60);
        mwdg_add(&mut wdg3, 70);
    }

    set_time(100); // All expired
    assert_eq!(
        unsafe { mwdg_check() },
        1,
        "mwdg_check must detect expiration"
    );
    let ids = collect_expired_ids();
    assert_eq!(ids.len(), 3, "All three nodes should be expired");
    assert!(ids.contains(&100));
    assert!(ids.contains(&200));
    assert!(ids.contains(&300));
}

#[test]
fn test_get_next_expired_default_id_zero() {
    reset();
    set_time(0);
    let mut wdg = new_wdg();
    // Do NOT assign an id — it should default to 0
    unsafe {
        mwdg_add(&mut wdg, 50);
    }

    set_time(100);
    assert_eq!(
        unsafe { mwdg_check() },
        1,
        "mwdg_check must detect expiration"
    );
    let ids = collect_expired_ids();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0], 0, "Default id should be 0");
}

#[test]
fn test_get_next_expired_null_cursor() {
    reset();
    let mut id: u32 = 0;
    let result = unsafe { mwdg_get_next_expired(ptr::null_mut(), &mut id) };
    assert_eq!(result, 0, "Null cursor should return 0");
}

#[test]
fn test_get_next_expired_null_out_id() {
    reset();
    let mut cursor: *mut mwdg_node = ptr::null_mut();
    let result = unsafe { mwdg_get_next_expired(&mut cursor, ptr::null_mut()) };
    assert_eq!(result, 0, "Null out_id should return 0");
}

#[test]
fn test_get_next_expired_both_null() {
    reset();
    let result = unsafe { mwdg_get_next_expired(ptr::null_mut(), ptr::null_mut()) };
    assert_eq!(result, 0, "Both params null should return 0");
}

#[test]
fn test_get_next_expired_after_feed() {
    reset();
    set_time(0);
    let mut wdg1 = new_wdg();
    let mut wdg2 = new_wdg();
    unsafe {
        mwdg_assign_id(&mut wdg1, 1);
        mwdg_assign_id(&mut wdg2, 2);
        mwdg_add(&mut wdg1, 100);
        mwdg_add(&mut wdg2, 100);
    }

    set_time(80);
    unsafe {
        mwdg_feed(&mut wdg1);
    } // reset wdg1 timer to 80
    set_time(150); // wdg1: elapsed=70 < 100 (ok), wdg2: elapsed=150 > 100 (expired)

    assert_eq!(
        unsafe { mwdg_check() },
        1,
        "mwdg_check must detect expiration"
    );
    let ids = collect_expired_ids();
    assert_eq!(ids.len(), 1, "Only unfed wdg2 should expire");
    assert_eq!(ids[0], 2);
}

#[test]
fn test_get_next_expired_wrapping_time() {
    reset();
    let near_max = u32::MAX - 50;
    set_time(near_max);
    let mut wdg = new_wdg();
    unsafe {
        mwdg_assign_id(&mut wdg, 77);
        mwdg_add(&mut wdg, 100);
    }

    // Wrap around: 150ms elapsed (past 100ms timeout)
    set_time(near_max.wrapping_add(150));
    assert_eq!(
        unsafe { mwdg_check() },
        1,
        "mwdg_check must detect expiration"
    );
    let ids = collect_expired_ids();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0], 77);
}

#[test]
fn test_get_next_expired_without_prior_check() {
    reset();
    set_time(0);
    let mut wdg = new_wdg();
    unsafe {
        mwdg_assign_id(&mut wdg, 5);
        mwdg_add(&mut wdg, 50);
    }

    // Advance time past timeout but do NOT call mwdg_check
    set_time(100);

    // mwdg_get_next_expired should return 0 because mwdg_check has not
    // set state.expired = true.
    let ids = collect_expired_ids();
    assert!(
        ids.is_empty(),
        "Iterator must return nothing when mwdg_check has not detected expiration"
    );
}

#[test]
fn test_get_next_expired_after_feed_race() {
    // Scenario: a frozen task's feed arrives between mwdg_check and
    // mwdg_get_next_expired.  Because the feed updated
    // last_touched_timestamp_ms to a value *after* the expired_at_ms
    // snapshot, the wrapping_sub would underflow.  The half-range guard
    // must detect this and skip the node.
    reset();
    set_time(0);
    let mut wdg = new_wdg();
    unsafe {
        mwdg_assign_id(&mut wdg, 42);
        mwdg_add(&mut wdg, 100);
    }

    // Advance past timeout
    set_time(200);
    assert_eq!(
        unsafe { mwdg_check() },
        1,
        "Should detect expiration at t=200"
    );

    // Simulate the frozen task recovering and feeding its watchdog
    // AFTER mwdg_check but BEFORE iterating.
    set_time(201);
    unsafe {
        mwdg_feed(&mut wdg);
    }

    // The node was fed after the snapshot (last_touched=201 > expired_at_ms=200).
    // next_expired cannot confirm the node was expired at snapshot time, so
    // it must be skipped.  check() already returned true — that is the
    // authoritative expiration signal.
    let ids = collect_expired_ids();
    assert_eq!(
        ids.len(),
        0,
        "Node fed after snapshot must not appear in expired list"
    );
}

#[test]
fn test_get_next_expired_feed_race_does_not_falsely_report_healthy_node() {
    // Scenario: two nodes registered.  check() detects one as expired and
    // freezes expired_at_ms.  Between check() and the iterator, the
    // *healthy* node is fed at a timestamp after the snapshot.  Without
    // the half-range guard the wrapping_sub would underflow and falsely
    // report the healthy node as expired.
    reset();
    set_time(0);
    let mut wdg1 = new_wdg();
    let mut wdg2 = new_wdg();
    unsafe {
        mwdg_assign_id(&mut wdg1, 1);
        mwdg_assign_id(&mut wdg2, 2);
        mwdg_add(&mut wdg1, 100); // timeout 100 ms
        mwdg_add(&mut wdg2, 200); // timeout 200 ms
    }

    // Feed wdg2 at t=350 so it stays healthy.
    set_time(350);
    unsafe {
        mwdg_feed(&mut wdg2);
    }

    // check() at t=450:
    //   wdg1: elapsed = 450 - 0   = 450 > 100 → expired
    //   wdg2: elapsed = 450 - 350 = 100 < 200 → healthy
    set_time(450);
    assert_eq!(unsafe { mwdg_check() }, 1, "Should detect wdg1 expiration");

    // Simulate race: wdg2 is fed AFTER the snapshot.
    set_time(460);
    unsafe {
        mwdg_feed(&mut wdg2);
    }

    // Only wdg1 should be reported.  Without the fix, wdg2 would also
    // appear because 450_u32.wrapping_sub(460) = u32::MAX - 9 > 200.
    let ids = collect_expired_ids();
    assert_eq!(ids, vec![1], "Only wdg1 should be expired");
}
