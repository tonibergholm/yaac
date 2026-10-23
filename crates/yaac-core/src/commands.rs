//! Command plans. Each command is validated against the current state and becomes a `Plan`. The
//! actor commits the plan before any engine command goes out.

use crate::error::{bad_request, conflict, position as position_error};
use crate::state::{Plan, State, Tx};
use crate::urls::{self, NEW_TAB_URL};
use yaac_api::{ApiError, Command, Placement, ResponseBody};
use yaac_engine::EngineCommand;
use yaac_model::{ContainerId, ContainerKind, Node, NodeId, NodeKind, Position, SpaceId, ViewId};

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

pub(crate) fn plan(state: &State, command: Command) -> Result<(Plan, Answer), ApiError> {
    let mut tx = Tx::new(state.clone());
    let answer = apply(&mut tx, command)?;
    Ok((tx.finish(), answer))
}

fn apply(tx: &mut Tx, command: Command) -> Result<Answer, ApiError> {
    Ok(match command {
        Command::OpenTab {
            placement,
            url,
            activate,
        } => {
            urls::check(&url)?;
            let node = tx.insert_tab(&placement, url)?;
            if activate {
                tx.select(node)?;
            }
            Answer::Opened(node)
        }
        Command::CloseTab { node } => {
            close(tx, node)?;
            Answer::Ack
        }
        Command::SelectTab { node } => {
            tx.select(node)?;
            Answer::Ack
        }
        Command::SelectSpace { space } => {
            select_space(tx, space)?;
            Answer::Ack
        }
        Command::MoveNode { node, to } => {
            move_node(tx, node, &to)?;
            Answer::Ack
        }
        Command::CreateFolder { placement, title } => {
            let title = non_empty(&title, "folder title")?;
            Answer::Opened(tx.insert_node(&placement, NodeKind::Folder, title, None)?)
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
            urls::check(&url)?;
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

/// Closes a tab, or deletes an empty folder or split. A folder with children is refused.
///
/// Closing the selected tab selects a replacement: the next tab sibling, else the previous one, else
/// another open tab of the Space. A new about:blank tab opens only when the Space has no other tab.
fn close(tx: &mut Tx, node: NodeId) -> Result<(), ApiError> {
    let closing = tx.node(node)?.clone();
    if closing.kind != NodeKind::Tab {
        if tx.has_children(node)? {
            return Err(conflict("folder not empty"));
        }
        tx.remove_subtree(node)?;
        return Ok(());
    }
    let successor = tx.neighbour_tab(&closing);
    let session = tx.session()?;
    let removed = tx.remove_subtree(node)?;
    let selection_removed = session
        .selected_node_id
        .is_some_and(|selected| removed.iter().any(|gone| gone.id == selected));
    if selection_removed {
        choose_replacement(tx, session.space_id, successor, Some(closing.container_id))?;
    }
    Ok(())
}

/// Selects `successor` when there is one, else the first other open tab of `space` (preferring
/// `prefer`), else a new about:blank tab in the Space's today container.
fn choose_replacement(
    tx: &mut Tx,
    space: SpaceId,
    successor: Option<NodeId>,
    prefer: Option<ContainerId>,
) -> Result<(), ApiError> {
    if let Some(next) = successor {
        return tx.select(next);
    }
    if let Some(other) = tx.tabs_in_space(space, prefer).first().copied() {
        return tx.select(other);
    }
    let placement = Placement {
        space,
        container: ContainerKind::Today,
        parent: None,
        before: None,
    };
    let tab = tx.insert_tab(&placement, NEW_TAB_URL.to_owned())?;
    tx.select(tab)
}

/// Makes a Space active. Its last selected tab is selected again; without one, its first today tab;
/// without that, a new about:blank tab.
fn select_space(tx: &mut Tx, space: SpaceId) -> Result<(), ApiError> {
    tx.space(space)?;
    if let Some(node) = tx.last_selected_tab(space) {
        return tx.select(node);
    }
    let today = tx.container_id(space, ContainerKind::Today)?;
    if let Some(first) = tx.tabs_of_container(today).first().copied() {
        return tx.select(first);
    }
    let placement = Placement {
        space,
        container: ContainerKind::Today,
        parent: None,
        before: None,
    };
    let tab = tx.insert_tab(&placement, NEW_TAB_URL.to_owned())?;
    tx.select(tab)
}

fn move_node(tx: &mut Tx, node: NodeId, to: &Placement) -> Result<(), ApiError> {
    let moving: Node = tx.node(node)?.clone();
    let source_profile = tx.container(moving.container_id)?.profile_id;
    let target = tx.container_id(to.space, to.container)?;
    let target_container = tx.container(target)?.clone();
    if target_container.profile_id != source_profile {
        return Err(conflict("a tab cannot move to another profile in M1"));
    }
    if to.before == Some(node) {
        return Err(bad_request("a node cannot be placed before itself"));
    }
    if tx.is_within(to.parent, node)? {
        return Err(conflict("a node cannot be moved into its own subtree"));
    }

    // A move that carries the selected tab elsewhere switches the active Space. A move into
    // the archive suspends the selected tab, so another tab is selected in its place.
    let selected = tx.selected();
    let moves_selection = match selected {
        Some(selected) => tx.is_within(Some(selected), node)?,
        None => false,
    };
    let leaves_space = moves_selection && target_container.kind == ContainerKind::Archive;
    let successor = if leaves_space {
        tx.neighbour_tab(&moving)
    } else {
        None
    };
    let active_space = tx.session()?.space_id;
    if leaves_space && let Some(view) = selected.and_then(|selected| tx.detach_view(selected)) {
        tx.emit(EngineCommand::Close { view });
    }

    let position = tx.slot(target, to.parent, to.before)?;
    tx.move_node(node, target, to.parent, position)?;

    if moves_selection {
        if leaves_space {
            choose_replacement(tx, active_space, successor, Some(moving.container_id))?;
        } else if let Some(space) = target_container.space_id {
            let mut session = tx.session()?;
            session.space_id = space;
            tx.put_session(session);
        }
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

    #[test]
    fn blank_titles_are_rejected() {
        assert_eq!(
            non_empty("   ", "title").unwrap_err().code,
            yaac_api::ErrorCode::BadRequest
        );
        assert_eq!(non_empty(" a ", "title").unwrap(), "a");
    }
}
