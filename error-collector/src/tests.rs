use super::*;
use actix_web::{App, http::StatusCode, test, web};
use std::sync::Mutex;
fn payload(id: &str) -> Vec<u8> {
    serde_json::to_vec(&error_report_model::Report {
        schema_version: 1,
        id: id.into(),
        occurred_at: chrono::Utc::now().to_rfc3339(),
        application: "Wega".into(),
        version: "test".into(),
        platform: "macos-aarch64".into(),
        source: "worker:1".into(),
        diagnostic: diagnostic_event::Diagnostic::error(
            diagnostic_event::Category::Javascript,
            "example\nstack",
            Some("https://example.com/".into()),
        ),
    })
    .unwrap()
}
fn signed(seed: &[u8; 32], client: &str, nonce: &str, time: i64, body: &[u8]) -> test::TestRequest {
    let timestamp = time.to_string();
    let signature = error_auth::sign(seed, client, &timestamp, nonce, body);
    test::TestRequest::post()
        .uri("/v1/errors")
        .insert_header(("X-Wega-Client", client))
        .insert_header(("X-Wega-Timestamp", timestamp))
        .insert_header(("X-Wega-Nonce", nonce))
        .insert_header(("X-Wega-Signature", signature))
        .set_payload(body.to_vec())
}
#[actix_web::test]
async fn approved_client_can_post_but_unknown_clients_tampering_replays_and_revocation_are_rejected()
 {
    let seed = [7; 32];
    let client = "11111111111111111111111111111111";
    let store = store::Store::open(std::path::Path::new(":memory:"), 30).unwrap();
    store
        .enroll(client, &error_auth::public_key(&seed))
        .unwrap();
    let state = web::Data::new(State {
        store: Mutex::new(store),
    });
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .app_data(web::PayloadConfig::new(MAX_BODY))
            .configure(routes::configure),
    )
    .await;
    let body = payload("22222222222222222222222222222222");
    let now = chrono::Utc::now().timestamp();
    let nonce = "33333333333333333333333333333333";
    let response =
        test::call_service(&app, signed(&seed, client, nonce, now, &body).to_request()).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let response =
        test::call_service(&app, signed(&seed, client, nonce, now, &body).to_request()).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let response = test::call_service(
        &app,
        signed(
            &seed,
            client,
            "44444444444444444444444444444444",
            now,
            &body,
        )
        .to_request(),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "freshly signed delivery retry is idempotent"
    );
    let response = test::call_service(
        &app,
        signed(
            &[8; 32],
            client,
            "55555555555555555555555555555555",
            now,
            &body,
        )
        .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = test::call_service(
        &app,
        signed(
            &seed,
            "99999999999999999999999999999999",
            "66666666666666666666666666666666",
            now,
            &body,
        )
        .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = test::call_service(
        &app,
        signed(
            &seed,
            client,
            "77777777777777777777777777777777",
            now - 1000,
            &body,
        )
        .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let request = signed(
        &seed,
        client,
        "88888888888888888888888888888888",
        now,
        &body,
    )
    .set_payload(payload("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"))
    .to_request();
    assert_eq!(
        test::call_service(&app, request).await.status(),
        StatusCode::FORBIDDEN
    );
    state.store.lock().unwrap().revoke(client).unwrap();
    assert_eq!(
        test::call_service(
            &app,
            signed(
                &seed,
                client,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                now,
                &body
            )
            .to_request()
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(state.store.lock().unwrap().export(100).unwrap().len(), 1);
}
#[actix_web::test]
async fn missing_headers_invalid_schema_and_oversized_payloads_do_not_reach_storage() {
    let seed = [3; 32];
    let client = "11111111111111111111111111111111";
    let store = store::Store::open(std::path::Path::new(":memory:"), 30).unwrap();
    store
        .enroll(client, &error_auth::public_key(&seed))
        .unwrap();
    let state = web::Data::new(State {
        store: Mutex::new(store),
    });
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .app_data(web::PayloadConfig::new(MAX_BODY))
            .configure(routes::configure),
    )
    .await;
    assert_eq!(
        test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/v1/errors")
                .set_payload("{}")
                .to_request()
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let now = chrono::Utc::now().timestamp();
    let nonce = "33333333333333333333333333333333";
    assert_eq!(
        test::call_service(&app, signed(&seed, client, nonce, now, b"{}").to_request())
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        test::call_service(
            &app,
            signed(&seed, client, nonce, now, &vec![b'x'; MAX_BODY + 1]).to_request()
        )
        .await
        .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(state.store.lock().unwrap().export(100).unwrap().len(), 0);
}
#[actix_web::test]
async fn health_has_no_enrollment_or_public_admin_side_effects() {
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(State {
                store: Mutex::new(
                    store::Store::open(std::path::Path::new(":memory:"), 30).unwrap(),
                ),
            }))
            .configure(routes::configure),
    )
    .await;
    assert_eq!(
        test::call_service(&app, test::TestRequest::get().uri("/healthz").to_request())
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/enroll")
                .set_payload("{}")
                .to_request()
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}

#[actix_web::test]
async fn durable_storage_keeps_enrollment_nonce_and_idempotency_across_restart() {
    use crate::store::Acceptance;
    let path = std::env::temp_dir().join(format!(
        "wega-collector-test-{}-{}.sqlite",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let client = "11111111111111111111111111111111";
    let body = payload("22222222222222222222222222222222");
    let report = serde_json::from_slice(&body).unwrap();
    let now = chrono::Utc::now().timestamp();
    {
        let mut store = store::Store::open(&path, 1).unwrap();
        store
            .enroll(client, &error_auth::public_key(&[7; 32]))
            .unwrap();
        assert_eq!(
            store.accept(client, "first", now, &report, &body).unwrap(),
            Acceptance::New
        );
    }
    {
        let mut store = store::Store::open(&path, 1).unwrap();
        assert!(store.key(client).unwrap().is_some());
        assert_eq!(
            store.accept(client, "first", now, &report, &body).unwrap(),
            Acceptance::Replay
        );
        assert_eq!(
            store.accept(client, "retry", now, &report, &body).unwrap(),
            Acceptance::Duplicate
        );
        let mut changed: error_report_model::Report = report.clone();
        changed.diagnostic.message = "different body".into();
        assert_eq!(
            store
                .accept(
                    client,
                    "conflict",
                    now,
                    &changed,
                    &serde_json::to_vec(&changed).unwrap()
                )
                .unwrap(),
            Acceptance::Conflict
        );
        let export: serde_json::Value =
            serde_json::from_str(&store.export(100).unwrap()[0]).unwrap();
        assert_eq!(export["client_id"], client);
        assert_eq!(export["report"]["id"], report.id);
        store.revoke(client).unwrap();
    }
    {
        let mut store = store::Store::open(&path, 1).unwrap();
        assert!(store.key(client).unwrap().is_none());
        let mut next = report.clone();
        next.id = "33333333333333333333333333333333".into();
        let bytes = serde_json::to_vec(&next).unwrap();
        assert_eq!(
            store
                .accept(client, "later", now + 86401, &next, &bytes)
                .unwrap(),
            Acceptance::New
        );
        assert_eq!(
            store.export(100).unwrap().len(),
            1,
            "expired reports are pruned"
        );
    }
    std::fs::remove_file(path).unwrap();
}
