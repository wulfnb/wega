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
        // Deno skips dispatch when the target itself has no listener; in a DOM the
        // event still travels to ancestors (event delegation).
        let shortcut = "    if (data === undefined || !data.listeners[event.type]) {";
        if text.matches(shortcut).count() != 1 {
            return Err(deno_error::JsErrorBox::generic(
                "Pinned Deno dispatch shortcut adapter no longer matches",
            ));
        }
        let adapted=text.replace("isNode(parent) &&\n        isShadowInclusiveAncestor(getRoot(targetImpl), parent)","(parent === globalThis_ || (isNode(parent) &&\n        isShadowInclusiveAncestor(getRoot(targetImpl), parent)))").replace(original,"return isNode(eventTarget) ? (eventTarget.nodeType === 9 ? eventTarget.defaultView : eventTarget.parentNode) : null;").replace(shortcut, "    if (data === undefined || (!data.listeners[event.type] && !isNode(self))) {");
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
        let kind = "    statusMessage: resp.statusText,\n    type: \"basic\",";
        let exports = "return { fetch, handleWasmStreaming, mainFetch };";
        if text.matches(before).count() != 1
            || text.matches(after).count() != 1
            || text.matches(kind).count() != 1
            || text.matches(exports).count() != 1
        {
            return Err(deno_error::JsErrorBox::generic(
                "Pinned fetch security adapter no longer matches",
            ));
        }
        let text = text
            .replace(before, FETCH_REQUEST_HOP)
            .replace(after, FETCH_RESPONSE_HOP)
            .replace(
                kind,
                "    statusMessage: resp.statusText,\n    type: wegaCross ? \"cors\" : \"basic\",",
            )
            .replace(exports, &format!("{FETCH_HELPERS}\n{exports}"));
        return Ok((text.into(), None));
    }
    if name.as_str() == "ext:deno_websocket/01_websocket.js" {
        let text = source.to_string();
        let check = "    const cancelRid = op_ws_check_permission_and_cancel_handle(";
        let headers = "        headers ? headerListFromHeaders(headers) : null,\n        caCerts,";
        if text.matches(check).count() != 1 || text.matches(headers).count() != 1 {
            return Err(deno_error::JsErrorBox::generic(
                "Pinned WebSocket security adapter no longer matches",
            ));
        }
        let text = text
            .replace(
                check,
                "    // Modified by Wega contributors, 2026: CSP connect-src and mixed content.\n    {\n      const blocked = core.ops.op_wega_connect_check(wsURL.href);\n      if (blocked) throw new DOMException(blocked, \"SecurityError\");\n    }\n    const cancelRid = op_ws_check_permission_and_cancel_handle(",
            )
            .replace(
                headers,
                "        // Modified by Wega contributors, 2026: Origin and SameSite cookies.\n        [\n          ...(headers ? headerListFromHeaders(headers) : []).filter(([name]) => !/^(origin|cookie)$/i.test(name)),\n          ...core.ops.op_wega_ws_headers(wsURL.href),\n        ],\n        caCerts,",
            );
        return Ok((text.into(), None));
    }
    Ok((source, None))
}

