//! Permissions and device capabilities now live in `wega-permissions`; this module
//! re-exports the previous public paths for compatibility.
pub use wega_permissions::{
    DeviceManager, DeviceTrack, DeviceWakeLock, MediaTrackKind, PermissionRegistry,
    RevocationResult, device, registry,
};
