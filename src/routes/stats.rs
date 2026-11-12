// src/routes/stats.rs

use crate::database::{DatabaseBackend, DatabaseError};
use crate::domain::ArtistStat;
use crate::utils::{compact_html, datastar_sse, error_chain_fmt};
use actix_web::{HttpResponse, ResponseError, web};
use datastar::prelude::*;
use std::sync::Arc;
use tera::{Context, Tera};

#[derive(thiserror::Error)]
pub enum ArtistStatError {
    #[error("database operation failed")]
    Database(#[from] DatabaseError),
    #[error("template rendering failed")]
    Template(#[from] tera::Error),
}

impl std::fmt::Debug for ArtistStatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl ResponseError for ArtistStatError {
    fn error_response(&self) -> HttpResponse {
        tracing::error!(error = ?self, "request failed");
        HttpResponse::InternalServerError().body("Something went wrong.")
    }
}

const PLACEHOLDER_ID: &str = "#artist-stats-loading";

/// How many artists the widget receives. The page is a leaderboard, not a
/// census: an unbounded grouping would grow with every artist ever logged.
const ARTIST_STATS_LIMIT: i64 = 20;

fn render_artist_stats(
    templates: &Tera,
    artist_stats: &[ArtistStat],
) -> Result<String, ArtistStatError> {
    let mut context = Context::new();
    context.insert("artist_stats", artist_stats);
    Ok(compact_html(
        &templates.render("artist_stats.html", &context)?,
    ))
}

pub async fn get_artist_stats(
    templates: web::Data<Tera>,
    database: web::Data<Arc<dyn DatabaseBackend>>,
) -> Result<HttpResponse, ArtistStatError> {
    let artist_stats = database
        .artist_stats(ARTIST_STATS_LIMIT)
        .await
        .map_err(ArtistStatError::Database)?;

    let rendered = render_artist_stats(&templates, &artist_stats)?;

    let patch = PatchElements::new(rendered)
        .selector("#artist-stats-list")
        .mode(ElementPatchMode::Append)
        .into_datastar_event();
    let clear_placeholder = PatchElements::new_remove(PLACEHOLDER_ID).into_datastar_event();

    Ok(datastar_sse(vec![patch, clear_placeholder]))
}
