//! Deno framing/events over the host-owned WebSocket upgrade and byte service.
mod support;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::Arc,
    time::Duration,
};
use support::{Reply, Server, open, wait};
use wega_engine::{Engine, Limits, NetworkPolicy};
#[test]
fn a_brokered_realm_can_send_and_receive_a_websocket_message() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use sha1::{Digest, Sha1};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let echo = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            header.push(byte[0]);
            assert!(header.len() < 8192);
        }
        let header = String::from_utf8(header).unwrap();
        let key = header
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(key, _)| key.eq_ignore_ascii_case("sec-websocket-key"))
                    .map(|(_, value)| value.trim())
            })
            .unwrap();
        let accept = STANDARD.encode(Sha1::digest(
            format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes(),
        ));
        write!(stream,"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").unwrap();
        let mut fixed = [0; 2];
        stream.read_exact(&mut fixed).unwrap();
        assert_eq!(fixed, [0x81, 0x82]);
        let mut masked = [0; 6];
        stream.read_exact(&mut masked).unwrap();
        assert_eq!([masked[4] ^ masked[0], masked[5] ^ masked[1]], *b"hi");
        stream.write_all(&[0x81, 2, b'h', b'i', 0x88, 0]).unwrap();
    });
    let server = Server::new(|_| Reply::html("<title>broker socket</title>"));
    let url = server.url("127.0.0.1", "/");
    let profile = tempfile::tempdir().unwrap();
    let broker = wega_network::Broker::new().unwrap();
    let policy = NetworkPolicy {
        allow_loopback: true,
        ..Default::default()
    };
    let session = broker
        .open_bound_session(profile.path().into(), policy.clone())
        .unwrap();
    session.begin_navigation(&url.parse().unwrap());
    let channels = session.inherited_channels(4).unwrap();
    let (_, token) = session.endpoint();
    let remote = wega_network::Remote::inherited(channels, token, session.identity()).unwrap();
    let engine = Engine::with_broker(Limits::default(), policy, Arc::new(|| {}), remote).unwrap();
    let mut view = open(&engine, &url);
    view.evaluate(&format!("globalThis.socket=new WebSocket({});socket.onopen=()=>socket.send('hi');socket.onmessage=event=>globalThis.received=event.data;socket.onerror=()=>globalThis.received='error'",serde_json::json!(format!("ws://{address}/")))).unwrap();
    assert_eq!(wait(&mut view, "globalThis.received??null"), "\"hi\"");
    echo.join().unwrap();
}
