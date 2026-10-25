//! The pinned Deno extensions use PermissionsContainer, rather than permission traits.
//! Map each policy-approved network check to one grant descriptor. Never grant net
//! globally: that fast path skips the descriptor parser and would bypass live policy.
use deno_permissions::*;
use std::{
    borrow::Cow,
    path::Path,
    sync::{Arc, RwLock},
};
#[derive(Debug)]
struct PolicyParser {
    inner: RuntimePermissionDescriptorParser<sys_traits::impls::RealSys>,
    policy: Arc<RwLock<crate::NetworkPolicy>>,
}
const GRANT: &str = "wega-policy-grant.invalid";
impl PolicyParser {
    fn allowed(&self, url: &url::Url) -> bool {
        let mut normalized = url.clone();
        match url.scheme() {
            "ws" => {
                let _ = normalized.set_scheme("http");
            }
            "wss" => {
                let _ = normalized.set_scheme("https");
            }
            _ => {}
        }
        self.policy
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .permits(&normalized)
    }
}
impl PermissionDescriptorParser for PolicyParser {
    fn parse_net_descriptor_from_url(
        &self,
        url: &url::Url,
    ) -> Result<NetDescriptor, NetDescriptorFromUrlParseError> {
        if !self.allowed(url) {
            return Err(NetDescriptorFromUrlParseError::MissingHost(url.clone()));
        }
        Ok(NetDescriptor::parse_for_query(GRANT).expect("static policy grant"))
    }
    fn parse_net_query(&self, text: &str) -> Result<NetDescriptor, NetDescriptorParseError> {
        let url = url::Url::parse(&format!("https://{text}"))
            .map_err(|_| NetDescriptorParseError::InvalidHost(text.into()))?;
        if !self.allowed(&url) {
            return Err(NetDescriptorParseError::InvalidHost(
                "Network request blocked by browsing policy".into(),
            ));
        }
        NetDescriptor::parse_for_query(GRANT)
    }
    fn parse_read_descriptor(&self, t: &str) -> Result<ReadDescriptor, PathResolveError> {
        self.inner.parse_read_descriptor(t)
    }
    fn parse_write_descriptor(&self, t: &str) -> Result<WriteDescriptor, PathResolveError> {
        self.inner.parse_write_descriptor(t)
    }
    fn parse_net_descriptor(&self, t: &str) -> Result<NetDescriptor, NetDescriptorParseError> {
        self.inner.parse_net_descriptor(t)
    }
    fn parse_import_descriptor(
        &self,
        t: &str,
    ) -> Result<ImportDescriptor, NetDescriptorParseError> {
        self.inner.parse_import_descriptor(t)
    }
    fn parse_env_descriptor(&self, t: &str) -> Result<EnvDescriptor, EnvDescriptorParseError> {
        self.inner.parse_env_descriptor(t)
    }
    fn parse_sys_descriptor(&self, t: &str) -> Result<SysDescriptor, SysDescriptorParseError> {
        self.inner.parse_sys_descriptor(t)
    }
    fn parse_allow_run_descriptor(
        &self,
        t: &str,
    ) -> Result<AllowRunDescriptorParseResult, RunDescriptorParseError> {
        self.inner.parse_allow_run_descriptor(t)
    }
    fn parse_deny_run_descriptor(&self, t: &str) -> Result<DenyRunDescriptor, PathResolveError> {
        self.inner.parse_deny_run_descriptor(t)
    }
    fn parse_ffi_descriptor(&self, t: &str) -> Result<FfiDescriptor, PathResolveError> {
        self.inner.parse_ffi_descriptor(t)
    }
    fn parse_path_query<'a>(
        &self,
        p: Cow<'a, Path>,
    ) -> Result<PathQueryDescriptor<'a>, PathResolveError> {
        self.inner.parse_path_query(p)
    }
    fn parse_special_file_descriptor<'a>(
        &self,
        p: PathQueryDescriptor<'a>,
    ) -> Result<SpecialFilePathQueryDescriptor<'a>, PathResolveError> {
        self.inner.parse_special_file_descriptor(p)
    }
    fn parse_run_query<'a>(
        &self,
        t: &'a str,
    ) -> Result<RunQueryDescriptor<'a>, RunDescriptorParseError> {
        self.inner.parse_run_query(t)
    }
}
pub(crate) fn permissions(policy: Arc<RwLock<crate::NetworkPolicy>>) -> PermissionsContainer {
    let parser = PolicyParser {
        inner: RuntimePermissionDescriptorParser::new(sys_traits::impls::RealSys),
        policy,
    };
    let permissions = Permissions::from_options(
        &parser,
        &PermissionsOptions {
            allow_net: Some(vec![GRANT.into()]),
            prompt: false,
            ..Default::default()
        },
    )
    .expect("static network grant");
    PermissionsContainer::new(Arc::new(parser), permissions)
}
pub(crate) fn extensions(dom: &crate::dom::Dom) -> Vec<deno_core::Extension> {
    use sha2::{Digest, Sha256};
    // reqwest and Deno enable different Rustls providers; select one explicitly.
    let _ = deno_tls::rustls::crypto::aws_lc_rs::default_provider().install_default();
    let url = dom.document.borrow().url().clone();
    let directory = if matches!(url.origin(), url::Origin::Tuple(..)) {
        dom.network.storage_dir.as_ref().map(|root| {
            root.join(format!(
                "{:x}",
                Sha256::digest(url.origin().ascii_serialization().as_bytes())
            ))
        })
    } else {
        None
    };
    vec![
        deno_webidl::deno_webidl::init(),
        deno_web::deno_web::init(
            deno_web::BlobStore::default_arc(),
            Some(url),
            false,
            Default::default(),
        ),
        deno_net::deno_net::init(None, None),
        deno_fetch::deno_fetch::init(Default::default()),
        deno_websocket::deno_websocket::init(),
        deno_webstorage::deno_webstorage::init(directory),
        deno_crypto::deno_crypto::init(None),
    ]
}

