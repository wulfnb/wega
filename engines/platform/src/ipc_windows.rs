use std::{
    io::{self, Read, Write},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{IO::*, Pipes::*},
};
struct Inner {
    handle: OwnedHandle,
    closed: AtomicBool,
    read_timeout: std::sync::atomic::AtomicU64,
    write_timeout: std::sync::atomic::AtomicU64,
}
#[derive(Clone)]
pub struct Channel {
    inner: Arc<Inner>,
}
impl Channel {
    fn owned(handle: OwnedHandle) -> Self {
        Self {
            inner: Arc::new(Inner {
                handle,
                closed: AtomicBool::new(false),
                read_timeout: std::sync::atomic::AtomicU64::new(u64::MAX),
                write_timeout: std::sync::atomic::AtomicU64::new(u64::MAX),
            }),
        }
    }
    pub fn pair() -> io::Result<(Self, Self)> {
        let mut nonce = [0; 32];
        getrandom::fill(&mut nonce).map_err(|error| io::Error::other(error.to_string()))?;
        let name = format!(
            r"\\.\pipe\wega-{}",
            nonce
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
        // No inherited handles until the launcher's explicit handle list. Default
        // pipe security grants the owner/system; remote clients are rejected.
        let server = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                65536,
                65536,
                5000,
                std::ptr::null(),
            )
        };
        if server == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let server = unsafe { OwnedHandle::from_raw_handle(server.cast()) };
        let client = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            )
        };
        if client == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let client = unsafe { OwnedHandle::from_raw_handle(client.cast()) };
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        let result = unsafe { ConnectNamedPipe(server.as_raw_handle().cast(), &mut overlapped) };
        if result == 0 && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_IO_PENDING as i32) {
                let mut count = 0;
                unsafe {
                    CancelIoEx(server.as_raw_handle().cast(), &overlapped);
                    GetOverlappedResult(server.as_raw_handle().cast(), &overlapped, &mut count, 1);
                }
            }
            return Err(error);
        }
        Ok((Self::owned(server), Self::owned(client)))
    }
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(self.clone())
    }
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.inner.read_timeout.store(
            timeout
                .map(|time| time.as_millis().min((u64::MAX - 1) as u128) as u64)
                .unwrap_or(u64::MAX),
            Ordering::Release,
        );
        Ok(())
    }
    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.inner.write_timeout.store(
            timeout
                .map(|time| time.as_millis().min((u64::MAX - 1) as u128) as u64)
                .unwrap_or(u64::MAX),
            Ordering::Release,
        );
        Ok(())
    }
    pub fn shutdown(&self, _: std::net::Shutdown) -> io::Result<()> {
        self.inner.closed.store(true, Ordering::Release);
        unsafe {
            let raw = self.as_raw_handle().cast();
            CancelIoEx(raw, std::ptr::null());
            let mut flags = 0;
            if GetNamedPipeInfo(
                raw,
                &mut flags,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) != 0
                && flags & PIPE_SERVER_END != 0
            {
                DisconnectNamedPipe(raw);
            }
        }
        Ok(())
    }
    pub(crate) fn from_grant(handle: OwnedHandle) -> io::Result<Self> {
        let raw = handle.as_raw_handle().cast();
        if unsafe { GetFileType(raw) } != FILE_TYPE_PIPE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Broker grant is not a pipe",
            ));
        }
        Ok(Self::owned(handle))
    }
    fn transfer(&self, bytes: *mut u8, length: usize, write: bool) -> io::Result<usize> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let event = unsafe {
            windows_sys::Win32::System::Threading::CreateEventW(
                std::ptr::null(),
                1,
                0,
                std::ptr::null(),
            )
        };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        let event = unsafe { OwnedHandle::from_raw_handle(event.cast()) };
        let mut operation: OVERLAPPED = unsafe { std::mem::zeroed() };
        operation.hEvent = event.as_raw_handle().cast();
        let mut transferred = 0;
        let length = u32::try_from(length.min(u32::MAX as usize)).unwrap();
        let raw = self.as_raw_handle().cast();
        // Every async operation keeps its buffer, OVERLAPPED and event live until
        // completion, including cancellation/timeouts. No stack pointer escapes.
        let started = unsafe {
            if write {
                WriteFile(raw, bytes, length, &mut transferred, &mut operation)
            } else {
                ReadFile(raw, bytes, length, &mut transferred, &mut operation)
            }
        };
        if started == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_BROKEN_PIPE || error == ERROR_PIPE_NOT_CONNECTED {
                return if write {
                    Err(io::ErrorKind::BrokenPipe.into())
                } else {
                    Ok(0)
                };
            }
            if error != ERROR_IO_PENDING {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
            if self.inner.closed.load(Ordering::Acquire) {
                unsafe {
                    CancelIoEx(raw, &operation);
                    GetOverlappedResult(raw, &operation, &mut transferred, 1);
                }
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            let timeout = if write {
                self.inner.write_timeout.load(Ordering::Acquire)
            } else {
                self.inner.read_timeout.load(Ordering::Acquire)
            };
            let milliseconds = if timeout == u64::MAX {
                u32::MAX
            } else {
                timeout.min((u32::MAX - 1) as u64) as u32
            };
            let waited = unsafe {
                windows_sys::Win32::System::Threading::WaitForSingleObject(
                    event.as_raw_handle().cast(),
                    milliseconds,
                )
            };
            if waited != WAIT_OBJECT_0 {
                unsafe {
                    CancelIoEx(raw, &operation);
                    GetOverlappedResult(raw, &operation, &mut transferred, 1);
                }
                return Err(if waited == WAIT_TIMEOUT {
                    io::ErrorKind::TimedOut.into()
                } else {
                    io::Error::last_os_error()
                });
            }
            if unsafe { GetOverlappedResult(raw, &operation, &mut transferred, 0) } == 0 {
                let error = io::Error::last_os_error();
                return if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) && !write {
                    Ok(0)
                } else {
                    Err(error)
                };
            }
        }
        Ok(transferred as usize)
    }
}
impl AsRawHandle for Channel {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        self.inner.handle.as_raw_handle()
    }
}
impl Read for Channel {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.transfer(bytes.as_mut_ptr(), bytes.len(), false)
    }
}
impl Write for Channel {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.transfer(bytes.as_ptr().cast_mut(), bytes.len(), true)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
