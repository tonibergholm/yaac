//! Engine events. Each one is checked against the current state and becomes a plan, is ignored, or
//! (for the core's own window closing) stops the actor.

use crate::error::internal;
use crate::navigation::{self, NavigationStep, TitleStep};
use crate::state::{Plan, State, Tx, now_millis};
use crate::urls;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use yaac_api::{ApiError, Placement};
use yaac_engine::EngineEvent;
use yaac_model::{ContainerKind, NodeId, SpaceId, ViewId};

/// Sidebar recreations allowed within `SIDEBAR_WINDOW`.
const SIDEBAR_BUDGET: usize = 3;
const SIDEBAR_WINDOW: Duration = Duration::from_secs(30);
/// A sidebar alive this long counts as healthy: its death starts a fresh budget.
const SIDEBAR_HEALTHY_AFTER: Duration = Duration::from_secs(10);
/// A close still unconfirmed after this long is reported once.
pub(crate) const CLOSE_STALL: Duration = Duration::from_secs(60);

pub(crate) enum Step {
    Ignore,
    Plan(Box<Plan>),
    Stop,
}

pub(crate) fn on_engine_event(state: &State, event: EngineEvent, now: Instant) -> Step {
    match event {
        EngineEvent::Created { view } => created(state, view, now),
        EngineEvent::LoadState { .. } => Step::Ignore,
        EngineEvent::Navigated { view, nav_gen, url } => navigated(state, view, nav_gen, url),
        EngineEvent::TitleChanged {
            view,
            nav_gen,
            title,
        } => title_changed(state, view, nav_gen, &title),
        EngineEvent::Closed { view } | EngineEvent::Crashed { view } => detached(state, view, now),
        EngineEvent::PopupRequested {
            opener,
            url,
            user_gesture,
        } => popup(state, opener, url, user_gesture),
        EngineEvent::CreateFailed { view } => create_failed(state, view, now),
        EngineEvent::WindowClosing { window } if window == state.window => closing(state, now),
        EngineEvent::WindowClosing { .. } => Step::Ignore,
        EngineEvent::WindowClosed { window } if window == state.window => Step::Stop,
        EngineEvent::WindowClosed { .. } => Step::Ignore,
    }
}

/// The window is closing. From now on nothing reaches the engine for it, and its views' exits are
/// not handled, so the sidebar is not recreated during quit. A repeat is ignored.
fn closing(state: &State, now: Instant) -> Step {
    if state.closing {
        return Step::Ignore;
    }
    let mut tx = Tx::new(state.clone());
    tx.begin_closing(now);
    Step::Plan(Box::new(tx.finish()))
}

/// A sidebar `Created` starts its healthy-life clock.
fn created(state: &State, view: ViewId, now: Instant) -> Step {
    if !state.sidebar_views.contains(&view) {
        return Step::Ignore;
    }
    let mut tx = Tx::new(state.clone());
    tx.set_sidebar_alive_since(Some((view, now)));
    Step::Plan(Box::new(tx.finish()))
}

/// A `CreateView` never produced a view. A content view's node becomes not live, and the selection
/// stays. A sidebar death is handled by `sidebar_died`.
fn create_failed(state: &State, view: ViewId, now: Instant) -> Step {
    if state.closing {
        return Step::Ignore;
    }
    if state.sidebar_views.contains(&view) {
        return sidebar_died(state, view, now);
    }
    let Some(&node) = state.view_nodes.get(&view) else {
        return Step::Ignore;
    };
    let mut tx = Tx::new(state.clone());
    tx.detach_view(node);
    Step::Plan(Box::new(tx.finish()))
}

/// A sidebar view died: it crashed, closed, or never came up. The death is recreated while the
/// budget lasts: at most `SIDEBAR_BUDGET` recreations within `SIDEBAR_WINDOW`, whatever the cause.
/// A sidebar that had lived for `SIDEBAR_HEALTHY_AFTER` resets the budget before it is counted.
fn sidebar_died(state: &State, view: ViewId, now: Instant) -> Step {
    let mut tx = Tx::new(state.clone());
    tx.forget_sidebar(view);
    tx.set_sidebar_alive_since(None);
    let mut recreations: VecDeque<Instant> = state.sidebar_recreations.clone();
    let healthy = matches!(
        state.sidebar_alive_since,
        Some((alive, since)) if alive == view && now.saturating_duration_since(since) >= SIDEBAR_HEALTHY_AFTER
    );
    if healthy {
        recreations.clear();
    }
    while recreations
        .front()
        .is_some_and(|at| now.saturating_duration_since(*at) >= SIDEBAR_WINDOW)
    {
        recreations.pop_front();
    }
    if recreations.len() >= SIDEBAR_BUDGET {
        tracing::error!(
            "the sidebar died {} times within {SIDEBAR_WINDOW:?}; not recreating it",
            recreations.len() + 1
        );
        tx.set_sidebar_recreations(recreations);
        return Step::Plan(Box::new(tx.finish()));
    }
    recreations.push_back(now);
    tx.set_sidebar_recreations(recreations);
    let Ok(profile) = tx.default_profile() else {
        return Step::Ignore;
    };
    tx.attach_sidebar(profile);
    Step::Plan(Box::new(tx.finish()))
}

