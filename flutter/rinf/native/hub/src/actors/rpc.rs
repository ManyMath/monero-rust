use crate::messages::*;
use async_trait::async_trait;
use messages::prelude::{Actor, Address, Context, Notifiable};
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::interval;
use tokio_with_wasm::alias as tokio;

pub struct RpcActor {
    daemon_url: Option<String>,
    is_connected: bool,
    daemon_height: u64,
    _owned_tasks: JoinSet<()>,
}

impl Actor for RpcActor {}

impl RpcActor {
    pub fn new(self_addr: Address<Self>) -> Self {
        let mut _owned_tasks = JoinSet::new();
        _owned_tasks.spawn(Self::health_check_loop(self_addr));

        RpcActor {
            daemon_url: None,
            is_connected: false,
            daemon_height: 0,
            _owned_tasks,
        }
    }

    async fn health_check_loop(mut self_addr: Address<Self>) {
        let mut health_interval = interval(Duration::from_secs(30));
        loop {
            health_interval.tick().await;
            let _ = self_addr.notify(HealthCheck).await;
        }
    }

    async fn check_connection(&mut self) {
        if self.daemon_url.is_some() {
            self.daemon_height += 1;
            self.is_connected = true;
        }
    }
}

#[derive(Debug, Clone)]
struct HealthCheck;

#[async_trait]
impl Notifiable<HealthCheck> for RpcActor {
    async fn notify(&mut self, _msg: HealthCheck, _ctx: &Context<Self>) {
        self.check_connection().await;
    }
}

#[async_trait]
impl Notifiable<QueryHeight> for RpcActor {
    async fn notify(&mut self, _msg: QueryHeight, _ctx: &Context<Self>) {
        self.daemon_height += 1;
    }
}

#[async_trait]
impl Notifiable<FetchBlock> for RpcActor {
    async fn notify(&mut self, _msg: FetchBlock, _ctx: &Context<Self>) {
        self.daemon_height += 1;
    }
}
