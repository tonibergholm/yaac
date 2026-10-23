//! The actor's state and `Tx`, the transaction that turns one command or engine event into the next
//! state. A `Tx` works on a copy. The actor swaps the copy in only after the store has committed the
//! transaction's ops, so a failed commit leaves the state and the engine untouched.

use crate::StoreOp;
use crate::error::{
    bad_request, internal, not_found, position as position_error, tree as tree_error,
};
use crate::navigation::NavTrack;
use crate::urls::{NEW_TAB_URL, SIDEBAR_URL};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};
use yaac_api::{ApiError, ContainerView, NodeView, Placement, Snapshot, SpaceView, Topic};
use yaac_engine::{EngineCommand, ViewKind, ViewSpec};
use yaac_model::{
    Container, ContainerId, ContainerKind, Millis, Node, NodeId, NodeKind, NodeState, Position,
    Profile, ProfileId, ProfileKind, SessionState, Space, SpaceId, TreeError, ViewId, WindowId,
    Workspace,
};

pub(crate) const SIDEBAR_WIDTH: u32 = 280;

/// Everything the actor keeps in memory. `ws` mirrors the store; the rest is runtime-only.
#[derive(Debug, Clone, Default)]
pub(crate) struct State {
    pub(crate) ws: Workspace,
    pub(crate) window: WindowId,
    pub(crate) sidebar_views: BTreeSet<ViewId>,
    pub(crate) views: BTreeMap<NodeId, ViewId>,
    pub(crate) view_nodes: BTreeMap<ViewId, NodeId>,
    pub(crate) nav: BTreeMap<ViewId, NavTrack>,
    pub(crate) next_view: u64,
}

/// One committed-or-discarded transaction: the next state, the store ops, the engine commands
/// (sent only after the commit), and the topic to publish (`None` when nothing visible changed).
#[derive(Debug)]
pub(crate) struct Plan {
    pub(crate) state: State,
    pub(crate) ops: Vec<StoreOp>,
    pub(crate) effects: Vec<EngineCommand>,
    pub(crate) topic: Option<Topic>,
}

pub(crate) struct Tx {
    state: State,
    ops: Vec<StoreOp>,
    effects: Vec<EngineCommand>,
    tree_changed: bool,
    session_changed: bool,
}

impl Tx {
    pub(crate) fn new(state: State) -> Self {
        Self {
            state,
            ops: Vec::new(),
            effects: Vec::new(),
            tree_changed: false,
            session_changed: false,
        }
    }

    /// Bumps the revision once if anything visible changed.
    pub(crate) fn finish(mut self) -> Plan {
        let topic = if self.tree_changed {
            Some(Topic::Tree)
        } else if self.session_changed {
            Some(Topic::Session)
        } else {
            None
        };
        if topic.is_some() {
            self.state.ws.rev += 1;
        }
        Plan {
            state: self.state,
            ops: self.ops,
            effects: self.effects,
            topic,
        }
    }

    pub(crate) fn state(&self) -> &State {
        &self.state
    }

    pub(crate) fn emit(&mut self, command: EngineCommand) {
        self.effects.push(command);
    }

    // Lookups.

    pub(crate) fn node(&self, id: NodeId) -> Result<&Node, ApiError> {
        self.state
            .ws
            .tree
            .get(id)
            .ok_or_else(|| not_found(format!("node {id} not found")))
    }

    pub(crate) fn container(&self, id: ContainerId) -> Result<&Container, ApiError> {
        self.state
            .ws
            .containers
            .get(&id)
            .ok_or_else(|| internal(format!("container {id} is missing")))
    }

    pub(crate) fn space(&self, id: SpaceId) -> Result<&Space, ApiError> {
        self.state
            .ws
            .spaces
            .get(&id)
            .ok_or_else(|| not_found(format!("space {id} not found")))
    }

    pub(crate) fn session(&self) -> Result<SessionState, ApiError> {
        self.state
            .ws
            .sessions
            .get(&self.state.window)
            .cloned()
            .ok_or_else(|| internal("the window has no session"))
    }

    pub(crate) fn selected(&self) -> Option<NodeId> {
        self.state
            .ws
            .sessions
            .get(&self.state.window)
            .and_then(|session| session.selected_node_id)
    }

    pub(crate) fn view_of(&self, node: NodeId) -> Option<ViewId> {
        self.state.views.get(&node).copied()
    }

    pub(crate) fn tab(&self, node: NodeId) -> Result<Node, ApiError> {
        let found = self.node(node)?;
        if found.kind == NodeKind::Tab {
            Ok(found.clone())
        } else {
            Err(bad_request("the node is not a tab"))
        }
    }

