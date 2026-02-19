use mwdg::*;

use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

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
    mwdg_init();
}

/// Helper to create a zeroed SoftwareWdg.
fn new_wdg() -> mwdg_node {
    mwdg_node {
        timeout_interval_ms: 0,
        last_touched_timestamp_ms: 0,
        next: ptr::null_mut(),
    }
}

#[test]
fn test_check_no_watchdogs() {
    reset();
    assert_eq!(mwdg_check(), 0, "Empty list should be healthy");
}

#[test]
fn test_check_add_null() {
    reset();

    mwdg_add(ptr::null_mut(), 100);
    mwdg_add(ptr::null_mut(), 200);
    mwdg_add(ptr::null_mut(), 300);

    assert_eq!(mwdg_check(), 0, "Empty list should be healthy");
}

#[test]
fn test_check_add_with_remove() {
    reset();

    let mut wdg = new_wdg();

    mwdg_add(&mut wdg, 100);
    set_time(200);
    mwdg_remove(&mut wdg);
    assert_eq!(
        mwdg_check(),
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

    mwdg_add(&mut wdg1, 100);
    mwdg_add(&mut wdg2, 300);
    mwdg_add(&mut wdg3, 199);
    set_time(200);
    mwdg_remove(&mut wdg1);
    mwdg_remove(&mut wdg3);

    assert_eq!(
        mwdg_check(),
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

    mwdg_init();
    mwdg_add(&mut wdg1, 100);
    mwdg_add(&mut wdg2, 300);
    mwdg_add(&mut wdg3, 199);
    set_time(200);
    mwdg_remove(&mut wdg1);
    mwdg_remove(&mut wdg3);

    assert_eq!(
        mwdg_check(),
        0,
        "Removed expired WDG should not trigger failure"
    );

    mwdg_add(&mut wdg4, 400);
    mwdg_remove(&mut wdg2);
    set_time(350);

    assert_eq!(
        mwdg_check(),
        0,
        "Removed expired WDG should not trigger failure"
    );
}

#[test]
fn test_check_remove_null() {
    reset();
    mwdg_remove(ptr::null_mut());
    assert_eq!(mwdg_check(), 0, "Empty list should be healthy");
}

#[test]
fn test_register_single_and_check_ok() {
    reset();
    set_time(1000);
    let mut wdg = new_wdg();
    mwdg_add(&mut wdg, 100);
    // Still at time 1000, no time has elapsed
    assert_eq!(mwdg_check(), 0);
}

#[test]
fn test_single_expired() {
    reset();
    set_time(1000);
    let mut wdg = new_wdg();
    mwdg_add(&mut wdg, 100);
    set_time(1150);
    assert_eq!(mwdg_check(), 1, "Should detect expired watchdog");
}

#[test]
fn test_feed_resets_timer() {
    reset();
    set_time(1000);
    let mut wdg = new_wdg();
    mwdg_add(&mut wdg, 100);
    // Advance 80ms and feed
    set_time(1080);
    mwdg_feed(&mut wdg);
    // Advance another 80ms (total 160ms from register, but only 80ms from last feed)
    set_time(1160);
    assert_eq!(mwdg_check(), 0, "Should be OK because we fed at 1080");
}

#[test]
fn test_multiple_all_ok() {
    reset();
    set_time(500);
    let mut wdg1 = new_wdg();
    let mut wdg2 = new_wdg();
    let mut wdg3 = new_wdg();

    mwdg_add(&mut wdg1, 100);
    mwdg_add(&mut wdg2, 200);
    mwdg_add(&mut wdg3, 300);

    assert_eq!(mwdg_check(), 0);
}

#[test]
fn test_multiple_one_expired() {
    reset();
    set_time(500);
    let mut wdg1 = new_wdg();
    let mut wdg2 = new_wdg();
    let mut wdg3 = new_wdg();

    mwdg_add(&mut wdg1, 100);
    mwdg_add(&mut wdg2, 200);
    mwdg_add(&mut wdg3, 300);

    set_time(650);
    assert_eq!(mwdg_check(), 1, "wdg1 should be expired");
}

#[test]
fn test_wrapping_no_expire() {
    reset();
    // Set time near u32::MAX
    let near_max = u32::MAX - 50;
    set_time(near_max);
    let mut wdg = new_wdg();

    mwdg_add(&mut wdg, 100);

    // Wrap around: 30ms past u32::MAX (i.e., elapsed = 80ms < 100ms)
    set_time(near_max.wrapping_add(80));
    assert_eq!(
        mwdg_check(),
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

    mwdg_add(&mut wdg, 100);

    // Wrap around: 150ms elapsed (past 100ms timeout)
    set_time(near_max.wrapping_add(150));
    assert_eq!(
        mwdg_check(),
        1,
        "150ms elapsed > 100ms timeout, should be expired across wrap"
    );
}

#[test]
fn test_register_sets_fields() {
    reset();
    set_time(42);
    let mut wdg = new_wdg();

    mwdg_add(&mut wdg, 250);
    assert_eq!(wdg.timeout_interval_ms, 250);
    assert_eq!(wdg.last_touched_timestamp_ms, 42);
}

#[test]
fn test_feed_updates_timestamp() {
    reset();
    set_time(100);
    let mut wdg = new_wdg();

    mwdg_add(&mut wdg, 500);
    assert_eq!(wdg.last_touched_timestamp_ms, 100);

    set_time(350);

    mwdg_feed(&mut wdg);
    assert_eq!(wdg.last_touched_timestamp_ms, 350);
}

#[test]
fn test_once_expired_always_expired() {
    reset();

    let mut wdg1 = new_wdg();
    let mut wdg2 = new_wdg();

    set_time(0);
    mwdg_add(&mut wdg1, 1);
    mwdg_add(&mut wdg2, 5);
    set_time(2);

    assert_eq!(1, mwdg_check(), "WDG1 should be already expired");
    mwdg_remove(&mut wdg1);
    assert_eq!(1, mwdg_check(), "Once expired should be always expired");
}

#[test]
fn test_multiple_add_of_the_same_node() {
    reset();

    let mut wdg = new_wdg();

    set_time(0);
    mwdg_add(&mut wdg, 1);
    set_time(2);
    mwdg_add(&mut wdg, 1);
    set_time(4);
    mwdg_add(&mut wdg, 1);

    assert_eq!(0, mwdg_check(), "Multiple add works as a feed");
}
