use crate::monero_backend::rpc::{RpcConnection, RpcError};
use async_trait::async_trait;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::JsFuture;
#[cfg(target_arch = "wasm32")]
use web_sys::{Request, RequestCredentials, RequestInit, RequestMode, Response};

#[derive(Clone, Debug)]
pub(crate) struct WasmRpcConnection {
    #[allow(dead_code)] // Used in WASM target's RpcConnection::post impl
    url: String,
}

impl WasmRpcConnection {
    pub(crate) fn new(url: String) -> Self {
        Self { url }
    }
}

#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
impl RpcConnection for WasmRpcConnection {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        let url = format!("{}/{}", self.url, route);

        let opts = RequestInit::new();
        opts.set_method("POST");
        opts.set_mode(RequestMode::Cors);
        opts.set_credentials(RequestCredentials::SameOrigin);
        opts.set_body(&JsValue::from(js_sys::Uint8Array::from(&body[..]).buffer()));

        let request =
            Request::new_with_str_and_init(&url, &opts).map_err(|_| RpcError::ConnectionError)?;

        let content_type = if route.ends_with(".bin") {
            "application/octet-stream"
        } else {
            "application/json"
        };
        request
            .headers()
            .set("Content-Type", content_type)
            .map_err(|_| RpcError::ConnectionError)?;

        let resp: Response = crate::wasm_fetch::fetch_with_request(&request)
            .await
            .map_err(|_| RpcError::ConnectionError)?;

        let status = resp.status();
        if status < 200 || status >= 300 {
            return Err(RpcError::ConnectionError);
        }

        let array_buffer =
            JsFuture::from(resp.array_buffer().map_err(|_| RpcError::ConnectionError)?)
                .await
                .map_err(|_| RpcError::ConnectionError)?;

        let uint8_array = js_sys::Uint8Array::new(&array_buffer);
        let mut result = vec![0u8; uint8_array.length() as usize];
        uint8_array.copy_to(&mut result);

        Ok(result)
    }
}
