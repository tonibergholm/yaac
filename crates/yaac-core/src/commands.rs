//! Command plans. Each command is validated against the current state and becomes a `Plan`. The
//! actor commits the plan before any engine command goes out.

use crate::auth::Kind;
use crate::error::{bad_request, conflict, position as position_error};
use crate::state::{Plan, State, Tx};
use crate::urls::{self, NEW_TAB_URL};
use yaac_api::{ApiError, Command, Placement, ResponseBody};
use yaac_engine::EngineCommand;
use yaac_model::{ContainerKind, Node, NodeId, NodeKind, Position, SpaceId, ViewId};

/// What a command answers with, once its revision is known.
pub(crate) enum Answer {
    Ack,
    Opened(NodeId),
    SpaceCreated(SpaceId),
}

impl Answer {
    pub(crate) fn into_body(self, rev: u64) -> ResponseBody {
        match self {
            Answer::Ack => ResponseBody::Ack { rev },
            Answer::Opened(node) => ResponseBody::Opened { node, rev },
            Answer::SpaceCreated(space) => ResponseBody::SpaceCreated { space, rev },
        }
    }
}

pub(crate) fn plan(
    state: &State,
    kind: Kind,
    command: Command,
) -> Result<(Plan, Answer), ApiError> {
    let mut tx = Tx::new(state.clone());
    let answer = apply(&mut tx, kind, command)?;
    Ok((tx.finish(), answer))
}

fn apply(tx: &mut Tx, kind: Kind, command: Command) -> Result<Answer, ApiError> {
    Ok(match command {
        Command::OpenTab {
            placement,
            url,
            activate,
        } => {
            urls::check(&url, kind.allows_app_urls())?;
            let node = tx.insert_tab(&placement, url)?;
            if activate {
                tx.select(node)?;
            }
            Answer::Opened(node)
        }
        Command::CloseTab { node } => {
            close_tab(tx, node)?;
            Answer::Ack
        }
        Command::SelectTab { node } => {
            tx.select(node)?;
            Answer::Ack
        }
        Command::MoveNode { node, to } => {
            move_node(tx, node, &to)?;
            Answer::Ack
        }
        Command::CreateFolder { placement, title } => {
            let title = non_empty(&title, "folder title")?;
            tx.insert_node(&placement, NodeKind::Folder, title, None)?;
            Answer::Ack
        }
        Command::RenameNode { node, title } => {
            rename(tx, node, &title)?;
            Answer::Ack
        }
        Command::CreateSpace { name } => {
            let name = non_empty(&name, "space name")?;
            let profile = tx.default_profile()?;
            let position = next_space_position(tx)?;
            Answer::SpaceCreated(tx.add_space(profile, name, position))
        }
        Command::Navigate { node, url } => {
            urls::check(&url, kind.allows_app_urls())?;
            navigate(tx, node, url)?;
            Answer::Ack
        }
        Command::Back { node } => {
            engine_navigation(tx, node, |view| EngineCommand::Back { view })?;
            Answer::Ack
        }
        Command::Forward { node } => {
            engine_navigation(tx, node, |view| EngineCommand::Forward { view })?;
            Answer::Ack
        }
        Command::Reload { node } => {
            engine_navigation(tx, node, |view| EngineCommand::Reload { view })?;
            Answer::Ack
        }
    })
}

/// Closes a node and its subtree. If the selection was inside it, the next tab sibling is
/// selected, else the previous one, else a new about:blank tab opens in the active Space.
fn close_tab(tx: &mut Tx, node: NodeId) -> Result<(), ApiError> {
    let closing = tx.node(node)?.clone();
    let successor = tx.neighbour_tab(&closing);
    let session = tx.session()?;
    let removed = tx.remove_subtree(node)?;
    let selection_removed = session
        .selected_node_id
        .is_some_and(|selected| removed.iter().any(|gone| gone.id == selected));
    if !selection_removed {
        return Ok(());
    }
    match successor {
        Some(next) => tx.select(next),
        None => {
            let placement = Placement {
                space: session.space_id,
                container: ContainerKind::Today,
                parent: None,
                before: None,
            };
            let tab = tx.insert_tab(&placement, NEW_TAB_URL.to_owned())?;
            tx.select(tab)
        }
    }
}

