//! Private, bounded JSON-lines diagnostics. No keystroke or IPC command logging.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
const MAX_BYTES: u64 = 5 * 1024 * 1024;
const BACKUPS: usize = 3;
static LOG: OnceLock<Mutex<Sink>> = OnceLock::new();
static REPORTER: OnceLock<crate::error_reporting::Reporter> = OnceLock::new();

struct Sink {
    directory: PathBuf,
    file: Option<File>,
    bytes: u64,
    limit: u64,
}
fn open(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
impl Sink {
    fn new(directory: &Path, limit: u64) -> io::Result<Self> {
        fs::create_dir_all(directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
        }
        let file = open(&directory.join("browser.jsonl"))?;
        let bytes = file.metadata()?.len();
        Ok(Self {
            directory: directory.to_owned(),
            file: Some(file),
            bytes,
            limit,
        })
    }
    fn record(&mut self, source: &str, message: &str) -> io::Result<()> {
        let message: String = message.chars().take(16384).collect();
        let entry = serde_json::json!({"time":chrono::Utc::now().to_rfc3339(),"pid":std::process::id(),"source":source,"message":message});
        let mut bytes = serde_json::to_vec(&entry)?;
        bytes.push(b'\n');
        if self.bytes > 0 && self.bytes + bytes.len() as u64 > self.limit {
            // Close before renaming so rotation also works on Windows.
            self.file.take();
            for i in (1..=BACKUPS).rev() {
                let old = if i == 1 {
                    self.directory.join("browser.jsonl")
                } else {
                    self.directory.join(format!("browser.{}.jsonl", i - 1))
                };
                let new = self.directory.join(format!("browser.{i}.jsonl"));
                if new.exists() {
                    fs::remove_file(&new)?;
                }
                if old.exists() {
                    fs::rename(old, new)?;
                }
            }
            self.file = Some(open(&self.directory.join("browser.jsonl"))?);
            self.bytes = 0;
        }
        if self.file.is_none() {
            self.file = Some(open(&self.directory.join("browser.jsonl"))?);
        }
        let file = self.file.as_mut().unwrap();
        file.write_all(&bytes)?;
        file.flush()?;
        self.bytes += bytes.len() as u64;
        Ok(())
    }
}
pub fn init(profile: &Path) -> io::Result<()> {
    let sink = Sink::new(&profile.join("logs"), MAX_BYTES)?;
    let _ = LOG.set(Mutex::new(sink));
    record("browser", "Session started");
    Ok(())
}
pub fn record(source: &str, message: &str) {
    if let Some(log) = LOG.get() {
        let _ = log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record(source, message);
    }
}
pub fn report(source: &str, diagnostic: crate::diagnostic_event::Diagnostic) {
    let diagnostic = diagnostic.bounded();
    if let Ok(json) = serde_json::to_string(&diagnostic) {
        record(source, &json);
    }
    submit(source, diagnostic);
}
fn submit(source: &str, diagnostic: crate::diagnostic_event::Diagnostic) {
    if let Some(reporter) = REPORTER.get()
        && reporter.enqueue(source, diagnostic).is_err()
    {
        record(
            "error-reporter",
            "Cannot persist report in outbox; the original diagnostic remains in the local log",
        );
    }
}
#[cfg(feature = "desktop")]
pub fn configure_reporting(profile: &std::path::Path) -> Result<(), String> {
    // A broken endpoint or identity must not prevent local error capture.
    let configured = (|| {
        crate::error_reporting::Identity::load(profile)
            .map_err(|_| "Cannot initialize error-report identity".to_owned())?;
        crate::error_reporting::Config::load(profile)
    })();
    let mut setup_error = None;
    let mut enabled = false;
    let reporter = match configured {
        Ok(Some(config)) => match crate::error_reporting::HttpTransport::new(config, profile) {
            Ok(transport) => {
                enabled = true;
                crate::error_reporting::Reporter::start(profile, transport)
            }
            Err(error) => {
                setup_error = Some(error);
                crate::error_reporting::Reporter::buffer(profile)
            }
        },
        other => {
            setup_error = other.err();
            crate::error_reporting::Reporter::buffer(profile)
        }
    }
    .map_err(|_| "Cannot create error-report outbox".to_owned())?;
    REPORTER
        .set(reporter)
        .map_err(|_| "Error reporter already initialized".to_owned())?;
    record(
        "error-reporter",
        if enabled {
            "Signed error reporting enabled"
        } else {
            "Errors are queued locally; collector URL is not configured"
        },
    );
    setup_error.map_or(Ok(()), Err)
}
#[derive(Default)]
struct Decoder {
    pending: Vec<u8>,
    discarding: bool,
}
impl Decoder {
    fn feed(&mut self, bytes: &[u8], mut receive: impl FnMut(crate::diagnostic_event::Diagnostic)) {
        for byte in bytes {
            if *byte == b'\n' {
                if !self.discarding
                    && let Some(json) = self
                        .pending
                        .strip_prefix(crate::diagnostic_event::PREFIX.as_bytes())
                    && let Ok(event) = serde_json::from_slice(json)
                {
                    receive(event);
                }
                self.pending.clear();
                self.discarding = false;
            } else if !self.discarding {
                if self.pending.len() < 256 * 1024 {
                    self.pending.push(*byte);
                } else {
                    self.pending.clear();
                    self.discarding = true;
                }
            }
        }
    }
}
/// Drain stderr incrementally with bounded memory, even for a line with no newline.
pub fn drain(source: &str, input: impl io::Read) {
    drain_copy(source, input, io::sink());
}
pub fn drain_copy(source: &str, mut input: impl io::Read, mut copy: impl io::Write) {
    let mut decoder = Decoder::default();
    let mut chunk = [0; 4096];
    while let Ok(count) = input.read(&mut chunk) {
        if count == 0 {
            break;
        }
        record(source, &String::from_utf8_lossy(&chunk[..count]));
        decoder.feed(&chunk[..count], |diagnostic| submit(source, diagnostic));
        let _ = copy.write_all(&chunk[..count]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_log_and_worker_stream_pipeline_persists_every_report_and_survives_outbox_failure() {
        use crate::diagnostic_event::{Category, Diagnostic, PREFIX};
        let directory = std::env::temp_dir().join(format!("wega-log-pipeline-{}", getrandom_id()));
        init(&directory).unwrap();
        assert!(
            REPORTER
                .set(crate::error_reporting::Reporter::buffer(&directory).unwrap())
                .is_ok()
        );
        report(
            "native",
            Diagnostic::error(Category::Javascript, "native exception", None),
        );
        let mut wire = b"ordinary worker stderr\n".to_vec();
        for index in 0..40 {
            let event = Diagnostic::error(
                Category::Css,
                format!("CSS failure {index} λ\nstack"),
                Some("https://example.com/style.css".into()),
            );
            wire.extend_from_slice(
                format!("{PREFIX}{}\n", serde_json::to_string(&event).unwrap()).as_bytes(),
            );
        }
        let mut tee = Vec::new();
        drain_copy("worker", io::Cursor::new(&wire), &mut tee);
        assert_eq!(wire, tee);
        let event = Diagnostic::error(Category::Html, "HTML parser error", None);
        drain(
            "worker",
            io::Cursor::new(format!(
                "{PREFIX}{}\n",
                serde_json::to_string(&event).unwrap()
            )),
        );
        let outbox = directory.join("logs/error-outbox");
        let reports: Vec<crate::error_report_model::Report> = fs::read_dir(&outbox)
            .unwrap()
            .map(|entry| serde_json::from_slice(&fs::read(entry.unwrap().path()).unwrap()).unwrap())
            .collect();
        assert_eq!(reports.len(), 42);
        for index in 0..40 {
            assert!(
                reports
                    .iter()
                    .any(|r| r.diagnostic.message == format!("CSS failure {index} λ\nstack"))
            );
        }
        let unavailable = directory.join("unavailable-outbox");
        fs::rename(&outbox, &unavailable).unwrap();
        report(
            "native",
            Diagnostic::error(
                Category::Browser,
                "outbox unavailable but local log survives",
                None,
            ),
        );
        fs::rename(unavailable, outbox).unwrap();
        let log = fs::read_to_string(directory.join("logs/browser.jsonl")).unwrap();
        assert!(log.contains("outbox unavailable but local log survives"));
        assert!(log.contains("Cannot persist report"));
        // Close the static log before removing its temporary Windows directory.
        LOG.get().unwrap().lock().unwrap().file.take();
        let _ = fs::remove_dir_all(directory);
    }
    #[test]
    fn rotation_keeps_recent_messages_and_multiline_errors_are_single_records() {
        let directory = std::env::temp_dir().join(format!("wega-log-test-{}", getrandom_id()));
        let mut sink = Sink::new(&directory, 200).unwrap();
        for i in 0..20 {
            sink.record("worker:42", &format!("error {i}\nstack trace"))
                .unwrap();
        }
        drop(sink);
        let files: Vec<_> = fs::read_dir(&directory).unwrap().collect();
        assert_eq!(files.len(), 4);
        let current = fs::read_to_string(directory.join("browser.jsonl")).unwrap();
        let value: serde_json::Value = serde_json::from_str(current.trim()).unwrap();
        assert_eq!(value["message"], "error 19\nstack trace");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(directory.join("browser.jsonl"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(directory).unwrap();
    }
    fn getrandom_id() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }
}

#[cfg(test)]
mod decoder_tests {
    use super::*;
    #[test]
    fn split_utf8_and_multiline_stacks_are_reassembled_without_losing_a_burst() {
        let mut decoder = Decoder::default();
        let mut events = vec![];
        let mut wire = vec![];
        for i in 0..40 {
            let diagnostic = crate::diagnostic_event::Diagnostic::error(
                crate::diagnostic_event::Category::Javascript,
                format!("exception {i}: 文\nstack\nnext frame"),
                None,
            );
            wire.extend_from_slice(
                format!(
                    "{}{}\n",
                    crate::diagnostic_event::PREFIX,
                    serde_json::to_string(&diagnostic).unwrap()
                )
                .as_bytes(),
            );
        }
        for chunk in wire.chunks(7) {
            decoder.feed(chunk, |event| events.push(event));
        }
        assert_eq!(events.len(), 40);
        assert_eq!(events[39].message, "exception 39: 文\nstack\nnext frame");
        decoder.feed(&vec![b'x'; 300 * 1024], |_| panic!("oversize line"));
        assert!(decoder.pending.len() <= 256 * 1024);
    }
}
