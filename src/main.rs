use std::net::SocketAddr;

use async_trait::async_trait;
use tracing::trace;

#[derive(Debug)]
struct Packet; // TODO
#[derive(Debug)]
struct ConnectionError; // TODO
#[derive(Debug)]
struct ServerAddress(SocketAddr); // TODO: make more generic?

#[async_trait]
trait Connection {
    async fn send(&self, server: ServerAddress, packet: Packet) -> Result<(), ConnectionError>;
    async fn listen(&mut self, listen_socket: ServerAddress) -> Result<(), ConnectionError>;
    async fn receive(&mut self) -> Result<Packet, ConnectionError>;
}

struct DummyConnection;

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

struct Server<C: Connection> {
    connection: C,
}

impl<C: Connection> Server<C> {
    fn new(connection: C) -> Self {
        Self {
            connection,
        }
    }

    async fn run(mut self, listen_socket: ServerAddress) -> Result<(), ConnectionError> {
        self.connection.listen(listen_socket).await?;
        while let Ok(packet) = self.connection.receive().await { // TODO: handle receive error?
            trace!(?packet);
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

    let connection = DummyConnection;
    let listen_socket = ServerAddress("127.0.0.1:8000".parse().unwrap());
    let server = Server::new(connection);
    server.run(listen_socket).await.expect("server.run panicked");
}
