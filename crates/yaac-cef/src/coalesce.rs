//! Bounds on what page-driven events can cost. `Coalescer` keeps only the latest navigation, title
//! and load state per view between flushes, so a page that changes its title 100k times produces
//! one event. Lifecycle events are never coalesced or dropped. `PopupLimiter` rate-limits the
//! popup reports per opener; the popups themselves are always denied.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};
use yaac_engine::EngineEvent;
use yaac_model::ViewId;

/// Which page-driven kind an event is, for coalescing.
fn coalesce_key(event: &EngineEvent) -> Option<(ViewId, u8)> {
    match event {
        EngineEvent::Navigated { view, .. } => Some((*view, 0)),
        EngineEvent::TitleChanged { view, .. } => Some((*view, 1)),
        EngineEvent::LoadState { view, .. } => Some((*view, 2)),
        _ => None,
    }
}

/// Pending events between two flushes. A coalescable event replaces the pending one of the same
/// view and kind, in its place, so the first-arrival order of the batch is kept. Other events are
/// appended in arrival order.
#[derive(Default)]
pub(crate) struct Coalescer {
    pending: Vec<EngineEvent>,
}

impl Coalescer {
    pub(crate) fn push(&mut self, event: EngineEvent) {
        if let Some(key) = coalesce_key(&event)
            && let Some(slot) = self.pending.iter_mut().find(|e| coalesce_key(e) == Some(key))
        {
            *slot = event;
            return;
        }
        self.pending.push(event);
    }

    /// Takes the batch, in order.
    pub(crate) fn take(&mut self) -> Vec<EngineEvent> {
        std::mem::take(&mut self.pending)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.pending.len()
    }
}

/// At most `max` popup reports per opener in any `window`. Excess popups are still denied, and
/// only counted.
pub(crate) struct PopupLimiter {
    max: usize,
    window: Duration,
    recent: HashMap<ViewId, VecDeque<Instant>>,
    suppressed: u64,
}

impl PopupLimiter {
    pub(crate) fn new(max: usize, window: Duration) -> Self {
        Self {
            max,
            window,
            recent: HashMap::new(),
            suppressed: 0,
        }
    }

    /// True if this popup may be reported.
    pub(crate) fn allow(&mut self, opener: ViewId, now: Instant) -> bool {
        // Keep the map small: forget openers with nothing recent.
        if self.recent.len() > 1024 {
            self.recent.clear();
        }
        let times = self.recent.entry(opener).or_default();
        while let Some(front) = times.front() {
            if now.duration_since(*front) >= self.window {
                times.pop_front();
            } else {
                break;
            }
        }
        if times.len() < self.max {
            times.push_back(now);
            true
        } else {
            self.suppressed += 1;
            false
        }
    }

    pub(crate) fn suppressed(&self) -> u64 {
        self.suppressed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yaac_model::WindowId;

    fn title(view: ViewId, n: usize) -> EngineEvent {
        EngineEvent::TitleChanged {
            view,
            nav_gen: 1,
            title: format!("t{n}"),
        }
    }

    #[test]
    fn keeps_only_the_latest_title_per_view() {
        let view = ViewId(7);
        let mut coalescer = Coalescer::default();
        for n in 0..100_000 {
            coalescer.push(title(view, n));
        }
        let batch = coalescer.take();
        assert_eq!(batch.len(), 1);
        assert!(matches!(&batch[0], EngineEvent::TitleChanged { title, .. } if title == "t99999"));
        assert_eq!(coalescer.len(), 0);
    }

    #[test]
    fn navigation_stays_before_its_title_within_a_batch() {
        let view = ViewId(1);
        let mut coalescer = Coalescer::default();
        coalescer.push(EngineEvent::Navigated {
            view,
            nav_gen: 2,
            url: "https://a.test/".into(),
        });
        coalescer.push(title(view, 1));
        coalescer.push(EngineEvent::Navigated {
            view,
            nav_gen: 2,
            url: "https://a.test/?x".into(),
        });
        let batch = coalescer.take();
        assert!(matches!(batch[0], EngineEvent::Navigated { nav_gen: 2, .. }));
        assert!(matches!(batch[1], EngineEvent::TitleChanged { .. }));
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn lifecycle_events_are_never_coalesced_or_dropped() {
        let a = ViewId(1);
        let b = ViewId(2);
        let w = WindowId::new();
        let mut coalescer = Coalescer::default();
        coalescer.push(EngineEvent::Created { view: a });
        coalescer.push(EngineEvent::WindowClosing { window: w });
        coalescer.push(EngineEvent::Created { view: a });
        coalescer.push(EngineEvent::Crashed { view: b });
        coalescer.push(EngineEvent::CreateFailed { view: b });
        coalescer.push(EngineEvent::PopupRequested {
            opener: a,
            url: "https://x.test/".into(),
            user_gesture: false,
        });
        coalescer.push(EngineEvent::Closed { view: a });
        coalescer.push(EngineEvent::WindowClosed { window: w });
        let batch = coalescer.take();
        assert_eq!(batch.len(), 8, "every lifecycle event is kept, in order");
        assert!(matches!(batch[0], EngineEvent::Created { .. }));
        assert!(matches!(batch[7], EngineEvent::WindowClosed { .. }));
    }

    #[test]
    fn coalescing_is_per_view() {
        let mut coalescer = Coalescer::default();
        coalescer.push(title(ViewId(1), 1));
        coalescer.push(title(ViewId(2), 1));
        coalescer.push(title(ViewId(1), 2));
        let batch = coalescer.take();
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn popups_are_limited_per_opener_and_recover_after_the_window() {
        let mut limiter = PopupLimiter::new(4, Duration::from_secs(1));
        let t0 = Instant::now();
        let opener = ViewId(3);
        let allowed: Vec<bool> = (0..10).map(|_| limiter.allow(opener, t0)).collect();
        assert_eq!(allowed.iter().filter(|a| **a).count(), 4);
        assert_eq!(limiter.suppressed(), 6);
        // Another opener has its own budget.
        assert!(limiter.allow(ViewId(4), t0));
        // After the window the budget is back.
        assert!(limiter.allow(opener, t0 + Duration::from_millis(1100)));
    }
}
