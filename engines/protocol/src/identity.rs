//! Host-issued identifiers. These are routing/authority keys, not bearer secrets.
use crate::{Command, ErrorCode, Event, ProtocolError};
use serde::{Deserialize, Serialize};

macro_rules! numeric_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u64);
    };
}
macro_rules! session_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub [u8; 16]);
    };
}
numeric_id!(TabId);
numeric_id!(FrameId);
numeric_id!(ActivationId);
session_id!(ProcessSessionId);
session_id!(ServiceSessionId);
session_id!(PartitionId);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceIdentity {
    pub service: ServiceSessionId,
    pub partition: PartitionId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionIdentity {
    pub tab: TabId,
    pub frame: FrameId,
    pub process: ProcessSessionId,
    pub service: Option<ServiceIdentity>,
}

impl SessionIdentity {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.tab.0 == 0
            || self.frame.0 == 0
            || self.process.0 == [0; 16]
            || self.service.is_some_and(|service| {
                service.service.0 == [0; 16] || service.partition.0 == [0; 16]
            })
        {
            return Err(ProtocolError::new(
                ErrorCode::InvalidRequest,
                "Invalid session identity",
            ));
        }
        Ok(())
    }
    pub fn command(&self, command: Command) -> Command {
        Command::Session {
            identity: *self,
            command: Box::new(command),
        }
    }
    pub fn event(&self, event: Event) -> Event {
        Event::Session {
            identity: *self,
            event: Box::new(event),
        }
    }
    pub fn receive_command(&self, packet: Command) -> Result<Command, ProtocolError> {
        match packet {
            Command::Session { identity, command }
                if identity == *self
                    && !matches!(*command, Command::Session { .. } | Command::Init { .. }) =>
            {
                Ok(*command)
            }
            _ => Err(ProtocolError::new(
                ErrorCode::InvalidRequest,
                "Command session/frame/partition mismatch",
            )),
        }
    }
    pub fn receive_event(&self, packet: Event) -> Result<Event, ProtocolError> {
        match packet {
            Event::Session { identity, event }
                if identity == *self && !matches!(*event, Event::Session { .. }) =>
            {
                Ok(*event)
            }
            _ => Err(ProtocolError::new(
                ErrorCode::InvalidRequest,
                "Event session/frame/partition mismatch",
            )),
        }
    }
}

/// Serialize without cloning potentially large frame or evaluation payloads.
pub fn write_event<W: std::io::Write>(
    output: &mut W,
    event: &Event,
    version: u32,
    identity: Option<SessionIdentity>,
) -> std::io::Result<()> {
    #[derive(Serialize)]
    enum Packet<'a> {
        Session {
            identity: SessionIdentity,
            event: &'a Event,
        },
    }
    if version < 11 {
        return crate::write(output, event);
    }
    let identity = identity.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "Missing worker session")
    })?;
    crate::write(output, &Packet::Session { identity, event })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> SessionIdentity {
        SessionIdentity {
            tab: TabId(1),
            frame: FrameId(1),
            process: ProcessSessionId([1; 16]),
            service: Some(ServiceIdentity {
                service: ServiceSessionId([2; 16]),
                partition: PartitionId([3; 16]),
            }),
        }
    }
    #[test]
    fn every_routing_dimension_is_checked_before_unwrapping() {
        let expected = session();
        expected.validate().unwrap();
        let mut wrong = vec![expected; 5];
        wrong[0].tab = TabId(2);
        wrong[1].frame = FrameId(2);
        wrong[2].process = ProcessSessionId([9; 16]);
        wrong[3].service.as_mut().unwrap().service = ServiceSessionId([9; 16]);
        wrong[4].service.as_mut().unwrap().partition = PartitionId([9; 16]);
        for identity in wrong {
            assert!(
                expected
                    .receive_command(
                        identity.command(Command::Evaluate("globalThis.mutated=true".into()))
                    )
                    .is_err()
            );
            assert!(
                expected
                    .receive_event(identity.event(Event::ShutdownComplete))
                    .is_err()
            );
        }
        assert!(expected.receive_command(Command::Shutdown).is_err());
        assert!(
            expected
                .receive_command(expected.command(expected.command(Command::Shutdown)))
                .is_err()
        );
        assert!(
            expected
                .receive_event(expected.event(expected.event(Event::ShutdownComplete)))
                .is_err()
        );
        assert!(matches!(
            expected.receive_command(expected.command(Command::Shutdown)),
            Ok(Command::Shutdown)
        ));
    }
    #[test]
    fn envelope_serialization_round_trips_and_older_versions_stay_unwrapped() {
        let identity = session();
        let mut bytes = Vec::new();
        write_event(
            &mut bytes,
            &Event::Ready { version: 11 },
            11,
            Some(identity),
        )
        .unwrap();
        let event = crate::read::<Event>(&mut bytes.as_slice())
            .unwrap()
            .unwrap();
        assert!(matches!(
            identity.receive_event(event),
            Ok(Event::Ready { version: 11 })
        ));
        assert!(write_event(&mut Vec::new(), &Event::ShutdownComplete, 11, None).is_err());
        bytes.clear();
        write_event(&mut bytes, &Event::Ready { version: 10 }, 10, None).unwrap();
        assert!(matches!(
            crate::read::<Event>(&mut bytes.as_slice()).unwrap(),
            Some(Event::Ready { version: 10 })
        ));
        assert!(!crate::capabilities(10).contains(&crate::Capability::SessionIdentity));
        assert!(crate::capabilities(11).contains(&crate::Capability::SessionIdentity));
        let mut invalid = identity;
        invalid.process = ProcessSessionId([0; 16]);
        assert!(invalid.validate().is_err());
    }
}
