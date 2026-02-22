/**
 * @file simple.c
 * @brief Minimal multi-threaded mwdg example using pthreads.
 *
 * Demonstrates how to use the mwdg library from C:
 *
 *  - Two worker threads each register a watchdog and periodically feed it.
 *  - After ~300 ms the main thread signals worker-1 to stop feeding,
 *    which causes mwdg_check() to detect expiration.
 *  - The main thread calls mwdg_check() in a loop and prints health status.
 *
 * Build (Linux, assuming libmwdg_ffi.a was produced by
 * `cargo rustc -p mwdg-ffi --release --features "pack" -- --crate-type staticlib`):
 *
 *   # Locate the generated header (under mwdg-ffi's build dir):
 *   HEADER_DIR=$(dirname "$(find target/release/build -name mwdg.h -path '*/include/*')")
 *
 *   gcc -o simple examples/simple.c \
 *       -I$HEADER_DIR \
 *       -Ltarget/release -lmwdg_ffi \
 *       -lpthread
 *
 * Or with a cross-compiled static library for an ARM target:
 *
 *   arm-none-eabi-gcc -o simple examples/simple.c \
 *       -Iinclude \
 *       -Llib/thumbv7em-none-eabihf -lmwdg_ffi \
 *       -lpthread
 */
#include "mwdg.h"

#include <stdatomic.h>
#include <stdio.h>
#include <stdint.h>
#include <pthread.h>
#include <time.h>

/** Global mutex used as a critical section for linked-list operations. */
static pthread_mutex_t g_critical_mutex = PTHREAD_MUTEX_INITIALIZER;

/** Returns monotonic time in milliseconds (wraps at uint32_t max). */
uint32_t mwdg_get_time_milliseconds(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint32_t)(ts.tv_sec * 1000U + ts.tv_nsec / 1000000U);
}

void mwdg_enter_critical(void)
{
    pthread_mutex_lock(&g_critical_mutex);
}

void mwdg_exit_critical(void)
{
    pthread_mutex_unlock(&g_critical_mutex);
}

static void sleep_ms(unsigned int ms)
{
    struct timespec ts = {
        .tv_sec  = ms / 1000,
        .tv_nsec = (long)(ms % 1000) * 1000000L
    };
    nanosleep(&ts, NULL);
}

/** Shared flag: when non-zero, worker-1 stops feeding. */
static volatile atomic_uint g_stop_feeding = 0;

/**
 * Worker 1: registers a 100 ms watchdog, feeds every 40 ms.
 * Stops feeding when g_stop_feeding is set by the main thread.
 */
static void *worker1_func(void *arg)
{
    static struct mwdg_node wdg = {0};

    (void)arg;

    mwdg_add(&wdg, 100);
    mwdg_assign_id(&wdg, 0xCAFE);
    printf("[worker-1] registered watchdog (timeout=100 ms, id=1)\n");

    while (!g_stop_feeding) {
        mwdg_feed(&wdg);
        sleep_ms(40);
    }

    printf("[worker-1] stopped feeding -- will expire soon\n");

    return NULL;
}

/**
 * Worker 2: registers a 200 ms watchdog, feeds every 80 ms
 * for the whole duration of the example.
 */
static void *worker2_func(void *arg)
{
    static struct mwdg_node wdg = {0};
    int i;

    (void)arg;

    mwdg_add(&wdg, 200);
    mwdg_assign_id(&wdg, 0xBEEF);
    printf("[worker-2] registered watchdog (timeout=200 ms, id=2)\n");

    for (i = 0; i < 30; i++) {
        mwdg_feed(&wdg);
        sleep_ms(80); /* well within 200 ms timeout */
    }

    printf("[worker-2] finished\n");

    return NULL;
}

/* -----------------------------------------------------------------------
 * Main
 * ----------------------------------------------------------------------- */

int main(void)
{
    pthread_t t1;
    pthread_t t2;
    int tick;

    /* Initialize the mwdg subsystem (must happen before any add/feed/check). */
    mwdg_init();
    printf("[main] mwdg subsystem initialized\n");

    /* Spawn worker threads. */
    pthread_create(&t1, NULL, worker1_func, NULL);
    pthread_create(&t2, NULL, worker2_func, NULL);

    /* Check health every 50 ms. */
    for (tick = 0; tick < 30; tick++) {
        int32_t status = mwdg_check();

        printf("[main] tick %2d: mwdg_check -> %s\n",
               tick, status == 0 ? "HEALTHY" : "EXPIRED");

        /* If expired, iterate to find which watchdog(s) caused it. */
        if (status != 0) {
            struct mwdg_node *cursor = NULL;
            uint32_t id;
            while (mwdg_get_next_expired(&cursor, &id) != 0) {
                printf("[main]   expired watchdog id: 0x%04X\n", id);
            }
        }

        /* After ~300 ms, tell worker-1 to stop feeding. */
        if (tick == 6) {
            printf("[main] signalling worker-1 to stop feeding\n");
            g_stop_feeding = 1;
        }

        sleep_ms(50);
    }

    pthread_join(t1, NULL);
    pthread_join(t2, NULL);
    printf("[main] done\n");

    return 0;
}