fn move_node(tx: &mut Tx, node: NodeId, to: &Placement) -> Result<(), ApiError> {
    let source_profile = tx.container(tx.node(node)?.container_id)?.profile_id;
    let target = tx.container_id(to.space, to.container)?;
    if tx.container(target)?.profile_id != source_profile {
        return Err(conflict("a tab cannot move to another profile in M1"));
    }
    if to.before == Some(node) {
        return Err(bad_request("a node cannot be placed before itself"));
    }
    if tx.is_within(to.parent, node)? {
        return Err(conflict("a node cannot be moved into its own subtree"));
    }
    let position = tx.slot(target, to.parent, to.before)?;
    tx.move_node(node, target, to.parent, position)?;
    let target_space = tx.container(target)?.space_id;
    if let Some(space) = target_space.filter(|_| tx.selected() == Some(node)) {
        let mut session = tx.session()?;
        session.space_id = space;
        tx.put_session(session);
    }
    Ok(())
}

fn rename(tx: &mut Tx, node: NodeId, title: &str) -> Result<(), ApiError> {
    let title = non_empty(title, "title")?;
    let mut updated: Node = tx.node(node)?.clone();
    updated.title = title;
    updated.updated_at = crate::state::now_millis();
    tx.put_node(updated)
}

/// Navigates a tab. A live tab loads the URL; a suspended one gets a view that starts on it, and
/// is shown only when it is the selected tab. The node's stored URL changes only from engine events.
fn navigate(tx: &mut Tx, node: NodeId, url: String) -> Result<(), ApiError> {
    tx.tab(node)?;
    match tx.view_of(node) {
        Some(view) => tx.emit(EngineCommand::Load { view, url }),
        None => {
            let show = tx.selected() == Some(node);
            tx.attach_view(node, url, show)?;
        }
    }
    Ok(())
}

fn engine_navigation(
    tx: &mut Tx,
    node: NodeId,
    command: fn(ViewId) -> EngineCommand,
) -> Result<(), ApiError> {
    tx.tab(node)?;
    let view = tx
        .view_of(node)
        .ok_or_else(|| conflict("the tab is suspended and has no page"))?;
    tx.emit(command(view));
    Ok(())
}

fn next_space_position(tx: &Tx) -> Result<Position, ApiError> {
    match tx.last_space_position() {
        Some(last) => Position::between(Some(&last), None).map_err(position_error),
        None => Ok(Position::first()),
    }
}

fn non_empty(value: &str, what: &str) -> Result<String, ApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(bad_request(format!("{what} is empty")))
    } else {
        Ok(trimmed.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::url_or_blank;

    #[test]
    fn blank_titles_are_rejected() {
        assert_eq!(
            non_empty("   ", "title").unwrap_err().code,
            yaac_api::ErrorCode::BadRequest
        );
        assert_eq!(non_empty(" a ", "title").unwrap(), "a");
    }

    #[test]
    #[ignore = "needs m1/model"]
    fn a_suspended_url_defaults_to_blank() {
        let node = Node {
            id: NodeId::new(),
            container_id: yaac_model::ContainerId::new(),
            parent_id: None,
            position: Position::first(),
            kind: NodeKind::Tab,
            title: String::new(),
            url: None,
            state: yaac_model::NodeState::Suspended,
            is_unread: false,
            created_at: 0,
            last_active_at: 0,
            updated_at: 0,
            origin_device: None,
            rev: 0,
        };
        assert_eq!(url_or_blank(&node), NEW_TAB_URL);
    }
}
