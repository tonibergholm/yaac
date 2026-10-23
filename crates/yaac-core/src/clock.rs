//! The time source. The actor reads time only through `Clock`, so tests can move it.

use std::time::Instant;

pub(crate) trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

pub(crate) struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}
