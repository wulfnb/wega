//! Capture native engine stderr as well as browser/IPC notices in one log.
pub fn init(profile: &std::path::Path) {
    if let Err(error) = wega::diagnostics::init(profile) {
        eprintln!("Cannot create browser logs: {error}");
        return;
    }
    if let Err(error) = wega::diagnostics::configure_reporting(profile) {
        wega::diagnostics::record("error-reporter", &error);
    }
    if log::set_logger(&NATIVE_LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
    #[cfg(unix)]
    if let Err(error) = capture_stderr() {
        eprintln!("Cannot capture native stderr: {error}");
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        wega::diagnostics::report(
            "browser:panic",
            wega::diagnostic_event::Diagnostic::error(
                wega::diagnostic_event::Category::Browser,
                info.to_string(),
                None,
            ),
        );
        previous(info);
    }));
}
struct NativeLogger;
static NATIVE_LOGGER: NativeLogger = NativeLogger;
impl log::Log for NativeLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
            || metadata.target().contains("error_reporting")
            || metadata.target().contains("servoparser")
            || metadata.target().contains("script::dom::window")
    }
    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        if ["reqwest", "hyper", "rustls"]
            .iter()
            .any(|prefix| record.target().starts_with(prefix))
        {
            wega::diagnostics::record(record.target(), &message);
            return;
        }
        let category = if record.target().contains("error_reporting")
            || record.target().contains("script::dom::window") && message.starts_with("Url:\t")
        {
            wega::diagnostic_event::Category::Css
        } else if record.target().contains("servoparser") && message.contains("Parse error:") {
            wega::diagnostic_event::Category::Html
        } else if record.level() <= log::Level::Warn {
            wega::diagnostic_event::Category::Browser
        } else {
            return;
        };
        let mut event = wega::diagnostic_event::Diagnostic::error(category, message, None);
        if event.category == wega::diagnostic_event::Category::Css
            && let Some((url, detail)) = event
                .message
                .strip_prefix("Url:\t")
                .and_then(|s| s.split_once('\n'))
        {
            event.url = Some(url.into());
            if let Some((position, _)) = detail.split_once(' ')
                && let Some((line, column)) = position.split_once(':')
            {
                event.line = line.parse::<u64>().ok().map(|line| line.saturating_add(1));
                event.column = column.parse().ok();
            }
        }
        event.severity = if record.level() == log::Level::Error {
            "error"
        } else {
            "warning"
        }
        .into();
        wega::diagnostics::report(&format!("servo:{}", record.target()), event);
    }
    fn flush(&self) {}
}

#[cfg(unix)]
fn capture_stderr() -> std::io::Result<()> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let (reader, writer) = std::os::unix::net::UnixStream::pair()?;
    // SAFETY: duplicate stderr before replacing it. The returned descriptor is
    // owned exactly once by File; dup2 leaves stderr owned by the process.
    let duplicate = unsafe { libc::dup(libc::STDERR_FILENO) };
    if duplicate < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let original = unsafe { std::fs::File::from_raw_fd(duplicate) };
    if unsafe { libc::dup2(writer.as_raw_fd(), libc::STDERR_FILENO) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    std::thread::spawn(move || {
        wega::diagnostics::drain_copy("browser:stderr", reader, original);
    });
    Ok(())
}
