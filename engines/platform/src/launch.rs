//! Host-side launch hygiene. Containment and environment cleanup are deliberately
//! separate from an OS content sandbox: neither is sufficient proof of isolation.
use crate::process::OwnedProcess;
use std::{io, path::Path, process::Command};

#[cfg(target_os = "linux")]
#[path = "launch_linux.rs"]
mod linux;
#[cfg(windows)]
#[path = "launch_windows.rs"]
mod windows;

pub const DEVELOPMENT_ENV: &str = "WEGA_UNSAFE_DEVELOPMENT_WORKER";
pub const DEVELOPMENT_ARGUMENT: &str = "--development-unsandboxed";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrokerTransport {
    Tcp,
    InheritedEndpoint,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Required,
    Development,
}
impl Mode {
    fn select(debug_build: bool, development_opt_in: bool) -> io::Result<Self> {
        if development_opt_in && !debug_build {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Unsandboxed workers are forbidden in release builds",
            ));
        }
        Ok(if development_opt_in {
            Self::Development
        } else {
            Self::Required
        })
    }
}

fn unavailable(transport: BrokerTransport) -> io::Error {
    let requirement = match transport {
        BrokerTransport::Tcp => "the broker still requires arbitrary TCP connections",
        BrokerTransport::InheritedEndpoint => {
            "OS content sandbox and resource brokers are not yet integrated and verified"
        }
    };
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "Wega sandbox setup is unavailable: {requirement}. Release browsing is blocked. A debug build can explicitly opt in with {DEVELOPMENT_ENV}=1."
        ),
    )
}

pub struct PreparedLaunch {
    directory: tempfile::TempDir,
}
impl PreparedLaunch {
    #[cfg(unix)]
    pub fn inherit_frame_file(
        &self,
        command: &mut Command,
        file: &std::fs::File,
    ) -> io::Result<u64> {
        crate::inherited::preserve_file(command, file)
    }
    /// Explicit fixture opt-in without mutating the test process environment.
    /// This cannot enable unsandboxed release workers.
    pub fn development_fixture(command: &mut Command) -> io::Result<Self> {
        Self::prepare(
            command,
            BrokerTransport::InheritedEndpoint,
            Mode::select(cfg!(debug_assertions), true)?,
        )
    }
    #[cfg(unix)]
    pub fn inherit_broker_channels(
        &self,
        command: &mut Command,
        channels: &[std::os::unix::net::UnixStream],
    ) -> io::Result<Vec<u64>> {
        crate::inherited::preserve(command, channels)
    }
    /// Read the escape hatch in the trusted browser, never in renderer-supplied IPC.
    /// Required mode remains fail-closed until Step 04 transport/resource handles
    /// and the per-platform sandbox backends actually exist; no claimed capability
    /// or environment variable can turn an unavailable backend into a sandbox.
    pub fn browser(command: &mut Command, transport: BrokerTransport) -> io::Result<Self> {
        let requested = std::env::var_os(DEVELOPMENT_ENV).is_some_and(|value| value == "1");
        Self::prepare(
            command,
            transport,
            Mode::select(cfg!(debug_assertions), requested)?,
        )
    }
    fn prepare(command: &mut Command, transport: BrokerTransport, mode: Mode) -> io::Result<Self> {
        if mode == Mode::Required {
            return Err(unavailable(transport));
        }
        if !Path::new(command.get_program()).is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Worker executable must have an absolute path",
            ));
        }
        let mut temporary = tempfile::Builder::new();
        temporary.prefix("wega-content-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temporary.permissions(std::fs::Permissions::from_mode(0o700));
        }
        let directory = temporary.tempdir()?;
        Self::sanitize(command, directory.path());
        command.arg(DEVELOPMENT_ARGUMENT);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(target_os = "linux")]
        linux::prepare(command);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        Ok(Self { directory })
    }
    fn sanitize(command: &mut Command, directory: &Path) {
        command.env_clear().current_dir(directory);
        // No PATH, loader injection, credentials, proxies, profile HOME or V8 flags.
        for name in ["LANG", "LC_ALL", "LC_CTYPE", "TZ"] {
            if let Some(value) = std::env::var_os(name)
                && value.len() <= 4096
            {
                command.env(name, value);
            }
        }
        #[cfg(windows)]
        for name in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
            .env("TMPDIR", directory)
            .env("TMP", directory)
            .env("TEMP", directory)
            .env("MALLOC_ARENA_MAX", "2");
    }
    /// Attach before any Init containing untrusted content is sent. The private
    /// directory and Windows job survive the Page and all normal reaper handoffs.
    pub fn attach(self, process: &mut OwnedProcess, maximum_commit_bytes: u64) -> io::Result<()> {
        process.retain_until_exit(self.directory);
        #[cfg(windows)]
        {
            let job = windows::attach(process, maximum_commit_bytes)?;
            process.retain_until_exit(job);
        }
        #[cfg(not(windows))]
        let _ = maximum_commit_bytes;
        Ok(())
    }
}

/// The standalone private worker also refuses an implicit unsandboxed launch.
/// Library embeddings have their own host policy; the browser must use browser().
pub fn validate_worker_start(arguments: impl IntoIterator<Item = String>) -> io::Result<()> {
    let opted_in = arguments
        .into_iter()
        .any(|argument| argument == DEVELOPMENT_ARGUMENT);
    match Mode::select(cfg!(debug_assertions), opted_in)? {
        Mode::Development => {
            #[cfg(target_os = "linux")]
            linux::worker_baseline()?;
            Ok(())
        }
        Mode::Required => Err(unavailable(BrokerTransport::InheritedEndpoint)),
    }
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;
