//! The privileged service performs TLS and the HTTP upgrade. A content worker
//! receives only a scoped, bounded byte transport for that approved WebSocket.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use sha1::{Digest, Sha1};
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::sync::Mutex as AsyncMutex;
struct Socket {
    reader: AsyncMutex<ReadHalf<reqwest::Upgraded>>,
    writer: AsyncMutex<WriteHalf<reqwest::Upgraded>>,
}
type Key = ([u8; 16], String);
#[derive(Default)]
pub(crate) struct Sockets {
    entries: HashMap<Key, Option<Arc<Socket>>>,
}
impl Sockets {
    pub fn revoke(&mut self, service: ServiceSessionId) {
        self.entries.retain(|(id, _), _| *id != service.0);
    }
}
struct Reservation<'a> {
    broker: &'a Broker,
    key: Key,
    committed: bool,
}
impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.broker
                .sockets
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .entries
                .remove(&self.key);
        }
    }
}
impl Broker {
    pub(crate) fn websocket_open(
        &self,
        session: &Session,
        address: &str,
        protocols: &str,
    ) -> Result<Vec<u8>, String> {
        if protocols.len() > 4096 {
            return Err("WebSocket protocols exceed header budget".into());
        }
        let requested = Url::parse(address).map_err(|error| error.to_string())?;
        if !matches!(requested.scheme(), "ws" | "wss")
            || !requested.username().is_empty()
            || requested.password().is_some()
            || requested.fragment().is_some()
        {
            return Err("Invalid WebSocket URL".into());
        }
        let document = session
            .document()
            .ok_or("No document WebSocket authority")?;
        if document.scheme() == "https" && requested.scheme() != "wss" {
            return Err("Mixed-content WebSocket blocked".into());
        }
        let mut target = requested.clone();
        let secure = requested.scheme() == "wss";
        target
            .set_scheme(if secure { "https" } else { "http" })
            .map_err(|_| "Invalid socket scheme")?;
        let policy = session
            .policy
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let request = Request::subresource(&target, ResourceDestination::WebSocket, MAX_BODY);
        policy.permits_request(&request, &target)?;
        let addresses = resolve_target(&target, &policy)?;
        let id: String = nonce()?
            .iter()
            .map(|value| format!("{value:02x}"))
            .collect();
        let key = (session.identity.service.0, id.clone());
        {
            let mut sockets = self
                .sockets
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if sockets.entries.len() >= 64
                || sockets
                    .entries
                    .keys()
                    .filter(|(service, _)| *service == key.0)
                    .count()
                    >= 8
            {
                return Err("WebSocket admission limit reached".into());
            }
            sockets.entries.insert(key.clone(), None);
        }
        let mut reservation = Reservation {
            broker: self,
            key: key.clone(),
            committed: false,
        };
        let cookie = session.cookies.get(
            &requested,
            false,
            Cookies::context(&requested, Some(&document), false),
        );
        let result = self.socket_runtime.block_on(async {
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(15))
                .resolve_to_addrs(
                    target.host_str().ok_or("Missing WebSocket host")?,
                    &addresses,
                )
                .build()
                .map_err(|error| error.to_string())?;
            let mut entropy = [0; 16];
            getrandom::fill(&mut entropy).map_err(|error| error.to_string())?;
            let handshake_key = STANDARD.encode(entropy);
            let mut builder = client
                .get(target)
                .header("connection", "Upgrade")
                .header("upgrade", "websocket")
                .header("sec-websocket-version", "13")
                .header("sec-websocket-key", &handshake_key)
                .header("origin", document.origin().ascii_serialization());
            if !protocols.is_empty() {
                builder = builder.header("sec-websocket-protocol", protocols);
            }
            if !cookie.is_empty() {
                builder = builder.header("cookie", cookie);
            }
            let response = builder.send().await.map_err(|error| error.to_string())?;
            let expected = STANDARD.encode(Sha1::digest(
                format!("{handshake_key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes(),
            ));
            let header = |name| {
                response
                    .headers()
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
            };
            if response.status() != 101
                || !header("upgrade").eq_ignore_ascii_case("websocket")
                || !header("connection")
                    .split(',')
                    .any(|value| value.trim().eq_ignore_ascii_case("upgrade"))
                || header("sec-websocket-accept") != expected
            {
                return Err("Invalid WebSocket upgrade response".into());
            }
            let protocol = header("sec-websocket-protocol").to_owned();
            if !protocol.is_empty() && !protocols.split(',').any(|value| value.trim() == protocol) {
                return Err("Server selected an unrequested WebSocket protocol".into());
            }
            if response.headers().contains_key("sec-websocket-extensions") {
                return Err("Unrequested WebSocket extension".into());
            }
            for cookie in response.headers().get_all("set-cookie") {
                if let Ok(cookie) = cookie.to_str() {
                    session.cookies.set(
                        &requested,
                        cookie,
                        false,
                        Cookies::context(&requested, Some(&document), false),
                    );
                }
            }
            let upgraded = response
                .upgrade()
                .await
                .map_err(|error| error.to_string())?;
            let (reader, writer) = tokio::io::split(upgraded);
            Ok::<_, String>((
                Arc::new(Socket {
                    reader: AsyncMutex::new(reader),
                    writer: AsyncMutex::new(writer),
                }),
                protocol,
            ))
        })?;
        let mut sockets = self
            .sockets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let slot = sockets
            .entries
            .get_mut(&key)
            .ok_or("WebSocket grant was revoked during upgrade")?;
        if session.revoked.load(Ordering::Acquire) {
            return Err("Revoked WebSocket session".into());
        }
        *slot = Some(result.0);
        reservation.committed = true;
        serde_json::to_vec(&json!({"id": id, "protocol":result.1}))
            .map_err(|error| error.to_string())
    }
    pub(crate) fn websocket_io(
        &self,
        session: &Session,
        id: &str,
        operation: &str,
        body: &[u8],
    ) -> Result<Vec<u8>, String> {
        let key = (session.identity.service.0, id.to_owned());
        let socket = {
            let mut sockets = self
                .sockets
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if operation == "close" {
                sockets.entries.remove(&key);
                return Ok(Vec::new());
            }
            sockets
                .entries
                .get(&key)
                .and_then(Clone::clone)
                .ok_or("Unknown or revoked WebSocket")?
        };
        let result = self.socket_runtime.block_on(async {
            match operation {
                "read" if body.is_empty() => {
                    let mut output = vec![0; 65537];
                    let mut reader = socket
                        .reader
                        .try_lock()
                        .map_err(|_| "WebSocket read already pending")?;
                    match tokio::time::timeout(
                        Duration::from_millis(100),
                        reader.read(&mut output[1..]),
                    )
                    .await
                    {
                        Err(_) => Ok(vec![0]), // pending; AsyncRead::read is cancellation-safe
                        Ok(Ok(0)) => Ok(vec![2]),
                        Ok(Ok(count)) => {
                            output[0] = 1;
                            output.truncate(count + 1);
                            Ok(output)
                        }
                        Ok(Err(error)) => Err(error.to_string()),
                    }
                }
                "write" if body.len() <= 65536 => {
                    let mut writer = socket
                        .writer
                        .try_lock()
                        .map_err(|_| "WebSocket write already pending")?;
                    tokio::time::timeout(Duration::from_secs(5), writer.write_all(body))
                        .await
                        .map_err(|_| "WebSocket write deadline exceeded")?
                        .map_err(|error| error.to_string())?;
                    Ok(Vec::new())
                }
                _ => Err("Invalid WebSocket byte operation".into()),
            }
        });
        if result.is_err() {
            self.sockets
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .entries
                .remove(&key);
        }
        result
    }
}
/// Resolve once and pin the client to the checked addresses. Host rules are not
/// sufficient when a public-looking DNS name resolves to a private interface.
pub(crate) fn resolve_target(url: &Url, policy: &Policy) -> Result<Vec<SocketAddr>, String> {
    use std::net::ToSocketAddrs;
    let host = url
        .host_str()
        .ok_or("Missing network host")?
        .trim_start_matches('[')
        .trim_end_matches(']');
    let port = url.port_or_known_default().ok_or("Missing network port")?;
    let addresses: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|error| error.to_string())?
        .take(32)
        .collect();
    if addresses.is_empty() {
        return Err("Network host has no addresses".into());
    }
    for address in &addresses {
        if !policy.allow_loopback && is_private_or_loopback(&address.ip().to_string()) {
            // An explicit host allowlist is a browser policy grant, never a
            // renderer field. Preserve it while checking each resolved address.
            if policy.allowed_hosts.trim().is_empty() || !policy.permits(url) {
                return Err("Private network target after DNS resolution".into());
            }
        }
    }
    Ok(addresses)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn broker_upgrades_websockets_with_host_cookies_and_revokes_the_byte_grant() {
        use std::io::{Read, Write};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
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
            let request = String::from_utf8(header).unwrap();
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("cookie: private=host")
            );
            let key = request
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("sec-websocket-key"))
                        .map(|(_, value)| value.trim())
                })
                .unwrap();
            let accept = STANDARD.encode(Sha1::digest(
                format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes(),
            ));
            write!(stream,"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").unwrap();
            let mut message = [0; 8];
            stream.read_exact(&mut message).unwrap();
            assert_eq!(message, [0x81, 0x82, 1, 2, 3, 4, b'h' ^ 1, b'i' ^ 2]);
            stream.write_all(&[0x81, 2, b'h', b'i']).unwrap();
        });
        let broker = Broker::new().unwrap();
        let root = tempfile::tempdir().unwrap();
        let session = broker
            .open_bound_session(
                root.path().into(),
                Policy {
                    allow_loopback: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let document = Url::parse(&format!("http://{address}/")).unwrap();
        session.begin_navigation(&document);
        session.session.navigation.lock().unwrap().committed = Some(document.clone());
        session.cookie_jar().set(
            &document,
            "private=host; HttpOnly; Path=/",
            false,
            SiteContext::SameSite,
        );
        let (address, token) = session.endpoint();
        let remote = Remote::bound(&address, token, session.identity()).unwrap();
        let opened = remote
            .resource(
                ResourceRequest::WebSocketOpen {
                    url: document.as_str().replacen("http:", "ws:", 1),
                    protocols: String::new(),
                },
                &[],
            )
            .unwrap();
        let opened: serde_json::Value = serde_json::from_slice(&opened).unwrap();
        let id = opened["id"].as_str().unwrap().to_owned();
        remote
            .resource(
                ResourceRequest::WebSocketIo {
                    id: id.clone(),
                    operation: "write".into(),
                },
                &[0x81, 0x82, 1, 2, 3, 4, b'h' ^ 1, b'i' ^ 2],
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut echoed = Vec::new();
        while echoed.len() < 4 {
            assert!(Instant::now() < deadline);
            let reply = remote
                .resource(
                    ResourceRequest::WebSocketIo {
                        id: id.clone(),
                        operation: "read".into(),
                    },
                    &[],
                )
                .unwrap();
            if reply[0] == 1 {
                echoed.extend_from_slice(&reply[1..]);
            } else {
                assert_eq!(reply[0], 0);
            }
        }
        assert_eq!(echoed, [0x81, 2, b'h', b'i']);
        session.revoke();
        assert!(
            remote
                .resource(
                    ResourceRequest::WebSocketIo {
                        id,
                        operation: "read".into()
                    },
                    &[]
                )
                .is_err()
        );
        server.join().unwrap();
    }
}
