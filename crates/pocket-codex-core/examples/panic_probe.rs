//! Probe: does a panic in a spawned thread abort the whole process?
//!
//! An EXAMPLE (not a test) so it is built with the real `profile.release`
//! settings — the test/bench harness force-overrides `panic = "abort"` back to
//! unwind, so a `cargo test` can never observe the abort. The desktop app is a
//! cdylib built with `profile.release`, so it inherits the real panic strategy.
//!
//! Run: `cargo run --release -p pocket-codex-core --example panic_probe`
//! - Under `panic = "abort"`: the child-thread panic aborts the process; it
//!   NEVER prints `PROBE: process survived` and exits with an abort status.
//! - Under `panic = "unwind"`: the panic is contained to the child thread and
//!   the process prints `PROBE: process survived` and exits 0.
//!
//! This is the exact mechanism behind the "phone enters the embedded
//! (自带/in-process) codex app-server → the whole PC process panics" crash: a
//! panic in a codex-spawned per-connection task must NOT abort the desktop.

fn main() {
    // Quiet the default hook so the intentional panic doesn't print a scary
    // backtrace; we only care whether the PROCESS survives it.
    std::panic::set_hook(Box::new(|_| {}));
    let handle = std::thread::spawn(|| panic!("intentional: panic-containment probe"));
    let contained = handle.join().is_err();
    // Restore so any later panic prints normally.
    let _ = std::panic::take_hook();
    println!("PROBE: process survived (child panic contained = {contained})");
}
