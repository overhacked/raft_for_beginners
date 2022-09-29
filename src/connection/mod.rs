mod udp;

use std::net::SocketAddr;

use async_trait::async_trait;
use tracing::trace;

#[derive(Debug)]
pub struct Packet {
    pub data: Vec<u8>, // TODO: constructor
} // TODO

impl Packet {
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }
}

#[derive(Debug)]
pub struct ServerAddress(pub SocketAddr); // TODO: make more generic?

#[derive(Debug)]
pub struct ConnectionError; // TODO

#[async_trait]
pub trait Connection {
    async fn listen(listen_socket: ServerAddress) -> Result<Self, ConnectionError>;
    async fn send(&self, server: ServerAddress, packet: Packet) -> Result<(), ConnectionError>;
    async fn receive(&mut self) -> Result<Packet, ConnectionError>;
}

pub struct DummyConnection;

#[async_trait]
impl Connection for DummyConnection {
    async fn send(&self, server: ServerAddress, packet: Packet) -> Result<(), ConnectionError> {
        trace!(?server, ?packet);
        Ok(()) // TODO
    }
    async fn listen(&mut self, listen_socket: ServerAddress) -> Result<(), ConnectionError> {
        trace!(?listen_socket);
        Ok(()) // TODO
    }
    async fn receive(&mut self) -> Result<Packet, ConnectionError> {
        trace!("receive");
        Ok(Packet) // TODO
    }
}
