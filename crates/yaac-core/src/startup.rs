//! Startup. An empty store is bootstrapped; a stored workspace is validated. Either way the window
//! is reconciled with the engine: every restored tab comes up suspended, and only the selected tab
//! gets a view.

use crate::error::internal;
use crate::state::{Plan, SIDEBAR_WIDTH, State, Tx, now_millis, url_or_blank};
use crate::urls::NEW_TAB_URL;
use crate::{CoreError, StoreError};
use yaac_api::{ApiError, Placement};
use yaac_engine::{EngineCommand, WindowSpec};
use yaac_model::{
    Container, ContainerId, ContainerKind, NodeId, NodeKind, Position, Profile, ProfileId,
    ProfileKind, SessionState, WindowId, Workspace,
};

const WINDOW_TITLE: &str = "yaac";
const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 800;

pub(crate) fn initialize(loaded: Workspace) -> Result<Plan, CoreError> {
    let window = loaded
        .sessions
        .keys()
        .next()
        .copied()
        .unwrap_or_else(WindowId::new);
    let mut tx = Tx::new(State {
        ws: loaded,
        window,
        ..State::default()
    });
    if tx.state().ws.profiles.is_empty() {
        bootstrap(&mut tx).map_err(startup_error)?;
    } else {
        validate(tx.state())?;
    }
    reconcile(&mut tx).map_err(startup_error)?;
    Ok(tx.finish())
}

fn startup_error(error: ApiError) -> CoreError {
    CoreError::Store(StoreError::Corrupt(error.message))
}

fn validate(state: &State) -> Result<(), CoreError> {
    state
        .ws
        .tree
        .validate()
        .map_err(|error| StoreError::Corrupt(error.to_string()))?;
    if state.ws.spaces.is_empty() {
        return Err(StoreError::Corrupt("the store has profiles but no space".to_owned()).into());
    }
    Ok(())
}

/// One default profile, a favorites container, "Space 1" with its containers, and one about:blank
/// tab in its today container, selected in the window.
fn bootstrap(tx: &mut Tx) -> Result<(), ApiError> {
    let profile = ProfileId::new();
    tx.put_profile(Profile {
        id: profile,
        kind: ProfileKind::Default,
        name: "Default".to_owned(),
        cache_dir: format!("profiles/{profile}"),
        created_at: now_millis(),
    });
    tx.put_container(Container {
        id: ContainerId::new(),
        profile_id: profile,
        space_id: None,
        kind: ContainerKind::Favorites,
    });
    let space = tx.add_space(profile, "Space 1".to_owned(), Position::first());
    let tab = tx.insert_tab(
        &Placement {
            space,
            container: ContainerKind::Today,
            parent: None,
            before: None,
        },
        NEW_TAB_URL.to_owned(),
    )?;
    let window = tx.state().window;
    tx.put_session(SessionState {
        window_id: window,
        space_id: space,
        selected_node_id: Some(tab),
        sidebar_width: SIDEBAR_WIDTH,
    });
    Ok(())
}

fn reconcile(tx: &mut Tx) -> Result<(), ApiError> {
    let window = tx.state().window;
    let spaces = tx.spaces_by_position();
    let first = *spaces
        .first()
        .ok_or_else(|| internal("the store has no space"))?;
    let mut session = tx
        .state()
        .ws
        .sessions
        .get(&window)
        .cloned()
        .unwrap_or(SessionState {
            window_id: window,
            space_id: first,
            selected_node_id: None,
            sidebar_width: SIDEBAR_WIDTH,
        });
    if !spaces.contains(&session.space_id) {
        session.space_id = first;
    }
    let node = match choose_selection(tx, &session)? {
        Some(node) => node,
        None => tx.insert_tab(
            &Placement {
                space: session.space_id,
                container: ContainerKind::Today,
                parent: None,
                before: None,
            },
            NEW_TAB_URL.to_owned(),
        )?,
    };
    if let Some(space) = tx.container(tx.node(node)?.container_id)?.space_id {
        session.space_id = space;
    }
    session.selected_node_id = Some(node);
    let profile = tx.default_profile()?;

    tx.emit(EngineCommand::CreateWindow {
        window,
        spec: WindowSpec {
            title: WINDOW_TITLE.to_owned(),
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        },
    });
    tx.attach_sidebar(profile);
    tx.put_session(session);
    let url = url_or_blank(tx.node(node)?);
    tx.attach_view(node, url, true)?;
    Ok(())
}

/// The stored selection if it is still a usable tab; else the first tab of the Space's today
/// container.
fn choose_selection(tx: &Tx, session: &SessionState) -> Result<Option<NodeId>, ApiError> {
    if let Some(id) = session
        .selected_node_id
        .filter(|id| usable_selection(tx, *id))
    {
        return Ok(Some(id));
    }
    let today = tx.container_id(session.space_id, ContainerKind::Today)?;
    Ok(tx
        .state()
        .ws
        .tree
        .children(today, None)
        .into_iter()
        .find(|node| node.kind == NodeKind::Tab)
        .map(|node| node.id))
}

fn usable_selection(tx: &Tx, id: NodeId) -> bool {
    let Ok(node) = tx.node(id) else {
        return false;
    };
    if node.kind != NodeKind::Tab {
        return false;
    }
    tx.container(node.container_id)
        .is_ok_and(|container| container.kind != ContainerKind::Archive)
}
