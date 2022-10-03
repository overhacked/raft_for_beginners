use async_trait::async_trait;
use tracing::trace;
use tokio::net::UdpSocket;

use super::*;

pub struct UdpConnection {
    socket: UdpSocket,
}

#[async_trait]
impl Connection for UdpConnection {
    async fn bind(bind_socket: ServerAddress) -> Result<Self, ConnectionError> {
        trace!(?bind_socket);
        let socket = UdpSocket::bind(bind_socket.0).await
            .expect("TODO: handle error");
        Ok(Self {
            socket,
        }) // TODO
    }

    async fn send(&self, packet: Packet) -> Result<(), ConnectionError> {
        trace!(?packet, "send");
        self.socket.send_to(packet.as_bytes(), packet.peer.0).await
            .expect("TODO: handle error");
        Ok(()) // TODO
    }

    async fn receive(&mut self) -> Result<Packet, ConnectionError> {
        let mut buf = vec![0; 65536];
        let (bytes_received, peer_addr) = self.socket.recv_from(&mut buf).await
            .expect("TODO: handle error");
        buf.truncate(bytes_received);
        trace!(?peer_addr, bytes_received, "receive"); // DEBUG

        Ok(Packet {
            data: buf,
            peer: ServerAddress(peer_addr),
        })
    }
}
