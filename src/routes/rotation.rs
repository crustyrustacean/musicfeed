// src/routes/rotation.rs

use crate::database::{DatabaseBackend, DatabaseError};
use crate::domain::{NewRotationEntry, RotationEntry};
use crate::metadata::AlbumQuery;
use crate::utils::{compact_html, error_chain_fmt};
use actix_sse::{Event, Sse};
use actix_web::{HttpResponse, Responder, ResponseError, http::StatusCode, web};
use chrono::Local;
use datastar::actix_web::ReadSignals;
use datastar::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tera::{Context, Tera};

#[derive(thiserror::Error)]
pub enum RotationEntryError {
    #[error("rotation entry not found")]
    NotFound,
    #[error("database operation failed")]
    Database(#[from] DatabaseError),
    #[error("template rendering failed")]
    Template(#[from] tera::Error),
}

impl std::fmt::Debug for RotationEntryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl ResponseError for RotationEntryError {
    fn status_code(&self) -> StatusCode {
        match self {
            RotationEntryError::NotFound => StatusCode::NOT_FOUND,
            RotationEntryError::Database(_) | RotationEntryError::Template(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    fn error_response(&self) -> HttpResponse {
        tracing::error!(error = ?self, "request failed");
        HttpResponse::build(self.status_code()).body("Something went wrong.")
    }
}

/// The element a consumer puts inside `#rotation-list` as a placeholder, removed
/// once a real entry is appended. Part of the SSE contract described in README.
const PLACEHOLDER_ID: &str = "#rotation-loading";

/// What the form sends. `cover` and `year` are absent by design — they are looked
/// up, not typed. `id` and `listened_date` are absent because the app controls
/// them, so a client cannot forge either.
#[derive(Clone, Deserialize, Serialize)]
pub struct RawRotationEntry {
    artist: String,
    album: String,
    note: String,
}

/// Signal used to tell the user the lookup failed without discarding the entry.
const METADATA_STATUS: &str = "metadata_status";
const LOOKUP_FAILED: &str = "Could not look that up - saved without a cover or year";

/// Render a single entry as either the public widget or a landing-page table row.
fn render_rotation(
    templates: &Tera,
    entry: &RotationEntry,
    template: &str,
) -> Result<String, RotationEntryError> {
    let mut context = Context::new();
    context.insert("rotation_entry", entry);
    Ok(compact_html(&templates.render(template, &context)?))
}

#[derive(Deserialize)]
pub struct RotationView {
    view: Option<String>,
}

pub async fn post_rotation_entry_ds(
    templates: web::Data<Tera>,
    database: web::Data<Arc<dyn DatabaseBackend>>,
    metadata: web::Data<crate::metadata::MetadataClient>,
    view: web::Query<RotationView>,
    ReadSignals(raw_rotation_entry): ReadSignals<RawRotationEntry>,
) -> Result<impl Responder, RotationEntryError> {
    // Look the album up before anything else. This never blocks the save: a
    // miss and a failure both come back as empty metadata, and the user is told
    // which happened via the status signal below.
    let query = AlbumQuery::new(&raw_rotation_entry.artist, &raw_rotation_entry.album);
    let looked_up = metadata.lookup(&query).await;

    let lookup_failed = looked_up.is_err();
    let metadata = looked_up.unwrap_or_default();

    let new_entry = NewRotationEntry {
        listened_date: Local::now().date_naive(),
        artist: raw_rotation_entry.artist,
        album: raw_rotation_entry.album,
        cover: metadata.cover,
        year: metadata.year,
        note: (!raw_rotation_entry.note.is_empty()).then_some(raw_rotation_entry.note),
    };
    let entry = database.insert(new_entry).await?;

    // The form requests a table row; the default remains the public widget
    // response used by existing clients of POST /rotation.
    let (template, selector) = if view.view.as_deref() == Some("table") {
        ("rotation_row.html", "#rotation-table-body")
    } else {
        ("rotation.html", "#rotation-list")
    };
    let rendered = render_rotation(&templates, &entry, template)?;

    let patch = PatchElements::new(rendered)
        .selector(selector)
        .mode(ElementPatchMode::Append)
        .into_datastar_event();

    let clear_empty = PatchElements::new_remove(PLACEHOLDER_ID).into_datastar_event();

    let clear = PatchSignals::new(format!(
        r#"{{"artist":"","album":"","note":"","{METADATA_STATUS}":""}}"#
    ))
    .into_datastar_event();

    let mut events = vec![patch, clear_empty, clear];

    if lookup_failed {
        let status = PatchSignals::new(format!(r#"{{"{METADATA_STATUS}":"{LOOKUP_FAILED}"}}"#))
            .into_datastar_event();
        events.push(status);
    }

    Ok(Sse::from_infallible_stream(tokio_stream::iter(
        events.into_iter().map(Event::from),
    )))
}

pub async fn get_rotation_entry(
    templates: web::Data<Tera>,
    database: web::Data<Arc<dyn DatabaseBackend>>,
) -> Result<impl Responder, RotationEntryError> {
    let entry = database.random().await.map_err(|error| match error {
        DatabaseError::NotFound => RotationEntryError::NotFound,
        other => RotationEntryError::Database(other),
    })?;

    let rendered = render_rotation(&templates, &entry, "rotation.html")?;

    let patch = PatchElements::new(rendered)
        .selector("#rotation-list")
        .mode(ElementPatchMode::Append)
        .into_datastar_event();
    let clear_placeholder = PatchElements::new_remove(PLACEHOLDER_ID).into_datastar_event();

    Ok(Sse::from_infallible_stream(tokio_stream::iter(
        [patch, clear_placeholder].map(Event::from),
    )))
}
