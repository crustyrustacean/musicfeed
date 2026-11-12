// src/middleware.rs

use crate::configuration::BasicAuthSettings;
use actix_web::body::{BoxBody, MessageBody};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use actix_web::middleware::Next;
use actix_web::{Error, HttpResponse, web};
use base64::{Engine, engine::general_purpose::STANDARD};

fn unauthorized() -> HttpResponse {
    HttpResponse::Unauthorized()
        .insert_header((WWW_AUTHENTICATE, r#"Basic realm="musicfeed"#))
        .finish()
}

pub async fn basic_auth(
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    let Some(basicauth) = req.app_data::<web::Data<BasicAuthSettings>>() else {
        return Ok(req.into_response(
            HttpResponse::InternalServerError()
                .body("Application state is not configured")
                .map_into_boxed_body(),
        ));
    };

    let credentials = match req.headers().get(AUTHORIZATION) {
        Some(c) => c,
        None => return Ok(req.into_response(unauthorized().map_into_boxed_body())),
    };
    let text = match credentials.to_str() {
        Ok(t) => t,
        Err(_) => return Ok(req.into_response(unauthorized().map_into_boxed_body())),
    };
    let encoded_user_pass = match text.strip_prefix("Basic ") {
        Some(eup) => eup,
        None => return Ok(req.into_response(unauthorized().map_into_boxed_body())),
    };
    let decoded_user_pass = match STANDARD.decode(encoded_user_pass) {
        Ok(dup) => dup,
        Err(_) => return Ok(req.into_response(unauthorized().map_into_boxed_body())),
    };
    let user_pass = match String::from_utf8(decoded_user_pass) {
        Ok(v) => v,
        Err(_) => return Ok(req.into_response(unauthorized().map_into_boxed_body())),
    };
    let parts: Vec<&str> = user_pass.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Ok(req.into_response(unauthorized().map_into_boxed_body()));
    }

    let (Some(username), Some(password)) = (parts.first(), parts.get(1)) else {
        return Ok(req.into_response(unauthorized().map_into_boxed_body()));
    };

    // Constant-time on both fields. `==` on a `&str` short-circuits at the first
    // differing byte, which leaks a prefix match through response timing. Both
    // comparisons always run so the pair does not leak which of the two was
    // wrong either.
    let username_ok = ct_eq(username.as_bytes(), basicauth.username.as_bytes());
    let password_ok = ct_eq(password.as_bytes(), basicauth.password.as_bytes());

    if (username_ok & password_ok).into() {
        next.call(req)
            .await
            .map(|response| response.map_into_boxed_body())
    } else {
        Ok(req.into_response(unauthorized().map_into_boxed_body()))
    }
}

fn ct_eq(a: &[u8], b: &[u8]) -> subtle::Choice {
    use subtle::ConstantTimeEq;
    if a.len() != b.len() {
        return subtle::Choice::from(0);
    }
    a.ct_eq(b)
}
