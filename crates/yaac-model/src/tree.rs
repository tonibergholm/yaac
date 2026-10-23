use crate::{ContainerId, Node, NodeId, Position};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TreeError {
    #[error("node not found: {0}")]
    NotFound(NodeId),
    #[error("node already exists: {0}")]
    Duplicate(NodeId),
    #[error("move would create a cycle")]
    Cycle,
    #[error("a child's container must equal its parent's")]
    ContainerMismatch,
    #[error("parent is not a folder or split")]
    ParentNotContainer,
    #[error("a split holds two to four tab children and nothing else")]
    InvalidSplit,
    #[error("sibling position key space exhausted; renumber the siblings")]
    PositionExhausted,
}

/// What a move changed, so the caller can persist exactly those rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveOutcome {
    /// The moved root and every descendant whose row changed (the root always; descendants only
    /// when `container_id` was rewritten).
    pub changed: Vec<NodeId>,
}

/// All nodes of a workspace, with the invariants from `docs/architecture.md` section 7 enforced on
/// every mutation: no cycles, a child's `container_id` equals its parent's, a split has two to four
/// tab children, siblings sort by (`position`, `id`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TreeState {
    pub(crate) nodes: BTreeMap<NodeId, Node>,
}

impl TreeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a tree from stored rows, validating every invariant.
    pub fn from_nodes(nodes: impl IntoIterator<Item = Node>) -> Result<Self, TreeError> {
        let _ = nodes.into_iter();
        todo!("m1/model")
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Children of `parent` (or the container's top level when `None`), sorted by (position, id).
    pub fn children(&self, container: ContainerId, parent: Option<NodeId>) -> Vec<&Node> {
        let _ = (container, parent);
        todo!("m1/model")
    }

    /// The position for a new or moved node placed before `before` (or last when `None`) among the
    /// children of `parent` in `container`.
    pub fn position_for(
        &self,
        container: ContainerId,
        parent: Option<NodeId>,
        before: Option<NodeId>,
    ) -> Result<Position, TreeError> {
        let _ = (container, parent, before);
        todo!("m1/model")
    }

    pub fn insert(&mut self, node: Node) -> Result<(), TreeError> {
        let _ = node;
        todo!("m1/model")
    }

    /// Removes the node and its whole subtree. Returns the removed nodes, root first.
    pub fn remove_subtree(&mut self, id: NodeId) -> Result<Vec<Node>, TreeError> {
        let _ = id;
        todo!("m1/model")
    }

    /// Moves the subtree rooted at `id` under `new_parent` in `new_container` at `new_position`.
    /// Rewrites `container_id` on every descendant when the container changes.
    pub fn move_node(
        &mut self,
        id: NodeId,
        new_container: ContainerId,
        new_parent: Option<NodeId>,
        new_position: Position,
    ) -> Result<MoveOutcome, TreeError> {
        let _ = (id, new_container, new_parent, new_position);
        todo!("m1/model")
    }

    /// Replaces a node's mutable fields (title, url, flags) without moving it.
    pub fn update(&mut self, node: Node) -> Result<(), TreeError> {
        let _ = node;
        todo!("m1/model")
    }

    /// Checks every invariant over the whole tree. Used after loading and in tests.
    pub fn validate(&self) -> Result<(), TreeError> {
        todo!("m1/model")
    }
}
