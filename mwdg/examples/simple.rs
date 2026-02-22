//! Minimal multi-threaded usage of the safe `mwdg` API.
//!
//! This example simulates an RTOS-like setup on a desktop OS:
//!
//! - **Two worker threads** each register a watchdog and periodically feed it.
//!   After a while, one thread intentionally stops feeding to demonstrate
//!   expiration detection.
//! - **The main thread** periodically calls `check` and prints the health
//!   status.
//!
//! # Thread safety
//!
//! The core `mwdg` library has no global state — the [`WatchdogRegistry`] is
//! an owned value.  In this example it is wrapped in a `Mutex` so that
//! multiple threads can share it safely.  In a real RTOS you would call the
//! registry methods inside a critical section instead.
//!
//! # Node lifetime
//!
//! Each worker thread pins its `WatchdogNode` on the stack with
//! `core::pin::pin!()`.  The nodes live as long as the thread, and are
//! removed from the registry before the thread exits.
//!
//! # Running
//! ```sh
//! cargo run -p mwdg --example simple
//! ```
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mwdg::{WatchdogNode, WatchdogRegistry};

/// Returns milliseconds elapsed since `origin` (wraps at `u32::MAX`).
fn now_ms(origin: Instant) -> u32 {
    let now = Instant::now().duration_since(origin).as_millis();
    u32::try_from(now % (u128::from(u32::MAX) + 1)).expect("now should be in bound")
}

/// Shared flag: when set, worker 1 stops feeding its watchdog.
static STOP_FEEDING: AtomicBool = AtomicBool::new(false);
/// Shared flag: when set, worker 1 removes its node and exits.
static WORKER1_EXIT: AtomicBool = AtomicBool::new(false);

fn main() {
    let origin = Instant::now();
    let registry = Arc::new(Mutex::new(WatchdogRegistry::new()));

    println!("[main] mwdg subsystem initialized");

    // Worker 1: feeds normally for ~300 ms, then stops.
    // The thread keeps running (and the node stays registered) until
    // WORKER1_EXIT is set, so the main loop can detect the expiration.
    let reg1 = Arc::clone(&registry);
    let handle1 = std::thread::spawn(move || {
        let mut node = core::pin::pin!(WatchdogNode::default());

        // Assign an ID and register with the registry.
        WatchdogRegistry::assign_id(node.as_mut(), 1);
        {
            let mut reg = reg1.lock().unwrap();
            reg.add(node.as_mut(), 100, now_ms(origin));
        }
        println!("[worker-1] registered watchdog (timeout=100 ms, id=1)");

        loop {
            if WORKER1_EXIT.load(Ordering::Relaxed) {
                break;
            }

            if !STOP_FEEDING.load(Ordering::Relaxed) {
                let _reg = reg1.lock().unwrap();
                WatchdogRegistry::feed(node.as_mut(), now_ms(origin));
            }

            std::thread::sleep(Duration::from_millis(40));
        }

        // Remove the node from the registry before the thread exits and
        // the pinned node is dropped.
        {
            let mut reg = reg1.lock().unwrap();
            reg.remove(node.as_mut());
        }
        println!("[worker-1] exiting");
    });

    // Worker 2: feeds reliably for the whole duration
    let reg2 = Arc::clone(&registry);
    let handle2 = std::thread::spawn(move || {
        let mut node = core::pin::pin!(WatchdogNode::default());

        WatchdogRegistry::assign_id(node.as_mut(), 2);
        {
            let mut reg = reg2.lock().unwrap();
            reg.add(node.as_mut(), 200, now_ms(origin));
        }
        println!("[worker-2] registered watchdog (timeout=200 ms, id=2)");

        for _ in 0..30 {
            {
                let _reg = reg2.lock().unwrap();
                WatchdogRegistry::feed(node.as_mut(), now_ms(origin));
            }
            std::thread::sleep(Duration::from_millis(80));
        }

        println!("[worker-2] finished");

        // Remove the node before the thread exits.
        {
            let mut reg = reg2.lock().unwrap();
            reg.remove(node.as_mut());
        }
    });

    // Main loop: check health every 50 ms
    for tick in 0..30 {
        let status = {
            let mut reg = registry.lock().unwrap();
            reg.check(now_ms(origin))
        };

        let label = if status { "EXPIRED" } else { "HEALTHY" };
        println!("[main] tick {tick:>2}: check -> {label}");

        // If expired, iterate to find which watchdog(s) caused it.
        if status {
            let reg = registry.lock().unwrap();
            let mut cursor: *const WatchdogNode = ptr::null();
            while let Some(id) = reg.next_expired(&mut cursor) {
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

    // Signal worker-1 to clean up and exit.
    WORKER1_EXIT.store(true, Ordering::Relaxed);

    handle1.join().unwrap();
    handle2.join().unwrap();
    println!("[main] done");
}
