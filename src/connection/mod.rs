pub mod udp;

use std::net::SocketAddr;

use async_trait::async_trait;
use derive_more::FromStr;
use tracing::trace;

#[derive(Debug)]
pub struct Packet {
    pub data: Vec<u8>, // TODO: constructor
    pub peer: ServerAddress,
} // TODO

impl Packet {
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }
}

#[derive(Clone, Debug, FromStr)]
pub struct ServerAddress(pub SocketAddr); // TODO: make more generic?

#[derive(Debug)]
pub struct ConnectionError; // TODO

#[async_trait]
pub trait Connection: Sized {
    async fn bind(bind_socket: ServerAddress) -> Result<Self, ConnectionError>;
    async fn send(&self, packet: Packet) -> Result<(), ConnectionError>;
    async fn receive(&mut self) -> Result<Packet, ConnectionError>;
}

pub struct DummyConnection;

#[async_trait]
impl Connection for DummyConnection {
    async fn bind(bind_socket: ServerAddress) -> Result<Self, ConnectionError> {
        trace!(?bind_socket);
        Ok(Self) // TODO
    }
    async fn send(&self, packet: Packet) -> Result<(), ConnectionError> {
        trace!(?packet);
        Ok(()) // TODO
    }

    async fn receive(&mut self) -> Result<Packet, ConnectionError> {
        trace!("receive");
        Ok(Packet {
            data: Vec::new(),
            peer: ServerAddress("0.0.0.0:0".parse().unwrap()),
        }) // TODO
    }
}
