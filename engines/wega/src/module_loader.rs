//! Module graphs use the same bounded network queue and live policy as page resources.
use crate::network::Network;
use deno_core::error::ModuleLoaderError;
use deno_core::{
    ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader, ModuleSource,
    ModuleSourceCode, ModuleSpecifier, ModuleType, ResolutionKind,
};
use deno_error::JsErrorBox;
use std::path::PathBuf;
pub(crate) struct Loader {
    network: Network,
    local: Option<PathBuf>,
    limit: usize,
}
impl Loader {
    pub fn new(network: Network, url: &url::Url, limit: usize) -> Self {
        let local = if url.scheme() == "file" {
            url.to_file_path()
                .ok()
                .and_then(|path| path.canonicalize().ok())
                .and_then(|path| path.parent().map(PathBuf::from))
        } else {
            None
        };
        Self {
            network,
            local,
            limit,
        }
    }
}
impl ModuleLoader for Loader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> Result<ModuleSpecifier, ModuleLoaderError> {
        let url = deno_core::resolve_import(specifier, referrer)
            .map_err(|e| JsErrorBox::generic(e.to_string()))?;
        if !matches!(url.scheme(), "http" | "https")
            && !(url.scheme() == "file" && self.local.is_some())
        {
            return Err(JsErrorBox::generic("Module scheme is not allowed"));
        }
        Ok(url)
    }
    fn load(
        &self,
        specifier: &ModuleSpecifier,
        _referrer: Option<&ModuleLoadReferrer>,
        _options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        let url = specifier.clone();
        let local = self.local.clone();
        let policy = self.network.policy.clone();
        let limit = self.limit;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let spawned = self.network.spawn(move |client| {
            let policy = policy
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let result = Network::fetch(client, &policy, &url, limit, local.as_deref());
            let _ = sender.send(result);
        });
        let specifier = specifier.clone();
        ModuleLoadResponse::Async(Box::pin(async move {
            spawned.map_err(JsErrorBox::generic)?;
            let (resolved, bytes) = receiver
                .await
                .map_err(|e| JsErrorBox::generic(e.to_string()))?
                .map_err(JsErrorBox::generic)?;
            let code = String::from_utf8(bytes).map_err(|e| JsErrorBox::generic(e.to_string()))?;
            Ok(ModuleSource::new_with_redirect(
                ModuleType::JavaScript,
                ModuleSourceCode::String(code.into()),
                &specifier,
                &resolved,
                None,
            ))
        }))
    }
}
