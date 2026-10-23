//! Restart-persistence check through the real stack: CEF, the core and the SQLite store.
//!
//! `--persist-write` opens tabs through the RPC path of the core, then quits. `--persist-verify`
//! starts again on the same data directory and checks the tree came back. Both print one
//! `SELFTEST` line and exit 0 only on success. Tabs other than the selected one use https URLs but
//! are never loaded, so the check makes no network request.

use std::sync::{Arc, Mutex};
use std::time::Duration;
use yaac_api::{
    API_VERSION, ApiResult, Body, Command, NodeView, Placement, Query, Request, ResponseBody,
    Snapshot,
};
use yaac_cef::CefEngine;
use yaac_engine::ViewKind;
use yaac_core::{CoreHandle, Principal};
use yaac_model::{ContainerKind, NodeId, NodeKind, ViewId};

const BLANK: &str = "about:blank";
const URL_B: &str = "https://example.com/b";
const URL_C: &str = "https://example.com/c";
const FOLDER: &str = "Work";

struct Client {
    core: CoreHandle,
    view: ViewId,
    next: u64,
}

impl Client {
    fn new(core: CoreHandle, sidebar: &Mutex<Option<ViewId>>) -> Result<Self, String> {
        let view = sidebar
            .lock()
            .map_err(|_| "sidebar lock poisoned".to_string())?
            .ok_or("the core has not created the sidebar view")?;
        Ok(Self {
            core,
            view,
            next: 0,
        })
    }

    async fn call(&mut self, body: Body) -> Result<ResponseBody, String> {
        self.next += 1;
        let request = Request {
            v: API_VERSION,
            id: self.next,
            body,
        };
        match self
            .core
            .request(Principal::Ui { view: self.view }, request)
            .await
            .result
        {
            ApiResult::Ok { body } => Ok(body),
            ApiResult::Err { error } => Err(format!("request failed: {error}")),
        }
    }

    async fn snapshot(&mut self) -> Result<Snapshot, String> {
        match self.call(Body::Query(Query::Snapshot)).await? {
            ResponseBody::Snapshot(snapshot) => Ok(snapshot),
            other => Err(format!("expected a snapshot, got {other:?}")),
        }
    }

    async fn open(
        &mut self,
        placement: Placement,
        url: &str,
        activate: bool,
    ) -> Result<NodeId, String> {
        let command = Command::OpenTab {
            placement,
            url: url.into(),
            activate,
        };
        match self.call(Body::Command(command)).await? {
            ResponseBody::Opened { node, .. } => Ok(node),
            other => Err(format!("expected Opened, got {other:?}")),
        }
    }
}

fn today(space: yaac_model::SpaceId, parent: Option<NodeId>) -> Placement {
    Placement {
        space,
        container: ContainerKind::Today,
        parent,
        before: None,
    }
}

fn today_nodes(snapshot: &Snapshot) -> Result<&[NodeView], String> {
    snapshot
        .spaces
        .iter()
        .find(|space| space.id == snapshot.active_space)
        .and_then(|space| {
            space
                .containers
                .iter()
                .find(|c| c.kind == ContainerKind::Today)
        })
        .map(|container| container.nodes.as_slice())
        .ok_or_else(|| "no today container in the active space".to_string())
}

pub async fn persist_write(
    core: CoreHandle,
    sidebar: Arc<Mutex<Option<ViewId>>>,
    engine: Arc<CefEngine>,
) {
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        wait_ready(&engine).await?;
        write(core, &sidebar).await
    })
    .await;
    finish(
        "write",
        result.map_err(|_| "timed out".to_string()).and_then(|r| r),
        &engine,
    );
}

pub async fn persist_verify(
    core: CoreHandle,
    sidebar: Arc<Mutex<Option<ViewId>>>,
    engine: Arc<CefEngine>,
) {
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        wait_ready(&engine).await?;
        verify(core, &sidebar).await
    })
    .await;
    finish(
        "verify",
        result.map_err(|_| "timed out".to_string()).and_then(|r| r),
        &engine,
    );
}

