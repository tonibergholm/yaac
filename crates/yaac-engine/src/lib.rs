//! What the core needs from a browser engine. A command-and-event interface: nothing here returns
//! a result from main-thread work. The core allocates every id; results come back as `EngineEvent`s.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use yaac_model::{ProfileId, ViewId, WindowId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSpec {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewKind {
    Sidebar,
    Content,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewSpec {
    pub kind: ViewKind,
    pub profile: ProfileId,
    pub url: String,
}

/// Core -> engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineCommand {
    CreateWindow {
        window: WindowId,
        spec: WindowSpec,
    },
    CreateView {
        view: ViewId,
        window: WindowId,
        spec: ViewSpec,
    },
    Load {
        view: ViewId,
        url: String,
    },
    Reload {
        view: ViewId,
    },
    Back {
        view: ViewId,
    },
    Forward {
        view: ViewId,
    },
    /// Make this content view the visible one in its window.
    Show {
        view: ViewId,
    },
    /// The result arrives as `EngineEvent::Closed`.
    Close {
        view: ViewId,
    },
}

/// Engine -> core. `nav_gen` is the engine's navigation generation for the view; the core drops
/// callbacks older than the latest one it has seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineEvent {
    Created {
        view: ViewId,
    },
    Navigated {
        view: ViewId,
        nav_gen: u64,
        url: String,
    },
    TitleChanged {
        view: ViewId,
        nav_gen: u64,
        title: String,
    },
    LoadState {
        view: ViewId,
        loading: bool,
        can_back: bool,
        can_forward: bool,
    },
    Closed {
        view: ViewId,
    },
    /// The engine denied the popup synchronously; the core decides whether to open a tab.
    PopupRequested {
        opener: ViewId,
        url: String,
        user_gesture: bool,
    },
    Crashed {
        view: ViewId,
    },
    WindowClosed {
        window: WindowId,
    },
}

/// Cheap to clone, `Send + Sync`. Implementations post work to the engine's UI thread.
pub trait EngineHandle: Send + Sync {
    fn send(&self, command: EngineCommand);

    /// A page driver for a content view, for the agent interface (M3). `None` until then.
    fn page(&self, view: ViewId) -> Option<Arc<dyn PageDriver>> {
        let _ = view;
        None
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub snapshot: bool,
    pub act: bool,
    pub eval: bool,
    pub screenshot: bool,
    pub console: bool,
}

/// The seam for agents (filled in at M3). Object-safe and capability-based.
pub trait PageDriver: Send + Sync {
    fn capabilities(&self) -> Capabilities;
}

/// Records every command; for tests of the core.
#[derive(Debug, Default, Clone)]
pub struct FakeEngine {
    commands: Arc<Mutex<Vec<EngineCommand>>>,
}

impl FakeEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the commands recorded so far.
    pub fn take_commands(&self) -> Vec<EngineCommand> {
        std::mem::take(&mut *self.commands.lock().unwrap())
    }
}

impl EngineHandle for FakeEngine {
    fn send(&self, command: EngineCommand) {
        self.commands.lock().unwrap().push(command);
    }
}
