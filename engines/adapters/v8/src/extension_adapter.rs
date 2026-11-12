/// Deno's EventTarget already implements the DOM path, but its embedder has no
/// Document/Window types. Supply HTML's Document -> Window parent rule.
/// Modified by Wega contributors, 2026 (Deno's original MIT source stays intact).
pub fn adapt_extension(
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
        let mut adapted = source.to_string();
        // Refuse a dependency update that changes any pinned semantic hook.
        // The active Deno dispatcher owns retargeting and closed composed paths;
        // Rust supplies its native parent, root and slot relationships.
        for (original, replacement) in [
            (
                "function getParent(eventTarget) {\n  return isNode(eventTarget) ? eventTarget.parentNode : null;\n}",
                "let wegaDomHooks;\nlet wegaCurrentEvent;\nlet wegaListenerPhase = \"bubbling\";\nfunction getWegaCurrentEvent() { return wegaCurrentEvent; }\nfunction setWegaDomHooks(hooks) { if (wegaDomHooks) throw new TypeError(\"DOM hooks already installed\"); wegaDomHooks = hooks; }\nfunction getParent(eventTarget, event = null, originalRoot = null) {\n  return isNode(eventTarget) ? wegaDomHooks.parent(eventTarget, event, originalRoot) : null;\n}",
            ),
            (
                "    if (data === undefined || !data.listeners[event.type]) {",
                "    if (data === undefined || (!data.listeners[event.type] && !isNode(self))) {",
            ),
            (
                "isNode(parent) &&\n        isShadowInclusiveAncestor(getRoot(targetImpl), parent)",
                "(parent === globalThis_ || (isNode(parent) &&\n        isShadowInclusiveAncestor(getRoot(targetImpl), parent)))",
            ),
            (
                "eventTarget.getRootNode({ composed: true })",
                "eventTarget.getRootNode()",
            ),
            (
                "let parent = getParent(targetImpl);",
                "let parent = getParent(targetImpl, eventImpl, getRoot(targetOverride));",
            ),
            (
                "parent = getParent(parent);",
                "parent = getParent(parent, eventImpl, getRoot(targetOverride));",
            ),
            (
                "return target?.[eventTargetData]?.host ?? null;",
                "return wegaDomHooks?.host(target) ?? null;",
            ),
            (
                "return target?.[eventTargetData]?.mode ?? null;",
                "return wegaDomHooks?.mode(target) ?? null;",
            ),
            (
                "return Boolean(target?.[eventTargetData]?.assignedSlot);",
                "return wegaDomHooks?.slot(target) ?? null;",
            ),
            (
                "  setTarget,\n};",
                "  setTarget,\n  setWegaDomHooks,\n  getWegaCurrentEvent,\n};",
            ),
            // Dispatch end clears the flag directly; the spec setter ignores false.
            (
                "  eventImpl.cancelBubble = false;\n  setStopImmediatePropagation(eventImpl, false);",
                "  eventImpl[_stopPropagationFlag] = false;\n  setStopImmediatePropagation(eventImpl, false);",
            ),
            // DOM dispatch: capture listeners at the target run in the capture pass.
            (
                "    for (let i = path.length - 1; i >= 0; --i) {\n      const tuple = path[i];\n\n      if (tuple.target === null) {\n        invokeEventListeners(tuple, eventImpl);\n      }\n    }",
                "    wegaListenerPhase = \"capturing\";\n    for (let i = path.length - 1; i >= 0; --i) {\n      const tuple = path[i];\n\n      setEventPhase(eventImpl, tuple.target !== null ? Event.AT_TARGET : Event.CAPTURING_PHASE);\n      invokeEventListeners(tuple, eventImpl);\n    }",
            ),
            (
                "    for (let i = 0; i < path.length; i++) {\n      const tuple = path[i];\n\n      if (tuple.target !== null) {",
                "    wegaListenerPhase = \"bubbling\";\n    for (let i = 0; i < path.length; i++) {\n      const tuple = path[i];\n\n      if (tuple.target !== null) {",
            ),
            (
                "    if (\n      (eventImpl.eventPhase === Event.CAPTURING_PHASE && !capture) ||\n      (eventImpl.eventPhase === Event.BUBBLING_PHASE && capture)\n    ) {",
                "    if (\n      (wegaListenerPhase === \"capturing\" && !capture) ||\n      (wegaListenerPhase === \"bubbling\" && capture)\n    ) {",
            ),
            // DOM: setting cancelBubble to false has no effect.
            (
                "    this[_stopPropagationFlag] = webidl.converters.boolean(value);",
                "    if (webidl.converters.boolean(value)) this[_stopPropagationFlag] = true;",
            ),
            // HTML window.event: the global's current event during listener
            // invocation, undefined for listeners inside a shadow tree.
            (
                "    try {\n      if (typeof listener.callback === \"object\") {",
                "    const wegaPreviousEvent = wegaCurrentEvent;\n    wegaCurrentEvent = isShadowRoot(getRoot(eventImpl.currentTarget)) ? undefined : eventImpl;\n    try {\n      if (typeof listener.callback === \"object\") {",
            ),
            (
                "    } catch (error) {\n      reportException(error);\n    }\n\n    setInPassiveListener(eventImpl, false);",
                "    } catch (error) {\n      reportException(error);\n    } finally {\n      wegaCurrentEvent = wegaPreviousEvent;\n    }\n\n    setInPassiveListener(eventImpl, false);",
            ),
            // Legacy DOM initEvent; the generated Event binding requires it.
            (
                "  [SymbolFor(\"Deno.privateCustomInspect\")](inspect, inspectOptions) {\n    return inspect(\n      getCreateFilteredInspectProxy()({\n        object: this,\n        evaluate: ObjectPrototypeIsPrototypeOf(EventPrototype, this),\n        keys: EVENT_PROPS,",
                "  initEvent(type, bubbles = false, cancelable = false) {\n    if (this[_dispatched]) return;\n    this[_stopPropagationFlag] = false;\n    this[_stopImmediatePropagationFlag] = false;\n    this[_canceledFlag] = false;\n    this[_isTrusted] = false;\n    this[_attributes].target = null;\n    this[_attributes].type = String(type);\n    this[_attributes].bubbles = !!bubbles;\n    this[_attributes].cancelable = !!cancelable;\n  }\n\n  [SymbolFor(\"Deno.privateCustomInspect\")](inspect, inspectOptions) {\n    return inspect(\n      getCreateFilteredInspectProxy()({\n        object: this,\n        evaluate: ObjectPrototypeIsPrototypeOf(EventPrototype, this),\n        keys: EVENT_PROPS,",
            ),
            (
                "function isSlottable(\n  /* nodeImpl, */\n) {\n  // TODO(marcosc90) currently there aren't any slottables nodes\n  // https://dom.spec.whatwg.org/#concept-slotable\n  // return isNode(nodeImpl) && ReflectHas(nodeImpl, \"assignedSlot\");\n  return false;\n}",
                "function isSlottable(nodeImpl) {\n  return isNode(nodeImpl) && (nodeImpl.nodeType === 1 || nodeImpl.nodeType === 3);\n}",
            ),
        ] {
            if adapted.matches(original).count() != 1 {
                return Err(deno_error::JsErrorBox::generic(format!(
                    "Pinned Deno event adapter no longer matches: {original}"
                )));
            }
            adapted = adapted.replace(original, replacement);
        }
        return Ok((
            format!(
                "// Modified by Wega contributors, 2026: native DOM event parent and shadow boundaries.\n{adapted}"
            )
            .into(),
            None,
        ));
    }
    if name.as_str() == "ext:deno_web/13_message_port.js" {
        let text = source.to_string();
        let original = "  const messageData = serializeJsMessageData(value, options.transfer);\n  return deserializeJsMessageData(messageData)[0];";
        if text.matches(original).count() != 1 {
            return Err(deno_error::JsErrorBox::generic(
                "Pinned structured-clone transfer adapter no longer matches",
            ));
        }
        return Ok((text.replace(original, "  try {\n    const messageData = serializeJsMessageData(value, options.transfer);\n    return deserializeJsMessageData(messageData)[0];\n  } catch (error) {\n    if (ObjectPrototypeIsPrototypeOf(TypeErrorPrototype, error)) throw new DOMException(error.message, \"DataCloneError\");\n    throw error;\n  }").into(),None));
    }
    if name.as_str() == "ext:deno_fetch/26_fetch.js" {
        let text = source.to_string();
        let transport = "  const { requestRid, cancelHandleRid } = op_fetch(\n    req.method,\n    req.currentUrl(),\n    req.headerList,\n    req.clientRid,\n    reqBody !== null || reqRid !== null,\n    reqBody,\n    reqRid,\n  );";
        if text.matches(transport).count() != 1 {
            return Err(deno_error::JsErrorBox::generic(
                "Pinned broker fetch transport no longer matches",
            ));
        }
        let text = text.replace(transport, "  const { requestRid, cancelHandleRid } = op_fetch(\n    req.method, req.currentUrl(), req.headerList, req.clientRid, reqBody !== null || reqRid !== null, reqBody, reqRid, wegaInclude, req.cacheMode ?? \"default\", req.mode ?? \"cors\",\n  );");
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
        let manual = "      case \"manual\":\n        break;";
        if text.matches(manual).count() != 1 {
            return Err(deno_error::JsErrorBox::generic(
                "Pinned manual redirect adapter no longer matches",
            ));
        }
        let text = text.replace(manual, r#"      case "manual":
        core.close(resp.responseRid);
        return { headerList: [], status: 0, statusMessage: "", type: "opaqueredirect", body: null, urlList: [], url() { return null; } };"#);
        let text = text.replace("= op_fetch(", "= wegaBrokerFetch(").replace(
            "resp = await opFetchSend(requestRid);",
            "resp = await wegaAwaitBroker(requestRid, terminator);",
        );
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
    const failure = await wegaPreflight(req, wegaUrl, wegaOrigin, wegaInclude, terminator);
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

const FETCH_HELPERS: &str = r#"
async function wegaAwaitBroker(promise, terminator) {
  let abort;
  const cancelled = new Promise((_, reject) => {
    abort = () => reject(terminator.reason);
    terminator[abortSignal.add](abort);
    if (terminator.aborted) abort();
  });
  promise.then(result => { if (terminator.aborted) core.tryClose(result.responseRid); }, () => {});
  try { return await Promise.race([promise, cancelled]); }
  finally { terminator[abortSignal.remove](abort); }
}

function wegaBrokerFetch(method, address, headerList, clientRid, hasBody, bytes, bodyRid, include = false, cache = "default", mode = "cors") {
  const requestRid = (async () => {
    const limit = core.ops.op_dom_read("limits", "", "", "").resourceBytes;
    if (bodyRid !== null) {
      const chunks = []; let length = 0;
      try {
        while (true) {
          const chunk = new Uint8Array(Math.min(65536, limit + 1 - length));
          const count = await core.read(bodyRid, chunk);
          if (!count) break;
          length += count;
          if (length > limit) throw new TypeError("Upload exceeds its size limit");
          chunks.push(chunk.subarray(0, count));
        }
      } finally { core.tryClose(bodyRid); }
      bytes = new Uint8Array(length); let offset = 0;
      for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
    }
    bytes ??= new Uint8Array();
    if (bytes.length > limit) throw new TypeError("Upload exceeds its size limit");
    return await core.ops.op_wega_fetch({url:address, method, headers:headerList, origin:"", credentials: include ? "include" : "omit", redirect:"manual", cache, mode, limit}, bytes);
  })();
  return {requestRid, cancelHandleRid:null};
}
// Wega contributors, 2026: CORS helpers for the adapter above (Fetch Standard section 4.9).
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
async function wegaPreflight(req, url, origin, include, terminator) {
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
