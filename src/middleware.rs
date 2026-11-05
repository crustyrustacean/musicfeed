// src/middleware.rs

use crate::state::AppState;
use axum::{
    extract::{Request, State},
    http::{
        HeaderMap, StatusCode,
        header::{AUTHORIZATION, WWW_AUTHENTICATE},
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(WWW_AUTHENTICATE, r#"Basic realm="musicfeed""#)],
    )
        .into_response()
}

pub async fn basic_auth(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let credentials = match headers.get(AUTHORIZATION) {
        Some(c) => c,
        None => return unauthorized(),
    };
    let text = match credentials.to_str() {
        Ok(t) => t,
        Err(_) => return unauthorized(),
    };
    let encoded_user_pass = match text.strip_prefix("Basic ") {
        Some(eup) => eup,
        None => return unauthorized(),
    };
    let decoded_user_pass = match STANDARD.decode(encoded_user_pass) {
        Ok(dup) => dup,
        Err(_) => return unauthorized(),
    };
    let user_pass = match String::from_utf8(decoded_user_pass) {
        Ok(v) => v,
        Err(_) => return unauthorized(),
    };
    let parts: Vec<&str> = user_pass.splitn(2, ':').collect();
    if parts.len() != 2 {
        return unauthorized();
    }

    let (Some(username), Some(password)) = (parts.first(), parts.get(1)) else {
        return unauthorized();
    };

    // Constant-time on both fields. `==` on a `&str` short-circuits at the first
    // differing byte, which leaks a prefix match through response timing. Both
    // comparisons always run so the pair does not leak which of the two was
    // wrong either.
    //
    // This is defence in depth, not the primary control: the real boundary is
    // that only one account exists and the service is HTTPS-only. It costs two
    // lines, so there is no reason to leave the timing channel open.
    let username_ok = ct_eq(username.as_bytes(), state.basicauth.username.as_bytes());
    let password_ok = ct_eq(password.as_bytes(), state.basicauth.password.as_bytes());

    if (username_ok & password_ok).into() {
        next.run(request).await
    } else {
        unauthorized()
    }
}

fn ct_eq(a: &[u8], b: &[u8]) -> subtle::Choice {
    use subtle::ConstantTimeEq;
    if a.len() != b.len() {
        return subtle::Choice::from(0);
    }
    a.ct_eq(b)
}
