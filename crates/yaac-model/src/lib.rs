//! Pure domain types and tree operations. No I/O, no async, no other yaac crate.

pub mod ids;
pub mod node;
pub mod position;
pub mod tree;
pub mod workspace;

pub use ids::{ContainerId, NodeId, ProfileId, SpaceId, ViewId, WindowId};
pub use node::{Container, ContainerKind, Node, NodeKind, NodeState, Profile, ProfileKind, Space};
pub use position::{Position, PositionError};
pub use tree::{MoveOutcome, TreeError, TreeState};
pub use workspace::{SessionState, Workspace};

/// Unix time in milliseconds.
pub type Millis = i64;
