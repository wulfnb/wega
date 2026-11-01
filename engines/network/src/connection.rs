//! Common framing over legacy TCP and preconnected, session-owned local channels.
use std::{
    io::{self, Read, Write},
    net::TcpStream,
};
pub(crate) enum Connection {
    Tcp(TcpStream),
    #[cfg(any(unix, windows))]
    Inherited(wega_platform::ipc::Channel),
}
impl Connection {
    pub fn try_clone(&self) -> io::Result<Self> {
        match self {
            Self::Tcp(stream) => stream.try_clone().map(Self::Tcp),
            #[cfg(any(unix, windows))]
            Self::Inherited(stream) => stream.try_clone().map(Self::Inherited),
        }
    }
}
impl Read for Connection {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(bytes),
            #[cfg(any(unix, windows))]
            Self::Inherited(stream) => stream.read(bytes),
        }
    }
}
impl Write for Connection {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(bytes),
            #[cfg(any(unix, windows))]
            Self::Inherited(stream) => stream.write(bytes),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            #[cfg(any(unix, windows))]
            Self::Inherited(stream) => stream.flush(),
        }
    }
}

#[cfg(any(unix, windows))]
pub(crate) struct Pool {
    channels: Vec<std::sync::Mutex<Option<Connection>>>,
}
#[cfg(any(unix, windows))]
impl Pool {
    pub fn new(channels: Vec<wega_platform::ipc::Channel>) -> Result<Self, String> {
        if channels.is_empty() || channels.len() > 4 {
            return Err("Invalid inherited broker channel count".into());
        }
        let mut slots = Vec::new();
        for channel in channels {
            channel
                .set_read_timeout(Some(std::time::Duration::from_secs(20)))
                .map_err(|error| error.to_string())?;
            channel
                .set_write_timeout(Some(std::time::Duration::from_secs(5)))
                .map_err(|error| error.to_string())?;
            slots.push(std::sync::Mutex::new(Some(Connection::Inherited(channel))));
        }
        Ok(Self { channels: slots })
    }
    pub fn call<T>(
        &self,
        mut request: impl FnMut(&mut Connection) -> io::Result<T>,
    ) -> Result<T, String> {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let mut usable = false;
            for slot in &self.channels {
                let mut slot = match slot.try_lock() {
                    Ok(slot) => slot,
                    Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
                    Err(std::sync::TryLockError::WouldBlock) => {
                        usable = true;
                        continue;
                    }
                };
                let Some(connection) = slot.as_mut() else {
                    continue;
                };
                match request(connection) {
                    Ok(value) => return Ok(value),
                    Err(error) => {
                        *slot = None;
                        return Err(error.to_string());
                    }
                }
            }
            if !usable {
                return Err("Inherited broker channels are closed".into());
            }
            if std::time::Instant::now() >= until {
                return Err("Inherited broker channels are busy".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}
