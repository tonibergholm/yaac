use crate::{
    Container, ContainerId, NodeId, Profile, ProfileId, Space, SpaceId, TreeState, WindowId,
};
use std::collections::BTreeMap;

/// Per-window state restored on start. `selected_node_id` may be `None` or stale after a restart;
/// the core falls back to the first `today` tab of the Space, or opens a new tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionState {
    pub window_id: WindowId,
    pub space_id: SpaceId,
    pub selected_node_id: Option<NodeId>,
    pub sidebar_width: u32,
}

/// Everything the core keeps in memory. The store holds the same data on disk.
#[derive(Debug, Clone, Default)]
pub struct Workspace {
    pub profiles: BTreeMap<ProfileId, Profile>,
    pub spaces: BTreeMap<SpaceId, Space>,
    pub containers: BTreeMap<ContainerId, Container>,
    pub tree: TreeState,
    pub sessions: BTreeMap<WindowId, SessionState>,
    /// Monotonic revision, bumped by the core on every committed change.
    pub rev: u64,
}