/// Before each fetch hop: connect-src and mixed content, request modes, Origin,
/// Referer, preflight, and cookies only when credentials apply.
const FETCH_REQUEST_HOP: &str = r#"  // Modified by Wega contributors, 2026: browser security at every fetch hop.
  const wegaUrl = req.currentUrl();
  const wegaBlocked = core.ops.op_wega_connect_check(wegaUrl);
  if (wegaBlocked) return networkError(wegaBlocked);
  const wegaOrigin = core.ops.op_wega_document_origin();
  if (new URL(wegaUrl).origin !== wegaOrigin) req.wegaCrossOrigin = true;
  const wegaCross = req.wegaCrossOrigin === true;
  const wegaMode = req.mode ?? "cors";
  const wegaCredentials = req.credentialsMode ?? "same-origin";
  if (wegaCross && wegaMode === "same-origin") {
    return networkError("Cross-origin request blocked: request mode is 'same-origin'");
  }
  if (wegaCross && wegaMode === "no-cors" && !wegaSafeMethod(req.method)) {
    return networkError("Only GET, HEAD and POST are allowed in 'no-cors' mode");
  }
  const wegaInclude = wegaCredentials === "include" ||
    (wegaCredentials === "same-origin" && !wegaCross);
  req.headerList = req.headerList.filter(([name]) =>
    !/^(cookie|cookie2|origin|referer)$/i.test(name)
  );
  if ((wegaCross && wegaMode === "cors") || !/^(GET|HEAD)$/i.test(req.method)) {
    req.headerList.push(["origin", wegaOrigin]);
  }
  const wegaReferrer = core.ops.op_wega_referrer(wegaUrl);
  if (wegaReferrer) req.headerList.push(["referer", wegaReferrer]);
  if (wegaCross && wegaMode === "cors" && wegaNeedsPreflight(req)) {
    const failure = await wegaPreflight(req, wegaUrl, wegaOrigin, wegaInclude);
    if (failure) return networkError("CORS preflight failed: " + failure);
  }
  if (wegaInclude) {
    const cookie = core.ops.op_wega_cookie_header(wegaUrl, true);
    if (cookie) req.headerList.push(["cookie", cookie]);
  }
  const { requestRid, cancelHandleRid } = op_fetch("#;

/// After each fetch hop: store cookies, check CORS, and filter what script sees.
const FETCH_RESPONSE_HOP: &str = r#"  // Modified by Wega contributors, 2026: cookies, CORS check and response tainting.
  for (const [name, value] of resp.headers) {
    if (byteLowerCase(name) === "set-cookie") {
      core.ops.op_wega_cookie_response(req.currentUrl(), wegaInclude, value);
    }
  }
  resp.headers = resp.headers.filter(([name]) => !/^set-cookie2?$/i.test(name));
  if (wegaCross && wegaMode === "cors") {
    const failure = wegaCorsCheck(resp.headers, wegaOrigin, wegaInclude);
    if (failure) {
      core.tryClose(resp.responseRid);
      return networkError("Cross-origin request blocked by CORS: " + failure);
    }
  }
  if (wegaCross && wegaMode === "no-cors" && !redirectStatus(resp.status)) {
    // An opaque response: script sees no status, headers or body.
    core.tryClose(resp.responseRid);
    processUrlList(req.urlList, req.urlListProcessed);
    return {
      headerList: [],
      status: 0,
      body: null,
      statusMessage: "",
      type: "opaque",
      url() {
        return null;
      },
      urlList: [],
    };
  }
  if (wegaCross && !redirectStatus(resp.status)) {
    resp.headers = wegaExposedHeaders(resp.headers, wegaInclude);
  }
  processUrlList(req.urlList, req.urlListProcessed);

  /** @type {InnerResponse} */"#;

const FETCH_HELPERS: &str = r#"// Wega contributors, 2026: CORS helpers for the adapter above (Fetch Standard §4.9).
function wegaSafeMethod(method) {
  return /^(GET|HEAD|POST)$/i.test(method);
}
function wegaSafelistedHeader(name, value) {
  const lower = name.toLowerCase();
  if (/^(accept|accept-language|content-language)$/.test(lower)) {
    return value.length <= 128 && !/[\x00-\x08\x0A-\x1F"():<>?@[\\\]{}\x7F]/.test(value);
  }
  if (lower === "content-type") {
    const essence = value.split(";")[0].trim().toLowerCase();
    return value.length <= 128 &&
      ["application/x-www-form-urlencoded", "multipart/form-data", "text/plain"].includes(essence);
  }
  return /^(range)$/.test(lower) && /^bytes=\d+-\d*$/.test(value);
}
function wegaUnsafeHeaderNames(req) {
  const names = [];
  for (const [name, value] of req.headerList) {
    const lower = name.toLowerCase();
    if (/^(origin|referer|cookie|cookie2|user-agent|accept-encoding|host|connection|content-length)$/.test(lower)) continue;
    if (!wegaSafelistedHeader(lower, value) && !names.includes(lower)) names.push(lower);
  }
  return names.sort();
}
function wegaNeedsPreflight(req) {
  return !wegaSafeMethod(req.method) || wegaUnsafeHeaderNames(req).length > 0;
}
function wegaHeader(list, name) {
  const found = list.find(([key]) => key.toLowerCase() === name);
  return found ? String(found[1]).trim() : null;
}
function wegaTokens(value) {
  return value ? value.split(",").map((token) => token.trim().toLowerCase()).filter(Boolean) : [];
}
function wegaCorsCheck(headers, origin, include) {
  const allowed = wegaHeader(headers, "access-control-allow-origin");
  if (allowed === null) return "no Access-Control-Allow-Origin header";
  if (allowed === "*" && !include) return null;
  if (allowed === "*") return "a wildcard origin is not allowed with credentials";
  if (allowed !== origin) return `Access-Control-Allow-Origin ${allowed} does not match ${origin}`;
  if (include && wegaHeader(headers, "access-control-allow-credentials") !== "true") {
    return "Access-Control-Allow-Credentials is not true";
  }
  return null;
}
async function wegaPreflight(req, url, origin, include) {
  const names = wegaUnsafeHeaderNames(req);
  const headerList = [
    ["origin", origin],
    ["access-control-request-method", req.method],
  ];
  if (names.length) headerList.push(["access-control-request-headers", names.join(",")]);
  const referrer = core.ops.op_wega_referrer(url);
  if (referrer) headerList.push(["referer", referrer]);
  const { requestRid, cancelHandleRid } = op_fetch(
    "OPTIONS",
    url,
    headerList,
    req.clientRid,
    false,
    null,
    null,
  );
  let resp;
  try {
    resp = await opFetchSend(requestRid);
  } finally {
    if (cancelHandleRid !== null) core.tryClose(cancelHandleRid);
  }
  if (resp.error !== null) return resp.error;
  core.tryClose(resp.responseRid);
  if (resp.status < 200 || resp.status > 299) return `status ${resp.status}`;
  const failure = wegaCorsCheck(resp.headers, origin, include);
  if (failure) return failure;
  const methods = wegaTokens(wegaHeader(resp.headers, "access-control-allow-methods"));
  const method = req.method.toLowerCase();
  if (!wegaSafeMethod(req.method) && !methods.includes(method) && !(methods.includes("*") && !include)) {
    return `method ${req.method} is not allowed`;
  }
  const allowedHeaders = wegaTokens(wegaHeader(resp.headers, "access-control-allow-headers"));
  for (const name of names) {
    if (!allowedHeaders.includes(name) && !(allowedHeaders.includes("*") && !include && name !== "authorization")) {
      return `header ${name} is not allowed`;
    }
  }
  return null;
}
function wegaExposedHeaders(headers, include) {
  const exposed = wegaTokens(wegaHeader(headers, "access-control-expose-headers"));
  const all = exposed.includes("*") && !include;
  return headers.filter(([name]) => {
    const lower = name.toLowerCase();
    return all ||
      /^(cache-control|content-language|content-length|content-type|expires|last-modified|pragma)$/.test(lower) ||
      exposed.includes(lower);
  });
}
"#;
