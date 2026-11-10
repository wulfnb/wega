//! Desktop cookie adapter and download coordination; no widget rendering.
use crate::{browser::Browser, downloads};
use std::path::Path;

/// Attach the platform's "downloaded from the internet" marker to a finished file so
/// the OS applies its own checks when it is opened (Gatekeeper on macOS, SmartScreen
/// and Office Protected View on Windows, `user.xdg.origin.url` on Linux).
/// Best effort: a filesystem without extended attributes keeps the download.
pub(crate) fn mark_downloaded(path: &Path, url: &str) {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use std::os::unix::ffi::OsStrExt;
        #[cfg(target_os = "macos")]
        let (name, value) = {
            // Gatekeeper records the agent; the source URL lives in its own database.
            let _ = url;
            (
                "com.apple.quarantine",
                downloads::quarantine_value(wega::profile::visit_time(), crate::brand::NAME),
            )
        };
        #[cfg(target_os = "linux")]
        let (name, value) = ("user.xdg.origin.url", url.to_owned());
        let (Ok(file), Ok(name)) = (
            std::ffi::CString::new(path.as_os_str().as_bytes()),
            std::ffi::CString::new(name),
        ) else {
            return;
        };
        // SAFETY: both strings are NUL-terminated and `value` outlives the call,
        // which only reads `value.len()` bytes from it.
        #[cfg(target_os = "macos")]
        let result = unsafe {
            libc::setxattr(
                file.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        // SAFETY: as above; Linux takes no position argument.
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::setxattr(
                file.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        };
        if result != 0 {
            eprintln!(
                "Could not mark {} as downloaded: {}",
                path.display(),
                std::io::Error::last_os_error()
            );
        }
    }
    #[cfg(windows)]
    {
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        if let Err(error) = std::fs::write(&stream, downloads::zone_identifier(url)) {
            eprintln!("Could not mark {} as downloaded: {error}", path.display());
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    let _ = (path, url);
}
impl Browser {
    pub fn offer_download(&mut self, url: &str, filename: &str) {
        self.download_ui.open = true;
        self.download_ui.url = url.into();
        self.download_ui.filename = filename.into();
        self.download_ui.notice = None;
    }
    pub fn begin_download(&mut self) {
        let result = (|| {
            let url = downloads::validate_url(&self.download_ui.url)?;
            let cookies = self
                .tab()
                .wega
                .as_ref()
                .map(|page| page.download_cookies(url.as_str()))
                .transpose()?
                .unwrap_or_else(|| "# Netscape HTTP Cookie File\n".into());
            self.start_download(
                url.as_str(),
                &self.download_ui.filename.clone(),
                self.download_ui.connections,
                cookies,
            )
        })();
        self.download_ui.notice = Some(match result {
            Ok(()) => "Download started.".into(),
            Err(error) => error,
        });
    }
}
