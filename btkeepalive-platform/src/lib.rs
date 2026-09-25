//! Windows platform glue: single instance, startup entry, device watch.
//!
//! Real implementations live in `#[cfg(windows)]` modules so the crate
//! builds and tests on Linux. Non-Windows builds get honest stubs that
//! report unsupported instead of pretending to work.

pub mod device;
pub mod instance;
pub mod startup;
