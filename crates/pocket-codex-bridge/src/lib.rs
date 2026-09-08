// The desktop host instantiates the embedded app-server's deeply nested
// futures.
#![recursion_limit = "256"]

pub mod api;
mod engine;
mod frb_generated;

// Manual (#[ignore]d) end-to-end harness — see its module docs for usage.
#[cfg(test)]
mod e2e_manual;
