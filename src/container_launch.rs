//! Authenticated, per-profile container-window handoff. Only URL opening/focus is
//! exposed, on loopback, with an OS-random token kept in a private profile file.
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};
const LIMIT: u64 = 16 * 1024;
#[derive(Serialize, Deserialize)]
struct Endpoint {
    address: SocketAddr,
    token: String,
}
#[derive(Serialize, Deserialize)]
struct Request {
    token: String,
    urls: Vec<String>,
}
pub enum Claim {
    Primary(Server),
    Forwarded,
}
pub struct Server {
    listener: Option<TcpListener>,
    endpoint: Endpoint,
    path: PathBuf,
    stop: Option<mpsc::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
    lock: File,
}
impl Server {
    /// A held profile-launch lock distinguishes a stale endpoint from a live
    /// process. Diagnostics never silently attach to a different invocation.
    pub fn claim(directory: &Path, urls: Vec<String>, may_forward: bool) -> Result<Claim, String> {
        validate_urls(&urls)?;
        fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("container-launch.lock"))
            .map_err(|e| e.to_string())?;
        let path = directory.join("container-launch.json");
        if lock.try_lock().is_err() {
            if !may_forward {
                return Err(
                    "This container is already open; use a separate profile for diagnostics".into(),
                );
            }
            // A concurrently starting primary publishes its endpoint after taking
            // the lock. Retry for a bounded time, never start an unisolated copy.
            for _ in 0..20 {
                if forward(&path, &urls).is_ok() {
                    return Ok(Claim::Forwarded);
                }
                thread::sleep(Duration::from_millis(100));
            }
            return Err("This container is already open but is not responding. Its site data has not been changed.".into());
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let endpoint = Endpoint {
            address: listener.local_addr().map_err(|e| e.to_string())?,
            token: format!(
                "{}{}",
                crate::containers::random_id()?,
                crate::containers::random_id()?
            ),
        };
        crate::storage::atomic_write(
            &path,
            &serde_json::to_vec(&endpoint).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        Ok(Claim::Primary(Self {
            listener: Some(listener),
            endpoint,
            path,
            stop: None,
            worker: None,
            lock,
        }))
    }
    pub fn start(
        &mut self,
        receive: impl Fn(Vec<String>) -> bool + Send + 'static,
    ) -> Result<(), String> {
        let listener = self
            .listener
            .take()
            .ok_or("Container launch server already started")?;
        let token = self.endpoint.token.clone();
        let (stop, receiver) = mpsc::channel();
        self.stop = Some(stop);
        self.worker = Some(
            thread::Builder::new()
                .name("container-launch".into())
                .spawn(move || {
                    loop {
                        if let Ok((mut stream, _)) = listener.accept() {
                            let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
                            let _ = stream.set_write_timeout(Some(Duration::from_millis(250)));
                            if let Ok(request) = read_request(&stream)
                                && request.token == token
                                && validate_urls(&request.urls).is_ok()
                                && receive(request.urls)
                            {
                                let _ = stream.write_all(b"ok\n");
                            }
                        }
                        match receiver.recv_timeout(Duration::from_millis(25)) {
                            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                        }
                    }
                })
                .map_err(|e| e.to_string())?,
        );
        Ok(())
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        // The lock is still held: another process cannot publish a new endpoint
        // between this removal and releasing ownership.
        let _ = fs::remove_file(&self.path);
        // Closing one descriptor does not release a flock while a duplicated
        // or inherited descriptor survives (e.g. during a concurrent spawn).
        // Release ownership explicitly, after the listener and endpoint are gone.
        let _ = self.lock.unlock();
    }
}
fn validate_urls(urls: &[String]) -> Result<(), String> {
    if urls.len() > 1 || crate::options::external_launch_urls(urls).len() != urls.len() {
        return Err("Container handoff accepts one web or file URL".into());
    }
    Ok(())
}
fn read_request(stream: &TcpStream) -> Result<Request, String> {
    let mut line = String::new();
    BufReader::new(stream.take(LIMIT + 1))
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    if line.len() as u64 > LIMIT || !line.ends_with('\n') {
        return Err("Invalid container launch request".into());
    }
    serde_json::from_str(&line).map_err(|e| e.to_string())
}
fn forward(path: &Path, urls: &[String]) -> Result<(), String> {
    let endpoint: Endpoint = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if endpoint.address.ip() != std::net::IpAddr::V4(Ipv4Addr::LOCALHOST)
        || endpoint.token.len() != 64
    {
        return Err("Invalid container endpoint".into());
    }
    let mut stream = TcpStream::connect_timeout(&endpoint.address, Duration::from_millis(250))
        .map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| e.to_string())?;
    let mut bytes = serde_json::to_vec(&Request {
        token: endpoint.token,
        urls: urls.to_vec(),
    })
    .map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    stream.write_all(&bytes).map_err(|e| e.to_string())?;
    let mut reply = [0; 3];
    stream.read_exact(&mut reply).map_err(|e| e.to_string())?;
    if &reply != b"ok\n" {
        return Err("Container launch was not acknowledged".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "container_launch_tests.rs"]
mod tests;
