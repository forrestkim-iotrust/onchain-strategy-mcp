//! Typed storage errors (D-06a). MCP error-code mapping lives in
//! `executor-mcp::errors::map_state_error` (Plan 02-02).

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("storage error: {0}")]
    Storage(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("strategy name conflict: {attempted_name}")]
    NameConflict {
        attempted_name: String,
        existing_strategy_id: String,
        existing_source_hash: String,
        existing_created_at: String,
    },

    #[error("input validation failed: {0}")]
    InvalidInput(String),
}

impl From<rusqlite::Error> for StateError {
    fn from(e: rusqlite::Error) -> Self {
        StateError::Storage(e.to_string())
    }
}
