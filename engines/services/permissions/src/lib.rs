//! Permissions and device capabilities subsystem, extracted from `wega-engine`.
//! `PermissionRegistry::query`/`request` take `wega_network::Policy` so the host's
//! blocked-host policy keeps applying; no new policy abstraction is introduced.
#![forbid(unsafe_code)]

pub mod device;
pub mod registry;

pub use device::{DeviceManager, DeviceTrack, DeviceWakeLock, MediaTrackKind};
pub use registry::{PermissionRegistry, RevocationResult};
