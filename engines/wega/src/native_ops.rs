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
    fn cookie_header(&self, address: &str, include: bool) -> String;
    fn cookie_response(&self, address: &str, include: bool, value: &str);
    fn document_origin(&self) -> String;
    fn ws_headers(&self, address: &str) -> Vec<(String, String)>;
    fn referrer(&self, address: &str) -> String;
    fn connect_check(&self, address: &str) -> String;
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
fn op_wega_cookie_header(state: &mut OpState, #[string] address: &str, include: bool) -> String {
    state
        .borrow::<Rc<dyn NativeHost>>()
        .cookie_header(address, include)
}
#[op2(fast)]
fn op_wega_cookie_response(
    state: &mut OpState,
    #[string] address: &str,
    include: bool,
    #[string] value: &str,
) {
    state
        .borrow::<Rc<dyn NativeHost>>()
        .cookie_response(address, include, value)
}
#[op2]
#[string]
fn op_wega_document_origin(state: &mut OpState) -> String {
    state.borrow::<Rc<dyn NativeHost>>().document_origin()
}
#[op2]
#[serde]
fn op_wega_ws_headers(state: &mut OpState, #[string] address: &str) -> Vec<(String, String)> {
    state.borrow::<Rc<dyn NativeHost>>().ws_headers(address)
}
#[op2]
#[string]
fn op_wega_referrer(state: &mut OpState, #[string] address: &str) -> String {
    state.borrow::<Rc<dyn NativeHost>>().referrer(address)
}
#[op2]
#[string]
fn op_wega_connect_check(state: &mut OpState, #[string] address: &str) -> String {
    state.borrow::<Rc<dyn NativeHost>>().connect_check(address)
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
    #[serde(rename = "responseRid")]
    response_rid: u32,
    #[serde(rename = "bodyDecoded")]
    body_decoded: bool,
    error: Option<String>,
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
        response_rid: state
            .borrow_mut()
            .resource_table
            .add(FetchBody(std::cell::RefCell::new(result.body.into()))),
        body_decoded: true,
        error: None,
    })
}
deno_core::extension!(
    wega_dom,
    ops = [
        op_dom_set_attr_ns,
        op_wega_cookie_header,
        op_wega_cookie_response,
        op_wega_document_origin,
        op_wega_ws_headers,
        op_wega_referrer,
        op_wega_connect_check,
        op_dom_read,
        op_dom_set_text,
        op_dom_set_attr,
        op_result,
        op_wega_fetch
    ]
);

// The broker returns a bounded body, exposed through Deno's existing stream lifecycle.
struct FetchBody(std::cell::RefCell<deno_core::BufView>);
impl deno_core::Resource for FetchBody {
    fn read(self: Rc<Self>, limit: usize) -> deno_core::AsyncResult<deno_core::BufView> {
        let mut bytes = self.0.borrow_mut();
        let count = limit.min(bytes.len());
        let chunk = bytes.split_to(count);
        Box::pin(std::future::ready(Ok(chunk)))
    }
    fn size_hint(&self) -> (u64, Option<u64>) {
        let len = self.0.borrow().len() as u64;
        (len, Some(len))
    }
}
