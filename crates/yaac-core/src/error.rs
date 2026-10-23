//! Maps core and model errors onto the wire error codes.

use yaac_api::{ApiError, ErrorCode};
use yaac_model::{PositionError, TreeError};

pub(crate) fn api(code: ErrorCode, message: impl Into<String>) -> ApiError {
    ApiError {
        code,
        message: message.into(),
    }
}

pub(crate) fn bad_request(message: impl Into<String>) -> ApiError {
    api(ErrorCode::BadRequest, message)
}

pub(crate) fn not_found(message: impl Into<String>) -> ApiError {
    api(ErrorCode::NotFound, message)
}

pub(crate) fn conflict(message: impl Into<String>) -> ApiError {
    api(ErrorCode::Conflict, message)
}

pub(crate) fn forbidden(message: impl Into<String>) -> ApiError {
    api(ErrorCode::Forbidden, message)
}

pub(crate) fn internal(message: impl Into<String>) -> ApiError {
    api(ErrorCode::Internal, message)
}

pub(crate) fn tree(error: TreeError) -> ApiError {
    match error {
        TreeError::NotFound(id) => not_found(format!("node {id} not found")),
        TreeError::Cycle => conflict("a node cannot be moved into its own subtree"),
        TreeError::InvalidSplit => conflict("a split holds two to four tabs"),
        TreeError::ParentNotContainer => bad_request("the parent is not a folder or split"),
        TreeError::PositionExhausted => conflict("sibling order is exhausted"),
        other @ (TreeError::Duplicate(_) | TreeError::ContainerMismatch) => {
            internal(other.to_string())
        }
    }
}

pub(crate) fn position(error: PositionError) -> ApiError {
    internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_errors_map_to_wire_codes() {
        let id = yaac_model::NodeId::new();
        assert_eq!(tree(TreeError::NotFound(id)).code, ErrorCode::NotFound);
        assert_eq!(tree(TreeError::Cycle).code, ErrorCode::Conflict);
        assert_eq!(tree(TreeError::InvalidSplit).code, ErrorCode::Conflict);
        assert_eq!(
            tree(TreeError::ParentNotContainer).code,
            ErrorCode::BadRequest
        );
        assert_eq!(tree(TreeError::Duplicate(id)).code, ErrorCode::Internal);
        assert_eq!(tree(TreeError::ContainerMismatch).code, ErrorCode::Internal);
    }
}
