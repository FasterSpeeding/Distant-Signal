//! Internal-auth gate for `private_router()`.
//!
//! One shared-secret header (`X-Internal-Token`), compared in fixed time
//! against `ServiceArguments::internal_token`. This is intentionally not a
//! general auth framework — just enough to keep the ingestion endpoints
//! from being reachable by anyone who can hit the API's port.

pub mod oidc;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use common::ingest::INTERNAL_TOKEN_HEADER;

use crate::app::App;

/// `axum::middleware::from_fn` handler enforcing the shared-secret header.
/// Applied only to `private_router()` — `public_router()` never sees this.
pub async fn require_internal_token(
    State(app): State<App>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let provided = request
        .headers()
        .get(INTERNAL_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");

    if constant_time_eq(provided.as_bytes(), app.config.internal_token.as_bytes()) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Fixed-time byte comparison: no early return based on *content*, so a
/// mismatching byte doesn't short-circuit the scan. (A length mismatch is
/// still rejected immediately — hiding token *length* isn't a goal here,
/// only avoiding a byte-by-byte timing oracle on a same-length guess.)
///
/// Hand-rolled rather than pulling in the `subtle` crate: this is a single
/// comparison in one call site, and `subtle::ConstantTimeEq` has the same
/// same-length requirement, so there's no behavioral gap being traded away.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_tokens_match() {
        assert!(constant_time_eq(b"super-secret", b"super-secret"));
    }

    #[test]
    fn different_content_same_length_does_not_match() {
        assert!(!constant_time_eq(b"super-secret", b"super-sekret"));
    }

    #[test]
    fn different_length_does_not_match() {
        assert!(!constant_time_eq(b"short", b"much-longer-token"));
    }

    #[test]
    fn empty_tokens_match() {
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn empty_provided_against_real_token_does_not_match() {
        assert!(!constant_time_eq(b"", b"super-secret"));
    }
}
