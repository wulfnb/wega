//! Startup validation runs before any profile or engine side effects.
use std::path::PathBuf;
#[derive(Debug, Default)]
pub struct Options {
    pub help: bool,
    pub profile_dir: Option<PathBuf>,
    pub container: Option<String>,
    /// Set by prepare_container before engine startup; never accepted from a URL.
    pub container_root: Option<PathBuf>,
    pub url: Option<String>,
    pub engine: Option<crate::engine_choice::EngineKind>,
    pub engine_test: bool,
    pub smoke_test: bool,
    pub download_test: bool,
    pub toolbox_test: bool,
    pub theme_test: bool,
    pub capabilities_test: bool,
    pub capture: Option<PathBuf>,
    pub remote_devtools: Option<u16>,
    pub webdriver_port: Option<u16>,
    pub multiprocess: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct BuildFeatures {
    pub multiprocess: bool,
    pub test_harness: bool,
}

impl Options {
    pub fn test_mode(&self) -> bool {
        self.smoke_test
            || self.download_test
            || self.toolbox_test
            || self.theme_test
            || self.capabilities_test
            || self.engine_test
    }

    pub fn parse_from(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        Self::parse_with(
            args,
            BuildFeatures {
                multiprocess: cfg!(feature = "multiprocess"),
                test_harness: cfg!(feature = "test-harness"),
            },
        )
    }

    pub(crate) fn parse_with(
        args: impl IntoIterator<Item = String>,
        features: BuildFeatures,
    ) -> Result<Self, String> {
        Self::parse_tokens(args.into_iter().collect(), features)
    }

    // Keep the parser concrete: callers can supply any iterator without creating
    // a separate copy of the validation state machine for every iterator type.
    fn parse_tokens(args: Vec<String>, features: BuildFeatures) -> Result<Self, String> {
        let mut result = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--container" => {
                    let id = args.next().ok_or("--container requires an ID")?;
                    if !crate::containers::valid_id(&id) {
                        return Err("Invalid container ID".into());
                    }
                    result.container = Some(id);
                }
                "--profile-dir" => {
                    result.profile_dir =
                        Some(args.next().ok_or("--profile-dir requires a path")?.into())
                }
                "--multiprocess" => {
                    if !features.multiprocess {
                        return Err("Rebuild with --features multiprocess first".into());
                    }
                    result.multiprocess = true;
                }
                "--engine" => {
                    result.engine = Some(crate::engine_choice::EngineKind::parse(
                        &args.next().ok_or("--engine requires servo or wega")?,
                    )?);
                }
                "--webdriver-port" => {
                    result.webdriver_port = Some(
                        args.next()
                            .ok_or("--webdriver-port requires a port")?
                            .parse()
                            .map_err(|_| "Invalid WebDriver port")?,
                    );
                }
                "--remote-devtools" => {
                    result.remote_devtools = Some(
                        args.next()
                            .ok_or("--remote-devtools requires a loopback port")?
                            .parse()
                            .map_err(|_| "Invalid DevTools port")?,
                    );
                }
                "--smoke-test" => result.smoke_test = true,
                "--download-test" => result.download_test = true,
                "--toolbox-test" => result.toolbox_test = true,
                "--capabilities-test" => result.capabilities_test = true,
                "--theme-test" => result.theme_test = true,
                "--engine-test" => result.engine_test = true,
                "--capture" => {
                    result.capture =
                        Some(args.next().ok_or("--capture requires a PNG path")?.into())
                }
                "--help" | "-h" => {
                    result.help = true;
                }
                "--" => {
                    for remainder in args {
                        if result.url.is_none() {
                            result.url = Some(remainder);
                        } else {
                            return Err("Only one startup URL is accepted".into());
                        }
                    }
                    break;
                }
                _ if arg.starts_with('-') => return Err(format!("Unknown option: {arg}")),
                _ if result.url.is_none() => result.url = Some(arg),
                _ => return Err("Only one startup URL is accepted".into()),
            }
        }
        if result
            .profile_dir
            .as_ref()
            .is_some_and(|path| path.as_os_str().is_empty())
        {
            return Err("--profile-dir requires a nonempty path".into());
        }
        if result.help {
            return Ok(result);
        }
        if (result.engine_test
            || result.smoke_test
            || result.download_test
            || result.toolbox_test
            || result.theme_test
            || result.capabilities_test)
            && result.profile_dir.is_none()
        {
            return Err("Integration tests require an isolated --profile-dir".into());
        }
        if result.test_mode() && !features.test_harness {
            return Err("Native test modes require --features test-harness".into());
        }
        if [
            result.smoke_test,
            result.download_test,
            result.toolbox_test,
            result.theme_test,
            result.capabilities_test,
            result.engine_test,
        ]
        .into_iter()
        .filter(|enabled| *enabled)
        .count()
            > 1
        {
            return Err("Choose only one native test mode".into());
        }
        if result.engine == Some(crate::engine_choice::EngineKind::Wega)
            && (result.remote_devtools.is_some()
                || result.multiprocess
                || (result.test_mode() && !result.engine_test))
        {
            return Err("These engine diagnostics require Servo; omit --engine wega".into());
        }
        Ok(result)
    }

    /// Resolve isolation before media registries, instance handoff or Servo can
    /// touch a profile. Unknown/corrupt identities never fall back to default.
    pub fn prepare_container(&mut self) -> Result<(), String> {
        let Some(id) = &self.container else {
            return Ok(());
        };
        if self.container_root.is_some() {
            return Ok(());
        }
        let root = self
            .profile_dir
            .clone()
            .unwrap_or_else(crate::profile::Profile::default_dir);
        crate::containers::Registry::load(&root)?.find(id)?;
        let directory = crate::containers::profile_directory(&root, id)?;
        self.container_root = Some(std::fs::canonicalize(root).map_err(|e| e.to_string())?);
        self.profile_dir = Some(directory);
        Ok(())
    }
}

/// Only declared web/document schemes may arrive from an OS open event.
/// This does not expose internal pages or command-line options to other apps.
pub fn external_launch_urls(addresses: &[String]) -> Vec<url::Url> {
    addresses
        .iter()
        .filter(|address| address.len() <= 8192)
        .filter_map(|address| url::Url::parse(address).ok())
        .filter(|url| matches!(url.scheme(), "http" | "https" | "file"))
        .take(32)
        .collect()
}

#[cfg(test)]
#[path = "options_tests.rs"]
mod tests;
