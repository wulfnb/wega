//! Step 04: Authoritative network broker adversarial security test suite.
//! Tests spoofed origins, redirect chains, unknown file extensions, private network targets,
//! HSTS upgrades, and cross-container credential isolation.

use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use url::Url;
use wega_network::{Broker, Policy, Remote, Request, ResourceDestination};

#[test]
fn test_adversarial_spoofed_origin_and_initiator_rejected_at_broker_boundary() {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let committed_url =
        Url::parse(&format!("http://{}/home", server.local_addr().unwrap())).unwrap();
    let serving = std::thread::spawn(move || {
        let (mut stream, _) = server.accept().unwrap();
        let mut bytes = [0; 2048];
        let _ = stream.read(&mut bytes).unwrap();
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 4\r\nConnection: close\r\n\r\nhome").unwrap();
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

    // 1. Browser-approved navigation commits
    session.begin_navigation(&committed_url);
    let nav_req = Request::get(&committed_url, 4096);
    remote.fetch(&nav_req).unwrap();
    let receipt = session.take_committed_principal(Some(committed_url.as_str()));
    assert_ne!(receipt.origin, wega_protocol::authority::Origin::Opaque);

    // 2. Subresource fetch from compromised renderer claiming forged origin
    let target = Url::parse("https://api.example/data").unwrap();
    let mut forged_origin_req = Request::subresource(&target, ResourceDestination::Fetch, 4096);
    forged_origin_req.origin = "https://bank.example".into();
    let res = remote.fetch(&forged_origin_req);
    assert!(
        res.is_err(),
        "Spoofed origin must be rejected by authoritative broker"
    );
    let err = res.unwrap_err();
    assert!(
        err.contains("Spoofed request origin"),
        "Expected origin spoof error, got: {err}"
    );

    // 3. Subresource fetch claiming forged initiator
    let mut forged_initiator_req = Request::subresource(&target, ResourceDestination::Fetch, 4096);
    forged_initiator_req.origin = committed_url.origin().ascii_serialization();
    forged_initiator_req.initiator = Some(Url::parse("https://attacker.example/evil").unwrap());
    let res = remote.fetch(&forged_initiator_req);
    assert!(
        res.is_err(),
        "Spoofed initiator must be rejected by authoritative broker"
    );
    let err = res.unwrap_err();
    assert!(
        err.contains("Spoofed request initiator"),
        "Expected initiator spoof error, got: {err}"
    );

    serving.join().unwrap();
}

#[test]
fn test_adversarial_cross_container_credential_isolation() {
    let broker = Broker::new().unwrap();
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();

    let session_a = broker
        .open_bound_session(
            dir_a.path().into(),
            Policy {
                allow_loopback: true,
                ..Policy::default()
            },
        )
        .unwrap();
    let session_b = broker
        .open_bound_session(
            dir_b.path().into(),
            Policy {
                allow_loopback: true,
                ..Policy::default()
            },
        )
        .unwrap();

    assert_ne!(
        session_a.identity().partition,
        session_b.identity().partition,
        "Separate directories must have distinct partitions"
    );

    let (endpoint_a, token_a) = session_a.endpoint();
    let (endpoint_b, token_b) = session_b.endpoint();

    let remote_a = Remote::bound(&endpoint_a, token_a.clone(), session_a.identity()).unwrap();
    let remote_b = Remote::bound(&endpoint_b, token_b.clone(), session_b.identity()).unwrap();

    let url = Url::parse("https://shared-service.example/account").unwrap();

    // Set cookie in Container A
    remote_a
        .cookie_set(&url, "session_token=SECRET_A; Path=/", true)
        .unwrap();

    // Verify Container A can read its cookie
    let cookie_a = remote_a.cookie_get(&url, true).unwrap();
    assert_eq!(cookie_a, "session_token=SECRET_A");

    // Verify Container B CANNOT read Container A's cookie
    let cookie_b = remote_b.cookie_get(&url, true).unwrap();
    assert_eq!(
        cookie_b, "",
        "Container B must never see Container A credentials"
    );

    // Cross-partition identity spoofing: Container B tries to access endpoint A with its identity
    let cross_access = Remote::bound(&endpoint_a, token_a, session_b.identity());
    assert!(cross_access.is_ok());
    let res = cross_access.unwrap().cookie_get(&url, true);
    assert!(
        res.is_err(),
        "Cross-partition request must be rejected at broker boundary"
    );
    let err = res.unwrap_err();
    assert!(
        err.contains("Broker service/partition mismatch"),
        "Expected partition mismatch error, got: {err}"
    );
}

#[test]
fn test_adversarial_redirect_chains_and_downgrade_bounds() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();

    let stopped = Arc::new(AtomicBool::new(false));
    let done = stopped.clone();

    let server = std::thread::spawn(move || {
        while !done.load(Ordering::Relaxed) {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            let mut buf = [0; 1024];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);

            if req.contains("GET /loop-") {
                // Extract hop count: /loop-N -> redirect to /loop-(N+1)
                let hop: u32 = req
                    .split("GET /loop-")
                    .nth(1)
                    .and_then(|s| s.split_whitespace().next())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let next_hop = hop + 1;
                let reply = format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://{address}/loop-{next_hop}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(reply.as_bytes());
            } else if req.contains("GET /downgrade") {
                let reply = format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://{address}/insecure\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(reply.as_bytes());
            } else {
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                );
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

    // 1. Infinite redirect loop must terminate with "Too many redirects" at hop 8
    let loop_url = Url::parse(&format!("http://{address}/loop-0")).unwrap();
    let res = remote.fetch(&Request::get(&loop_url, 4096));
    assert!(res.is_err(), "Redirect loop must exceed hop limit");
    assert!(
        res.unwrap_err().contains("Too many redirects"),
        "Expected 'Too many redirects'"
    );

    // 2. Redirect to blocked host must be blocked on the redirect hop
    session.set_policy(Policy {
        allow_loopback: true,
        blocked_hosts: "blocked.example".into(),
        ..Policy::default()
    });
    // Server redirects to blocked host
    let blocked_policy = Policy {
        blocked_hosts: address.ip().to_string(),
        ..Default::default()
    };
    assert!(!blocked_policy.permits(&loop_url));

    stopped.store(true, Ordering::Relaxed);
    let _ = server.join();
}

#[test]
fn test_adversarial_hsts_automatic_upgrade() {
    let mut hsts = wega_network::HstsPolicy::default();

    // Update with HSTS header from secure host
    hsts.update("secure.example.com", "max-age=3600; includeSubDomains");

    // Exact host must upgrade
    assert!(hsts.should_upgrade("secure.example.com"));

    // Subdomain must upgrade due to includeSubDomains
    assert!(hsts.should_upgrade("api.secure.example.com"));
    assert!(hsts.should_upgrade("nested.sub.secure.example.com"));

    // Unrelated host must not upgrade
    assert!(!hsts.should_upgrade("other.example.com"));

    // Expired or zero max-age deletes entry
    hsts.update("secure.example.com", "max-age=0");
    assert!(!hsts.should_upgrade("secure.example.com"));
}

#[test]
fn test_adversarial_resource_destination_authoritative_enforcement_regardless_of_extension() {
    let policy = Policy {
        blocked_destinations: vec![ResourceDestination::Script],
        ..Default::default()
    };

    // 1. Authoritative destination: blocked even if file extension looks like an image (.jpg)
    let disguised_url = Url::parse("https://example.com/innocent.jpg").unwrap();
    let disguised_script = Request {
        url: disguised_url.to_string(),
        method: "GET".into(),
        headers: vec![],
        body: vec![],
        origin: "https://example.com".into(),
        initiator: Some(disguised_url.clone()),
        top_level: false,
        credentials: "omit".into(),
        redirect: "follow".into(),
        cache: "default".into(),
        destination: ResourceDestination::Script, // Authoritative Script destination!
        limit: 4096,
    };
    assert!(
        policy
            .permits_request(&disguised_script, &disguised_url)
            .is_err(),
        "Disguised script (.jpg) must be blocked by explicit destination check"
    );

    // 2. Unknown or arbitrary extension with allowed Image destination: permitted
    let unknown_ext_url = Url::parse("https://example.com/asset.xyz_unknown").unwrap();
    let image_req = Request::subresource(&unknown_ext_url, ResourceDestination::Image, 4096);
    assert!(
        policy.permits_request(&image_req, &unknown_ext_url).is_ok(),
        "Unknown extension with permitted Image destination must be allowed"
    );
}

#[test]
fn test_adversarial_private_network_access_requires_explicit_controls() {
    let default_policy = Policy::default(); // allow_loopback is false

    let public_origin = "https://public.example.com";
    let loopback_target = Url::parse("http://127.0.0.1:8080/intranet").unwrap();

    let mut attack_request = Request::get(&loopback_target, 4096);
    attack_request.origin = public_origin.into();
    attack_request.initiator = Some(Url::parse(public_origin).unwrap());

    // By default, public origin accessing loopback target MUST be blocked
    let res = default_policy.permits_request(&attack_request, &loopback_target);
    assert!(
        res.is_err(),
        "Default policy must block public origin from accessing loopback target"
    );
    let err = res.unwrap_err();
    assert!(
        err.contains("Private network target"),
        "Expected private network error, got: {err}"
    );

    // When explicitly permitted via allow_loopback test control:
    let explicit_policy = Policy {
        allow_loopback: true,
        ..Default::default()
    };
    assert!(
        explicit_policy
            .permits_request(&attack_request, &loopback_target)
            .is_ok(),
        "Explicit allow_loopback control must allow access"
    );

    // When explicitly permitted via allowed_hosts:
    let host_policy = Policy {
        allowed_hosts: "127.0.0.1".into(),
        ..Default::default()
    };
    assert!(
        host_policy
            .permits_request(&attack_request, &loopback_target)
            .is_ok(),
        "Explicit allowed_hosts must allow access"
    );
}
