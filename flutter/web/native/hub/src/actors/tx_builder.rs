use crate::messages::*;
use crate::signals::*;
use async_trait::async_trait;
use messages::prelude::{Actor, Address, Context, Handler, Notifiable};
use rinf::{DartSignal, RustSignal};
use tokio::task::JoinSet;
use tokio_with_wasm::alias as tokio;

pub struct TxBuilderActor {
    wallet_actor: Option<Address<super::wallet::WalletActor>>,
    rpc_actor: Option<Address<super::rpc::RpcActor>>,
    _owned_tasks: JoinSet<()>,
}

impl Actor for TxBuilderActor {}

impl TxBuilderActor {
    pub fn new(self_addr: Address<Self>) -> Self {
        let mut _owned_tasks = JoinSet::new();
        _owned_tasks.spawn(Self::listen_to_tx_requests(self_addr));

        TxBuilderActor {
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

    async fn listen_to_tx_requests(mut self_addr: Address<Self>) {
        let receiver = CreateTransactionRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
            let _ = self_addr
                .notify(BuildTransaction {
                    destination: request.destination,
                    amount: request.amount,
                })
                .await;
        }
    }
}

#[async_trait]
impl Notifiable<BuildTransaction> for TxBuilderActor {
    async fn notify(&mut self, msg: BuildTransaction, _ctx: &Context<Self>) {
        let fee = msg.amount / 100;

        TransactionCreatedResponse {
            tx_id: format!("tx_{}", msg.amount),
            fee,
        }
        .send_signal_to_dart();
    }
}
