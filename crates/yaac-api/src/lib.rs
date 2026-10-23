//! Wire types shared by the web UI, the CLI and MCP. Rust is the source of truth; the TypeScript
//! types are generated from these (feature `ts`). There is no caller identity on the wire: the
//! core derives a `Principal` from the ingress that carried the request.

use serde::{Deserialize, Serialize};
use yaac_model::{ContainerKind, NodeId, NodeKind, SpaceId, WindowId};

#[cfg(feature = "ts")]
use ts_rs::TS;

pub const API_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct Request {
    pub v: u16,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub id: u64,
    pub body: Body,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(tag = "kind", content = "body", rename_all = "snake_case")]
pub enum Body {
    Command(Command),
    Query(Query),
}

/// Where a node goes: a container of a Space, optionally under a folder, before a sibling.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct Placement {
    pub space: SpaceId,
    pub container: ContainerKind,
    pub parent: Option<NodeId>,
    /// Insert before this sibling; `None` appends.
    pub before: Option<NodeId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    OpenTab {
        placement: Placement,
        url: String,
        activate: bool,
    },
    CloseTab {
        node: NodeId,
    },
    SelectTab {
        node: NodeId,
    },
    /// Switch the window's active Space. Selects that Space's last selected tab, or its first `today` tab.
    SelectSpace {
        space: SpaceId,
    },
    MoveNode {
        node: NodeId,
        to: Placement,
    },
    CreateFolder {
        placement: Placement,
        title: String,
    },
    RenameNode {
        node: NodeId,
        title: String,
    },
    CreateSpace {
        name: String,
    },
    Navigate {
        node: NodeId,
        url: String,
    },
    Back {
        node: NodeId,
    },
    Forward {
        node: NodeId,
    },
    Reload {
        node: NodeId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Query {
    /// The whole state the window needs to draw itself.
    Snapshot,
    /// Changes since `rev`. In M1 this returns a full snapshot; a bounded history comes later.
    Since {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        rev: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct Response {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub id: u64,
    pub result: ApiResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ApiResult {
    Ok { body: ResponseBody },
    Err { error: ApiError },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseBody {
    Ack {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        rev: u64,
    },
    /// A node was created: the answer to `OpenTab` and `CreateFolder`.
    Opened {
        node: NodeId,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        rev: u64,
    },
    SpaceCreated {
        space: SpaceId,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        rev: u64,
    },
    Snapshot(Snapshot),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[error("{code:?}: {message}")]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadRequest,
    NotFound,
    Forbidden,
    Conflict,
    UnsupportedVersion,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct Snapshot {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub rev: u64,
    pub window: WindowId,
    pub active_space: SpaceId,
    pub selected: Option<NodeId>,
    pub spaces: Vec<SpaceView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SpaceView {
    pub id: SpaceId,
    pub name: String,
    pub theme: Option<String>,
    pub containers: Vec<ContainerView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct ContainerView {
    pub kind: ContainerKind,
    pub nodes: Vec<NodeView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct NodeView {
    pub id: NodeId,
    pub kind: NodeKind,
    pub title: String,
    pub url: Option<String>,
    pub is_unread: bool,
    /// Runtime fact: the node currently has a browser. Never persisted.
    pub live: bool,
    pub children: Vec<NodeView>,
}

/// Pushed to the UI. Carries a revision, never state; the UI pulls with `Query::Since`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct Event {
    pub topic: Topic,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub rev: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    Tree,
    Session,
    Settings,
}
