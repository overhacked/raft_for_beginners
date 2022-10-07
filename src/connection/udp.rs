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
        let data = postcard::to_allocvec(&packet).expect("serialization failed");
        self.socket.send_to(&data, packet.peer.0).await
            .expect("TODO: handle error");
        Ok(()) // TODO
    }

    async fn receive(&self) -> Result<Packet, ConnectionError> {
        let mut buf = vec![0; 65536];
        let (bytes_received, peer_addr) = self.socket.recv_from(&mut buf).await
            .expect("TODO: handle error");
        buf.truncate(bytes_received);
        trace!(?peer_addr, bytes_received, "receive"); // DEBUG

        tracing::warn!(?buf, "TODO: deserialize packet");
        let packet = postcard::from_bytes(&buf).expect("deserialization failed");
        Ok(packet)
    }
}
