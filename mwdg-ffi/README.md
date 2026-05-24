[![](https://img.shields.io/crates/v/mwdg-ffi)](https://crates.io/crates/mwdg-ffi)
[![docs.rs](https://img.shields.io/badge/docs.rs-mwdg--ffi-66c2a5?logo=docs.rs&label=docs.rs)](https://docs.rs/mwdg-ffi)

# mwdg-ffi

C FFI bindings for the mwdg micro-watchdog library.

This crate provides C-compatible bindings for the `mwdg` library, enabling the use of the multi-watchdog system in C/C++ embedded projects.

## Overview

The library allows C applications to register software watchdogs, track liveness, and detect timeouts centrally. It requires the user to provide platform-specific callbacks for time tracking and critical section management.

## Integration

Include the generated `include/mwdg.h` header in your C code.

## Build static library

To use in C/C++ projects, you need to build the static library:

```sh
# Build for target (e.g., ARMv7-M FP)
cargo rustc -p mwdg-ffi --target <target-triple> --features "pack"
```

The `target/<target-triple>/release` directory will contain `libmwdg_ffi.rlib` (or `libmwdg_ffi.a` depending on configuration). You can link this file along with the header `mwdg.h` (generated in the build process under the `include/` directory) in your C/C++ project.

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
