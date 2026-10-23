//! Text encodings for ids, positions and enums. Enum text is the snake_case name serde uses for the
//! model type. Decoding goes through serde, so it accepts exactly what serde accepts; encoding is
//! spelled out below and a test checks that the two agree.

use crate::corrupt;
use serde::de::{DeserializeOwned, IntoDeserializer, value::Error as DeError};
use std::{fmt::Display, str::FromStr};
use yaac_core::StoreError;
use yaac_model::{ContainerKind, NodeKind, NodeState, Position, ProfileKind};

pub(crate) fn parse_id<T>(raw: &str) -> Result<T, StoreError>
where
    T: FromStr,
    T::Err: Display,
{
    raw.parse().map_err(corrupt)
}

/// Validates a stored position through the model. This is the one call to `Position::from_raw`.
pub(crate) fn parse_position(raw: &str) -> Result<Position, StoreError> {
    Position::from_raw(raw.to_owned()).map_err(corrupt)
}

/// Decodes an enum from its serde name. Unknown text is `Corrupt`.
pub(crate) fn decode_enum<T: DeserializeOwned>(raw: &str) -> Result<T, StoreError> {
    T::deserialize(IntoDeserializer::<DeError>::into_deserializer(raw))
        .map_err(|e| corrupt(format!("unknown enum value {raw:?}: {e}")))
}

pub(crate) fn flag(raw: i64) -> Result<bool, StoreError> {
    match raw {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(corrupt(format!("boolean column holds {other}"))),
    }
}

pub(crate) fn to_u32(raw: i64) -> Result<u32, StoreError> {
    u32::try_from(raw).map_err(|_| corrupt(format!("sidebar_width {raw} is out of range")))
}

pub(crate) fn profile_kind_text(kind: ProfileKind) -> &'static str {
    match kind {
        ProfileKind::Default => "default",
        ProfileKind::Standard => "standard",
    }
}

pub(crate) fn container_kind_text(kind: ContainerKind) -> &'static str {
    match kind {
        ContainerKind::Favorites => "favorites",
        ContainerKind::Pinned => "pinned",
        ContainerKind::Today => "today",
        ContainerKind::Archive => "archive",
    }
}

pub(crate) fn node_kind_text(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Tab => "tab",
        NodeKind::Folder => "folder",
        NodeKind::Split => "split",
    }
}

pub(crate) fn node_state_text(state: NodeState) -> &'static str {
    match state {
        NodeState::Suspended => "suspended",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoders_match_serde_names() {
        for kind in [ProfileKind::Default, ProfileKind::Standard] {
            assert_eq!(
                decode_enum::<ProfileKind>(profile_kind_text(kind)).unwrap(),
                kind
            );
        }
        for kind in [
            ContainerKind::Favorites,
            ContainerKind::Pinned,
            ContainerKind::Today,
            ContainerKind::Archive,
        ] {
            assert_eq!(
                decode_enum::<ContainerKind>(container_kind_text(kind)).unwrap(),
                kind
            );
        }
        for kind in [NodeKind::Tab, NodeKind::Folder, NodeKind::Split] {
            assert_eq!(decode_enum::<NodeKind>(node_kind_text(kind)).unwrap(), kind);
        }
        assert_eq!(
            decode_enum::<NodeState>(node_state_text(NodeState::Suspended)).unwrap(),
            NodeState::Suspended
        );
    }

    #[test]
    fn unknown_enum_text_is_corrupt() {
        assert!(matches!(
            decode_enum::<ContainerKind>("Favorites"),
            Err(StoreError::Corrupt(_))
        ));
        assert!(matches!(
            decode_enum::<NodeKind>(""),
            Err(StoreError::Corrupt(_))
        ));
    }
}
