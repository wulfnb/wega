use crate::State;
use actix_web::{HttpResponse, web};
pub async fn health(state: web::Data<State>) -> HttpResponse {
    if state.store.lock().is_ok_and(|store| store.healthy()) {
        HttpResponse::Ok().body("ok")
    } else {
        HttpResponse::ServiceUnavailable().finish()
    }
}
