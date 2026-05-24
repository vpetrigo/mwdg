[![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/vpetrigo/mwdg/ci.yml?logo=github)](https://github.com/vpetrigo/mwdg/actions/workflows/ci.yml)
[![](https://img.shields.io/crates/v/mwdg)](https://crates.io/crates/mwdg)
[![docs.rs](https://img.shields.io/badge/docs.rs-mwdg-66c2a5?logo=docs.rs&label=docs.rs)](https://docs.rs/mwdg)
[![codecov](https://codecov.io/github/vpetrigo/mwdg/graph/badge.svg?token=7zad9eKKx7)](https://codecov.io/github/vpetrigo/mwdg)

# mwdg - Micro-Watchdog Library

A `no_std` software micro-watchdog library for embedded RTOS systems.
Each RTOS task registers a `WatchdogNode` with a timeout interval via `WatchdogRegistry::add`.
The task periodically calls `WatchdogRegistry::feed` to signal liveness.
A central `WatchdogRegistry::check` method verifies all registered watchdogs are healthy, enabling the main loop to gate hardware watchdog resets.

# C FFI

The `mwdg-ffi` crate provides C-compatible bindings for this library. These are exposed without mangling, enabling the library to be linked from C/C++ code. Use the generated `mwdg.h` header for proper interface declaration.

# Usage

To use in Rust projects:

```toml
[dependencies]
mwdg = "0.3"
```

To use in C/C++ projects, see info below.

## Build workspace

```
# Build for ARMv7-M FP
cargo build --workspace --target thumbv7em-none-eabihf
```

The `target/thumbv7em-none-eabihf/release` will contain the `libmwdg_ffi.rlib` that can be renamed into the
`libmwdg_ffi.a` and used along with the header `mwdg.h` file from the `include/` directory in your C/C++ project.

# Examples

Simple examples on how to use library in Rust and C/C++ are available in the [`examples`](examples) directory.

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