    pub(crate) fn default_profile(&self) -> Result<ProfileId, ApiError> {
        self.state
            .ws
            .profiles
            .values()
            .find(|profile| profile.kind == ProfileKind::Default)
            .map(|profile| profile.id)
            .ok_or_else(|| internal("the store has no default profile"))
    }

    pub(crate) fn spaces_by_position(&self) -> Vec<SpaceId> {
        spaces_sorted(&self.state.ws)
            .iter()
            .map(|space| space.id)
            .collect()
    }

    pub(crate) fn last_space_position(&self) -> Option<Position> {
        spaces_sorted(&self.state.ws)
            .last()
            .map(|space| space.position.clone())
    }

    pub(crate) fn container_id(
        &self,
        space: SpaceId,
        kind: ContainerKind,
    ) -> Result<ContainerId, ApiError> {
        let profile = self.space(space)?.profile_id;
        find_container(&self.state.ws, profile, Some(space), kind)
            .map(|container| container.id)
            .ok_or_else(|| not_found(format!("the space has no {kind:?} container")))
    }

    // Durable state.

    pub(crate) fn put_profile(&mut self, profile: Profile) {
        self.state.ws.profiles.insert(profile.id, profile.clone());
        self.ops.push(StoreOp::UpsertProfile(profile));
        self.tree_changed = true;
    }

    pub(crate) fn put_container(&mut self, container: Container) {
        self.state
            .ws
            .containers
            .insert(container.id, container.clone());
        self.ops.push(StoreOp::UpsertContainer(container));
        self.tree_changed = true;
    }

    fn put_space(&mut self, space: Space) {
        self.state.ws.spaces.insert(space.id, space.clone());
        self.ops.push(StoreOp::UpsertSpace(space));
        self.tree_changed = true;
    }

    /// A new Space with its pinned, today and archive containers. The favorites container is
    /// per profile and is not created here.
    pub(crate) fn add_space(
        &mut self,
        profile: ProfileId,
        name: String,
        position: Position,
    ) -> SpaceId {
        let space = Space {
            id: SpaceId::new(),
            profile_id: profile,
            name,
            theme: None,
            position,
            agent_enabled: false,
        };
        let id = space.id;
        self.put_space(space);
        for kind in [
            ContainerKind::Pinned,
            ContainerKind::Today,
            ContainerKind::Archive,
        ] {
            self.put_container(Container {
                id: ContainerId::new(),
                profile_id: profile,
                space_id: Some(id),
                kind,
            });
        }
        id
    }

    /// Inserts or updates a node and persists it.
    pub(crate) fn put_node(&mut self, node: Node) -> Result<(), ApiError> {
        let id = node.id;
        if self.state.ws.tree.get(id).is_some() {
            self.state.ws.tree.update(node).map_err(tree_error)?;
        } else {
            self.state.ws.tree.insert(node).map_err(tree_error)?;
        }
        self.persist_node(id)
    }

    fn persist_node(&mut self, id: NodeId) -> Result<(), ApiError> {
        let node = self.node(id)?.clone();
        self.ops.push(StoreOp::UpsertNode(node));
        self.tree_changed = true;
        Ok(())
    }

    /// Adds a node at a placement. Does not select or create a view.
    pub(crate) fn insert_node(
        &mut self,
        placement: &Placement,
        kind: NodeKind,
        title: String,
        url: Option<String>,
    ) -> Result<NodeId, ApiError> {
        let container = self.container_id(placement.space, placement.container)?;
        let position = self.slot(container, placement.parent, placement.before)?;
        let now = now_millis();
        let node = Node {
            id: NodeId::new(),
            container_id: container,
            parent_id: placement.parent,
            position,
            kind,
            title,
            url,
            state: NodeState::Suspended,
            is_unread: false,
            created_at: now,
            last_active_at: now,
            updated_at: now,
            origin_device: None,
            rev: 0,
        };
        let id = node.id;
        self.put_node(node)?;
        Ok(id)
    }

    pub(crate) fn insert_tab(
        &mut self,
        placement: &Placement,
        url: String,
    ) -> Result<NodeId, ApiError> {
        self.insert_node(placement, NodeKind::Tab, String::new(), Some(url))
    }

    /// Removes a node and its subtree. Their live views are closed.
    pub(crate) fn remove_subtree(&mut self, id: NodeId) -> Result<Vec<Node>, ApiError> {
        let removed = self.state.ws.tree.remove_subtree(id).map_err(tree_error)?;
        for node in &removed {
            self.ops.push(StoreOp::DeleteNode(node.id));
            if let Some(view) = self.detach_view(node.id) {
                self.effects.push(EngineCommand::Close { view });
            }
        }
        self.tree_changed = true;
        Ok(removed)
    }

