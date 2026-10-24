use axum::http::{HeaderMap, StatusCode, header};

use crate::problem::Problem;

pub fn etag(version: u64) -> String {
    format!("\"{version}\"")
}

pub fn if_match(headers: &HeaderMap) -> Result<Option<u64>, Problem> {
    let Some(value) = headers.get(header::IF_MATCH) else {
        return Ok(None);
    };
    let text = value.to_str().map_err(|_| {
        Problem::new(
            StatusCode::BAD_REQUEST,
            "bad_if_match",
            "If-Match is not text",
        )
    })?;
    let text = text.trim().trim_matches('"');
    text.parse::<u64>().map(Some).map_err(|_| {
        Problem::new(
            StatusCode::BAD_REQUEST,
            "bad_if_match",
            "If-Match must be an integer version",
        )
    })
}

pub fn require_if_match(headers: &HeaderMap) -> Result<u64, Problem> {
    if_match(headers)?.ok_or_else(|| {
        Problem::new(
            StatusCode::PRECONDITION_REQUIRED,
            "if_match_required",
            "If-Match is required",
        )
    })
}
