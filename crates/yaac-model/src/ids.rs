use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

macro_rules! uuid_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, type = "string"))]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// A new time-ordered (UUIDv7) id.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }
            pub fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::from_str(s).map(Self)
            }
        }
    };
}

uuid_id!(
    /// Identifies a browsing profile (one CEF request context).
    ProfileId
);
uuid_id!(
    /// Identifies a Space.
    SpaceId
);
uuid_id!(
    /// Identifies a container (favorites, pinned, today or archive).
    ContainerId
);
uuid_id!(
    /// Identifies a node in the tab tree. Durable.
    NodeId
);
uuid_id!(
    /// Identifies a browser window. Durable: the core allocates it and reuses it after a restart.
    WindowId
);

/// Identifies a CEF browser view. Runtime-only: allocated by the core, never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, type = "number"))]
#[serde(transparent)]
pub struct ViewId(pub u64);

impl fmt::Display for ViewId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "view-{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Debug;

    fn assert_round_trip<T>(value: T)
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + Debug,
    {
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<T>(&json).unwrap(), value);
    }

    #[test]
    fn uuid_ids_serialize_as_plain_strings_and_round_trip() {
        let node = NodeId::new();
        assert_eq!(serde_json::to_string(&node).unwrap(), format!("\"{node}\""));
        assert_round_trip(node);
        assert_round_trip(ProfileId::new());
        assert_round_trip(SpaceId::new());
        assert_round_trip(ContainerId::new());
        assert_round_trip(WindowId::new());
    }

    #[test]
    fn view_id_round_trips_as_a_number() {
        assert_eq!(serde_json::to_string(&ViewId(7)).unwrap(), "7");
        assert_round_trip(ViewId(7));
    }

    #[test]
    fn uuid_ids_parse_from_their_string_form() {
        let node = NodeId::new();
        assert_eq!(node.to_string().parse::<NodeId>().unwrap(), node);
        assert!("not-a-uuid".parse::<NodeId>().is_err());
    }
}
