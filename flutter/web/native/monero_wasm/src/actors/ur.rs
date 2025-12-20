use crate::ffi_web::SendToDart;
use crate::messages::{ResetUrDecoder, StartUrEncoder, StopUrEncoder, UrDecodeFrame};
use crate::signals::{
    QrFrameResponse, UrDecodeCompleteResponse, UrDecodeProgressResponse,
};
use async_trait::async_trait;
use messages::prelude::{Actor, Address, Context, Notifiable};
use monero_rust::ur_codec::{UrDecoder, UrEncoder};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio_with_wasm::alias as tokio;

/// Generation counter to cancel stale encoder loops.
static ENCODER_GENERATION: AtomicU64 = AtomicU64::new(0);

pub struct UrActor {
    decoder: UrDecoder,
}

impl Actor for UrActor {}

impl UrActor {
    pub fn new(self_addr: Address<Self>) -> Self {
        tokio::spawn(Self::listen_to_start_encoder(self_addr.clone()));
        tokio::spawn(Self::listen_to_stop_encoder(self_addr.clone()));
        tokio::spawn(Self::listen_to_decode_frame(self_addr.clone()));
        tokio::spawn(Self::listen_to_reset_decoder(self_addr));
        UrActor {
            decoder: UrDecoder::new(),
        }
    }

    async fn listen_to_start_encoder(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_start_ur_encoder_request_receiver();
        while let Some(req) = receiver.recv().await {
            let data = match hex::decode(&req.data_hex) {
                Ok(d) => d,
                Err(_) => continue,
            };
            let _ = self_addr
                .notify(StartUrEncoder {
                    data,
                    ur_type: req.ur_type,
                    max_fragment_len: req.max_fragment_len as usize,
                })
                .await;
        }
    }

    async fn listen_to_stop_encoder(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_stop_ur_encoder_request_receiver();
        while let Some(_req) = receiver.recv().await {
            let _ = self_addr.notify(StopUrEncoder).await;
        }
    }

    async fn listen_to_decode_frame(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_ur_decode_frame_request_receiver();
        while let Some(req) = receiver.recv().await {
            let _ = self_addr.notify(UrDecodeFrame { uri: req.uri }).await;
        }
    }

    async fn listen_to_reset_decoder(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_reset_ur_decoder_request_receiver();
        while let Some(_req) = receiver.recv().await {
            let _ = self_addr.notify(ResetUrDecoder).await;
        }
    }
}

#[async_trait]
impl Notifiable<StartUrEncoder> for UrActor {
    async fn notify(&mut self, msg: StartUrEncoder, _ctx: &Context<Self>) {
        // Bump generation to cancel any running encoder loop
        let generation = ENCODER_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

        // Leak the UR type string so it lives for 'static (required by ur::Encoder)
        let ur_type: &'static str = Box::leak(msg.ur_type.into_boxed_str());

        let encoder = match UrEncoder::new(&msg.data, ur_type, msg.max_fragment_len) {
            Ok(e) => Arc::new(std::sync::Mutex::new(e)),
            Err(e) => {
                #[cfg(target_arch = "wasm32")]
                web_sys::console::error_1(&format!("UR encoder error: {}", e).into());
                let _ = e;
                return;
            }
        };

        // Spawn frame-emitting loop at ~12.5 fps (80ms interval)
        let encoder_clone = encoder.clone();
        tokio::spawn(async move {
            let interval = std::time::Duration::from_millis(80);
            loop {
                if ENCODER_GENERATION.load(Ordering::SeqCst) != generation {
                    break;
                }

                let frame = {
                    let lock = encoder_clone.lock();
                    match lock {
                        Ok(mut enc) => enc.next_frame(),
                        Err(_) => break,
                    }
                };

                match frame {
                    Ok(f) => {
                        QrFrameResponse {
                            modules: f.modules,
                            size: f.size as u32,
                            seq_num: f.seq_num as u32,
                            seq_len: f.seq_len as u32,
                            uri: f.uri,
                        }
                        .send_signal_to_dart();
                    }
                    Err(_) => break,
                }

                tokio::time::sleep(interval).await;
            }
        });
    }
}

#[async_trait]
impl Notifiable<StopUrEncoder> for UrActor {
    async fn notify(&mut self, _msg: StopUrEncoder, _ctx: &Context<Self>) {
        // Bump generation to cancel the running encoder loop
        ENCODER_GENERATION.fetch_add(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl Notifiable<UrDecodeFrame> for UrActor {
    async fn notify(&mut self, msg: UrDecodeFrame, _ctx: &Context<Self>) {
        match self.decoder.receive(&msg.uri) {
            Ok(progress) => {
                UrDecodeProgressResponse {
                    progress: progress.progress,
                    is_complete: progress.is_complete,
                }
                .send_signal_to_dart();

                if progress.is_complete {
                    // Try multi-part message first, fall back to single-part
                    let data = if let Ok(Some(bytes)) = self.decoder.message() {
                        bytes
                    } else {
                        match monero_rust::ur_codec::decode_single(&msg.uri) {
                            Ok(bytes) => bytes,
                            Err(_) => return,
                        }
                    };

                    // Extract UR type from the URI: "ur:TYPE/..."
                    let ur_type = msg
                        .uri
                        .strip_prefix("ur:")
                        .and_then(|s| s.split('/').next())
                        .unwrap_or("bytes")
                        .to_string();

                    UrDecodeCompleteResponse {
                        data_hex: hex::encode(&data),
                        ur_type,
                    }
                    .send_signal_to_dart();
                }
            }
            Err(e) => {
                #[cfg(target_arch = "wasm32")]
                web_sys::console::error_1(&format!("UR decode error: {}", e).into());
                let _ = e;
            }
        }
    }
}

#[async_trait]
impl Notifiable<ResetUrDecoder> for UrActor {
    async fn notify(&mut self, _msg: ResetUrDecoder, _ctx: &Context<Self>) {
        self.decoder.reset();
    }
}
