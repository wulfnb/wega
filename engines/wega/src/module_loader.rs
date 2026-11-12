//! Module graphs use the same bounded network queue and live policy as page resources.
use crate::script_net::ScriptNetwork;
use deno_core::error::ModuleLoaderError;
use deno_core::{
    ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader, ModuleSource,
    ModuleSourceCode, ModuleSpecifier, ModuleType, ResolutionKind,
};
use deno_error::JsErrorBox;
use std::{path::PathBuf, sync::Arc};
pub(crate) struct Loader {
    network: Arc<dyn ScriptNetwork>,
    local: Option<PathBuf>,
    limit: usize,
    document: url::Url,
}
impl Loader {
    pub fn new(network: Arc<dyn ScriptNetwork>, url: &url::Url, limit: usize) -> Self {
        let local = if url.scheme() == "file" && network.remote().is_none() {
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
            document: url.clone(),
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
            && !(url.scheme() == "file"
                && (self.local.is_some()
                    || (self.document.scheme() == "file" && self.network.remote().is_some())))
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
        let limit = self.limit;
        let fetched = self.network.fetch_module(&self.document, url, limit, local);
        let specifier = specifier.clone();
        ModuleLoadResponse::Async(Box::pin(async move {
            let (resolved, bytes) = fetched.await.map_err(JsErrorBox::generic)?;
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
