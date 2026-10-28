use crate::{State, error_auth, error_report_model::Report, store::Acceptance, validators};
use actix_web::{HttpRequest, HttpResponse, web};
fn reject(reason: &str, client: Option<&str>) {
    eprintln!(
        "{}",
        serde_json::json!({"event":"request_rejected","reason":reason,"client_id":client})
    );
}
pub async fn collect(
    request: HttpRequest,
    body: web::Bytes,
    state: web::Data<State>,
) -> HttpResponse {
    let now = chrono::Utc::now().timestamp();
    let headers = match validators::signature(&request, now) {
        Ok(headers) => headers,
        Err(error) => {
            reject(error.reason(), None);
            return HttpResponse::Unauthorized().finish();
        }
    };
    let Ok(mut store) = state.store.lock() else {
        return HttpResponse::ServiceUnavailable().finish();
    };
    let key = match store.key(&headers.client) {
        Ok(Some(key)) => key,
        Ok(None) => {
            reject("unknown_or_revoked_client", Some(&headers.client));
            return HttpResponse::Forbidden().finish();
        }
        Err(_) => return HttpResponse::ServiceUnavailable().finish(),
    };
    if !error_auth::verify(
        &key,
        &headers.signature,
        &headers.client,
        &headers.timestamp,
        &headers.nonce,
        &body,
    ) {
        reject("invalid_signature", Some(&headers.client));
        return HttpResponse::Forbidden().finish();
    }
    let report: Report = match serde_json::from_slice(&body) {
        Ok(report) => report,
        Err(_) => return HttpResponse::BadRequest().finish(),
    };
    if !validators::report(&report) {
        return HttpResponse::BadRequest().finish();
    }
    match store.accept(&headers.client, &headers.nonce, now, &report, &body) {
        Ok(Acceptance::New) => {
            HttpResponse::Accepted().json(serde_json::json!({"accepted":true,"id":report.id}))
        }
        Ok(Acceptance::Duplicate) => HttpResponse::Ok()
            .json(serde_json::json!({"accepted":true,"duplicate":true,"id":report.id})),
        Ok(reason @ (Acceptance::Replay | Acceptance::Conflict)) => {
            reject(
                if reason == Acceptance::Replay {
                    "replayed_nonce"
                } else {
                    "report_id_body_conflict"
                },
                Some(&headers.client),
            );
            HttpResponse::Conflict().finish()
        }
        Ok(Acceptance::RateLimited) => HttpResponse::TooManyRequests()
            .insert_header(("Retry-After", "60"))
            .finish(),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}
