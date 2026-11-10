//! Structured clone, transferable ownership, and MessagePort entanglement.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PortId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortMessage {
    pub data: String,
    pub ports: Vec<PortId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OffscreenCanvasTransfer {
    pub canvas_id: u64,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrayBufferTransfer {
    pub byte_length: usize,
    pub bytes: Vec<u8>,
}

impl ArrayBufferTransfer {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            byte_length: bytes.len(),
            bytes,
        }
    }

    /// Detaches the source buffer and transfers its byte data.
    /// The source buffer is left with 0 length and empty allocation.
    pub fn detach(source: &mut Vec<u8>) -> Self {
        let detached = std::mem::take(source);
        Self {
            byte_length: detached.len(),
            bytes: detached,
        }
    }
}

struct PortState {
    entangled: Option<PortId>,
    queue: VecDeque<PortMessage>,
    closed: bool,
    started: bool,
}

#[derive(Default)]
pub struct MessagePortHub {
    ports: HashMap<PortId, PortState>,
    next_id: u64,
}

impl MessagePortHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an entangled pair of ports (port1, port2).
    pub fn create_channel(&mut self) -> (PortId, PortId) {
        let id1 = PortId(self.next_id + 1);
        let id2 = PortId(self.next_id + 2);
        self.next_id += 2;

        self.ports.insert(
            id1,
            PortState {
                entangled: Some(id2),
                queue: VecDeque::new(),
                closed: false,
                started: false,
            },
        );
        self.ports.insert(
            id2,
            PortState {
                entangled: Some(id1),
                queue: VecDeque::new(),
                closed: false,
                started: false,
            },
        );

        (id1, id2)
    }

    /// Post a message to a port's entangled counterpart.
    pub fn post_message(
        &mut self,
        source: PortId,
        data: String,
        transferred_ports: Vec<PortId>,
    ) -> Result<(), String> {
        let state = self.ports.get(&source).ok_or("Invalid port ID")?;
        if state.closed {
            return Ok(());
        }
        let Some(target_id) = state.entangled else {
            return Ok(());
        };
        let target_state = self.ports.get_mut(&target_id).ok_or("Target port closed")?;
        if target_state.closed {
            return Ok(());
        }

        target_state.queue.push_back(PortMessage {
            data,
            ports: transferred_ports,
        });
        Ok(())
    }

    /// Start dispatching messages on this port.
    pub fn start(&mut self, port: PortId) {
        if let Some(state) = self.ports.get_mut(&port) {
            state.started = true;
        }
    }

    /// Close a port, detangling it and discarding its message queue.
    pub fn close(&mut self, port: PortId) {
        if let Some(state) = self.ports.get_mut(&port) {
            state.closed = true;
            state.queue.clear();
            if let Some(other_id) = state.entangled.take()
                && let Some(other_state) = self.ports.get_mut(&other_id)
            {
                other_state.entangled = None;
            }
        }
    }

    /// Poll pending messages delivered to this port.
    pub fn poll_messages(&mut self, port: PortId) -> Vec<PortMessage> {
        if let Some(state) = self.ports.get_mut(&port)
            && !state.closed
        {
            return state.queue.drain(..).collect();
        }
        Vec::new()
    }
}

pub type SharedPortHub = Arc<Mutex<MessagePortHub>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_channel_entanglement_and_closure() {
        let mut hub = MessagePortHub::new();
        let (port1, port2) = hub.create_channel();

        // 1. Post from port1 to port2
        hub.post_message(port1, "hello port2".into(), vec![])
            .unwrap();
        let messages = hub.poll_messages(port2);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].data, "hello port2");

        // 2. Post from port2 to port1
        hub.post_message(port2, "reply from port2".into(), vec![])
            .unwrap();
        let replies = hub.poll_messages(port1);
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].data, "reply from port2");

        // 3. Close port1: subsequent posts fail or are dropped
        hub.close(port1);
        assert!(hub.post_message(port2, "dropped".into(), vec![]).is_ok());
        assert!(hub.poll_messages(port1).is_empty());
    }

    #[test]
    fn transferable_array_buffer_detachment() {
        let mut buffer = vec![1u8, 2, 3, 4, 5];
        assert_eq!(buffer.len(), 5);

        let transferred = ArrayBufferTransfer::detach(&mut buffer);
        assert_eq!(buffer.len(), 0, "Source buffer must be detached");
        assert_eq!(transferred.byte_length, 5);
        assert_eq!(transferred.bytes, vec![1, 2, 3, 4, 5]);
    }
}
