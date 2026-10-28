use crate::{error_auth, error_report_model::Report};
use actix_web::HttpRequest;
pub struct SignatureHeaders {
    pub client: String,
    pub timestamp: String,
    pub nonce: String,
    pub signature: String,
}
#[derive(Debug)]
pub enum SignatureError {
    InvalidHeaders,
    Expired,
    UnexpectedQuery,
}
impl SignatureError {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::InvalidHeaders => "missing_or_invalid_signature_headers",
            Self::Expired => "stale_signature_timestamp",
            Self::UnexpectedQuery => "unexpected_query_string",
        }
    }
}
fn header(request: &HttpRequest, name: &str) -> Result<String, SignatureError> {
    if request.headers().get_all(name).count() != 1 {
        return Err(SignatureError::InvalidHeaders);
    }
    request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .ok_or(SignatureError::InvalidHeaders)
}
pub fn signature(request: &HttpRequest, now: i64) -> Result<SignatureHeaders, SignatureError> {
    let client = header(request, "x-wega-client")?;
    let timestamp = header(request, "x-wega-timestamp")?;
    let nonce = header(request, "x-wega-nonce")?;
    let signature = header(request, "x-wega-signature")?;
    error_auth::unhex::<16>(&client).map_err(|_| SignatureError::InvalidHeaders)?;
    error_auth::unhex::<16>(&nonce).map_err(|_| SignatureError::InvalidHeaders)?;
    error_auth::unhex::<64>(&signature).map_err(|_| SignatureError::InvalidHeaders)?;
    let time = timestamp
        .parse::<i64>()
        .map_err(|_| SignatureError::InvalidHeaders)?;
    if now.abs_diff(time) > 300 {
        return Err(SignatureError::Expired);
    }
    if !request.query_string().is_empty() {
        return Err(SignatureError::UnexpectedQuery);
    }
    Ok(SignatureHeaders {
        client,
        timestamp,
        nonce,
        signature,
    })
}
pub fn report(report: &Report) -> bool {
    report.schema_version == 1
        && report.application == "Wega"
        && error_auth::unhex::<16>(&report.id).is_ok()
        && chrono::DateTime::parse_from_rfc3339(&report.occurred_at).is_ok()
        && report.version.len() <= 64
        && report.platform.len() <= 64
        && report.source.len() <= 256
        && report.diagnostic.message.chars().count() <= 16384
        && report
            .diagnostic
            .url
            .as_ref()
            .is_none_or(|s| s.chars().count() <= 8192)
        && matches!(report.diagnostic.severity.as_str(), "warning" | "error")
}
