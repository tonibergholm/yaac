//! Tests that drive the core through `CoreHandle` need `m1/model` (`Position`, `TreeState`) and are
//! marked `#[ignore = "needs m1/model"]`. The store-thread tests run against `FakeStore` alone.

mod core;
mod harness;
mod store;