    pub(crate) fn move_node(
        &mut self,
        node: NodeId,
        container: ContainerId,
        parent: Option<NodeId>,
        position: Position,
    ) -> Result<(), ApiError> {
        let outcome = self
            .state
            .ws
            .tree
            .move_node(node, container, parent, position)
            .map_err(tree_error)?;
        for changed in outcome.changed {
            self.persist_node(changed)?;
        }
        self.tree_changed = true;
        Ok(())
    }

    /// Records the window session. A session identical to the stored one changes nothing.
    pub(crate) fn put_session(&mut self, session: SessionState) {
        if self.state.ws.sessions.get(&session.window_id) == Some(&session) {
            return;
        }
        self.state
            .ws
            .sessions
            .insert(session.window_id, session.clone());
        self.ops.push(StoreOp::UpsertSession(session));
        self.session_changed = true;
    }

    // Views. Runtime only: no store op, but every change bumps the revision.

    fn alloc_view(&mut self) -> ViewId {
        self.state.next_view += 1;
        ViewId(self.state.next_view)
    }

    pub(crate) fn attach_sidebar(&mut self, profile: ProfileId) {
        let view = self.alloc_view();
        self.effects.push(EngineCommand::CreateView {
            view,
            window: self.state.window,
            spec: ViewSpec {
                kind: ViewKind::Sidebar,
                profile,
                url: SIDEBAR_URL.to_owned(),
            },
        });
        self.state.sidebar_views.insert(view);
    }

    /// Creates the content view of a node with `url` as its starting page, and shows it when asked.
    pub(crate) fn attach_view(
        &mut self,
        node: NodeId,
        url: String,
        show: bool,
    ) -> Result<ViewId, ApiError> {
        let container = self.node(node)?.container_id;
        let profile = self.container(container)?.profile_id;
        let view = self.alloc_view();
        self.effects.push(EngineCommand::CreateView {
            view,
            window: self.state.window,
            spec: ViewSpec {
                kind: ViewKind::Content,
                profile,
                url,
            },
        });
        if show {
            self.effects.push(EngineCommand::Show { view });
        }
        self.state.views.insert(node, view);
        self.state.view_nodes.insert(view, node);
        self.tree_changed = true;
        Ok(view)
    }

    pub(crate) fn detach_view(&mut self, node: NodeId) -> Option<ViewId> {
        let view = self.state.views.remove(&node)?;
        self.state.view_nodes.remove(&view);
        self.state.nav.remove(&view);
        self.tree_changed = true;
        Some(view)
    }

    pub(crate) fn nav_track(&self, view: ViewId) -> NavTrack {
        self.state.nav.get(&view).cloned().unwrap_or_default()
    }

    pub(crate) fn set_nav(&mut self, view: ViewId, track: NavTrack) {
        self.state.nav.insert(view, track);
    }

    /// Makes a tab the selected one. A suspended tab gets its view first.
    pub(crate) fn select(&mut self, node: NodeId) -> Result<(), ApiError> {
        let tab = self.tab(node)?;
        match self.view_of(node) {
            Some(view) => self.emit(EngineCommand::Show { view }),
            None => {
                self.attach_view(node, url_or_blank(&tab), true)?;
            }
        }
        let mut session = self.session()?;
        session.selected_node_id = Some(node);
        if let Some(space) = self.container(tab.container_id)?.space_id {
            session.space_id = space;
        }
        self.put_session(session);
        Ok(())
    }

    // Placement.

    /// The sort key for a child of `parent` (or the container's top level) placed before `before`.
    /// Renumbers the siblings once if the key space is exhausted.
    pub(crate) fn slot(
        &mut self,
        container: ContainerId,
        parent: Option<NodeId>,
        before: Option<NodeId>,
    ) -> Result<Position, ApiError> {
        if let Some(parent) = parent {
            let folder = self.node(parent)?;
            if !matches!(folder.kind, NodeKind::Folder | NodeKind::Split) {
                return Err(bad_request("the parent is not a folder or split"));
            }
            if folder.container_id != container {
                return Err(bad_request("the parent is in another container"));
            }
        }
        if let Some(before) = before {
            let sibling = self.node(before)?;
            if sibling.container_id != container || sibling.parent_id != parent {
                return Err(bad_request(
                    "before is not a sibling at the target position",
                ));
            }
        }
        match self.state.ws.tree.position_for(container, parent, before) {
            Err(TreeError::PositionExhausted) => {
                self.renumber(container, parent)?;
                self.state
                    .ws
                    .tree
                    .position_for(container, parent, before)
                    .map_err(tree_error)
            }
            other => other.map_err(tree_error),
        }
    }

