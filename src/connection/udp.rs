use std::net::SocketAddr;

use async_trait::async_trait;
use tracing::trace;
use tokio::net::UdpSocket;

use super::*;

pub struct UdpConnection {
    socket: UdpSocket,
}

#[async_trait]
impl Connection for UdpConnection {
    async fn send(&self, server: ServerAddress, packet: Packet) -> Result<(), ConnectionError> {
        trace!(?server, ?packet);
        self.socket.send_to(packet.as_bytes(), server.0).await
            .expect("TODO: handle error");
        Ok(()) // TODO
    }
    async fn listen(listen_socket: ServerAddress) -> Result<Self, ConnectionError> {
        trace!(?listen_socket);
        let socket = UdpSocket::bind(listen_socket.0).await
            .expect("TODO: handle error");
        Ok(Self {
            socket,
        }) // TODO
    }
    async fn receive(&mut self) -> Result<Packet, ConnectionError> {
        trace!("receive");
        let packet = Packet {
            data: vec![0; 65536],
        };
        self.socket.recv(&mut packet.data).await
            .expect("TODO: handle error");
        Ok(packet) // TODO
    }
}
