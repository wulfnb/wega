//! Profile cookie access is local for library users and brokered in tab workers.
use std::sync::Arc;
use url::Url;
pub(crate) enum Cookies {
    Local(Arc<wega_network::Cookies>),
    Remote(wega_network::Remote),
}
impl Cookies {
    pub fn set_directory(&self, directory: std::path::PathBuf) {
        if let Self::Local(jar) = self {
            jar.set_directory(directory);
        }
    }
    pub fn get(&self, url: &Url, script: bool) -> String {
        match self {
            Self::Local(jar) => jar.get(url, script),
            Self::Remote(remote) => remote.cookie_get(url, script).unwrap_or_default(),
        }
    }
    pub fn set(&self, url: &Url, value: &str, script: bool) {
        match self {
            Self::Local(jar) => jar.set(url, value, script),
            Self::Remote(remote) => {
                let _ = remote.cookie_set(url, value, script);
            }
        }
    }
}
