use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use wega_network::{Broker, Policy, Remote, Request};
#[test]
fn real_http_is_shared_but_credentials_policy_and_cache_are_profile_partitioned() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let count = Arc::new(AtomicUsize::new(0));
    let done = stopped.clone();
    let calls = count.clone();
    let server = std::thread::spawn(move || {
        while !done.load(Ordering::Relaxed) {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut header = vec![];
            let mut byte = [0];
            while !header.ends_with(b"\r\n\r\n") {
                if stream.read_exact(&mut byte).is_err() {
                    break;
                };
                header.push(byte[0]);
                assert!(header.len() < 65536);
            }
            let header = String::from_utf8(header).unwrap();
            calls.fetch_add(1, Ordering::Relaxed);
            let (status, extra, body) = if header.starts_with("GET /redirect ") {
                (
                    "302 Found",
                    format!("Location: http://localhost:{}/private\r\n", address.port()),
                    "".to_string(),
                )
            } else if header.starts_with("GET /cookie ") {
                (
                    "200 OK",
                    "Cache-Control: private\r\nSet-Cookie: token=shared; Path=/; HttpOnly\r\n"
                        .into(),
                    header.clone(),
                )
            } else {
                (
                    "200 OK",
                    "Cache-Control: public, max-age=60\r\n".into(),
                    "cached".into(),
                )
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    let broker = Broker::new().unwrap();
    let profile = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let session = |dir| broker.open_session(dir, Policy::default()).unwrap();
    let a = session(profile.path().into());
    let b = session(profile.path().into());
    let c = session(other.path().into());
    let remote = |session: &wega_network::SessionHandle| {
        let (addr, token) = session.endpoint();
        Remote::new(&addr, token).unwrap()
    };
    let url = url::Url::parse(&format!("http://{address}/public")).unwrap();
    let request = Request::get(&url, 1024);
    assert_eq!(remote(&a).fetch(&request).unwrap().body, b"cached");
    assert_eq!(remote(&b).fetch(&request).unwrap().body, b"cached");
    assert_eq!(count.load(Ordering::Relaxed), 1);
    remote(&c).fetch(&request).unwrap();
    assert_eq!(count.load(Ordering::Relaxed), 2);
    let mut small = request.clone();
    small.limit = 2;
    assert!(remote(&b).fetch(&small).is_err());
    a.set_policy(Policy {
        offline: true,
        ..Default::default()
    });
    assert!(remote(&a).fetch(&request).is_err());
    let cookie = url.join("/cookie").unwrap();
    remote(&b).fetch(&Request::get(&cookie, 2048)).unwrap();
    let response = remote(&b).fetch(&Request::get(&cookie, 2048)).unwrap();
    assert!(
        String::from_utf8(response.body)
            .unwrap()
            .to_lowercase()
            .contains("cookie: token=shared")
    );
    assert_eq!(remote(&b).cookie_get(&cookie, true).unwrap(), "");
    assert_eq!(
        remote(&b).cookie_get(&cookie, false).unwrap(),
        "token=shared"
    );
    assert_eq!(remote(&c).cookie_get(&cookie, false).unwrap(), "");
    b.set_policy(Policy {
        allowed_hosts: "127.0.0.1".into(),
        ..Default::default()
    });
    let redirected = remote(&b).fetch(&Request::get(&url.join("/redirect").unwrap(), 1024));
    assert!(redirected.unwrap_err().contains("policy"));
    stopped.store(true, Ordering::Relaxed);
    server.join().unwrap();
}

#[test]
fn cookie_values_use_bounded_body_framing_without_a_metadata_size_regression() {
    let broker = Broker::new().unwrap();
    let profile = tempfile::tempdir().unwrap();
    let session = broker
        .open_session(profile.path().into(), Policy::default())
        .unwrap();
    let (address, token) = session.endpoint();
    let remote = Remote::new(&address, token).unwrap();
    let url = url::Url::parse("https://fixture.test/").unwrap();
    for index in 0..40 {
        remote
            .cookie_set(
                &url,
                &format!("key{index}={}; Path=/", "x".repeat(2048)),
                true,
            )
            .unwrap();
    }
    let cookies = remote.cookie_get(&url, true).unwrap();
    assert!(cookies.len() > 64 * 1024);
    assert!(cookies.contains("key39="));
}
