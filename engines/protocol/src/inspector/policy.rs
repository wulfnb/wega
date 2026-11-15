//! Who may be granted JavaScript evaluation, decided from the *committed security principal*
//! (`authority::Principal`), never from a URL.
//!
//! The browser decides ([`authorise`]) from the principal its `FrameAuthority` bound at commit
//! (`wega_network::principal`: a tuple origin only for a response the broker observed over
//! http/https, `Opaque` for everything else, including `file:`, `data:`, `about:` and any
//! document the broker could not corroborate). The content process cannot authenticate that
//! principal, so it only checks [consistency](verify_scope) of the browser's claim with the
//! origin of its own document: a URL comparison is a sanity check, not proof that the claim
//! came from the browser (that proof is the identity-wrapped pipe only the browser writes to).
use super::{InspectorError, InspectorErrorCode};
use crate::authority::{Origin, Principal};

/// What a grant covers: the browser-asserted tuple origin, or `None` for an opaque document
/// together with the user's explicit override.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantScope {
    pub origin: Option<String>,
    pub opaque_override: bool,
}

fn denied(message: &str) -> InspectorError {
    InspectorError::new(InspectorErrorCode::PolicyDenied, message)
}

/// Browser side. `principal` is `None` when the document has no established principal (not
/// committed, replaced, or unbound).
pub fn authorise(
    principal: Option<&Principal>,
    opaque_override: bool,
) -> Result<GrantScope, InspectorError> {
    match principal.map(|principal| &principal.origin) {
        None => Err(denied(
            "The document's security principal is not established",
        )),
        // A tuple origin needs no override; a stray override flag is dropped, never stored.
        Some(Origin::Tuple(origin)) => Ok(GrantScope {
            origin: Some(origin.clone()),
            opaque_override: false,
        }),
        Some(Origin::Opaque) if opaque_override => Ok(GrantScope {
            origin: None,
            opaque_override: true,
        }),
        Some(Origin::Opaque) => Err(denied(
            "This document has an opaque or internal origin; evaluation needs your explicit override",
        )),
    }
}

/// Content-process side: the claim must be consistent with what the engine itself knows.
/// `engine_origin` is the tuple origin of the engine's document, `None` when it has none.
pub fn verify_scope(
    origin: Option<&str>,
    opaque_override: bool,
    engine_origin: Option<&str>,
) -> Result<GrantScope, InspectorError> {
    match (origin, opaque_override) {
        (Some(claimed), _) => {
            if !well_formed_origin(claimed) {
                return Err(InspectorError::new(
                    InspectorErrorCode::InvalidPayload,
                    "Malformed origin in the grant",
                ));
            }
            if engine_origin == Some(claimed) {
                Ok(GrantScope {
                    origin: Some(claimed.to_string()),
                    opaque_override: false,
                })
            } else {
                Err(denied("The grant's origin does not match this document"))
            }
        }
        // The browser's principal is stricter than the engine's URL (e.g. an https document the
        // broker could not corroborate): the user's override is honoured.
        (None, true) => Ok(GrantScope {
            origin: None,
            opaque_override: true,
        }),
        (None, false) => Err(denied(
            "Evaluation is not permitted for an opaque or internal document",
        )),
    }
}

/// `scheme://host[:port]`, bounded; the shape `Url::origin().ascii_serialization()` produces.
pub fn well_formed_origin(origin: &str) -> bool {
    origin.len() <= 2048
        && origin.split_once("://").is_some_and(|(scheme, rest)| {
            !scheme.is_empty()
                && scheme
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.')
                && !rest.is_empty()
                && !rest.contains(['/', '?', '#', ' ', '\n', '\r', '\0'])
        })
}
