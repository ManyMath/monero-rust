use monero_rust::encryption;
use crate::signals::{
    DeriveEncryptionKeyRequest, EncryptionKeyDerivedResponse, LoadWalletDataRequest,
    SaveWalletDataRequest, SaveWithDerivedKeyRequest, WalletDataLoadedResponse,
    WalletDataSavedResponse,
};
use async_trait::async_trait;
use messages::prelude::{Actor, Address, Context, Notifiable};
use crate::ffi_web::SendToDart;

pub struct StorageActor {}

impl Actor for StorageActor {}

impl StorageActor {
    pub fn new(self_addr: Address<Self>) -> Self {
        tokio_with_wasm::alias::spawn(Self::listen_to_save_requests(self_addr.clone()));
        tokio_with_wasm::alias::spawn(Self::listen_to_load_requests(self_addr.clone()));
        tokio_with_wasm::alias::spawn(Self::listen_to_derive_key_requests(self_addr.clone()));
        tokio_with_wasm::alias::spawn(Self::listen_to_save_with_key_requests(self_addr));
        StorageActor {}
    }

    async fn listen_to_save_requests(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_save_wallet_data_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let _ = self_addr
                .notify(SaveWalletData {
                    password: request.password,
                    wallet_data_json: request.wallet_data_json,
                })
                .await;
        }
    }

    async fn listen_to_load_requests(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_load_wallet_data_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let _ = self_addr
                .notify(LoadWalletData {
                    password: request.password,
                    encrypted_data: request.encrypted_data,
                })
                .await;
        }
    }

    async fn listen_to_derive_key_requests(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_derive_encryption_key_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let _ = self_addr
                .notify(DeriveKey {
                    password: dart_msg.password,
                })
                .await;
        }
    }

    async fn listen_to_save_with_key_requests(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_save_with_derived_key_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let _ = self_addr
                .notify(SaveWithKey {
                    key_hex: request.key_hex,
                    salt_hex: request.salt_hex,
                    wallet_data_json: request.wallet_data_json,
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
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
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
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
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
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
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
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
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
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
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
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
                    wallet_data_json: None,
                }
                .send_signal_to_dart();
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeriveKey {
    pub password: String,
}

#[async_trait]
impl Notifiable<DeriveKey> for StorageActor {
    async fn notify(&mut self, msg: DeriveKey, _ctx: &Context<Self>) {
        match encryption::derive_key_fresh(&msg.password) {
            Ok((key, salt)) => {
                EncryptionKeyDerivedResponse {
                    success: true,
                    error: None,
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
                    key_hex: Some(hex::encode(key)),
                    salt_hex: Some(hex::encode(salt)),
                }
                .send_signal_to_dart();
            }
            Err(e) => {
                EncryptionKeyDerivedResponse {
                    success: false,
                    error: Some(format!("Key derivation failed: {}", e)),
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
                    key_hex: None,
                    salt_hex: None,
                }
                .send_signal_to_dart();
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct SaveWithKey {
    pub key_hex: String,
    pub salt_hex: String,
    pub wallet_data_json: String,
}

#[async_trait]
impl Notifiable<SaveWithKey> for StorageActor {
    async fn notify(&mut self, msg: SaveWithKey, _ctx: &Context<Self>) {
        let key_bytes = match hex::decode(&msg.key_hex) {
            Ok(b) if b.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&b);
                arr
            }
            _ => {
                WalletDataSavedResponse {
                    success: false,
                    error: Some("Invalid derived key".to_string()),
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
                    encrypted_data: None,
                }
                .send_signal_to_dart();
                return;
            }
        };

        let salt_bytes = match hex::decode(&msg.salt_hex) {
            Ok(b) if b.len() == 16 => {
                let mut arr = [0u8; 16];
                arr.copy_from_slice(&b);
                arr
            }
            _ => {
                WalletDataSavedResponse {
                    success: false,
                    error: Some("Invalid salt".to_string()),
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
                    encrypted_data: None,
                }
                .send_signal_to_dart();
                return;
            }
        };

        match encryption::encrypt_with_key(msg.wallet_data_json.as_bytes(), &key_bytes, &salt_bytes)
        {
            Ok(encrypted_bytes) => {
                let encrypted_base64 = base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    &encrypted_bytes,
                );

                WalletDataSavedResponse {
                    success: true,
                    error: None,
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
                    encrypted_data: Some(encrypted_base64),
                }
                .send_signal_to_dart();
            }
            Err(e) => {
                WalletDataSavedResponse {
                    success: false,
                    error: Some(format!("Encryption failed: {}", e)),
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
                    encrypted_data: None,
                }
                .send_signal_to_dart();
            }
        }
    }
}
