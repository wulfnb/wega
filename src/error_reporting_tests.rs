use super::*;
use crate::{
    diagnostic_event::{Category, Diagnostic},
    error_auth,
};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).unwrap();
        let path = std::env::temp_dir().join(format!("wega-report-{}", error_auth::hex(&bytes)));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[derive(Default)]
struct Mock {
    fail: bool,
    seen: Vec<Report>,
}
impl Transport for Mock {
    fn post(&mut self, report: &Report) -> Result<(), String> {
        self.seen.push(report.clone());
        if self.fail {
            Err("offline".into())
        } else {
            Ok(())
        }
    }
}
#[test]
fn unsigned_remote_http_and_credential_urls_are_rejected() {
    for endpoint in [
        "http://example.com/v1/errors",
        "https://u:p@example.com/v1/errors",
        "https://example.com/v1/errors#x",
        "file:///tmp/errors",
        "https://example.com/other",
    ] {
        assert!(
            Config {
                endpoint: endpoint.into()
            }
            .validate()
            .is_err(),
            "{endpoint}"
        );
    }
    assert!(
        Config {
            endpoint: "https://example.com/v1/errors".into()
        }
        .validate()
        .is_ok()
    );
    assert!(
        Config {
            endpoint: "http://127.0.0.1:1234/v1/errors".into()
        }
        .validate()
        .is_ok()
    );
}
#[test]
fn outbox_survives_failures_restart_and_retains_each_exception() {
    let directory = Directory::new();
    let outbox = Outbox::new(&directory.0).unwrap();
    for category in [Category::Css, Category::Html, Category::Javascript] {
        outbox
            .enqueue(
                "worker:7",
                Diagnostic::error(
                    category,
                    "exception\nstack",
                    Some("https://example.com/".into()),
                ),
            )
            .unwrap();
    }
    let mut transport = Mock {
        fail: true,
        ..Default::default()
    };
    assert!(outbox.deliver_one(&mut transport).is_err());
    let failed = transport.seen[0].id.clone();
    drop(outbox);
    let outbox = Outbox::new(&directory.0).unwrap();
    transport.fail = false;
    while outbox.deliver_one(&mut transport).unwrap() {}
    assert_eq!(transport.seen.len(), 4);
    assert!(transport.seen[1..].iter().any(|r| r.id == failed));
    assert_eq!(fs::read_dir(outbox.directory).unwrap().count(), 0);
}
#[test]
fn missing_endpoint_buffers_errors_and_corrupt_files_do_not_block_later_delivery() {
    let directory = Directory::new();
    let reporter = Reporter::buffer(&directory.0).unwrap();
    for index in 0..40 {
        reporter
            .enqueue(
                "worker",
                Diagnostic::error(Category::Javascript, format!("failure {index}"), None),
            )
            .unwrap();
    }
    drop(reporter);
    let outbox = Outbox::new(&directory.0).unwrap();
    fs::write(outbox.directory.join("damaged.json"), b"truncated {").unwrap();
    let mut transport = Mock::default();
    while outbox.deliver_one(&mut transport).unwrap() {}
    assert_eq!(transport.seen.len(), 40);
    assert!(outbox.directory.join("damaged.corrupt").exists());
    assert_eq!(fs::read_dir(&outbox.directory).unwrap().count(), 1);
}
#[test]
fn configuration_failures_never_turn_into_an_unvalidated_endpoint() {
    let directory = Directory::new();
    assert!(Config::load(&directory.0).unwrap().is_none());
    let path = directory.0.join("error-reporting.json");
    fs::write(&path, b"{").unwrap();
    assert!(Config::load(&directory.0).is_err());
    fs::write(&path, br#"{"endpoint":"http://example.com/v1/errors"}"#).unwrap();
    assert!(Config::load(&directory.0).is_err());
    fs::write(
        &path,
        br#"{"endpoint":"https://example.com/v1/errors","secret":"unsupported"}"#,
    )
    .unwrap();
    assert!(Config::load(&directory.0).is_err());
    fs::write(&path, br#"{"endpoint":"https://example.com/v1/errors"}"#).unwrap();
    assert_eq!(
        Config::load(&directory.0).unwrap().unwrap().endpoint,
        "https://example.com/v1/errors"
    );
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(Config::load(&directory.0).is_err());
}

#[test]
fn background_delivery_retries_a_durable_report_without_blocking_new_errors() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Background {
        failing: Arc<AtomicBool>,
        attempts: mpsc::Sender<String>,
    }
    impl Transport for Background {
        fn post(&mut self, report: &Report) -> Result<(), String> {
            let failing = self.failing.load(Ordering::SeqCst);
            self.attempts.send(report.id.clone()).unwrap();
            if failing {
                Err("offline".into())
            } else {
                Ok(())
            }
        }
    }
    let directory = Directory::new();
    let outbox = Outbox::new(&directory.0).unwrap();
    outbox
        .enqueue(
            "worker",
            Diagnostic::error(Category::Html, "queued before restart", None),
        )
        .unwrap();
    let failing = Arc::new(AtomicBool::new(true));
    let (attempts, receiver) = mpsc::channel();
    let reporter = Reporter::start(
        &directory.0,
        Background {
            failing: failing.clone(),
            attempts,
        },
    )
    .unwrap();
    let first = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
    // Producers can persist another error while the transport is backing off.
    reporter
        .enqueue(
            "worker",
            Diagnostic::error(Category::Css, "another error", None),
        )
        .unwrap();
    assert_eq!(fs::read_dir(&outbox.directory).unwrap().count(), 2);
    failing.store(false, Ordering::SeqCst);
    let mut delivered = Vec::new();
    for _ in 0..2 {
        delivered.push(receiver.recv_timeout(Duration::from_secs(4)).unwrap());
    }
    assert!(delivered.contains(&first));
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while fs::read_dir(&outbox.directory).unwrap().next().is_some() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    drop(reporter);
}
#[test]
fn identity_is_private_stable_and_never_silently_replaced() {
    let directory = Directory::new();
    let first = Identity::load(&directory.0).unwrap();
    let second = Identity::load(&directory.0).unwrap();
    assert_eq!(first.client_id, second.client_id);
    assert_eq!(first.seed, second.seed);
    let path = directory.0.join("error-reporting/identity.json");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let body = b"report";
    let signature = first.sign("123", "nonce", body);
    assert!(error_auth::verify(
        &error_auth::public_key(&first.seed),
        &signature,
        &first.client_id,
        "123",
        "nonce",
        body
    ));
    assert!(!error_auth::verify(
        &error_auth::public_key(&first.seed),
        &signature,
        &first.client_id,
        "123",
        "nonce",
        b"tampered"
    ));
    fs::write(&path, b"corrupt identity").unwrap();
    assert!(Identity::load(&directory.0).is_err());
    assert_eq!(fs::read(path).unwrap(), b"corrupt identity");
}
#[test]
fn malformed_signatures_and_invalid_public_keys_are_rejected_without_panics() {
    assert!(error_auth::unhex::<1>("gg").is_err());
    assert!(error_auth::unhex::<1>("λ").is_err());
    let invalid = (0..=255)
        .map(|byte| [byte; 32])
        .find(|key| ed25519_dalek::VerifyingKey::from_bytes(key).is_err())
        .unwrap();
    assert!(!error_auth::verify(
        &invalid,
        &"00".repeat(64),
        "client",
        "1",
        "nonce",
        b"body"
    ));
    assert!(!error_auth::verify(
        &error_auth::public_key(&[3; 32]),
        "invalid",
        "client",
        "1",
        "nonce",
        b"body"
    ));
}
#[cfg(feature = "desktop")]
#[test]
fn real_http_posts_are_signed_and_retry_with_the_same_id_and_a_fresh_nonce() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let directory = Directory::new();
    let identity = Identity::load(&directory.0).unwrap();
    let key = error_auth::public_key(&identity.seed);
    let server = std::thread::spawn(move || {
        let mut ids = vec![];
        let mut nonces = vec![];
        for status in ["503 Service Unavailable", "202 Accepted"] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = vec![];
            let end = loop {
                let mut b = [0; 1024];
                let n = stream.read(&mut b).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&b[..n]);
                if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
            };
            let head = String::from_utf8(bytes[..end].to_vec()).unwrap();
            let headers: std::collections::HashMap<_, _> = head
                .lines()
                .skip(1)
                .filter_map(|s| s.split_once(':'))
                .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_owned()))
                .collect();
            let length: usize = headers["content-length"].parse().unwrap();
            while bytes.len() < end + length {
                let mut b = [0; 1024];
                let n = stream.read(&mut b).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&b[..n]);
            }
            let body = &bytes[end..end + length];
            assert!(error_auth::verify(
                &key,
                &headers["x-wega-signature"],
                &headers["x-wega-client"],
                &headers["x-wega-timestamp"],
                &headers["x-wega-nonce"],
                body
            ));
            let report: Report = serde_json::from_slice(body).unwrap();
            ids.push(report.id);
            nonces.push(headers["x-wega-nonce"].clone());
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        }
        assert_eq!(ids[0], ids[1]);
        assert_ne!(nonces[0], nonces[1]);
    });
    let outbox = Outbox::new(&directory.0).unwrap();
    outbox
        .enqueue(
            "worker",
            Diagnostic::error(Category::Javascript, "failure", None),
        )
        .unwrap();
    let mut transport = HttpTransport::new(
        Config {
            endpoint: format!("http://{address}/v1/errors"),
        },
        &directory.0,
    )
    .unwrap();
    assert!(outbox.deliver_one(&mut transport).is_err());
    assert!(outbox.deliver_one(&mut transport).unwrap());
    assert!(!outbox.deliver_one(&mut transport).unwrap());
    server.join().unwrap();
}
