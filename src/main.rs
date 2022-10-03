mod config;
mod connection;

use clap::Parser;
use tracing::trace;

use crate::config::Config;
use crate::connection::{Connection, ConnectionError, udp::UdpConnection, ServerAddress, Packet};

struct Server<C: Connection> {
    connection: C,
}

impl<C: Connection> Server<C> {
    fn new(connection: C) -> Self {
        Self {
            connection,
        }
    }

    async fn run(mut self) -> Result<(), ConnectionError> {
        while let Ok(packet) = self.connection.receive().await { // TODO: handle receive error?
            let parsed = String::from_utf8_lossy(&packet.data);
            trace!(?packet, %parsed);
            let reply = Packet {
                data: "BONG\n".into(),
                peer: packet.peer,
            };
            let _who_cares = self.connection.send(reply).await; // TODO
        } 
        Ok(())
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
    let server = Server::new(connection);
    server.run().await.expect("server.run panicked");
}
