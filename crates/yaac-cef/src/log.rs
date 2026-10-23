//! Structured log lines on stdout. The harness and the bundle checks parse these.

use std::io::Write;
use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

pub(crate) fn line(args: std::fmt::Arguments<'_>) {
    let start = START.get_or_init(Instant::now);
    let mut out = std::io::stdout().lock();
    let _ = writeln!(
        out,
        "ENGINE t={:05}ms {}",
        start.elapsed().as_millis(),
        args
    );
    let _ = out.flush();
}

macro_rules! elog {
    ($($arg:tt)*) => {
        $crate::log::line(format_args!($($arg)*))
    };
}
pub(crate) use elog;

/// Logs only in debug and `dev-tools` builds. Used for per-event callbacks that page content can
/// drive, so a release build writes nothing per event.
macro_rules! dev_log {
    ($($arg:tt)*) => {
        if cfg!(any(debug_assertions, feature = "dev-tools")) {
            $crate::log::line(format_args!($($arg)*))
        }
    };
}
pub(crate) use dev_log;
