mod config;
mod connection;

use std::time::Duration;

use clap::Parser;
use tokio::time::sleep;
use tracing::trace;

use crate::config::Config;
use crate::connection::{Connection, ConnectionError, udp::UdpConnection, ServerAddress, Packet};

struct Server<C: Connection> {
    connection: C,
    peers: Vec<ServerAddress>,
    heartbeat_interval: Duration,
}

impl<C: Connection> Server<C> {
    fn new(connection: C, peers: Vec<ServerAddress>, heartbeat_interval: Duration) -> Self {
        Self {
            connection,
            peers,
            heartbeat_interval,
        }
    }

    async fn receive_packet(&mut self) -> Result<Packet, ConnectionError> {
        let packet = self.connection.receive().await?;
        let parsed = String::from_utf8_lossy(&packet.data);
        trace!(?packet, %parsed);
        Ok(packet)
    }

    async fn run(mut self) -> Result<(), ConnectionError> {
        let heartbeat_interval = self.heartbeat_interval.clone();
        loop {
            tokio::select! {
                packet = self.receive_packet() => {
                    let packet = packet.expect("HANDLE RECEIVE ERROR");
                    if packet.data == b"HEARTBEAT" {
                        let reply = Packet {
                            data: "ACK HEARTBEAT".into(),
                            peer: packet.peer,
                        };
                        let _who_cares = self.connection.send(reply).await; // TODO
                    }
                },
                _ = sleep(heartbeat_interval) => {
                    for peer in &self.peers {
                        let peer_request = Packet {
                            data: "HEARTBEAT".into(),
                            peer: peer.to_owned(),
                        };
                        let _who_cares = self.connection.send(peer_request).await; // TODO
                    }
                }
            }
        } 
        //Ok(())
    }
}

#[tokio::main]
async fn main() {
    use tracing_subscriber::{
        registry::Registry,
        layer::SubscriberExt,
    };

    let subscriber = Registry::default().with(tracing_tree::HierarchicalLayer::new(2));
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let opts = Config::parse();

    let connection = UdpConnection::bind(opts.listen_socket).await.unwrap();
    let server = Server::new(connection, opts.peers, opts.heartbeat_interval);
    server.run().await.expect("server.run panicked");
}
