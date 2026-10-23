use yaac_model::{
    Container, ContainerId, Node, NodeId, Profile, SessionState, Space, WindowId, Workspace,
};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("storage backend error: {0}")]
    Backend(String),
    #[error("stored data is invalid: {0}")]
    Corrupt(String),
}

/// One durable change. The core sends a batch per committed command; the store applies the batch
/// in a single transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreOp {
    UpsertProfile(Profile),
    UpsertSpace(Space),
    UpsertContainer(Container),
    UpsertNode(Node),
    /// Removes the node. The core lists every node of a removed subtree explicitly.
    DeleteNode(NodeId),
    DeleteContainer(ContainerId),
    UpsertSession(SessionState),
    DeleteSession(WindowId),
}

/// The persistence port. Runs on the store thread; calls are synchronous.
pub trait Store: Send + 'static {
    /// Everything on disk, validated. An empty store yields an empty `Workspace`.
    fn load(&mut self) -> Result<Workspace, StoreError>;
    /// Applies the batch atomically: all of it, or none of it.
    fn apply(&mut self, batch: Vec<StoreOp>) -> Result<(), StoreError>;
}
