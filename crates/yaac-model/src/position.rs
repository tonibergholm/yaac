use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PositionError {
    #[error("invalid position key: {0:?}")]
    Invalid(String),
    #[error("no position fits between the given bounds")]
    NoRoom,
}

/// A fractional-index sort key for siblings. Base-62 strings; compared bytewise.
/// Ties between siblings are broken by node id by the caller, not here.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Position(String);

impl Position {
    /// The key for the first element of an empty sibling list.
    pub fn first() -> Self {
        todo!("m1/model")
    }

    /// A key that sorts strictly after `before` and strictly before `after` (`None` = open end).
    /// Keys grow by a character when there is no room; callers renumber past 32 characters.
    pub fn between(
        before: Option<&Position>,
        after: Option<&Position>,
    ) -> Result<Self, PositionError> {
        let _ = (before, after);
        todo!("m1/model")
    }

    /// Validates a key read from storage.
    pub fn from_raw(raw: String) -> Result<Self, PositionError> {
        let _ = raw;
        todo!("m1/model")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
