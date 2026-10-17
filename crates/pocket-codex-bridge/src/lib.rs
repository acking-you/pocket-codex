#[cfg(feature = "flutter")]
pub mod api;
// Headless hosts use the hosting subset; Flutter also consumes controller
// exports.
#[cfg_attr(not(feature = "flutter"), allow(dead_code, unused_imports))]
mod engine;
#[cfg(feature = "flutter")]
mod frb_generated;

pub use engine::daemon as headless;

// Manual (#[ignore]d) end-to-end harness — see its module docs for usage.
#[cfg(all(test, feature = "flutter"))]
mod e2e_manual;
