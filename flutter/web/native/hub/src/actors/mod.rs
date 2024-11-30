pub mod wallet;
pub mod rpc;
pub mod tx_builder;
pub mod storage;

#[cfg(test)]
mod wallet_test;

use messages::prelude::Context;
use wallet::WalletActor;
use rpc::RpcActor;
use tx_builder::TxBuilderActor;
use storage::StorageActor;
use tokio::spawn;
use tokio_with_wasm::alias as tokio;

pub async fn create_actors() {
    let wallet_context = Context::new();
    let wallet_addr = wallet_context.address();

    let rpc_context = Context::new();
    let rpc_addr = rpc_context.address();

    let tx_builder_context = Context::new();
    let tx_builder_addr = tx_builder_context.address();

    let storage_context = Context::new();
    let storage_addr = storage_context.address();

    let mut wallet_actor = WalletActor::new(wallet_addr.clone());
    wallet_actor.set_rpc_actor(rpc_addr.clone());

    let rpc_actor = RpcActor::new(rpc_addr.clone());

    let mut tx_builder_actor = TxBuilderActor::new(tx_builder_addr.clone());
    tx_builder_actor.set_wallet_actor(wallet_addr.clone());
    tx_builder_actor.set_rpc_actor(rpc_addr.clone());

    let storage_actor = StorageActor::new(storage_addr.clone());

    spawn(wallet_context.run(wallet_actor));
    spawn(rpc_context.run(rpc_actor));
    spawn(tx_builder_context.run(tx_builder_actor));
    spawn(storage_context.run(storage_actor));
}
