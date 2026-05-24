[![](https://img.shields.io/crates/v/mwdg)](https://crates.io/crates/mwdg)
[![docs.rs](https://img.shields.io/badge/docs.rs-mwdg-66c2a5?logo=docs.rs&label=docs.rs)](https://docs.rs/mwdg)

# mwdg

Micro-watchdog library for embedded RTOS/async systems.

A `no_std` software multi-watchdog library designed for embedded systems where multiple tasks need to monitor their liveness and report status to a central supervisor.

## Features

- **Zero-cost abstractions:** Built on Rust's type system with no runtime overhead for its safe interface.
- **Embedded-friendly:** Designed for `no_std` environments, requiring no memory allocation.
- **Intrusive design:** Minimal memory footprint through intrusive linked lists.
- **Async/RTOS-ready:** Integrates seamlessly into multi-tasking environments.
- **Thread-safe:** Core logic can be used safely in concurrent systems when wrapped in appropriate synchronization primitives (e.g., Mutex, critical sections).

# License

<sup>
Licensed under either of <a href="LICENSE-APACHE">Apache License, Version 2.0</a> or
<a href="LICENSE-MIT">MIT license</a> at your option.
</sup>

<br>

<sub>
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this codebase by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
</sub>
