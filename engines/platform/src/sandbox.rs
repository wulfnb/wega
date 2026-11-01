//! Required native policy entry. Call on the initial thread before any VM,
//! decoder, runtime or worker thread exists. Failure is terminal for that worker.
use std::io;
#[cfg(target_os = "linux")]
#[path = "sandbox_linux.rs"]
mod linux;
#[cfg(target_os = "macos")]
#[path = "sandbox_macos.rs"]
mod macos;
pub const REQUIRED_ARGUMENT: &str = "--content-sandbox";
pub fn enter() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux::enter()
    }
    #[cfg(target_os = "macos")]
    {
        macos::verify()
    }
    #[cfg(windows)]
    {
        crate::windows_spawn::verify()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Native content sandbox is unavailable on this platform",
        ))
    }
}

/// Apply the descriptor ceiling only after numeric grants were duplicated. High
/// inherited FD numbers remain valid; new allocations use the remaining low slots.
pub fn finish_grants() -> io::Result<()> {
    #[cfg(target_os = "macos")]
    macos::finish_grants()?;
    #[cfg(unix)]
    {
        let mut current = std::mem::MaybeUninit::<libc::rlimit>::uninit();
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, current.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let current = unsafe { current.assume_init() };
        let limit = libc::rlimit {
            rlim_cur: current.rlim_cur.min(128),
            rlim_max: current.rlim_max.min(128),
        };
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