/// Reports a close that has gone unconfirmed for `CLOSE_STALL`, once. The core keeps refusing the
/// commands that create views and keeps answering queries.
pub(crate) fn stalled(state: &State, now: Instant) -> Option<Plan> {
    let since = state.closing_since?;
    if state.closing_warned || now.saturating_duration_since(since) < CLOSE_STALL {
        return None;
    }
    tracing::warn!(
        "the window has been closing for {CLOSE_STALL:?} without WindowClosed; commands that create views stay refused"
    );
    let mut tx = Tx::new(state.clone());
    tx.mark_closing_warned();
    Some(tx.finish())
}

/// A URL or title change. It applies in memory at once; the actor persists it on its next flush.
fn coalesced(tx: Tx) -> Step {
    let mut plan = tx.finish();
    plan.coalesce = true;
    Step::Plan(Box::new(plan))
}

fn navigated(state: &State, view: ViewId, nav_gen: u64, url: String) -> Step {
    let Some(&node_id) = state.view_nodes.get(&view) else {
        return Step::Ignore;
    };
    let mut tx = Tx::new(state.clone());
    let mut track = tx.nav_track(view);
    let title = match navigation::navigated(&mut track, nav_gen) {
        NavigationStep::Stale => return Step::Ignore,
        NavigationStep::Redirect => None,
        NavigationStep::Start { title } => Some(title.unwrap_or_default()),
    };
    tx.set_nav(view, track);
    let Ok(current) = tx.node(node_id).cloned() else {
        return Step::Ignore;
    };
    let mut updated = current.clone();
    if let Some(title) = title {
        updated.title = title;
    }
    updated.url = Some(url);
    if updated.url != current.url || updated.title != current.title {
        updated.updated_at = now_millis();
        if tx.put_node(updated).is_err() {
            return Step::Ignore;
        }
    }
    coalesced(tx)
}

fn title_changed(state: &State, view: ViewId, nav_gen: u64, title: &str) -> Step {
    let Some(&node_id) = state.view_nodes.get(&view) else {
        return Step::Ignore;
    };
    let mut tx = Tx::new(state.clone());
    let mut track = tx.nav_track(view);
    let step = navigation::title_changed(&mut track, nav_gen, title);
    if step == TitleStep::Stale {
        return Step::Ignore;
    }
    tx.set_nav(view, track);
    if step == TitleStep::Current {
        let Ok(current) = tx.node(node_id).cloned() else {
            return Step::Ignore;
        };
        if current.title != title {
            let mut updated = current;
            updated.title = title.to_owned();
            updated.updated_at = now_millis();
            if tx.put_node(updated).is_err() {
                return Step::Ignore;
            }
        }
    }
    coalesced(tx)
}

/// A view closed or crashed on its own. A content view's node stays, no longer live. A sidebar
/// death goes through `sidebar_died`.
fn detached(state: &State, view: ViewId, now: Instant) -> Step {
    if state.closing {
        return Step::Ignore;
    }
    if state.sidebar_views.contains(&view) {
        return sidebar_died(state, view, now);
    }
    let Some(&node) = state.view_nodes.get(&view) else {
        return Step::Ignore;
    };
    let mut tx = Tx::new(state.clone());
    tx.detach_view(node);
    Step::Plan(Box::new(tx.finish()))
}

/// A user-gesture popup opens a tab in the opener's Space. Other popups are dropped.
fn popup(state: &State, opener: ViewId, url: String, user_gesture: bool) -> Step {
    // Page content never names an app:// URL, and `check` refuses it for everyone.
    if state.closing || !user_gesture || urls::check(&url).is_err() {
        return Step::Ignore;
    }
    let Some(&opener_node) = state.view_nodes.get(&opener) else {
        return Step::Ignore;
    };
    let mut tx = Tx::new(state.clone());
    let Ok(space) = popup_space(&tx, opener_node) else {
        return Step::Ignore;
    };
    let placement = Placement {
        space,
        container: ContainerKind::Today,
        parent: None,
        before: None,
    };
    let Ok(tab) = tx.insert_tab(&placement, url) else {
        return Step::Ignore;
    };
    if tx.select(tab).is_err() {
        return Step::Ignore;
    }
    Step::Plan(Box::new(tx.finish()))
}

/// The Space a popup opens in. A tab in a Space uses that Space. A favorites tab uses the window's
/// active Space when it belongs to the same profile, else the profile's first Space by position.
fn popup_space(tx: &Tx, opener: NodeId) -> Result<SpaceId, ApiError> {
    let container = tx.container(tx.node(opener)?.container_id)?;
    if let Some(space) = container.space_id {
        return Ok(space);
    }
    let profile = container.profile_id;
    let active = tx.session()?.space_id;
    if tx
        .space(active)
        .is_ok_and(|space| space.profile_id == profile)
    {
        return Ok(active);
    }
    tx.spaces_by_position()
        .into_iter()
        .find(|id| tx.space(*id).is_ok_and(|space| space.profile_id == profile))
        .ok_or_else(|| internal("the profile has no space"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_close_that_stalls_is_reported_once() {
        let start = Instant::now();
        let mut tx = Tx::new(State::default());
        tx.begin_closing(start);
        let state = tx.finish().state;

        assert!(stalled(&state, start + CLOSE_STALL - Duration::from_secs(1)).is_none());
        let reported = stalled(&state, start + CLOSE_STALL).expect("the stall is reported");
        assert!(reported.state.closing_warned);
        assert!(stalled(&reported.state, start + CLOSE_STALL * 3).is_none());
    }
}
