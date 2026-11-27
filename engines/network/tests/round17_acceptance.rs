//! Round 17 Acceptance Test Suite: Networking and Security Hardening.
//! Covers:
//! - Redirect method rewriting and credential stripping.
//! - MIME type nosniff validation for scripts and styles.
//! - Mixed-content blocking from HTTPS contexts.
//! - Referrer-Policy calculations.
//! - Oversized body and hostile response bounds.

use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use url::Url;
use wega_network::{Broker, Policy, Remote, Request, ResourceDestination, compute_referrer};

#[test]
fn test_referrer_policy_computations() {
    let https_page = Url::parse("https://example.com/path/to/page.html?query=1#frag").unwrap();
    let https_same = Url::parse("https://example.com/api/data").unwrap();
    let https_cross = Url::parse("https://other.org/resource").unwrap();
    let http_downgrade = Url::parse("http://insecure.net/download").unwrap();

    // 1. no-referrer
    assert_eq!(
        compute_referrer(&https_page, &https_same, "no-referrer"),
        None
    );
    assert_eq!(
        compute_referrer(&https_page, &https_cross, "no-referrer"),
        None
    );

    // 2. same-origin
    assert_eq!(
        compute_referrer(&https_page, &https_same, "same-origin"),
        Some("https://example.com/path/to/page.html?query=1".into())
    );
    assert_eq!(
        compute_referrer(&https_page, &https_cross, "same-origin"),
        None
    );

    // 3. origin
    assert_eq!(
        compute_referrer(&https_page, &https_same, "origin"),
        Some("https://example.com/".into())
    );
    assert_eq!(
        compute_referrer(&https_page, &https_cross, "origin"),
        Some("https://example.com/".into())
    );

    // 4. strict-origin
    assert_eq!(
        compute_referrer(&https_page, &https_cross, "strict-origin"),
        Some("https://example.com/".into())
    );
    assert_eq!(
        compute_referrer(&https_page, &http_downgrade, "strict-origin"),
        None
    );

    // 5. origin-when-cross-origin
    assert_eq!(
        compute_referrer(&https_page, &https_same, "origin-when-cross-origin"),
        Some("https://example.com/path/to/page.html?query=1".into())
    );
    assert_eq!(
        compute_referrer(&https_page, &https_cross, "origin-when-cross-origin"),
        Some("https://example.com/".into())
    );

    // 6. strict-origin-when-cross-origin (modern default)
    assert_eq!(
        compute_referrer(&https_page, &https_same, "strict-origin-when-cross-origin"),
        Some("https://example.com/path/to/page.html?query=1".into())
    );
    assert_eq!(
        compute_referrer(&https_page, &https_cross, "strict-origin-when-cross-origin"),
        Some("https://example.com/".into())
    );
    assert_eq!(
        compute_referrer(
            &https_page,
            &http_downgrade,
            "strict-origin-when-cross-origin"
        ),
        None
    );

    // 7. unsafe-url
    assert_eq!(
        compute_referrer(&https_page, &http_downgrade, "unsafe-url"),
        Some("https://example.com/path/to/page.html?query=1".into())
    );
}

#[test]
fn test_mixed_content_blocking() {
    let policy = Policy {
        allow_loopback: true,
        ..Default::default()
    };

    let target_http = Url::parse("http://example.com/script.js").unwrap();

    // Request from HTTPS initiator to HTTP target with active destination
    let mut req_script = Request::subresource(&target_http, ResourceDestination::Script, 4096);
    req_script.origin = "https://secure.example.com".into();
    req_script.initiator = Some(Url::parse("https://secure.example.com/page").unwrap());

    let res = policy.permits_request(&req_script, &target_http);
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("Active mixed content blocked"), "Got: {err}");

    // Request for Fetch / XHR / Style / Module / WebSocket also blocked
    for dest in [
        ResourceDestination::Fetch,
        ResourceDestination::Xhr,
        ResourceDestination::Style,
        ResourceDestination::Module,
        ResourceDestination::WebSocket,
    ] {
        let mut req = Request::subresource(&target_http, dest, 4096);
        req.origin = "https://secure.example.com".into();
        assert!(policy.permits_request(&req, &target_http).is_err());
    }

    // Request from HTTP initiator to HTTP target is permitted
    let mut req_http = Request::subresource(&target_http, ResourceDestination::Script, 4096);
    req_http.origin = "http://insecure.example.com".into();
    req_http.initiator = Some(Url::parse("http://insecure.example.com/page").unwrap());
    assert!(policy.permits_request(&req_http, &target_http).is_ok());
}

