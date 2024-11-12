use crate::messages::*;
use crate::signals::*;
use async_trait::async_trait;
use messages::prelude::{Actor, Address, Context, Handler, Notifiable};
use rinf::{DartSignal, RustSignal};
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::interval;
use tokio_with_wasm::alias as tokio;

pub struct SyncActor {
    is_syncing: bool,
    current_height: u64,
    daemon_height: u64,
    wallet_actor: Option<Address<super::wallet::WalletActor>>,
    rpc_actor: Option<Address<super::rpc::RpcActor>>,
    _owned_tasks: JoinSet<()>,
}

impl Actor for SyncActor {}

impl SyncActor {
    pub fn new(self_addr: Address<Self>) -> Self {
        let mut _owned_tasks = JoinSet::new();
        _owned_tasks.spawn(Self::listen_to_start_sync(self_addr.clone()));
        _owned_tasks.spawn(Self::background_sync_loop(self_addr));

        SyncActor {
            is_syncing: false,
            current_height: 0,
            daemon_height: 0,
            wallet_actor: None,
            rpc_actor: None,
            _owned_tasks,
        }
    }

    pub fn set_wallet_actor(&mut self, addr: Address<super::wallet::WalletActor>) {
        self.wallet_actor = Some(addr);
    }

    pub fn set_rpc_actor(&mut self, addr: Address<super::rpc::RpcActor>) {
        self.rpc_actor = Some(addr);
    }

    async fn listen_to_start_sync(mut self_addr: Address<Self>) {
        let receiver = StartSyncRequest::get_dart_signal_receiver();
        while let Some(_signal_pack) = receiver.recv().await {
            let _ = self_addr.notify(StartSync).await;
        }
    }

    async fn background_sync_loop(mut self_addr: Address<Self>) {
        let mut sync_interval = interval(Duration::from_secs(5));
        loop {
            sync_interval.tick().await;
            let _ = self_addr.notify(TickSync).await;
        }
    }

    async fn scan_next_block(&mut self) {
        if let Some(rpc_actor) = &mut self.rpc_actor {
            let _ = rpc_actor.notify(QueryHeight).await;
            self.daemon_height += 1;

            if self.current_height < self.daemon_height {
                let _ = rpc_actor
                    .notify(FetchBlock {
                        height: self.current_height,
                    })
                    .await;

                self.current_height += 1;

                SyncProgressResponse {
                    current_height: self.current_height,
                    daemon_height: self.daemon_height,
                    is_synced: self.current_height >= self.daemon_height,
                    is_scanning: self.is_syncing,
                }
                .send_signal_to_dart();
            }
        }
    }
}

#[derive(Debug, Clone)]
struct StartSync;

#[async_trait]
impl Notifiable<StartSync> for SyncActor {
    async fn notify(&mut self, _msg: StartSync, _ctx: &Context<Self>) {
        self.is_syncing = true;
    }
}

#[derive(Debug, Clone)]
struct TickSync;

#[async_trait]
impl Notifiable<TickSync> for SyncActor {
    async fn notify(&mut self, _msg: TickSync, _ctx: &Context<Self>) {
        if self.is_syncing {
            self.scan_next_block().await;
        }
    }
}
