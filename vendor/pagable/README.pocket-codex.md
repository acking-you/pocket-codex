# pagable 0.4.1

This directory contains the crates.io release of `pagable` 0.4.1, originating
from facebook/starlark-rust commit `8978f3cd91b2908e1e61f6b58f84293097abf8f1`.
The registry package checksum is
`3658968938a4d1eaa1987e69dcd84b01fb067c5b3416dccc8d71373b6ded6821`. The Apache 2.0 license is included as `LICENSE-APACHE`.

The only source change is in `src/pagable_arc.rs`: retain the exact 64-bit size
assertion, but check the 32-bit allocation's maximum footprint instead of
requiring wasm32's exact padding on ARMv7. The released assertion expects
48 bytes; ARMv7's actual layout is 40 bytes. Runtime code is unchanged.

The normalized `Cargo.toml` keeps dependencies on their registry versions.
This crate is excluded from first-party workspace formatting and linting.
Remove this patch once a compatible upstream release supports both layouts.
