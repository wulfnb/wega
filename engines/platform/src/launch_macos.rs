use std::{io, os::unix::process::CommandExt, process::Command};
pub(super) fn prepare(command: &mut Command) {
    // SAFETY: after fork this callback uses only scalar fcntl/getrlimit syscalls.
    // CLOEXEC preserves Rust's exec-error pipe; explicit grants are restored by
    // later callbacks. Sweep the actual child limit, including non-Rust handles.
    unsafe {
        command.pre_exec(|| {
            let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
            if libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) != 0 {
                return Err(io::Error::last_os_error());
            }
            let limit = limit.assume_init().rlim_cur;
            if limit > 1_048_576 {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Descriptor limit exceeds launch sweep budget",
                ));
            }
            for descriptor in 3..limit as i32 {
                let flags = libc::fcntl(descriptor, libc::F_GETFD);
                if flags < 0 {
                    if io::Error::last_os_error().raw_os_error() == Some(libc::EBADF) {
                        continue;
                    }
                    return Err(io::Error::last_os_error());
                }
                if libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
}
