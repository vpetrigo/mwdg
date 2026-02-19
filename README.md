[![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/vpetrigo/mwdg/ci.yml?logo=github)](https://github.com/vpetrigo/mwdg/actions/workflows/ci.yml)
[![](https://img.shields.io/crates/v/mwdg)](https://crates.io/crates/mwdg)
[![docs.rs](https://img.shields.io/badge/docs.rs-mwdg-66c2a5?logo=docs.rs&label=docs.rs)](https://docs.rs/mwdg)
[![codecov](https://codecov.io/github/vpetrigo/mwdg/graph/badge.svg?token=7zad9eKKx7)](https://codecov.io/github/vpetrigo/mwdg)

# mwdg - Micro-Watchdog Library

A `no_std` software micro-watchdog library for embedded RTOS systems.
Each RTOS task registers a [`mwdg_node`] with a timeout interval.
The task periodically calls [`mwdg_feed`] to signal liveness.
A central [`mwdg_check`] function that verifies all registered watchdogs
are healthy, enabling the main loop to gate hardware watchdog resets.

# C FFI

All public functions declared to be exposed without mangling, so the library can be
linked from C/C++ code. Use the generated `include/mwdg.h` header for having proper
interface declaration.

# Usage

To use in Rust projects:

```toml
[dependencies]
mwdg = "0.1"
```

To use in C/C++ projects, see info below.

## Build static library

```
# Build for ARMv7-M FP
cargo rustc --target thumbv7em-none-eabihf --features "pack" -- --crate-type staticlib
```

The `target/thumbv7em-none-eabihf/release` will contain the `libmwdg.rlib` that can be renamed into the
`libmwdg.a` and used along with the header `mwdg.h` file from the `include/` directory in your C/C++ project.

# Contribution

Contributions are always welcome! If you have an idea, it's best to float it by me before working on it to ensure no
effort is wasted. If there's already an open issue for it, knock yourself out. See the
[**contributing section**](CONTRIBUTING.md) for additional details

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
