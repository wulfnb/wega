use actix_web::web;
pub fn configure(config: &mut web::ServiceConfig) {
    config
        .route("/healthz", web::get().to(crate::handlers::health))
        .route("/v1/errors", web::post().to(crate::controllers::collect));
}
