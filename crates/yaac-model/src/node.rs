use crate::{ContainerId, Millis, NodeId, Position, ProfileId, SpaceId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ProfileKind {
    Default,
    Standard,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: ProfileId,
    pub kind: ProfileKind,
    pub name: String,
    pub cache_dir: String,
    pub created_at: Millis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Space {
    pub id: SpaceId,
    pub profile_id: ProfileId,
    pub name: String,
    pub theme: Option<String>,
    pub position: Position,
    /// User-set. The core allows it only for the sole Space of a non-default profile.
    pub agent_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ContainerKind {
    Favorites,
    Pinned,
    Today,
    Archive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Container {
    pub id: ContainerId,
    pub profile_id: ProfileId,
    /// `None` for the profile-level favorites container.
    pub space_id: Option<SpaceId>,
    pub kind: ContainerKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Tab,
    Folder,
    /// Two to four tab children shown together.
    Split,
}

/// Persisted node state. `live` (has a browser) is runtime-only and is not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    Suspended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    /// The single ownership root: profile and Space are derived from the container.
    pub container_id: ContainerId,
    /// `None` = directly under the container. A child's container equals its parent's.
    pub parent_id: Option<NodeId>,
    pub position: Position,
    pub kind: NodeKind,
    pub title: String,
    pub url: Option<String>,
    pub state: NodeState,
    pub is_unread: bool,
    pub created_at: Millis,
    pub last_active_at: Millis,
    pub updated_at: Millis,
    /// Reserved for a later sync; not a sync design.
    pub origin_device: Option<String>,
    pub rev: i64,
}
