use monero_rpc::{Rpc, RpcError};
use monero_simple_request_rpc::SimpleRequestRpc;
use crate::mock_rpc::MockRpc;

#[derive(Clone)]
pub enum DynRpc {
    Real(SimpleRequestRpc),
    Mock(MockRpc),
}

impl Rpc for DynRpc {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        match self {
            DynRpc::Real(rpc) => rpc.post(route, body).await,
            DynRpc::Mock(rpc) => rpc.post(route, body).await,
        }
    }
}

impl From<SimpleRequestRpc> for DynRpc {
    fn from(rpc: SimpleRequestRpc) -> Self {
        DynRpc::Real(rpc)
    }
}

impl From<MockRpc> for DynRpc {
    fn from(rpc: MockRpc) -> Self {
        DynRpc::Mock(rpc)
    }
}
