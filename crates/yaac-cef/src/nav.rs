//! Navigation generation for one view.
//!
//! `nav_gen` is a per-view counter that the core uses to drop stale callbacks. It changes in two
//! places only:
//! - a main-frame `on_load_start` starts a new generation;
//! - a main-frame `on_address_change` that no load start announced (a same-document navigation,
//!   such as `pushState`) starts a new generation. An address change that follows a load start
//!   belongs to that load and keeps its generation.
//!
//! `TitleChanged` carries the generation current when the title arrived, not the one that was
//! current when the page began loading.

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NavTracker {
    generation: u64,
    load_announced: bool,
}

impl NavTracker {
    /// A main-frame load started. Returns the new generation.
    pub(crate) fn load_start(&mut self) -> u64 {
        self.generation += 1;
        self.load_announced = true;
        self.generation
    }

    /// A main-frame address change. Returns the generation for the `Navigated` event.
    pub(crate) fn address_change(&mut self) -> u64 {
        if self.load_announced {
            self.load_announced = false;
        } else {
            self.generation += 1;
        }
        self.generation
    }

    /// The generation current now, for `TitleChanged`.
    pub(crate) fn current(&self) -> u64 {
        self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_start_then_address_change_share_one_generation() {
        let mut nav = NavTracker::default();
        assert_eq!(nav.load_start(), 1);
        assert_eq!(
            nav.address_change(),
            1,
            "the address change belongs to the load"
        );
        assert_eq!(nav.current(), 1);
        assert_eq!(nav.load_start(), 2, "the next load starts a new generation");
        assert_eq!(nav.address_change(), 2);
    }

    #[test]
    fn address_change_then_load_start_counts_both_and_keeps_the_later_load() {
        let mut nav = NavTracker::default();
        // A same-document change first: a generation of its own.
        assert_eq!(nav.address_change(), 1);
        // The load that follows starts a further generation, and its address change keeps it.
        assert_eq!(nav.load_start(), 2);
        assert_eq!(nav.address_change(), 2);
        assert_eq!(nav.current(), 2);
    }

    #[test]
    fn same_document_navigations_each_start_a_generation() {
        let mut nav = NavTracker::default();
        assert_eq!(nav.load_start(), 1);
        assert_eq!(nav.address_change(), 1);
        // No load start announced: pushState-style changes each count.
        assert_eq!(nav.address_change(), 2);
        assert_eq!(nav.address_change(), 3);
        assert_eq!(nav.current(), 3);
    }

    #[test]
    fn a_title_after_a_same_document_change_carries_its_generation() {
        let mut nav = NavTracker::default();
        nav.load_start();
        nav.address_change();
        let gen_at_navigation = nav.current();
        nav.address_change();
        // A title that arrives now is labelled with the newer generation.
        assert_eq!(nav.current(), gen_at_navigation + 1);
    }
}
