//! Who may issue which request. The principal comes from the ingress, never from the request.
//! `permits` lists every command explicitly: a new `Command` variant does not compile until it is
//! listed here, so it cannot become reachable by accident.

use crate::Principal;
use crate::error::{api, forbidden};
use crate::state::State;
use yaac_api::{API_VERSION, ApiError, Body, Command, ErrorCode, Query, Request};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// The main frame of a registered UI browser.
    Ui,
    System,
    /// A principal with no authority. Exists only to test refusals through the handle.
    #[cfg(test)]
    Restricted,
}

/// Checks the protocol version, then the principal's authority for the request body.
pub(crate) fn admit(
    state: &State,
    principal: &Principal,
    request: &Request,
) -> Result<Kind, ApiError> {
    if request.v != API_VERSION {
        return Err(api(
            ErrorCode::UnsupportedVersion,
            format!("request version {} is not {API_VERSION}", request.v),
        ));
    }
    let kind = match principal {
        Principal::Ui { view } if state.sidebar_views.contains(view) => Kind::Ui,
        Principal::Ui { .. } => return Err(forbidden("the view is not a registered UI view")),
        Principal::System => Kind::System,
        #[cfg(test)]
        Principal::Restricted => Kind::Restricted,
    };
    if permits(kind, &request.body) {
        Ok(kind)
    } else {
        Err(forbidden("this principal may not issue the request"))
    }
}

pub(crate) fn permits(kind: Kind, body: &Body) -> bool {
    match (kind, body) {
        (Kind::Ui | Kind::System, Body::Query(query)) => match query {
            Query::Snapshot | Query::Since { .. } => true,
        },
        (Kind::Ui | Kind::System, Body::Command(command)) => match command {
            Command::OpenTab { .. }
            | Command::CloseTab { .. }
            | Command::SelectTab { .. }
            | Command::SelectSpace { .. }
            | Command::MoveNode { .. }
            | Command::CreateFolder { .. }
            | Command::RenameNode { .. }
            | Command::CreateSpace { .. }
            | Command::Navigate { .. }
            | Command::Back { .. }
            | Command::Forward { .. }
            | Command::Reload { .. } => true,
        },
        #[cfg(test)]
        (Kind::Restricted, _) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yaac_model::{NodeId, SpaceId, ViewId};

    fn every_command() -> Vec<Command> {
        let node = NodeId::new();
        let placement = yaac_api::Placement {
            space: SpaceId::new(),
            container: yaac_model::ContainerKind::Today,
            parent: None,
            before: None,
        };
        vec![
            Command::OpenTab {
                placement: placement.clone(),
                url: "about:blank".to_owned(),
                activate: true,
            },
            Command::CloseTab { node },
            Command::SelectTab { node },
            Command::SelectSpace {
                space: SpaceId::new(),
            },
            Command::MoveNode {
                node,
                to: placement.clone(),
            },
            Command::CreateFolder {
                placement,
                title: "f".to_owned(),
            },
            Command::RenameNode {
                node,
                title: "t".to_owned(),
            },
            Command::CreateSpace {
                name: "s".to_owned(),
            },
            Command::Navigate {
                node,
                url: "about:blank".to_owned(),
            },
            Command::Back { node },
            Command::Forward { node },
            Command::Reload { node },
        ]
    }

    #[test]
    fn ui_and_system_may_issue_every_m1_command_and_query() {
        for kind in [Kind::Ui, Kind::System] {
            for command in every_command() {
                assert!(permits(kind, &Body::Command(command)), "{kind:?}");
            }
            assert!(permits(kind, &Body::Query(Query::Snapshot)));
            assert!(permits(kind, &Body::Query(Query::Since { rev: 0 })));
        }
    }

    #[test]
    fn a_version_mismatch_is_refused_before_authority_is_checked() {
        let state = State::default();
        let request = Request {
            v: API_VERSION + 1,
            id: 1,
            body: Body::Query(Query::Snapshot),
        };
        let error = admit(&state, &Principal::System, &request).unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsupportedVersion);
    }

    #[test]
    fn a_ui_principal_must_name_a_registered_ui_view() {
        let state = State::default();
        let request = Request {
            v: API_VERSION,
            id: 1,
            body: Body::Query(Query::Snapshot),
        };
        let unknown = Principal::Ui { view: ViewId(999) };
        assert_eq!(
            admit(&state, &unknown, &request).unwrap_err().code,
            ErrorCode::Forbidden
        );
        assert_eq!(
            admit(&state, &Principal::System, &request).unwrap(),
            Kind::System
        );
    }
}
