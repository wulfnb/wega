//! Safe ownership wrappers for host-selected, preconnected Unix service grants.
//! All raw-descriptor operations stay outside the DOM/VM crate.
use std::{
    io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{net::UnixStream, process::CommandExt},
    },
    process::Command,
    sync::Arc,
};
pub const MAX_CHANNELS: usize = 4;

pub(crate) fn preserve(command: &mut Command, channels: &[UnixStream]) -> io::Result<Vec<u64>> {
    if channels.is_empty() || channels.len() > MAX_CHANNELS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid inherited service grants",
        ));
    }
    let owned: Vec<OwnedFd> = channels
        .iter()
        .map(|channel| clone_descriptor(channel.as_raw_fd()))
        .collect::<io::Result<_>>()?;
    preserve_owned(command, owned)
}
fn clone_descriptor(raw: i32) -> io::Result<OwnedFd> {
    // SAFETY: fcntl duplicates, without adopting the caller's descriptor. Keep
    // grants above stdio even when the launching GUI has closed standard streams.
    let duplicate = unsafe { libc::fcntl(raw, libc::F_DUPFD_CLOEXEC, 3) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(duplicate) })
}
pub(crate) fn preserve_file(command: &mut Command, file: &std::fs::File) -> io::Result<u64> {
    Ok(preserve_owned(command, vec![clone_descriptor(file.as_raw_fd())?])?[0])
}
fn preserve_owned(command: &mut Command, owned: Vec<OwnedFd>) -> io::Result<Vec<u64>> {
    let descriptors = owned
        .iter()
        .map(|descriptor| descriptor.as_raw_fd() as u64)
        .collect();
    let owned = Arc::new(owned);
    // SAFETY: descriptors are owned by this command and stay open until after
    // spawn. The post-fork closure only calls async-signal-safe fcntl operations.
    // Registration follows Linux's close_range(CLOEXEC), so only these grants
    // regain inheritance; Rust's exec-error pipe remains intact until exec.
    unsafe {
        command.pre_exec(move || {
            for descriptor in owned.iter() {
                let raw = descriptor.as_raw_fd();
                let flags = libc::fcntl(raw, libc::F_GETFD);
                if flags < 0 || libc::fcntl(raw, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    Ok(descriptors)
}
/// Duplicate an inherited file grant without closing/adopting another Rust owner's FD.
pub fn duplicate_file(descriptor: u64) -> io::Result<std::fs::File> {
    let raw = i32::try_from(descriptor)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid file descriptor"))?;
    if raw < 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Standard streams are not frame grants",
        ));
    }
    let file = std::fs::File::from(clone_descriptor(raw)?);
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Frame grant is not a regular file",
        ));
    }
    // SAFETY: descriptor flags change without taking ownership of the original.
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
    if flags < 0 || unsafe { libc::fcntl(raw, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

/// Duplicate rather than assume Rust ownership of a numeric descriptor. Invalid,
/// duplicate, non-socket and unconnected/non-Unix grants fail before use. Original
/// inherited descriptors remain scoped to process exit and regain CLOEXEC.
pub fn duplicate_channels(descriptors: &[u64]) -> io::Result<Vec<UnixStream>> {
    if descriptors.is_empty() || descriptors.len() > MAX_CHANNELS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid inherited channel count",
        ));
    }
    let mut channels = Vec::new();
    let minimum = descriptors
        .iter()
        .copied()
        .max()
        .and_then(|maximum| maximum.checked_add(1))
        .and_then(|minimum| i32::try_from(minimum).ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid inherited descriptor range",
            )
        })?;
    for (index, descriptor) in descriptors.iter().enumerate() {
        let raw = i32::try_from(*descriptor).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "Invalid inherited descriptor")
        })?;
        if raw < 3 || descriptors[..index].contains(descriptor) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid or repeated inherited descriptor",
            ));
        }
        // SAFETY: fcntl accepts a numeric descriptor and fails safely for EBADF.
        // It creates a fresh CLOEXEC descriptor, whose ownership is transferred
        // exactly once to OwnedFd. No caller-owned descriptor is adopted/closed.
        let duplicate = unsafe { libc::fcntl(raw, libc::F_DUPFD_CLOEXEC, minimum) };
        if duplicate < 0 {
            return Err(io::Error::last_os_error());
        }
        let owned = unsafe { OwnedFd::from_raw_fd(duplicate) };
        let mut kind = 0_i32;
        let mut kind_len = std::mem::size_of::<i32>() as libc::socklen_t;
        let mut peer = std::mem::MaybeUninit::<libc::sockaddr_storage>::zeroed();
        let mut peer_len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        // SAFETY: owned retains the descriptor; all outputs have their exact
        // capacities and are read only after the kernel reports success.
        let valid = unsafe {
            libc::getsockopt(
                duplicate,
                libc::SOL_SOCKET,
                libc::SO_TYPE,
                (&mut kind as *mut i32).cast(),
                &mut kind_len,
            ) == 0
                && kind == libc::SOCK_STREAM
                && libc::getpeername(duplicate, peer.as_mut_ptr().cast(), &mut peer_len) == 0
        };
        if !valid {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Inherited grant is not a connected stream",
            ));
        }
        if unsafe { peer.assume_init() }.ss_family as i32 != libc::AF_UNIX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Inherited grant is not a Unix channel",
            ));
        }
        // SAFETY: setting CLOEXEC changes descriptor flags without taking ownership.
        let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
        if flags < 0 || unsafe { libc::fcntl(raw, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
        channels.push(UnixStream::from(owned));
    }
    Ok(channels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    #[test]
    fn frame_grants_duplicate_regular_files_without_adopting_the_original() {
        let mut original = tempfile::tempfile().unwrap();
        original.write_all(b"frame").unwrap();
        let grant = duplicate_file(original.as_raw_fd() as u64).unwrap();
        assert_eq!(grant.metadata().unwrap().len(), 5);
        drop(grant);
        assert!(original.metadata().is_ok());
        let (_, socket) = UnixStream::pair().unwrap();
        assert!(duplicate_file(socket.as_raw_fd() as u64).is_err());
        assert!(duplicate_file(0).is_err());
        assert!(duplicate_file(u64::MAX).is_err());
    }
    #[test]
    fn only_connected_unix_channels_can_be_duplicated_and_original_ownership_survives() {
        let (mut parent, child) = UnixStream::pair().unwrap();
        let raw = child.as_raw_fd() as u64;
        let mut copy = duplicate_channels(&[raw]).unwrap().pop().unwrap();
        parent.write_all(b"x").unwrap();
        let mut byte = [0];
        copy.read_exact(&mut byte).unwrap();
        assert_eq!(&byte, b"x");
        drop(copy);
        assert!(child.peer_addr().is_ok());
        assert!(duplicate_channels(&[raw, raw]).is_err());
        let file = tempfile::tempfile().unwrap();
        assert!(duplicate_channels(&[file.as_raw_fd() as u64]).is_err());
        assert!(duplicate_channels(&[0]).is_err());
        assert!(duplicate_channels(&[u64::MAX]).is_err());
    }
}
