#![cfg(target_arch = "wasm32")]

use async_trait::async_trait;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

use monero_serai::rpc::RpcError;

#[derive(Clone, Debug)]
pub struct WasmRpcAdapter {
    url: String,
}

impl WasmRpcAdapter {
    pub fn new(url: String) -> Self {
        Self { url }
    }
}

#[async_trait(?Send)]
impl monero_serai::rpc::RpcConnection for WasmRpcAdapter {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        use web_sys::console;

        let window = web_sys::window().ok_or(RpcError::ConnectionError)?;
        let full_url = format!("{}/{}", self.url, route);

        let opts = RequestInit::new();
        opts.set_method("POST");
        opts.set_mode(RequestMode::Cors);

        let body_array = js_sys::Uint8Array::from(&body[..]);
        opts.set_body(&body_array);

        let request = Request::new_with_str_and_init(&full_url, &opts)
            .map_err(|_| RpcError::ConnectionError)?;

        let headers = request.headers();
        let content_type = if route.ends_with(".bin") {
            "application/octet-stream"
        } else {
            "application/json"
        };
        headers.set("Content-Type", content_type).map_err(|_| RpcError::ConnectionError)?;

        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| {
                if let Some(s) = e.as_string() {
                    console::log_1(&format!("RPC error: {}", s).into());
                }
                RpcError::ConnectionError
            })?;

        let resp: Response = resp_value.dyn_into().map_err(|_| RpcError::ConnectionError)?;
        let status = resp.status();

        if status < 200 || status >= 300 {
            return Err(RpcError::ConnectionError);
        }

        if route.ends_with(".bin") {
            let array_buffer_promise = resp.array_buffer().map_err(|_| RpcError::ConnectionError)?;
            let array_buffer_value = JsFuture::from(array_buffer_promise)
                .await
                .map_err(|_| RpcError::ConnectionError)?;

            let uint8_array = js_sys::Uint8Array::new(&array_buffer_value);
            let mut result = vec![0u8; uint8_array.length() as usize];
            uint8_array.copy_to(&mut result);
            Ok(result)
        } else {
            let text_promise = resp.text().map_err(|_| RpcError::ConnectionError)?;
            let text_value = JsFuture::from(text_promise)
                .await
                .map_err(|_| RpcError::ConnectionError)?;

            let text = text_value.as_string().ok_or(RpcError::ConnectionError)?;
            Ok(text.into_bytes())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_rpc_adapter_creation() {
        let adapter = WasmRpcAdapter::new("http://stagenet.melo.tools:38081".to_string());
        assert_eq!(adapter.url, "http://stagenet.melo.tools:38081");
    }
}
