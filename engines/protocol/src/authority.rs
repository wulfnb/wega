//! Browser-owned native-request attribution. Renderer values never select an origin or partition.
use crate::{
    DocumentContext,
    identity::{ActivationId, SessionIdentity},
};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Tuple(String),
    /// Opaque/unknown origins cannot receive remembered or device permissions.
    Opaque,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Principal {
    pub origin: Origin,
    pub top_level_site: Option<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedRequest {
    pub session: SessionIdentity,
    pub document: DocumentContext,
    pub principal: Principal,
}

pub struct FrameAuthority {
    session: SessionIdentity,
    current: Option<(DocumentContext, Principal)>,
    activation: Option<(DocumentContext, ActivationId, Instant)>,
    next_activation: u64,
}

impl FrameAuthority {
    pub fn new(session: SessionIdentity) -> Self {
        Self {
            session,
            current: None,
            activation: None,
            next_activation: 0,
        }
    }
    /// Called only with a principal resolved by the host/service, never a page-provided origin.
    pub fn bind(&mut self, context: DocumentContext, principal: Principal) {
        if self
            .current
            .as_ref()
            .is_some_and(|(old, old_principal)| *old == context && *old_principal == principal)
        {
            return;
        }
        self.invalidate();
        self.current = Some((context, principal));
    }
    pub fn invalidate(&mut self) {
        self.current = None;
        self.activation = None;
    }
    pub fn clear_activation(&mut self) {
        self.activation = None;
    }
    pub fn resolve(&self, context: DocumentContext) -> Option<TrustedRequest> {
        let (current, principal) = self.current.as_ref()?;
        (*current == context).then(|| TrustedRequest {
            session: self.session,
            document: context,
            principal: principal.clone(),
        })
    }
    /// Only the trusted input adapter calls this. Evaluation and synthetic DOM events do not.
    pub fn activate(&mut self, context: DocumentContext, now: Instant) -> Option<ActivationId> {
        self.resolve(context)?;
        let next = self.next_activation.checked_add(1)?;
        self.next_activation = next;
        let id = ActivationId(next);
        self.activation = Some((context, id, now.checked_add(Duration::from_secs(5))?));
        Some(id)
    }
    pub fn consume_activation(
        &mut self,
        context: DocumentContext,
        id: ActivationId,
        now: Instant,
    ) -> bool {
        if self.has_activation(context, id, now) {
            self.activation = None;
            true
        } else {
            false
        }
    }
    pub fn has_activation(&self, context: DocumentContext, id: ActivationId, now: Instant) -> bool {
        self.resolve(context).is_some()
            && self
                .activation
                .is_some_and(|(owner, token, until)| owner == context && token == id && until > now)
    }
}

/// Conservative admission for host-attributed HTTPS permission requesters.
/// Device backends and persistent grants remain Step 21; this never invents an OS grant.
pub fn can_request_permission(request: &TrustedRequest) -> bool {
    matches!(&request.principal.origin, Origin::Tuple(origin) if origin.starts_with("https://"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DocumentId, DocumentRevision,
        identity::{FrameId, ProcessSessionId, TabId},
    };
    #[test]
    fn native_authority_is_document_bound_expiring_and_single_use() {
        let session = SessionIdentity {
            tab: TabId(1),
            frame: FrameId(1),
            process: ProcessSessionId([1; 16]),
            service: None,
        };
        let mut authority = FrameAuthority::new(session);
        let context = DocumentContext {
            document: DocumentId(4),
            revision: DocumentRevision(2),
        };
        let now = Instant::now();
        assert!(authority.activate(context, now).is_none());
        authority.bind(
            context,
            Principal {
                origin: Origin::Tuple("https://example.test".into()),
                top_level_site: Some(("https".into(), "example.test".into())),
            },
        );
        assert!(can_request_permission(&authority.resolve(context).unwrap()));
        let token = authority.activate(context, now).unwrap();
        assert!(!authority.consume_activation(context, ActivationId(token.0 + 1), now));
        assert!(authority.consume_activation(context, token, now));
        assert!(!authority.consume_activation(context, token, now));
        let token = authority.activate(context, now).unwrap();
        assert!(!authority.consume_activation(context, token, now + Duration::from_secs(5)));
        let token = authority.activate(context, now).unwrap();
        authority.bind(
            DocumentContext {
                revision: DocumentRevision(3),
                ..context
            },
            Principal {
                origin: Origin::Opaque,
                top_level_site: None,
            },
        );
        assert!(!authority.consume_activation(context, token, now));
        assert!(authority.resolve(context).is_none());
        assert!(!can_request_permission(
            &authority
                .resolve(DocumentContext {
                    revision: DocumentRevision(3),
                    ..context
                })
                .unwrap()
        ));
        authority.invalidate();
        assert!(authority.current.is_none());
        authority.next_activation = u64::MAX;
        authority.bind(
            context,
            Principal {
                origin: Origin::Opaque,
                top_level_site: None,
            },
        );
        assert!(authority.activate(context, now).is_none());
    }
}