    fn renumber(&mut self, container: ContainerId, parent: Option<NodeId>) -> Result<(), ApiError> {
        let ids: Vec<NodeId> = self
            .state
            .ws
            .tree
            .children(container, parent)
            .iter()
            .map(|node| node.id)
            .collect();
        let mut previous: Option<Position> = None;
        for id in ids {
            let position = match &previous {
                None => Position::first(),
                Some(last) => Position::between(Some(last), None).map_err(position_error)?,
            };
            self.move_node(id, container, parent, position.clone())?;
            previous = Some(position);
        }
        Ok(())
    }

    /// True when `candidate` is `root` or lies beneath it.
    pub(crate) fn is_within(
        &self,
        candidate: Option<NodeId>,
        root: NodeId,
    ) -> Result<bool, ApiError> {
        let mut current = candidate;
        while let Some(id) = current {
            if id == root {
                return Ok(true);
            }
            current = self.node(id)?.parent_id;
        }
        Ok(false)
    }

    /// The next tab sibling after `node`, else the previous one.
    pub(crate) fn neighbour_tab(&self, node: &Node) -> Option<NodeId> {
        let siblings = self
            .state
            .ws
            .tree
            .children(node.container_id, node.parent_id);
        let index = siblings.iter().position(|sibling| sibling.id == node.id)?;
        siblings[index + 1..]
            .iter()
            .find(|sibling| sibling.kind == NodeKind::Tab)
            .or_else(|| {
                siblings[..index]
                    .iter()
                    .rev()
                    .find(|sibling| sibling.kind == NodeKind::Tab)
            })
            .map(|sibling| sibling.id)
    }
}

pub(crate) fn now_millis() -> Millis {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

pub(crate) fn url_or_blank(node: &Node) -> String {
    node.url.clone().unwrap_or_else(|| NEW_TAB_URL.to_owned())
}

/// The container of `kind` for a Space. Favorites belong to the profile and have no Space.
pub(crate) fn find_container(
    ws: &Workspace,
    profile: ProfileId,
    space: Option<SpaceId>,
    kind: ContainerKind,
) -> Option<&Container> {
    ws.containers.values().find(|container| {
        container.kind == kind
            && match kind {
                ContainerKind::Favorites => {
                    container.space_id.is_none() && container.profile_id == profile
                }
                _ => container.space_id == space,
            }
    })
}

fn spaces_sorted(ws: &Workspace) -> Vec<&Space> {
    let mut spaces: Vec<&Space> = ws.spaces.values().collect();
    spaces.sort_by(|a, b| a.position.cmp(&b.position).then(a.id.cmp(&b.id)));
    spaces
}

fn node_views(state: &State, container: ContainerId, parent: Option<NodeId>) -> Vec<NodeView> {
    state
        .ws
        .tree
        .children(container, parent)
        .into_iter()
        .map(|node| NodeView {
            id: node.id,
            kind: node.kind,
            title: node.title.clone(),
            url: node.url.clone(),
            is_unread: node.is_unread,
            live: state.views.contains_key(&node.id),
            children: node_views(state, container, Some(node.id)),
        })
        .collect()
}

/// The whole state the window draws. Favorites appear in every Space of the profile.
pub(crate) fn snapshot(state: &State) -> Result<Snapshot, ApiError> {
    let session = state
        .ws
        .sessions
        .get(&state.window)
        .ok_or_else(|| internal("the window has no session"))?;
    let spaces = spaces_sorted(&state.ws)
        .into_iter()
        .map(|space| SpaceView {
            id: space.id,
            name: space.name.clone(),
            theme: space.theme.clone(),
            containers: [
                ContainerKind::Favorites,
                ContainerKind::Pinned,
                ContainerKind::Today,
                ContainerKind::Archive,
            ]
            .into_iter()
            .filter_map(|kind| find_container(&state.ws, space.profile_id, Some(space.id), kind))
            .map(|container| ContainerView {
                kind: container.kind,
                nodes: node_views(state, container.id, None),
            })
            .collect(),
        })
        .collect();
    Ok(Snapshot {
        rev: state.ws.rev,
        window: state.window,
        active_space: session.space_id,
        selected: session.selected_node_id,
        spaces,
    })
}
