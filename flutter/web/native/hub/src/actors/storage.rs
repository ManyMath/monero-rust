use crate::encryption;
use crate::signals::*;
use async_trait::async_trait;
use messages::prelude::{Actor, Address, Context, Notifiable};
use rinf::{DartSignal, RustSignal};

pub struct StorageActor {}

impl Actor for StorageActor {}

impl StorageActor {
    pub fn new(self_addr: Address<Self>) -> Self {
        tokio_with_wasm::alias::spawn(Self::listen_to_save_requests(self_addr.clone()));
        tokio_with_wasm::alias::spawn(Self::listen_to_load_requests(self_addr));
        StorageActor {}
    }

    async fn listen_to_save_requests(mut self_addr: Address<Self>) {
        let receiver = SaveWalletDataRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
            let _ = self_addr
                .notify(SaveWalletData {
                    password: request.password,
                    wallet_data_json: request.wallet_data_json,
                })
                .await;
        }
    }

    async fn listen_to_load_requests(mut self_addr: Address<Self>) {
        let receiver = LoadWalletDataRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
            let _ = self_addr
                .notify(LoadWalletData {
                    password: request.password,
                    encrypted_data: request.encrypted_data,
                })
                .await;
        }
    }
}

#[derive(Debug, Clone)]
pub struct SaveWalletData {
    pub password: String,
    pub wallet_data_json: String,
}

#[derive(Debug, Clone)]
pub struct LoadWalletData {
    pub password: String,
    pub encrypted_data: String,
}

#[async_trait]
impl Notifiable<SaveWalletData> for StorageActor {
    async fn notify(&mut self, msg: SaveWalletData, _ctx: &Context<Self>) {
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&"Encrypting wallet data...".into());

        // Encrypt the wallet data
        match encryption::encrypt(msg.wallet_data_json.as_bytes(), &msg.password) {
            Ok(encrypted_bytes) => {
                // Encode as base64 for storage
                let encrypted_base64 = base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    &encrypted_bytes,
                );

                #[cfg(target_arch = "wasm32")]
                web_sys::console::log_1(&format!("Wallet data encrypted successfully ({} bytes)", encrypted_bytes.len()).into());

                WalletDataSavedResponse {
                    success: true,
                    error: None,
                    encrypted_data: Some(encrypted_base64),
                }
                .send_signal_to_dart();
            }
            Err(e) => {
                #[cfg(target_arch = "wasm32")]
                web_sys::console::error_1(&format!("Encryption failed: {}", e).into());

                WalletDataSavedResponse {
                    success: false,
                    error: Some(format!("Encryption failed: {}", e)),
                    encrypted_data: None,
                }
                .send_signal_to_dart();
            }
        }
    }
}

#[async_trait]
impl Notifiable<LoadWalletData> for StorageActor {
    async fn notify(&mut self, msg: LoadWalletData, _ctx: &Context<Self>) {
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&"Decrypting wallet data...".into());

        // Decode base64
        let encrypted_bytes = match base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &msg.encrypted_data,
        ) {
            Ok(bytes) => bytes,
            Err(e) => {
                #[cfg(target_arch = "wasm32")]
                web_sys::console::error_1(&format!("Base64 decode failed: {}", e).into());

                WalletDataLoadedResponse {
                    success: false,
                    error: Some(format!("Invalid encrypted data: {}", e)),
                    wallet_data_json: None,
                }
                .send_signal_to_dart();
                return;
            }
        };

        // Decrypt the wallet data
        match encryption::decrypt(&encrypted_bytes, &msg.password) {
            Ok(decrypted_bytes) => {
                match String::from_utf8(decrypted_bytes) {
                    Ok(wallet_data_json) => {
                        #[cfg(target_arch = "wasm32")]
                        web_sys::console::log_1(&"Wallet data decrypted successfully".into());

                        WalletDataLoadedResponse {
                            success: true,
                            error: None,
                            wallet_data_json: Some(wallet_data_json),
                        }
                        .send_signal_to_dart();
                    }
                    Err(e) => {
                        #[cfg(target_arch = "wasm32")]
                        web_sys::console::error_1(&format!("UTF-8 decode failed: {}", e).into());

                        WalletDataLoadedResponse {
                            success: false,
                            error: Some(format!("Invalid decrypted data: {}", e)),
                            wallet_data_json: None,
                        }
                        .send_signal_to_dart();
                    }
                }
            }
            Err(e) => {
                #[cfg(target_arch = "wasm32")]
                web_sys::console::error_1(&format!("Decryption failed: {}", e).into());

                WalletDataLoadedResponse {
                    success: false,
                    error: Some(format!("Decryption failed: {} (wrong password?)", e)),
                    wallet_data_json: None,
                }
                .send_signal_to_dart();
            }
        }
    }
}
