//! Permissions and device capabilities subsystem.
pub mod device;
pub mod registry;

pub use device::{DeviceManager, DeviceTrack, DeviceWakeLock, MediaTrackKind};
pub use registry::{PermissionRegistry, RevocationResult};
