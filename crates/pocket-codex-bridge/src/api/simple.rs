//! Sanity-check API surface that the Flutter app uses to verify the
//! Rust↔Dart bridge is wired up correctly.

/// Greet a Pocket-Codex user.
///
/// `flutter_rust_bridge` exposes this synchronously to Dart so the
/// front-end can call it without needing `await`.
#[flutter_rust_bridge::frb(sync)]
pub fn greet(name: String) -> String {
    format!("Hello, {name}! — pocket-codex {}", env!("CARGO_PKG_VERSION"))
}

/// Return the bridge crate's package version. Useful as a "ping" the
/// UI can show to confirm the native library loaded.
#[flutter_rust_bridge::frb(sync)]
pub fn bridge_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Initialise FRB-side utilities (panic hooks, logging, etc.). Must be
/// called from the Dart side before any other API is invoked.
#[flutter_rust_bridge::frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}
