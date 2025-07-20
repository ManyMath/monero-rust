use wasm_bindgen::prelude::*;
use web_sys::{Request, Response};
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = fetch)]
    fn global_fetch(input: &Request) -> js_sys::Promise;
}

/// Call the global `fetch()` function with a `Request`.
/// Works in both Window and Web Worker contexts.
pub async fn fetch_with_request(request: &Request) -> Result<Response, JsValue> {
    let resp = JsFuture::from(global_fetch(request)).await?;
    resp.dyn_into::<Response>()
}
