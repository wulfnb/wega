//! Async byte transport over the existing bounded host-service dispatcher.
//! Deno owns WebSocket framing; the host owns dialing, TLS, cookies and grants.
use crate::network::Network;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use wega_network::ResourceRequest;
type Operation = Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send>>;
fn operation(network: Network, request: ResourceRequest, body: Vec<u8>) -> Operation {
    Box::pin(async move {
        let remote = network.remote.clone().ok_or("Missing WebSocket service")?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        network.spawn(move |_| {
            let _ = sender.send(remote.resource(request, &body));
        })?;
        receiver.await.map_err(|_| "WebSocket dispatcher stopped")?
    })
}
pub(crate) struct Connector {
    pub network: Network,
}
impl deno_websocket::BrokerConnector for Connector {
    fn connect(
        &self,
        url: String,
        protocols: String,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<(Box<dyn deno_websocket::BrokerStream>, String), String>>
                + Send,
        >,
    > {
        let network = self.network.clone();
        Box::pin(async move {
            let remote = network.remote.clone().ok_or("Missing WebSocket service")?;
            let (sender, receiver) = tokio::sync::oneshot::channel();
            network.spawn(move |_| {
                let result =
                    remote.resource(ResourceRequest::WebSocketOpen { url, protocols }, &[]);
                // Cancellation while the host connects must not leak a socket.
                if let Err(Ok(reply)) = sender.send(result)
                    && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&reply)
                    && let Some(id) = value.get("id").and_then(|id| id.as_str())
                {
                    let _ = remote.resource(
                        ResourceRequest::WebSocketIo {
                            id: id.into(),
                            operation: "close".into(),
                        },
                        &[],
                    );
                }
            })?;
            let reply = receiver
                .await
                .map_err(|_| "WebSocket connect dispatcher stopped")??;
            let value: serde_json::Value =
                serde_json::from_slice(&reply).map_err(|error| error.to_string())?;
            let id = value
                .get("id")
                .and_then(|value| value.as_str())
                .ok_or("Missing WebSocket grant")?
                .to_owned();
            let protocol = value
                .get("protocol")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_owned();
            Ok((
                Box::new(Stream {
                    network,
                    id,
                    reading: None,
                    writing: None,
                    buffered: Vec::new(),
                    offset: 0,
                    write_len: 0,
                    closing: None,
                    eof: false,
                }) as Box<dyn deno_websocket::BrokerStream>,
                protocol,
            ))
        })
    }
}
struct Stream {
    network: Network,
    id: String,
    reading: Option<Operation>,
    writing: Option<Operation>,
    closing: Option<Operation>,
    buffered: Vec<u8>,
    offset: usize,
    write_len: usize,
    eof: bool,
}
impl Stream {
    fn request(&self, operation: &str, body: Vec<u8>) -> Operation {
        operation_fn(
            self.network.clone(),
            ResourceRequest::WebSocketIo {
                id: self.id.clone(),
                operation: operation.into(),
            },
            body,
        )
    }
}
fn operation_fn(network: Network, request: ResourceRequest, body: Vec<u8>) -> Operation {
    operation(network, request, body)
}
impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        loop {
            if this.offset < this.buffered.len() {
                let count = output.remaining().min(this.buffered.len() - this.offset);
                output.put_slice(&this.buffered[this.offset..this.offset + count]);
                this.offset += count;
                return Poll::Ready(Ok(()));
            }
            if this.eof {
                return Poll::Ready(Ok(()));
            }
            if this.reading.is_none() {
                this.reading = Some(this.request("read", Vec::new()));
            }
            let result = std::task::ready!(this.reading.as_mut().unwrap().as_mut().poll(cx));
            this.reading = None;
            let data = result.map_err(io::Error::other)?;
            match data.first() {
                Some(0) => continue,
                Some(1) if data.len() > 1 => {
                    this.buffered = data;
                    this.offset = 1;
                }
                Some(2) if data.len() == 1 => {
                    this.eof = true;
                }
                _ => return Poll::Ready(Err(io::Error::other("Invalid WebSocket byte response"))),
            }
        }
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if this.writing.is_none() {
            this.write_len = bytes.len().min(65536);
            this.writing = Some(this.request("write", bytes[..this.write_len].to_vec()));
        }
        let result = std::task::ready!(this.writing.as_mut().unwrap().as_mut().poll(cx));
        this.writing = None;
        result.map_err(io::Error::other)?;
        Poll::Ready(Ok(this.write_len))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.eof {
            return Poll::Ready(Ok(()));
        }
        if this.closing.is_none() {
            this.closing = Some(this.request("close", Vec::new()));
        }
        let result = std::task::ready!(this.closing.as_mut().unwrap().as_mut().poll(cx));
        this.eof = true;
        this.closing = None;
        Poll::Ready(result.map(|_| ()).map_err(io::Error::other))
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        if let Some(remote) = self.network.remote.clone() {
            let id = self.id.clone();
            let _ = self.network.spawn(move |_| {
                let _ = remote.resource(
                    ResourceRequest::WebSocketIo {
                        id,
                        operation: "close".into(),
                    },
                    &[],
                );
            });
        }
    }
}
