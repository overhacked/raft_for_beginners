mod connection;

use tracing::trace;

use crate::connection::{Connection, ConnectionError, DummyConnection, ServerAddress};

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
