//! A conformance suite for `Store` implementations. `yaac-store` runs it against SQLite; the same
//! checks must pass for `FakeStore` where they apply. Written by the store work (m1/store).

use crate::Store;

/// Runs every check against stores produced by `make`. Each call to `make` must return an empty,
/// independent store. Panics on the first failed check.
pub fn run_all(make: &dyn Fn() -> Box<dyn Store>) {
    let _ = make;
    todo!("m1/store")
}
