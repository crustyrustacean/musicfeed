// src/routes/stats.rs

use crate::database::DatabaseError;
use crate::domain::ArtistStat;
use crate::state::AppState;
use crate::utils::{compact_html, error_chain_fmt};
use axum::response::{
    IntoResponse, Response,
    sse::{Event, Sse},
};
use axum::{extract::State, http::StatusCode};
use axum_macros::debug_handler;
use datastar::prelude::*;
use std::convert::Infallible;
use tera::Context;

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

impl IntoResponse for ArtistStatError {
    fn into_response(self) -> Response {
        tracing::error!(error = ?self, "request failed");
        (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong.").into_response()
    }
}

const PLACEHOLDER_ID: &str = "#artist-stats-loading";

/// How many artists the widget receives. The page is a leaderboard, not a
/// census: an unbounded grouping would grow with every artist ever logged.
const ARTIST_STATS_LIMIT: i64 = 20;

fn render_artist_stats(
    state: &AppState,
    artist_stats: &[ArtistStat],
) -> Result<String, ArtistStatError> {
    let mut context = Context::new();
    context.insert("artist_stats", artist_stats);
    Ok(compact_html(
        &state.templates.render("artist_stats.html", &context)?,
    ))
}

#[debug_handler]
pub async fn get_artist_stats(
    State(state): State<AppState>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ArtistStatError> {
    let artist_stats = state
        .database
        .artist_stats(ARTIST_STATS_LIMIT)
        .await
        .map_err(ArtistStatError::Database)?;

    let rendered = render_artist_stats(&state, &artist_stats)?;

    let patch = PatchElements::new(rendered)
        .selector("#artist-stats-list")
        .mode(ElementPatchMode::Append)
        .write_as_axum_sse_event();

    let clear_placeholder = PatchElements::new_remove(PLACEHOLDER_ID).write_as_axum_sse_event();

    let sse_event = Sse::new(tokio_stream::iter(vec![Ok(patch), Ok(clear_placeholder)]));

    Ok(sse_event)
}
