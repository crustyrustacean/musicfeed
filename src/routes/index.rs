// src/routes/index.rs

use crate::database::{DatabaseBackend, DatabaseError};
use crate::utils::error_chain_fmt;
use actix_web::{HttpResponse, ResponseError, web};
use std::sync::Arc;
use tera::{Context, Tera};

#[derive(thiserror::Error)]
pub enum IndexError {
    #[error("database operation failed")]
    Database(#[from] DatabaseError),
    #[error("template rendering failed")]
    Template(#[from] tera::Error),
}

impl std::fmt::Debug for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl ResponseError for IndexError {
    fn error_response(&self) -> HttpResponse {
        tracing::error!(error = ?self, "index page failed");
        HttpResponse::InternalServerError().body("Something went wrong.")
    }
}

pub async fn get_index_page(
    templates: web::Data<Tera>,
    database: web::Data<Arc<dyn DatabaseBackend>>,
) -> Result<HttpResponse, IndexError> {
    let rotation_entries = database.list().await?;
    let mut context = Context::new();
    context.insert("rotation_entries", &rotation_entries);

    let body = templates.render("index.html", &context)?;

    Ok(HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(body))
}
