//! Permission registry with origin-scoped grants, prompt coordination and revocation.
use super::device::{DeviceManager, MediaTrackKind};
use std::collections::HashMap;
use wega_protocol::{
    DocumentContext,
    authority::Origin,
    permissions::{PermissionFeature, PermissionState},
};

#[derive(Clone, Debug, Default)]
pub struct RevocationResult {
    pub stopped_tracks: Vec<String>,
    pub released_locks: Vec<u64>,
}

#[derive(Default, Debug)]
pub struct PermissionRegistry {
    grants: HashMap<(String, PermissionFeature), PermissionState>,
    pending_prompts: HashMap<u64, (DocumentContext, String, PermissionFeature)>,
    pub devices: DeviceManager,
    next_prompt_id: u64,
}

impl PermissionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Query current permission state for an origin and feature.
    /// Opaque origins always return Denied.
    /// If blocked by policy or explicitly set to Denied, returns Denied.
    /// If granted, returns Granted.
    /// Otherwise returns Prompt.
    pub fn query(
        &self,
        origin: &Origin,
        feature: PermissionFeature,
        policy: &crate::network::NetworkPolicy,
    ) -> PermissionState {
        let origin_str = match origin {
            Origin::Tuple(s) => s,
            Origin::Opaque => return PermissionState::Denied,
        };

        // Check if origin host is blocked in policy
        if let Ok(url) = url::Url::parse(origin_str) {
            if let Some(host) = url.host_str() {
                let matches_blocked = policy.blocked_hosts.split_whitespace().any(|rule| {
                    let rule = rule
                        .trim_start_matches("*.")
                        .trim_end_matches('.')
                        .to_ascii_lowercase();
                    !rule.is_empty() && (host == rule || host.ends_with(&format!(".{rule}")))
                });
                if matches_blocked {
                    return PermissionState::Denied;
                }
            }
        }

        if let Some(state) = self.grants.get(&(origin_str.clone(), feature)) {
            return *state;
        }

        PermissionState::Prompt
    }

    /// Request permission for a document.
    /// Returns (immediate_state, optional_prompt_id_if_queued).
    pub fn request(
        &mut self,
        origin: &Origin,
        document: DocumentContext,
        feature: PermissionFeature,
        policy: &crate::network::NetworkPolicy,
    ) -> (PermissionState, Option<u64>) {
        let state = self.query(origin, feature, policy);
        match state {
            PermissionState::Granted | PermissionState::Denied => (state, None),
            PermissionState::Prompt => {
                let origin_str = match origin {
                    Origin::Tuple(s) => s.clone(),
                    Origin::Opaque => return (PermissionState::Denied, None),
                };
                self.next_prompt_id += 1;
                let id = self.next_prompt_id;
                self.pending_prompts
                    .insert(id, (document, origin_str, feature));
                (PermissionState::Prompt, Some(id))
            }
        }
    }

    /// Answers a pending prompt.
    /// Updates the grant for the prompt's origin and returns (origin, feature, allow) if matched.
    pub fn answer_prompt(
        &mut self,
        id: u64,
        allow: bool,
    ) -> Option<(String, PermissionFeature, bool)> {
        let (_doc, origin_str, feature) = self.pending_prompts.remove(&id)?;
        let state = if allow {
            PermissionState::Granted
        } else {
            PermissionState::Denied
        };
        self.grants.insert((origin_str.clone(), feature), state);
        Some((origin_str, feature, allow))
    }

    /// Sets grant directly (e.g. from policy or configuration).
    pub fn set_grant(
        &mut self,
        origin: String,
        feature: PermissionFeature,
        state: PermissionState,
    ) {
        self.grants.insert((origin, feature), state);
    }

    /// Revokes permission for origin and feature.
    /// Invalidates grant and immediately shuts down all active resources for origin/feature.
    pub fn revoke(&mut self, origin: &str, feature: PermissionFeature) -> RevocationResult {
        self.grants.remove(&(origin.to_string(), feature));

        let mut stopped_tracks = Vec::new();
        let mut released_locks = Vec::new();

        match feature {
            PermissionFeature::Camera => {
                stopped_tracks = self
                    .devices
                    .stop_tracks_for_origin_and_kind(origin, MediaTrackKind::Video);
            }
            PermissionFeature::Microphone => {
                stopped_tracks = self
                    .devices
                    .stop_tracks_for_origin_and_kind(origin, MediaTrackKind::Audio);
            }
            PermissionFeature::ScreenWakeLock => {
                released_locks = self.devices.release_wake_locks_for_origin(origin);
            }
            _ => {}
        }

        RevocationResult {
            stopped_tracks,
            released_locks,
        }
    }

    /// Cancels all pending prompts for a document context (called upon navigation/detach).
    pub fn cancel_document(&mut self, document: DocumentContext) -> (Vec<String>, Vec<u64>) {
        self.pending_prompts
            .retain(|_, (doc, _, _)| *doc != document);
        self.devices.cleanup_document(document)
    }

    /// Clear all permissions and device leases (e.g. on browsing data clear).
    pub fn clear(&mut self) {
        self.grants.clear();
        self.pending_prompts.clear();
    }
}
