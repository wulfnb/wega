//! Shared interaction rules for native chrome and dialogs.
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Severity {
    Info,
    Success,
    Warning,
    Error,
}
pub struct Notice {
    pub severity: Severity,
    pub message: String,
    pub path: Option<PathBuf>,
    pub created: Instant,
}
impl Notice {
    pub fn new(severity: Severity, message: impl Into<String>, path: Option<PathBuf>) -> Self {
        Self {
            severity,
            message: message.into(),
            path,
            created: Instant::now(),
        }
    }
    pub fn expired(&self) -> bool {
        matches!(self.severity, Severity::Info | Severity::Success)
            && self.created.elapsed() > Duration::from_secs(8)
    }
}

/// Only ordinary Enter submits. Shift+Enter stays available for multiline input.
pub fn enter(ctx: &egui::Context) -> bool {
    ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
}
pub fn escape(ctx: &egui::Context) -> bool {
    ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
}
pub fn validate_files(input: &str, multiple: bool) -> Result<Vec<PathBuf>, String> {
    let paths: Vec<_> = input
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();
    if paths.is_empty() {
        return Err("Choose a file or enter its full path.".into());
    }
    if !multiple && paths.len() != 1 {
        return Err("This website accepts one file at a time.".into());
    }
    for path in &paths {
        if !path.is_absolute() {
            return Err(format!("Use a full path: {}", path.display()));
        }
        if !path.is_file() {
            return Err(format!("File not found or unavailable: {}", path.display()));
        }
    }
    Ok(paths)
}
/// Launch paths as separate arguments; never pass user-controlled text to a shell.
pub fn open_path(path: &Path, reveal: bool) -> Result<(), String> {
    let absolute =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    let path = absolute.as_path();
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("/usr/bin/open");
        if reveal {
            c.arg("-R");
        }
        c
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = std::process::Command::new("explorer.exe");
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = std::process::Command::new("xdg-open");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let path = if reveal {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    #[cfg(target_os = "windows")]
    if reveal {
        command.arg(format!("/select,{}", path.display()));
    } else {
        command.arg(path);
    }
    #[cfg(not(target_os = "windows"))]
    command.arg(path);
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open {}: {e}", path.display()))
}
/// Launch only after the existing external-application confirmation.
pub fn open_external(url: &url::Url) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn ShellExecuteW(
                window: *mut std::ffi::c_void,
                operation: *const u16,
                file: *const u16,
                parameters: *const u16,
                directory: *const u16,
                show: i32,
            ) -> isize;
        }
        let target: Vec<u16> = std::ffi::OsStr::new(url.as_str())
            .encode_wide()
            .chain(Some(0))
            .collect();
        // No command interpreter or arguments: the OS resolves the confirmed URI.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                std::ptr::null(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        };
        if result <= 32 {
            return Err(format!(
                "Could not open external application (Windows error {result})."
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let launcher = if cfg!(target_os = "macos") {
            "/usr/bin/open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(launcher)
            .arg(url.as_str())
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Could not open external application: {error}"))
    }
}

pub fn compact(width: f32) -> bool {
    width < 1100.0
}

impl crate::browser::Browser {
    pub(crate) fn notify(
        &mut self,
        severity: Severity,
        message: impl Into<String>,
        path: Option<PathBuf>,
    ) {
        self.status = None;
        self.chrome.notice = Some(Notice::new(severity, message, path));
        self.window.request_redraw();
    }
    pub(crate) fn prepare_dialog(&mut self, ctx: &egui::Context, key: impl Into<String>) -> bool {
        let key = key.into();
        if self.chrome.dialog_key.as_ref() == Some(&key) {
            return false;
        }
        if self.chrome.dialog_key.is_none() {
            self.chrome.dialog_previous_focus = ctx.memory(|m| m.focused());
        }
        self.chrome.dialog_key = Some(key);
        self.chrome.file_error = None;
        true
    }
    pub(crate) fn finish_dialog(&mut self, ctx: &egui::Context) {
        self.chrome.file_handoff = None;
        if self.chrome.dialog_key.take().is_some() {
            if let Some(id) = self.chrome.dialog_previous_focus.take() {
                ctx.memory_mut(|m| m.request_focus(id));
            } else {
                self.chrome.page_focused = true;
                if let Some(view) = self.tab().view() {
                    view.set_focused(true);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "ux_tests.rs"]
mod tests;
