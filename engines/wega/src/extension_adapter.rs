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
