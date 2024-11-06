use async_trait::async_trait;
use monero_serai::rpc::{RpcConnection, RpcError};

#[cfg(any(test, feature = "test-helpers"))]
use monero_serai::rpc::Rpc;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::{JsValue, JsCast};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::JsFuture;
#[cfg(target_arch = "wasm32")]
use web_sys::{Request, RequestInit, RequestMode, RequestCredentials, Response};

#[derive(Clone, Debug)]
pub struct WasmRpcConnection {
    url: String,
}

impl WasmRpcConnection {
    pub fn new(url: String) -> Self {
        Self { url }
    }
}

#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
impl RpcConnection for WasmRpcConnection {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        let window = web_sys::window()
            .ok_or(RpcError::ConnectionError)?;

        let url = format!("{}/{}", self.url, route);

        let opts = RequestInit::new();
        opts.set_method("POST");
        opts.set_mode(RequestMode::Cors);
        opts.set_credentials(RequestCredentials::SameOrigin);
        opts.set_body(&JsValue::from(
            js_sys::Uint8Array::from(&body[..]).buffer()
        ));

        let request = Request::new_with_str_and_init(&url, &opts)
            .map_err(|_| RpcError::ConnectionError)?;

        request
            .headers()
            .set("Content-Type", "application/json")
            .map_err(|_| RpcError::ConnectionError)?;

        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|_| RpcError::ConnectionError)?;

        let resp: Response = resp_value
            .dyn_into()
            .map_err(|_| RpcError::ConnectionError)?;

        let status = resp.status();
        if status < 200 || status >= 300 {
            return Err(RpcError::ConnectionError);
        }

        let array_buffer = JsFuture::from(
            resp.array_buffer()
                .map_err(|_| RpcError::ConnectionError)?
        )
        .await
        .map_err(|_| RpcError::ConnectionError)?;

        let uint8_array = js_sys::Uint8Array::new(&array_buffer);
        let mut result = vec![0u8; uint8_array.length() as usize];
        uint8_array.copy_to(&mut result);

        Ok(result)
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl RpcConnection for WasmRpcConnection {
    async fn post(&self, _route: &str, _body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        Err(RpcError::InternalError("WasmRpcConnection only works in WASM context"))
    }
}

#[cfg(any(test, feature = "test-helpers"))]
pub fn create_rpc_client(url: String) -> Rpc<WasmRpcConnection> {
    Rpc::new_with_connection(WasmRpcConnection::new(url))
}

#[cfg(not(any(test, feature = "test-helpers")))]
pub fn create_rpc_client(_url: String) -> ! {
    panic!("create_rpc_client requires test-helpers feature or use HttpRpc::new for native")
}
