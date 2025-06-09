use std::net::{SocketAddr, TcpStream};
use std::sync::OnceLock;
use std::time::Duration;

static STAGENET_AVAILABLE: OnceLock<bool> = OnceLock::new();

pub fn stagenet_available() -> bool {
    *STAGENET_AVAILABLE.get_or_init(|| {
        let addr: SocketAddr = "127.0.0.1:38081".parse().unwrap();
        TcpStream::connect_timeout(&addr, Duration::from_secs(1)).is_ok()
    })
}