#[test]
fn test_redirect_method_rewriting_and_credential_stripping() {
    let listener_origin_a = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_a = listener_origin_a.local_addr().unwrap();

    let listener_origin_b = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_b = listener_origin_b.local_addr().unwrap();

    let stopped = Arc::new(AtomicBool::new(false));
    let done_a = stopped.clone();
    let done_b = stopped.clone();

    let server_a = thread::spawn(move || {
        while !done_a.load(Ordering::Relaxed) {
            listener_origin_a.set_nonblocking(true).unwrap();
            let Ok((mut stream, _)) = listener_origin_a.accept() else {
                thread::sleep(Duration::from_millis(2));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            let mut buf = [0; 2048];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);

            if req.starts_with("POST /redirect-303") {
                // Cross-origin redirect to origin B
                let resp = format!(
                    "HTTP/1.1 303 See Other\r\nLocation: http://{addr_b}/target\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(resp.as_bytes());
            } else {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n");
            }
        }
    });

    let server_b = thread::spawn(move || {
        while !done_b.load(Ordering::Relaxed) {
            listener_origin_b.set_nonblocking(true).unwrap();
            let Ok((mut stream, _)) = listener_origin_b.accept() else {
                thread::sleep(Duration::from_millis(2));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            let mut buf = [0; 2048];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);

            if req.starts_with("GET /target") {
                // Verify that cross-origin redirect stripped Authorization header and became GET
                let auth_present = req.to_ascii_lowercase().contains("authorization:");
                let body = if auth_present {
                    "has-auth"
                } else {
                    "clean-get"
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            } else {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n");
            }
        }
    });

    let broker = Broker::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let session = broker
        .open_session(
            dir.path().into(),
            Policy {
                allow_loopback: true,
                ..Policy::default()
            },
        )
        .unwrap();
    let (endpoint, token) = session.endpoint();
    let remote = Remote::new(&endpoint, token).unwrap();

    let start_url = Url::parse(&format!("http://{addr_a}/redirect-303")).unwrap();
    let mut req = Request::subresource(&start_url, ResourceDestination::Fetch, 4096);
    req.method = "POST".into();
    req.body = b"{\"payload\":42}".to_vec();
    req.headers = vec![
        ("authorization".into(), "Bearer sensitive-token".into()),
        ("content-type".into(), "application/json".into()),
    ];

    let response = remote.fetch(&req).expect("Redirect chain succeeded");
    assert_eq!(response.status, 200);
    assert_eq!(String::from_utf8_lossy(&response.body), "clean-get");
    assert!(response.redirected);

    stopped.store(true, Ordering::Relaxed);
    let _ = server_a.join();
    let _ = server_b.join();
}

#[test]
fn test_nosniff_mime_enforcement() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let done = stopped.clone();

    let server = thread::spawn(move || {
        while !done.load(Ordering::Relaxed) {
            listener.set_nonblocking(true).unwrap();
            let Ok((mut stream, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(2));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            let mut buf = [0; 1024];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);

            if req.contains("GET /index.html") {
                let resp = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 10\r\nConnection: close\r\n\r\n<p>doc</p>";
                let _ = stream.write_all(resp.as_bytes());
            } else if req.contains("GET /bad-script") {
                let resp = "HTTP/1.1 200 OK\r\nX-Content-Type-Options: nosniff\r\nContent-Type: text/plain\r\nContent-Length: 12\r\nConnection: close\r\n\r\nalert('bad')";
                let _ = stream.write_all(resp.as_bytes());
            } else if req.contains("GET /good-script") {
                let resp = "HTTP/1.1 200 OK\r\nX-Content-Type-Options: nosniff\r\nContent-Type: application/javascript\r\nContent-Length: 13\r\nConnection: close\r\n\r\nalert('good')";
                let _ = stream.write_all(resp.as_bytes());
            } else if req.contains("GET /bad-style") {
                let resp = "HTTP/1.1 200 OK\r\nX-Content-Type-Options: nosniff\r\nContent-Type: text/html\r\nContent-Length: 14\r\nConnection: close\r\n\r\nbody{color:red}";
                let _ = stream.write_all(resp.as_bytes());
            } else if req.contains("GET /good-style") {
                let resp = "HTTP/1.1 200 OK\r\nX-Content-Type-Options: nosniff\r\nContent-Type: text/css\r\nContent-Length: 14\r\nConnection: close\r\n\r\nbody{color:red}";
                let _ = stream.write_all(resp.as_bytes());
            } else {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n");
            }
        }
    });

    let broker = Broker::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let session = broker
        .open_bound_session(
            dir.path().into(),
            Policy {
                allow_loopback: true,
                ..Policy::default()
            },
        )
        .unwrap();
    let (endpoint, token) = session.endpoint();
    let remote = Remote::bound(&endpoint, token, session.identity()).unwrap();

    let doc_url = Url::parse(&format!("http://{addr}/index.html")).unwrap();
    session.begin_navigation(&doc_url);
    remote
        .fetch(&Request::get(&doc_url, 4096))
        .expect("Initial doc nav");

    // 1. Script with text/plain and nosniff must be rejected
    let bad_script_url = Url::parse(&format!("http://{addr}/bad-script")).unwrap();
    let mut req = Request::subresource(&bad_script_url, ResourceDestination::Script, 4096);
    req.origin = doc_url.origin().ascii_serialization();
    let res = remote.fetch(&req);
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(
        err.contains("blocked by nosniff"),
        "Expected nosniff block, got: {err}"
    );

    // 2. Script with application/javascript and nosniff must succeed
    let good_script_url = Url::parse(&format!("http://{addr}/good-script")).unwrap();
    let mut req = Request::subresource(&good_script_url, ResourceDestination::Script, 4096);
    req.origin = doc_url.origin().ascii_serialization();
    let res = remote.fetch(&req);
    assert!(
        res.is_ok(),
        "Good script MIME should succeed: {:?}",
        res.err()
    );

    // 3. Style with text/html and nosniff must be rejected
    let bad_style_url = Url::parse(&format!("http://{addr}/bad-style")).unwrap();
    let mut req = Request::subresource(&bad_style_url, ResourceDestination::Style, 4096);
    req.origin = doc_url.origin().ascii_serialization();
    let res = remote.fetch(&req);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("blocked by nosniff"));

    // 4. Style with text/css and nosniff must succeed
    let good_style_url = Url::parse(&format!("http://{addr}/good-style")).unwrap();
    let mut req = Request::subresource(&good_style_url, ResourceDestination::Style, 4096);
    req.origin = doc_url.origin().ascii_serialization();
    assert!(remote.fetch(&req).is_ok());

    stopped.store(true, Ordering::Relaxed);
    let _ = server.join();
}

#[test]
fn test_oversized_response_body_rejected() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let done = stopped.clone();

    let server = thread::spawn(move || {
        while !done.load(Ordering::Relaxed) {
            listener.set_nonblocking(true).unwrap();
            let Ok((mut stream, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(2));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            let mut buf = [0; 512];
            let _ = stream.read(&mut buf);
            let big_payload = vec![b'A'; 8192];
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                big_payload.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(&big_payload);
        }
    });

    let broker = Broker::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let session = broker
        .open_session(
            dir.path().into(),
            Policy {
                allow_loopback: true,
                ..Policy::default()
            },
        )
        .unwrap();
    let (endpoint, token) = session.endpoint();
    let remote = Remote::new(&endpoint, token).unwrap();

    let url = Url::parse(&format!("http://{addr}/big")).unwrap();
    // Request with limit 1024 bytes against 8192 bytes payload
    let req = Request::get(&url, 1024);
    let res = remote.fetch(&req);
    assert!(res.is_err(), "Oversized response must be rejected");
    assert!(res.unwrap_err().contains("exceeds its size limit"));

    stopped.store(true, Ordering::Relaxed);
    let _ = server.join();
}
