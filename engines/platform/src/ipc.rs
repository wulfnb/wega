//! Preconnected duplex channels. Connection discovery is never content authority.
#[cfg(unix)]
pub type Channel = std::os::unix::net::UnixStream;
#[cfg(windows)]
#[path = "ipc_windows.rs"]
mod windows;
#[cfg(windows)]
pub use windows::Channel;
