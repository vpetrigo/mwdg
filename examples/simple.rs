//! Minimal multi-threaded usage of the `mwdg` crate.
//!
//! This example simulates an RTOS-like setup on a desktop OS:
//!
//! - **Two worker threads** each register a watchdog and periodically feed it.
//!   After a while, one thread intentionally stops feeding to demonstrate
//!   expiration detection.
//! - **The main thread** periodically calls `mwdg_check` and prints the
//!   health status.
//!
//! # Node lifetime
//!
//! The `mwdg_node` structs are allocated in `main` and outlive every spawned
//! thread.  Only raw pointers are sent into the closures, matching the
//! ownership model of a real RTOS where nodes are `static` or stack-allocated
//! in long-lived tasks.
//!
//! # User-provided callbacks
//!
//! The `mwdg` library requires three external C functions:
//!   - `mwdg_get_time_milliseconds` – returns the current time in ms.
//!   - `mwdg_enter_critical` / `mwdg_exit_critical` – enter/exit a critical
//!     section (here implemented with a global `Mutex`).
//!
//! # Running
//! ```sh
//! cargo run --example simple
//! ```
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mwdg::{self, mwdg_node};

/// Monotonic time origin, set once at program start.
static mut TIME_ORIGIN: Option<Instant> = None;

/// Returns milliseconds elapsed since program start (wraps at `u32::MAX`).
///
/// # Panics
/// Panics if called before `TIME_ORIGIN` is initialized in `main`.
#[unsafe(no_mangle)]
#[allow(clippy::cast_possible_truncation)] // intentional u32 wrap
pub extern "C" fn mwdg_get_time_milliseconds() -> u32 {
    let origin = unsafe { TIME_ORIGIN.unwrap() };
    Instant::now().duration_since(origin).as_millis() as u32
}

/// Global mutex used as a critical section for the linked-list operations.
static CRITICAL: Mutex<()> = Mutex::new(());

/// Guard stashed while inside the critical section.
static mut CRITICAL_GUARD: Option<std::sync::MutexGuard<'static, ()>> = None;

/// # Panics
/// Panics if the mutex is poisoned.
#[unsafe(no_mangle)]
pub extern "C" fn mwdg_enter_critical() {
    let guard = CRITICAL.lock().unwrap();
    // SAFETY: paired with mwdg_exit_critical; the library guarantees
    // non-reentrant, balanced enter/exit calls.
    unsafe {
        CRITICAL_GUARD = Some(guard);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mwdg_exit_critical() {
    // SAFETY: dropping the guard releases the mutex.
    unsafe {
        CRITICAL_GUARD = None;
    }
}

/// A raw `*mut mwdg_node` that can be sent across threads.
///
/// # Safety
/// The pointed-to `mwdg_node` must remain valid (not dropped or moved) for
/// the entire lifetime of this handle, and all access must go through the
/// library's critical-section-protected API.
struct NodePtr(*mut mwdg_node);

// SAFETY: the mwdg library serialises all access to the node behind a
// critical section; the pointed-to memory is pinned in `main` and outlives
// every thread.
unsafe impl Send for NodePtr {}

impl NodePtr {
    fn get(&self) -> *mut mwdg_node {
        self.0
    }
}

/// Shared flag: when set, worker 1 stops feeding its watchdog.
static STOP_FEEDING: AtomicBool = AtomicBool::new(false);

fn main() {
    // Initialize the monotonic clock origin.
    unsafe {
        TIME_ORIGIN = Some(Instant::now());
        mwdg::mwdg_init();
    }

    println!("[main] mwdg subsystem initialized");
    let mut node1 = core::pin::pin!(mwdg_node::default());
    let mut node2 = core::pin::pin!(mwdg_node::default());

    // Obtain stable raw pointers before spawning threads.
    // SAFETY: Pin guarantees the address won't change; the nodes are dropped
    // only after all threads are joined (see bottom of main).
    let ptr1 = NodePtr(unsafe { ptr::from_mut(node1.as_mut().get_unchecked_mut()) });
    let ptr2 = NodePtr(unsafe { ptr::from_mut(node2.as_mut().get_unchecked_mut()) });

    // Assign user-defined IDs so expired nodes can be identified.
    unsafe {
        mwdg::mwdg_assign_id(ptr1.get(), 1);
        mwdg::mwdg_assign_id(ptr2.get(), 2);
    }

    // Worker 1: feeds normally for ~300 ms, then stops
    let handle1 = std::thread::spawn(move || {
        unsafe {
            mwdg::mwdg_add(ptr1.get(), 100); // 100 ms timeout
        }
        println!("[worker-1] registered watchdog (timeout=100 ms, id=1)");

        loop {
            if STOP_FEEDING.load(Ordering::Relaxed) {
                println!("[worker-1] stopped feeding -- will expire soon");
                return;
            }

            unsafe {
                mwdg::mwdg_feed(ptr1.get());
            }
            std::thread::sleep(Duration::from_millis(40)); // well within 100 ms
        }
    });

    // Worker 2: feeds reliably for the whole duration
    let handle2 = std::thread::spawn(move || {
        unsafe {
            mwdg::mwdg_add(ptr2.get(), 200);
        }
        println!("[worker-2] registered watchdog (timeout=200 ms, id=2)");

        for _ in 0..30 {
            unsafe {
                mwdg::mwdg_feed(ptr2.get());
            }
            std::thread::sleep(Duration::from_millis(80));
        }

        println!("[worker-2] finished");
    });

    // Main loop: check health every 50 ms
    for tick in 0..30 {
        let status = unsafe { mwdg::mwdg_check() };
        let label = if status == 0 { "HEALTHY" } else { "EXPIRED" };
        println!("[main] tick {tick:>2}: mwdg_check -> {label}");

        // If expired, iterate to find which watchdog(s) caused it.
        if status != 0 {
            let mut cursor: *mut mwdg_node = ptr::null_mut();
            let mut id: u32 = 0;
            while unsafe { mwdg::mwdg_get_next_expired(&mut cursor, &mut id) } != 0 {
                println!("[main]   expired watchdog id: {id}");
            }
        }

        // After ~300 ms, tell worker-1 to stop feeding.
        if tick == 6 {
            println!("[main] signalling worker-1 to stop feeding");
            STOP_FEEDING.store(true, Ordering::Relaxed);
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    handle1.join().unwrap();
    handle2.join().unwrap();
    println!("[main] done");
}
