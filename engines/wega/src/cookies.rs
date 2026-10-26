//! Profile cookie access is local for library users and brokered in tab workers.
use std::sync::Arc;
use url::Url;
pub(crate) use wega_network::SiteContext;
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
    pub fn context(&self, url: &Url, initiator: Option<&Url>, top_level_safe: bool) -> SiteContext {
        wega_network::Cookies::context(url, initiator, top_level_safe)
    }
    pub fn netscape(&self, url: &Url) -> String {
        match self {
            Self::Local(jar) => jar.netscape(url),
            Self::Remote(remote) => remote
                .cookie_read(url, false, SiteContext::SameSite, true)
                .unwrap_or_default(),
        }
    }
    pub fn get(&self, url: &Url, script: bool, context: SiteContext) -> String {
        match self {
            Self::Local(jar) => jar.get(url, script, context),
            Self::Remote(remote) => remote
                .cookie_read(url, script, context, false)
                .unwrap_or_default(),
        }
    }
    pub fn set(&self, url: &Url, value: &str, script: bool, context: SiteContext) {
        match self {
            Self::Local(jar) => jar.set(url, value, script, context),
            Self::Remote(remote) => {
                let _ = remote.cookie_write(url, value, script, context);
            }
        }
    }
}
