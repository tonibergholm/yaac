//! Engine events. Each one is checked against the current state and becomes a plan, is ignored, or
//! (for the core's own window closing) stops the actor.

use crate::navigation::{self, NavigationStep, TitleStep};
use crate::state::{Plan, State, Tx, now_millis};
use crate::urls;
use yaac_api::{ApiError, Placement};
use yaac_engine::EngineEvent;
use yaac_model::{ContainerKind, NodeId, SpaceId, ViewId};

pub(crate) enum Step {
    Ignore,
    Plan(Box<Plan>),
    Stop,
}

pub(crate) fn on_engine_event(state: &State, event: EngineEvent) -> Step {
    match event {
        EngineEvent::Created { .. } | EngineEvent::LoadState { .. } => Step::Ignore,
        EngineEvent::Navigated { view, nav_gen, url } => navigated(state, view, nav_gen, url),
        EngineEvent::TitleChanged {
            view,
            nav_gen,
            title,
        } => title_changed(state, view, nav_gen, &title),
        EngineEvent::Closed { view } | EngineEvent::Crashed { view } => detached(state, view),
        EngineEvent::PopupRequested {
            opener,
            url,
            user_gesture,
        } => popup(state, opener, url, user_gesture),
        EngineEvent::WindowClosed { window } if window == state.window => Step::Stop,
        EngineEvent::WindowClosed { .. } => Step::Ignore,
    }
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
    Step::Plan(Box::new(tx.finish()))
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
    Step::Plan(Box::new(tx.finish()))
}

/// A view closed or crashed on its own. The node stays; it is no longer live.
fn detached(state: &State, view: ViewId) -> Step {
    let Some(&node) = state.view_nodes.get(&view) else {
        return Step::Ignore;
    };
    let mut tx = Tx::new(state.clone());
    tx.detach_view(node);
    Step::Plan(Box::new(tx.finish()))
}

/// A user-gesture popup opens a tab in the opener's Space. Other popups are dropped.
fn popup(state: &State, opener: ViewId, url: String, user_gesture: bool) -> Step {
    // Page content never names an app:// URL, whatever the opener.
    if !user_gesture || urls::check(&url, false).is_err() {
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

fn popup_space(tx: &Tx, opener: NodeId) -> Result<SpaceId, ApiError> {
    let container = tx.container(tx.node(opener)?.container_id)?;
    match container.space_id {
        Some(space) => Ok(space),
        None => Ok(tx.session()?.space_id),
    }
}
