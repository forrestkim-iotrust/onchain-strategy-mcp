//! Content-addressed strategy CRUD (D-01..D-02, D-07a..c).
//!
//! Task 1 lands the type stubs so `lib.rs` re-exports compile.
//! Full CRUD ships in Task 2 (this same plan).

#[derive(Debug, Clone)]
pub struct Strategy {
    pub id: String,
    pub name: String,
    pub source: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub created_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StrategySummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub created_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone)]
pub enum RegisterOutcome {
    Created(Strategy),
    AlreadyExists(Strategy),
}
