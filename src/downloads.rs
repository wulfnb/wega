//! Bounded, streaming HTTP downloads. curl supplies certificate-checked HTTPS;
//! files are published only after validated parts have been assembled.
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const SEGMENT: u64 = 2 * 1024 * 1024;
/// Prefix of in-progress files in the download folder (hidden on Unix).
const TEMP_PREFIX: &str = ".wega-";

/// A worker that panicked while holding a lock must not take the UI down with it;
/// progress is plain data, so the last written state is still meaningful.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Called with the published path and source URL so the desktop layer can attach
/// platform "mark of the web" metadata. Best effort; it must not fail the download.
pub type Marker = Arc<dyn Fn(&Path, &str) + Send + Sync>;
const RUN: u8 = 0;
const PAUSE: u8 = 1;
const CANCEL: u8 = 2;

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Connecting,
    Downloading,
    Paused,
    Complete,
    Cancelled,
    Failed,
}
#[derive(Clone)]
pub struct Progress {
    pub phase: Phase,
    pub filename: String,
    pub total: Option<u64>,
    pub connections: usize,
    pub message: String,
    pub path: Option<PathBuf>,
}
pub struct Job {
    pub url: String,
    pub progress: Mutex<Progress>,
    command: AtomicU8,
    completed: AtomicU64,
    inflight: Vec<AtomicU64>,
    started: Instant,
}
impl Job {
    pub fn bytes(&self) -> u64 {
        self.completed.load(Ordering::Relaxed)
            + self
                .inflight
                .iter()
                .map(|v| v.load(Ordering::Relaxed))
                .sum::<u64>()
    }
    pub fn speed(&self) -> f64 {
        self.bytes() as f64 / self.started.elapsed().as_secs_f64().max(1.0)
    }
    pub fn pause(&self) {
        self.command.store(PAUSE, Ordering::Relaxed);
    }
    fn resume(&self) {
        self.command.store(RUN, Ordering::Relaxed);
    }
    pub fn cancel(&self) {
        self.command.store(CANCEL, Ordering::Relaxed);
    }
    /// Snapshot of the current progress; never panics on a poisoned lock.
    pub fn snapshot(&self) -> Progress {
        lock(&self.progress).clone()
    }
    fn phase(&self, phase: Phase, message: &str) {
        let mut p = lock(&self.progress);
        p.phase = phase;
        p.message = message.into();
    }
}
struct Item {
    pub job: Arc<Job>,
    worker: Option<JoinHandle<()>>,
}
impl Drop for Item {
    fn drop(&mut self) {
        self.job.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
/// Port used by application download commands. Workers and processes stay in the adapter.
pub trait DownloadPort {
    fn start(
        &mut self,
        url: &str,
        filename: &str,
        connections: usize,
        cookies: String,
    ) -> Result<(), String>;
}
impl DownloadPort for Manager {
    fn start(
        &mut self,
        url: &str,
        filename: &str,
        connections: usize,
        cookies: String,
    ) -> Result<(), String> {
        Manager::start(self, url, filename, connections, cookies)
    }
}
pub struct Manager {
    items: Vec<Item>,
    pub directory: PathBuf,
    work: PathBuf,
    next: u64,
    pub marker: Option<Marker>,
}
impl Manager {
    pub fn new(profile: &Path) -> Self {
        let directory = std::env::var_os("BROWSER_DOWNLOAD_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                directories::UserDirs::new().and_then(|u| u.download_dir().map(Path::to_path_buf))
            })
            .unwrap_or_else(|| profile.join("Downloads"));
        Self {
            items: Vec::new(),
            directory,
            work: profile.join("download-work"),
            next: 0,
            marker: None,
        }
    }
    pub fn jobs(&self) -> Vec<Arc<Job>> {
        self.items.iter().map(|i| i.job.clone()).collect()
    }
    /// Forget one finished entry. Active and paused transfers are kept, so a
    /// stale click can never drop a download that is still in progress.
    pub fn remove(&mut self, job: &Arc<Job>) {
        self.items.retain(|i| {
            !Arc::ptr_eq(&i.job, job)
                || !matches!(
                    lock(&i.job.progress).phase,
                    Phase::Complete | Phase::Failed | Phase::Cancelled
                )
        });
    }
    pub fn clear_finished(&mut self) {
        self.items.retain(|i| {
            !matches!(
                lock(&i.job.progress).phase,
                Phase::Complete | Phase::Failed | Phase::Cancelled
            )
        });
    }
    pub fn resume(&mut self, job: &Job) -> Result<(), String> {
        if self.active_count() >= 3 {
            Err("Wait for an active download to finish or pause it first.".into())
        } else {
            job.resume();
            Ok(())
        }
    }
    fn active_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| {
                i.job.command.load(Ordering::Relaxed) == RUN
                    && !matches!(
                        lock(&i.job.progress).phase,
                        Phase::Complete | Phase::Failed | Phase::Cancelled
                    )
            })
            .count()
    }
    pub fn start(
        &mut self,
        raw: &str,
        filename: &str,
        connections: usize,
        cookies: String,
    ) -> Result<(), String> {
        let url = validate_url(raw)?;
        if self.items.len() >= 64 {
            return Err("Clear finished downloads before adding more.".into());
        }
        if self.active_count() >= 3 {
            return Err("Up to three downloads can run at once.".into());
        }
        fs::create_dir_all(&self.directory).map_err(|e| e.to_string())?;
        fs::create_dir_all(&self.work).map_err(|e| e.to_string())?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        self.next += 1;
        let work = self
            .work
            .join(format!("{}-{nonce}-{}", std::process::id(), self.next));
        fs::create_dir(&work).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&work, fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let job = Arc::new(Job {
            url: url.to_string(),
            progress: Mutex::new(Progress {
                phase: Phase::Connecting,
                filename: if filename.is_empty() {
                    url_filename(&url)
                } else {
                    safe_filename(filename)
                },
                total: None,
                connections: 1,
                message: String::new(),
                path: None,
            }),
            command: AtomicU8::new(RUN),
            completed: AtomicU64::new(0),
            inflight: (0..8).map(|_| AtomicU64::new(0)).collect(),
            started: Instant::now(),
        });
        let target = self.directory.clone();
        let user_name = filename.to_owned();
        let background = job.clone();
        let marker = self.marker.clone();
        let worker = thread::Builder::new()
            .name("download".into())
            .spawn(move || {
                let result = run(
                    &background,
                    &work,
                    &target,
                    &user_name,
                    connections.clamp(1, 8),
                    &cookies,
                );
                match result {
                    Ok(path) => {
                        if let Some(mark) = &marker {
                            mark(&path, &background.url);
                        }
                        let mut p = lock(&background.progress);
                        p.filename = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        p.path = Some(path);
                        p.phase = Phase::Complete;
                        p.message = "Saved".into();
                    }
                    Err(_) if background.command.load(Ordering::Relaxed) == CANCEL => {
                        background.phase(Phase::Cancelled, "Cancelled")
                    }
                    Err(error) => background.phase(Phase::Failed, &error),
                }
                let _ = fs::remove_dir_all(work);
            })
            .map_err(|e| e.to_string())?;
        self.items.push(Item {
            job,
            worker: Some(worker),
        });
        Ok(())
    }
}

