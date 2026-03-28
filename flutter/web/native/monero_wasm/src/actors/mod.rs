pub mod storage;
pub mod tx_builder;
pub mod ur;
pub mod wallet;

#[cfg(test)]
mod wallet_test;

use messages::prelude::Context;
use storage::StorageActor;
use tokio::spawn;
use tokio_with_wasm::alias as tokio;
use tx_builder::TxBuilderActor;
use ur::UrActor;
use wallet::WalletActor;

pub async fn create_actors() {
    let wallet_context = Context::new();
    let wallet_addr = wallet_context.address();

    let tx_builder_context = Context::new();
    let tx_builder_addr = tx_builder_context.address();

    let storage_context = Context::new();
    let storage_addr = storage_context.address();

    let ur_context = Context::new();
    let ur_addr = ur_context.address();

    let wallet_actor = WalletActor::new(wallet_addr.clone());

    let mut tx_builder_actor = TxBuilderActor::new(tx_builder_addr.clone());
    tx_builder_actor.set_wallet_actor(wallet_addr.clone());

    let storage_actor = StorageActor::new(storage_addr.clone());
    let ur_actor = UrActor::new(ur_addr.clone());

    spawn(wallet_context.run(wallet_actor));
    spawn(tx_builder_context.run(tx_builder_actor));
    spawn(storage_context.run(storage_actor));
    spawn(ur_context.run(ur_actor));
}