/// Deno's EventTarget already implements the DOM path, but its embedder has no
/// Document/Window types. Supply HTML's Document -> Window parent rule.
/// Modified by Wega contributors, 2026 (Deno's original MIT source stays intact).
pub(crate) fn adapt_extension(
    name: deno_core::ModuleName,
    source: deno_core::ModuleCodeString,
) -> Result<
    (
        deno_core::ModuleCodeString,
        Option<deno_core::SourceMapData>,
    ),
    deno_error::JsErrorBox,
> {
    if name.as_str() == "ext:deno_web/02_event.js" {
        let original = "return isNode(eventTarget) ? eventTarget.parentNode : null;";
        let text = source.to_string();
        if text.matches(original).count() != 1 {
            return Err(deno_error::JsErrorBox::generic(
                "Pinned Deno event parent adapter no longer matches",
            ));
        }
        let adapted=text.replace("isNode(parent) &&\n        isShadowInclusiveAncestor(getRoot(targetImpl), parent)","(parent === globalThis_ || (isNode(parent) &&\n        isShadowInclusiveAncestor(getRoot(targetImpl), parent)))").replace(original,"return isNode(eventTarget) ? (eventTarget.nodeType === 9 ? eventTarget.defaultView : eventTarget.parentNode) : null;");
        return Ok((
            format!(
                "// Modified by Wega contributors, 2026: HTML Document event parent.\n{adapted}"
            )
            .into(),
            None,
        ));
    }
    if name.as_str() == "ext:deno_fetch/26_fetch.js" {
        let text = source.to_string();
        let before = "  const { requestRid, cancelHandleRid } = op_fetch(";
        let after = "  processUrlList(req.urlList, req.urlListProcessed);\n\n  /** @type {InnerResponse} */";
        if text.matches(before).count() != 1 || text.matches(after).count() != 1 {
            return Err(deno_error::JsErrorBox::generic(
                "Pinned fetch cookie adapter no longer matches",
            ));
        }
        let text=text.replace(before,r#"  // Modified by Wega contributors, 2026: browser cookies at every redirect hop.
  req.headerList = req.headerList.filter(([name]) => name.toLowerCase() !== "cookie");
  const cookie = core.ops.op_wega_cookie_header(req.currentUrl(), req.credentialsMode ?? "same-origin");
  if (cookie) req.headerList.push(["cookie",cookie]);
  const { requestRid, cancelHandleRid } = op_fetch("#).replace(after,r#"  for (const [name,value] of resp.headers) {
    if (name.toLowerCase() === "set-cookie") core.ops.op_wega_cookie_response(req.currentUrl(),req.credentialsMode ?? "same-origin",value);
  }
  resp.headers = resp.headers.filter(([name]) => name.toLowerCase() !== "set-cookie" && name.toLowerCase() !== "set-cookie2");
  processUrlList(req.urlList, req.urlListProcessed);

  /** @type {InnerResponse} */"#);
        return Ok((text.into(), None));
    }
    Ok((source, None))
}
