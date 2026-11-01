//! Real browser-approved/observed documents for service-bound fixture sessions.
#![allow(dead_code)]
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};
use wega_network::{Remote, Request, SessionHandle};
pub fn documents(sessions: &[(&SessionHandle, &Remote)]) -> url::Url {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = url::Url::parse(&format!(
        "http://{}/fixture",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let count = sessions.len();
    let server = std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(5);
        for _ in 0..count {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < until);
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("Document fixture: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                header.push(byte[0]);
                assert!(header.len() < 8192);
            }
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").unwrap();
        }
    });
    for (session, remote) in sessions {
        session.begin_navigation(&url);
        remote.fetch(&Request::get(&url, 4096)).unwrap();
        assert_ne!(
            session.take_committed_principal(Some(url.as_str())).origin,
            wega_protocol::authority::Origin::Opaque
        );
    }
    server.join().unwrap();
    url
}
