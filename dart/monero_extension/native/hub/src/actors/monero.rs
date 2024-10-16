use crate::signals::{MoneroTestRequest, MoneroTestResponse};
use messages::prelude::{Actor, Address};
use rinf::{DartSignal, RustSignal};
use tokio::task::JoinSet;

use tokio_with_wasm::alias as tokio;

pub struct MoneroActor {
    _owned_tasks: JoinSet<()>,
}

impl Actor for MoneroActor {}

impl MoneroActor {
    pub fn new(self_addr: Address<Self>) -> Self {
        let mut _owned_tasks = JoinSet::new();
        _owned_tasks.spawn(Self::listen_to_test(self_addr));
        MoneroActor { _owned_tasks }
    }

    async fn listen_to_test(mut self_addr: Address<Self>) {
        let receiver = MoneroTestRequest::get_dart_signal_receiver();
        while let Some(_signal_pack) = receiver.recv().await {
            let result = monero_wasm::test_integration();
            MoneroTestResponse { result }.send_signal_to_dart();
        }
    }
}
