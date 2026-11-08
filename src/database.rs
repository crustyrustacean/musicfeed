// src/database.rs

use crate::domain::{ArtistStat, NewRotationEntry, RotationEntry};
use crate::utils::error_chain_fmt;
use async_trait::async_trait;

pub mod sqlite;

pub use sqlite::*;

#[derive(thiserror::Error)]
pub enum DatabaseError {
    #[error("record not found")]
    NotFound,
    #[error(transparent)]
    Operation(#[from] anyhow::Error),
}

impl std::fmt::Debug for DatabaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[async_trait]
pub trait DatabaseBackend: Send + Sync {
    async fn insert(&self, entry: NewRotationEntry) -> Result<RotationEntry, DatabaseError>;

    async fn list(&self) -> Result<Vec<RotationEntry>, DatabaseError>;

    async fn random(&self) -> Result<RotationEntry, DatabaseError>;

    async fn artist_stats(&self, limit: i64) -> Result<Vec<ArtistStat>, DatabaseError>;

    async fn list_incomplete(
        &self,
        max_attempts: u32,
        limit: i64,
    ) -> Result<Vec<RotationEntry>, DatabaseError>;

    async fn update_metadata(
        &self,
        id: u64,
        cover: Option<String>,
        year: Option<i32>,
    ) -> Result<(), DatabaseError>;

    async fn record_heal_failure(&self, id: u64) -> Result<(), DatabaseError>;
}
