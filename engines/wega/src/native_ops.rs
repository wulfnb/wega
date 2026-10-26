//! The same op table is registered in the build snapshot and every restored runtime.
use deno_core::{JsBuffer, OpState, op2};
use deno_error::JsErrorBox;
use serde_json::{Value, json};
use std::rc::Rc;
pub(crate) trait NativeHost {
    fn fetch(
        &self,
        request: wega_network::Request,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<wega_network::Response, String>>>>;
    fn call(&self, args: &[Value]) -> Result<Value, String>;
    fn cookie_header(&self, address: &str, mode: &str) -> String;
    fn cookie_response(&self, address: &str, mode: &str, value: &str);
}
#[op2]
#[serde]
fn op_dom_read(
    state: &mut OpState,
    #[string] op: &str,
    #[string] a: &str,
    #[string] b: &str,
    #[string] c: &str,
) -> Result<serde_json::Value, JsErrorBox> {
    state
        .borrow::<Rc<dyn NativeHost>>()
        .call(&[json!(op), json!(a), json!(b), json!(c)])
        .map_err(JsErrorBox::generic)
}
#[op2(fast)]
fn op_dom_set_text(
    state: &mut OpState,
    #[string] id: &str,
    #[string] text: &str,
) -> Result<(), JsErrorBox> {
    state
        .borrow::<Rc<dyn NativeHost>>()
        .call(&[json!("setText"), json!(id), json!(text)])
        .map(|_| ())
        .map_err(JsErrorBox::generic)
}
#[op2(fast)]
fn op_dom_set_attr(
    state: &mut OpState,
    #[string] id: &str,
    #[string] name: &str,
    #[string] value: &str,
) -> Result<(), JsErrorBox> {
    state
        .borrow::<Rc<dyn NativeHost>>()
        .call(&[json!("setAttr"), json!(id), json!(name), json!(value)])
        .map(|_| ())
        .map_err(JsErrorBox::generic)
}
#[op2(fast)]
fn op_dom_set_attr_ns(
    state: &mut OpState,
    #[string] id: &str,
    #[string] namespace: &str,
    #[string] name: &str,
    #[string] value: &str,
) -> Result<(), JsErrorBox> {
    state
        .borrow::<Rc<dyn NativeHost>>()
        .call(&[
            json!("setAttrNS"),
            json!(id),
            json!(namespace),
            json!(name),
            json!(value),
        ])
        .map(|_| ())
        .map_err(JsErrorBox::generic)
}
#[op2]
#[string]
fn op_wega_cookie_header(
    state: &mut OpState,
    #[string] address: &str,
    #[string] mode: &str,
) -> String {
    state
        .borrow::<Rc<dyn NativeHost>>()
        .cookie_header(address, mode)
}
#[op2(fast)]
fn op_wega_cookie_response(
    state: &mut OpState,
    #[string] address: &str,
    #[string] mode: &str,
    #[string] value: &str,
) {
    state
        .borrow::<Rc<dyn NativeHost>>()
        .cookie_response(address, mode, value);
}
#[op2(fast)]
fn op_result(state: &mut OpState, #[string] value: String) {
    *state.borrow_mut::<String>() = value;
}
#[derive(serde::Serialize)]
struct FetchResult {
    url: String,
    status: u16,
    #[serde(rename = "statusText")]
    status_text: String,
    headers: Vec<(String, String)>,
    redirected: bool,
    body: deno_core::serde_v8::ToJsBuffer,
}
#[op2]
#[serde]
async fn op_wega_fetch(
    state: Rc<std::cell::RefCell<OpState>>,
    #[serde] mut request: wega_network::Request,
    #[buffer] body: JsBuffer,
) -> Result<FetchResult, JsErrorBox> {
    request.body = body.to_vec();
    drop(body);
    let future = state.borrow().borrow::<Rc<dyn NativeHost>>().fetch(request);
    let result = future.await.map_err(JsErrorBox::generic)?;
    Ok(FetchResult {
        status_text: result.status_text().into(),
        url: result.url,
        status: result.status,
        headers: result.headers,
        redirected: result.redirected,
        body: result.body.into(),
    })
}
deno_core::extension!(
    wega_dom,
    ops = [
        op_dom_set_attr_ns,
        op_wega_cookie_header,
        op_wega_cookie_response,
        op_dom_read,
        op_dom_set_text,
        op_dom_set_attr,
        op_result,
        op_wega_fetch
    ]
);