pub fn validate_url(raw: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(raw.trim()).map_err(|e| e.to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("Downloads require an HTTP or HTTPS URL.".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(
            "Use a signed URL or the browser's site cookies rather than credentials in the URL."
                .into(),
        );
    }
    if raw.len() > 8192 {
        return Err("Download URL is too long.".into());
    }
    Ok(url)
}
pub fn safe_filename(name: &str) -> String {
    let mut name: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "/\\<>:\"|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    name = name.trim_matches([' ', '.']).to_string();
    while name.len() > 180 {
        name.pop();
    }
    if name.is_empty() {
        return "download.bin".into();
    }
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = Path::new(&name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    if RESERVED.iter().any(|&r| stem.eq_ignore_ascii_case(r)) {
        name = format!("_{name}");
    }
    name
}
fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(a), Some(b)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push((a * 16 + b) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into()
}
fn url_filename(url: &url::Url) -> String {
    safe_filename(&decode(
        url.path_segments()
            .and_then(|mut s| s.next_back())
            .filter(|s| !s.is_empty())
            .unwrap_or("download.bin"),
    ))
}
#[derive(Clone, Default, Debug, PartialEq)]
struct Meta {
    status: u16,
    headers: BTreeMap<String, String>,
}
impl Meta {
    fn length(&self) -> Option<u64> {
        self.headers.get("content-length")?.parse().ok()
    }
    fn validator(&self) -> Option<String> {
        self.headers
            .get("etag")
            .filter(|s| !s.starts_with("W/"))
            .or_else(|| self.headers.get("last-modified"))
            .cloned()
    }
    fn filename(&self) -> Option<String> {
        let value = self.headers.get("content-disposition")?;
        let fields: Vec<_> = value
            .split(';')
            .filter_map(|s| s.trim().split_once('='))
            .collect();
        if let Some((_, v)) = fields
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("filename*"))
            && let Some(v) = v
                .trim_matches('"')
                .strip_prefix("UTF-8''")
                .or_else(|| v.trim_matches('"').strip_prefix("utf-8''"))
        {
            return Some(safe_filename(&decode(v)));
        }
        fields
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("filename"))
            .map(|(_, v)| safe_filename(v.trim_matches('"')))
    }
}
fn headers(path: &Path) -> Result<Meta, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(256 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    // HTTP/1 header values may be legacy Latin-1, not UTF-8. Preserve those
    // filenames without rejecting an otherwise valid binary transfer.
    let text = String::from_utf8(bytes)
        .unwrap_or_else(|error| error.into_bytes().into_iter().map(char::from).collect());
    let mut result = Meta::default();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with("HTTP/") {
            result = Meta {
                status: line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
                ..Default::default()
            };
        } else if let Some((key, value)) = line.split_once(':') {
            result
                .headers
                .insert(key.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    Ok(result)
}
/// Absolute path of the system curl. PATH is not consulted, so a writable directory
/// earlier in PATH cannot substitute the program that receives session cookies.
pub fn curl_program() -> Option<PathBuf> {
    curl_candidates().into_iter().find(|path| path.is_file())
}
fn curl_candidates() -> Vec<PathBuf> {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .filter(|root| root.is_absolute())
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        vec![root.join("System32").join("curl.exe")]
    } else {
        [
            "/usr/bin/curl",
            "/bin/curl",
            "/usr/local/bin/curl",
            "/run/current-system/sw/bin/curl",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    }
}

/// Windows "mark of the web" stream content (Internet zone).
pub fn zone_identifier(url: &str) -> String {
    let url = url.replace(['\r', '\n'], "");
    format!("[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl={url}\r\nHostUrl={url}\r\n")
}
/// macOS `com.apple.quarantine` value: flags; hex seconds since the epoch; agent; event id.
pub fn quarantine_value(unix_seconds: u64, agent: &str) -> String {
    format!("0081;{unix_seconds:08x};{agent};")
}
/// Files that run code when opened. Opening them from Downloads needs confirmation.
pub fn opens_as_program(path: &Path) -> bool {
    const RISKY: &[&str] = &[
        "app",
        "command",
        "tool",
        "sh",
        "bash",
        "zsh",
        "csh",
        "ksh",
        "fish",
        "pkg",
        "mpkg",
        "dmg",
        "exe",
        "msi",
        "msix",
        "appx",
        "bat",
        "cmd",
        "com",
        "scr",
        "pif",
        "cpl",
        "ps1",
        "psm1",
        "vbs",
        "vbe",
        "js",
        "jse",
        "wsf",
        "wsh",
        "hta",
        "lnk",
        "reg",
        "jar",
        "jnlp",
        "scpt",
        "applescript",
        "workflow",
        "terminal",
        "desktop",
        "appimage",
        "run",
        "bin",
        "deb",
        "rpm",
        "py",
        "pl",
        "rb",
        "php",
        "url",
        "webloc",
        "inetloc",
        "iso",
        "img",
        "vhd",
        "vhdx",
    ];
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| RISKY.iter().any(|risky| ext.eq_ignore_ascii_case(risky)))
}
fn curl(
    job: &Job,
    work: &Path,
    files: (&Path, &Path),
    head: bool,
    range: Option<(u64, u64, &str)>,
    slot: usize,
    abort: &AtomicBool,
) -> Result<Meta, String> {
    let (body, header) = files;
    let error = body.with_extension("error");
    let mut command = Command::new(curl_program().ok_or(
        "curl was not found in the system directories; it is required for secure streaming downloads.",
    )?);
    command
        .args([
            "--disable",
            "--location",
            "--max-redirs",
            "10",
            "--proto",
            "=http,https",
            "--proto-redir",
            "=http,https",
            "--connect-timeout",
            "15",
            "--fail",
            "--silent",
            "--show-error",
            "--header",
            "Accept-Encoding: identity",
            "--cookie",
        ])
        .arg(work.join("cookies"))
        .arg("--dump-header")
        .arg(header)
        .arg("--output")
        .arg(body)
        .arg("--url")
        .arg(&job.url)
        .stdout(Stdio::null())
        .stderr(File::create(&error).map_err(|e| e.to_string())?);
    if head {
        command.arg("--head").args(["--max-time", "30"]);
    }
    if let Some((start, end, validator)) = range {
        command
            .arg("--range")
            .arg(format!("{start}-{end}"))
            .arg("--header")
            .arg(format!("If-Range: {validator}"))
            .arg("--max-filesize")
            .arg((end - start + 1).to_string());
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Cannot start curl: {e}"))?;
    loop {
        if job.command.load(Ordering::Relaxed) != RUN || abort.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Interrupted".into());
        }
        if !head {
            job.inflight[slot].store(
                fs::metadata(body).map(|m| m.len()).unwrap_or(0),
                Ordering::Relaxed,
            );
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let meta = headers(header)?;
                if !status.success() {
                    let mut message = String::new();
                    if let Ok(file) = File::open(&error) {
                        let _ = file.take(2048).read_to_string(&mut message);
                    }
                    return Err(if message.trim().is_empty() {
                        format!("HTTP {} / transfer failed", meta.status)
                    } else {
                        message.trim().to_owned()
                    });
                }
                if !(200..300).contains(&meta.status) {
                    return Err(format!("HTTP {}", meta.status));
                }
                return Ok(meta);
            }
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
        }
    }
}
fn run(
    job: &Arc<Job>,
    work: &Path,
    target: &Path,
    user_name: &str,
    connections: usize,
    cookies: &str,
) -> Result<PathBuf, String> {
    let cookie_path = work.join("cookies");
    let mut file = File::create(&cookie_path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    file.write_all(cookies.as_bytes())
        .map_err(|e| e.to_string())?;
    drop(file);
    let abort = AtomicBool::new(false);
    let mut previous: Option<Meta> = None;
    let mut force_single = false;
    loop {
        while job.command.load(Ordering::Relaxed) == PAUSE {
            job.phase(Phase::Paused, "Paused; completed segments are retained.");
            thread::sleep(Duration::from_millis(100));
        }
        if job.command.load(Ordering::Relaxed) == CANCEL {
            return Err("Cancelled".into());
        }
        job.phase(Phase::Connecting, "Checking server…");
        let meta = match curl(
            job,
            work,
            (&work.join("head.body"), &work.join("head.headers")),
            true,
            None,
            0,
            &abort,
        ) {
            Ok(m) => m,
            Err(_) if job.command.load(Ordering::Relaxed) != RUN => continue,
            Err(_) => Meta::default(),
        };
        let multipart = !force_single
            && connections > 1
            && meta.length().is_some_and(|n| n > SEGMENT)
            && meta
                .headers
                .get("accept-ranges")
                .is_some_and(|v| v.eq_ignore_ascii_case("bytes"))
            && meta.validator().is_some();
        let name = if user_name.is_empty() {
            meta.filename()
                .unwrap_or_else(|| url_filename(&validate_url(&job.url).unwrap()))
        } else {
            safe_filename(user_name)
        };
        if previous
            .as_ref()
            .is_some_and(|old| old.length() != meta.length() || old.validator() != meta.validator())
        {
            clear_parts(work)?;
        }
        previous = Some(meta.clone());
        job.completed.store(0, Ordering::Relaxed);
        for v in &job.inflight {
            v.store(0, Ordering::Relaxed);
        }
        {
            let mut p = lock(&job.progress);
            p.phase = Phase::Downloading;
            p.filename = name.clone();
            p.total = meta.length();
            p.connections = if multipart { connections } else { 1 };
            p.message = if multipart {
                "Parallel ranges; resume retains completed 2 MiB segments.".into()
            } else {
                "Single connection (range support and a stable validator are required for multipart). Resume restarts this transfer.".into()
            };
        }
        let result = if multipart {
            parallel(job, work, &meta, connections)
        } else {
            single(job, work)
        };
        if job.command.load(Ordering::Relaxed) != RUN {
            continue;
        }
        match result {
            Ok(body) => {
                let actual = fs::metadata(&body).map_err(|e| e.to_string())?.len();
                job.completed.store(actual, Ordering::Relaxed);
                for v in &job.inflight {
                    v.store(0, Ordering::Relaxed);
                }
                lock(&job.progress).total = Some(actual);
                return publish(&body, target, &name);
            }
            Err(_) if multipart => {
                clear_parts(work)?;
                force_single = true;
            }
            Err(error) => return Err(error),
        }
    }
}
fn clear_parts(work: &Path) -> Result<(), String> {
    for entry in fs::read_dir(work).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_name().to_string_lossy().starts_with("part-") {
            fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn parallel(
    job: &Arc<Job>,
    work: &Path,
    meta: &Meta,
    connections: usize,
) -> Result<PathBuf, String> {
    let total = meta.length().unwrap();
    let count = total.div_ceil(SEGMENT) as usize;
    if count > 100_000 {
        return Err("Too many download segments; using a single connection.".into());
    }
    let next = AtomicUsize::new(0);
    let abort = AtomicBool::new(false);
    let error = Mutex::new(None);
    thread::scope(|scope| {
        for slot in 0..connections.min(count) {
            let (next, abort, error) = (&next, &abort, &error);
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= count
                        || abort.load(Ordering::Relaxed)
                        || job.command.load(Ordering::Relaxed) != RUN
                    {
                        break;
                    }
                    let start = index as u64 * SEGMENT;
                    let end = (start + SEGMENT).min(total) - 1;
                    let size = end - start + 1;
                    let part = work.join(format!("part-{index:06}"));
                    if fs::metadata(&part).is_ok_and(|m| m.len() == size) {
                        job.completed.fetch_add(size, Ordering::Relaxed);
                        continue;
                    }
                    let incoming = part.with_extension("incoming");
                    let header = part.with_extension("headers");
                    let result = curl(
                        job,
                        work,
                        (&incoming, &header),
                        false,
                        Some((start, end, meta.validator().as_deref().unwrap())),
                        slot,
                        abort,
                    )
                    .and_then(|response| {
                        let expected = format!("bytes {start}-{end}/{total}");
                        if response.status != 206
                            || response.headers.get("content-range") != Some(&expected)
                            || response
                                .headers
                                .get("content-encoding")
                                .is_some_and(|v| !v.eq_ignore_ascii_case("identity"))
                            || fs::metadata(&incoming).map_err(|e| e.to_string())?.len() != size
                        {
                            return Err("Server returned an invalid byte range.".into());
                        }
                        if let Some(etag) = meta.headers.get("etag")
                            && response.headers.get("etag").is_some_and(|v| v != etag)
                        {
                            return Err("File changed during download.".into());
                        }
                        fs::rename(&incoming, &part).map_err(|e| e.to_string())?;
                        Ok(())
                    });
                    job.inflight[slot].store(0, Ordering::Relaxed);
                    match result {
                        Ok(()) => {
                            job.completed.fetch_add(size, Ordering::Relaxed);
                        }
                        Err(e) => {
                            let _ = fs::remove_file(incoming);
                            let mut first = lock(error);
                            if first.is_none() {
                                *first = Some(e)
                            }
                            abort.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
            });
        }
    });
    if let Some(error) = error.into_inner().unwrap_or_else(PoisonError::into_inner) {
        return Err(error);
    }
    if job.command.load(Ordering::Relaxed) != RUN {
        return Err("Interrupted".into());
    }
    let path = work.join("assembled");
    let mut out = File::create(&path).map_err(|e| e.to_string())?;
    for index in 0..count {
        if job.command.load(Ordering::Relaxed) != RUN {
            return Err("Interrupted".into());
        }
        let mut file =
            File::open(work.join(format!("part-{index:06}"))).map_err(|e| e.to_string())?;
        io::copy(&mut file, &mut out).map_err(|e| e.to_string())?;
    }
    out.sync_all().map_err(|e| e.to_string())?;
    Ok(path)
}
fn single(job: &Job, work: &Path) -> Result<PathBuf, String> {
    let path = work.join("single");
    let meta = curl(
        job,
        work,
        (&path, &work.join("single.headers")),
        false,
        None,
        0,
        &AtomicBool::new(false),
    )?;
    let length = fs::metadata(&path).map_err(|e| e.to_string())?.len();
    if meta.length().is_some_and(|n| n != length) {
        return Err("Server response length did not match the downloaded file.".into());
    }
    Ok(path)
}
fn publish(body: &Path, target: &Path, name: &str) -> Result<PathBuf, String> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = target.join(format!("{TEMP_PREFIX}{nonce}.tmp"));
    let result = (|| {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            output
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        io::copy(
            &mut File::open(body).map_err(|e| e.to_string())?,
            &mut output,
        )
        .map_err(|e| e.to_string())?;
        output.sync_all().map_err(|e| e.to_string())?;
        drop(output);
        let p = Path::new(name);
        let stem = p.file_stem().unwrap_or_default().to_string_lossy();
        let ext = p
            .extension()
            .map(|s| format!(".{}", s.to_string_lossy()))
            .unwrap_or_default();
        for index in 0..10_000 {
            let path = target.join(if index == 0 {
                name.into()
            } else {
                format!("{stem} ({index}){ext}")
            });
            match fs::hard_link(&temp, &path) {
                Ok(()) => return Ok(path),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(_) => {
                    // Filesystem may not support hard links (e.g. FAT32, exFAT, network shares).
                    // Fall back to atomic rename if the target path does not exist.
                    if !path.exists() && fs::rename(&temp, &path).is_ok() {
                        return Ok(path);
                    }
                    if path.exists() {
                        continue;
                    }
                }
            }
        }
        Err("Too many files with the same name.".into())
    })();
    let _ = fs::remove_file(temp);
    result
}

#[cfg(test)]
#[path = "downloads_tests.rs"]
mod tests;
