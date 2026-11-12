// src/routes/health_check.rs

use actix_web::{HttpResponse, Responder};

pub async fn health_check() -> impl Responder {
    HttpResponse::Ok().finish()
}