/// The check is only meaningful once the real stack is up: one sidebar and one content view are
/// live in the engine, and the sidebar page has had an RPC answered through the `app://ui` scheme
/// (attribution by browser id and the RPC bridge both ran). Waits up to 20 s.
async fn wait_ready(engine: &CefEngine) -> Result<(), String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let sidebars = engine.view_count(ViewKind::Sidebar);
        let contents = engine.view_count(ViewKind::Content);
        let answered = engine.rpc_answered();
        if sidebars == 1 && contents == 1 && answered >= 1 {
            println!("SELFTEST ready sidebar=1 content=1 rpc_answered={answered}");
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "not ready: sidebar={sidebars} content={contents} rpc_answered={answered}"
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn finish(stage: &str, result: Result<String, String>, engine: &CefEngine) {
    match result {
        Ok(detail) => {
            println!("SELFTEST {stage} ok {detail}");
            engine.quit(0);
        }
        Err(reason) => {
            println!("SELFTEST {stage} FAIL {reason}");
            engine.quit(1);
        }
    }
}

/// Expected today tree after the write stage:
/// `about:blank` (bootstrap), `URL_B`, folder `Work` holding `URL_C`, then a selected `about:blank`.
async fn write(core: CoreHandle, sidebar: &Mutex<Option<ViewId>>) -> Result<String, String> {
    let mut client = Client::new(core, sidebar)?;
    let snapshot = client.snapshot().await?;
    let space = snapshot.active_space;
    let existing = today_nodes(&snapshot)?.len();
    if existing != 1 {
        return Err(format!(
            "expected a fresh data directory with one bootstrap tab, found {existing} nodes"
        ));
    }
    client.open(today(space, None), URL_B, false).await?;
    let folder = match client
        .call(Body::Command(Command::CreateFolder {
            placement: today(space, None),
            title: FOLDER.into(),
        }))
        .await?
    {
        ResponseBody::Opened { node, .. } => node,
        other => return Err(format!("expected Opened for the folder, got {other:?}")),
    };
    client
        .open(today(space, Some(folder)), URL_C, false)
        .await?;
    let selected = client.open(today(space, None), BLANK, true).await?;
    let after = client.snapshot().await?;
    if after.selected != Some(selected) {
        return Err("the new tab is not selected".into());
    }
    Ok(format!(
        "nodes={} selected={selected}",
        today_nodes(&after)?.len()
    ))
}

async fn verify(core: CoreHandle, sidebar: &Mutex<Option<ViewId>>) -> Result<String, String> {
    let mut client = Client::new(core, sidebar)?;
    let snapshot = client.snapshot().await?;
    let nodes = today_nodes(&snapshot)?;
    let urls: Vec<Option<&str>> = nodes.iter().map(|n| n.url.as_deref()).collect();
    let want: Vec<Option<&str>> = vec![Some(BLANK), Some(URL_B), None, Some(BLANK)];
    if urls != want {
        return Err(format!("today tree differs: got {urls:?}, want {want:?}"));
    }
    let folder = &nodes[2];
    if folder.kind != NodeKind::Folder || folder.title != FOLDER {
        return Err(format!(
            "folder not restored: {:?} {:?}",
            folder.kind, folder.title
        ));
    }
    if folder.children.len() != 1 || folder.children[0].url.as_deref() != Some(URL_C) {
        return Err("the tab inside the folder is not restored".into());
    }
    let selected = nodes[3].id;
    if snapshot.selected != Some(selected) {
        return Err(format!(
            "selected tab not restored: got {:?}, want {selected}",
            snapshot.selected
        ));
    }
    let live: Vec<NodeId> = nodes
        .iter()
        .flat_map(|n| std::iter::once(n).chain(n.children.iter()))
        .filter(|n| n.live)
        .map(|n| n.id)
        .collect();
    if live != vec![selected] {
        return Err(format!(
            "only the selected tab may have a browser, live={live:?}"
        ));
    }
    Ok(format!(
        "nodes={} selected={selected} rev={}",
        nodes.len(),
        snapshot.rev
    ))
}
