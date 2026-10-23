//! Keeps a view's URL and title on one navigation generation. A callback from an older generation
//! is stale and dropped. A title that arrives before its own navigation is held until that
//! navigation starts, so the UI never pairs a new URL with an old title.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NavTrack {
    latest: Option<u64>,
    held_title: Option<(u64, String)>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NavigationStep {
    Stale,
    /// The same navigation reported a new URL. The title stays.
    Redirect,
    /// A new navigation. The stale title is cleared, or replaced by a title that arrived first.
    Start {
        title: Option<String>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TitleStep {
    Stale,
    /// The title belongs to the latest navigation and applies now.
    Current,
    /// The title belongs to a navigation not yet seen. It waits in the track.
    Held,
}

pub(crate) fn navigated(track: &mut NavTrack, nav_gen: u64) -> NavigationStep {
    match track.latest {
        Some(latest) if nav_gen < latest => NavigationStep::Stale,
        Some(latest) if nav_gen == latest => NavigationStep::Redirect,
        _ => {
            track.latest = Some(nav_gen);
            let title = match track.held_title.take() {
                Some((held, title)) if held == nav_gen => Some(title),
                Some((held, title)) if held > nav_gen => {
                    track.held_title = Some((held, title));
                    None
                }
                _ => None,
            };
            NavigationStep::Start { title }
        }
    }
}

pub(crate) fn title_changed(track: &mut NavTrack, nav_gen: u64, title: &str) -> TitleStep {
    match track.latest {
        Some(latest) if nav_gen < latest => TitleStep::Stale,
        Some(latest) if nav_gen == latest => TitleStep::Current,
        _ => {
            track.held_title = Some((nav_gen, title.to_owned()));
            TitleStep::Held
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_generations_are_stale() {
        let mut track = NavTrack::default();
        assert_eq!(
            navigated(&mut track, 5),
            NavigationStep::Start { title: None }
        );
        assert_eq!(navigated(&mut track, 4), NavigationStep::Stale);
        assert_eq!(title_changed(&mut track, 4, "old"), TitleStep::Stale);
        assert_eq!(title_changed(&mut track, 5, "new"), TitleStep::Current);
    }

    #[test]
    fn same_generation_keeps_the_title() {
        let mut track = NavTrack::default();
        navigated(&mut track, 1);
        assert_eq!(navigated(&mut track, 1), NavigationStep::Redirect);
    }

    #[test]
    fn a_title_that_arrives_first_waits_for_its_navigation() {
        let mut track = NavTrack::default();
        navigated(&mut track, 1);
        assert_eq!(title_changed(&mut track, 2, "Next"), TitleStep::Held);
        assert_eq!(
            navigated(&mut track, 2),
            NavigationStep::Start {
                title: Some("Next".to_owned())
            }
        );
    }

    #[test]
    fn a_held_title_for_a_later_generation_survives_an_earlier_start() {
        let mut track = NavTrack::default();
        navigated(&mut track, 1);
        assert_eq!(title_changed(&mut track, 3, "Later"), TitleStep::Held);
        assert_eq!(
            navigated(&mut track, 2),
            NavigationStep::Start { title: None }
        );
        assert_eq!(
            navigated(&mut track, 3),
            NavigationStep::Start {
                title: Some("Later".to_owned())
            }
        );
    }
}
