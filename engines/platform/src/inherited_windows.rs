//! Duplicate host-selected HANDLE grants without adopting arbitrary caller ownership.
use std::{
    io,
    os::windows::io::{AsRawHandle, OwnedHandle},
};
use windows_sys::Win32::{
    Foundation::*,
    System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
};
pub const MAX_CHANNELS: usize = 4;
fn duplicate(descriptor: u64) -> io::Result<OwnedHandle> {
    let value = usize::try_from(descriptor)
        .ok()
        .filter(|value| *value > 0 && *value <= isize::MAX as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid inherited handle"))?;
    let raw = value as HANDLE;
    for standard in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        if raw == unsafe { GetStdHandle(standard) } {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Standard stream is not a service grant",
            ));
        }
    }
    let owned = crate::windows_spawn::duplicate(raw, false)?;
    if unsafe { SetHandleInformation(raw, HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(owned)
}
pub fn duplicate_channels(descriptors: &[u64]) -> io::Result<Vec<crate::ipc::Channel>> {
    if descriptors.is_empty() || descriptors.len() > MAX_CHANNELS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid channel grant count",
        ));
    }
    let mut channels = Vec::new();
    for (index, descriptor) in descriptors.iter().enumerate() {
        if descriptors[..index].contains(descriptor) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Repeated channel handle",
            ));
        }
        channels.push(crate::ipc::Channel::from_grant(duplicate(*descriptor)?)?);
    }
    Ok(channels)
}
pub fn duplicate_file(descriptor: u64) -> io::Result<std::fs::File> {
    let file = std::fs::File::from(duplicate(descriptor)?);
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Frame handle is not a regular file",
        ));
    }
    Ok(file)
}
pub(crate) fn preserve(handles: &mut Vec<OwnedHandle>, raw: &impl AsRawHandle) -> io::Result<u64> {
    let owned = crate::windows_spawn::duplicate(raw.as_raw_handle().cast(), true)?;
    let value = owned.as_raw_handle() as usize as u64;
    handles.push(owned);
    Ok(value)
}
