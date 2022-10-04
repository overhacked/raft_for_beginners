mod config;
mod connection;

use std::sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use tokio::sync::RwLock;
use tokio::sync::mpsc::{Sender, Receiver};
use tokio::task::JoinSet;
use tokio::time::sleep;
use tracing::trace;

use crate::config::Config;
use crate::connection::{Connection, ConnectionError, udp::UdpConnection, ServerAddress, Packet};

struct Server {
    peers: RwLock<Vec<ServerAddress>>,
    heartbeat_millis: AtomicU64,
    tasks: Mutex<JoinSet<Result<(), ConnectionError>>>,
}

impl Server {
    async fn receive_loop(mut connection: impl Connection, incoming: Sender<Packet>, mut outgoing: Receiver<Packet>) -> Result<(), ConnectionError> {
        loop {
            tokio::select! {
                packet = connection.receive() => {
                    let packet = packet?;
                    trace!(?packet, parsed = %String::from_utf8_lossy(&packet.data), "receive");
                    incoming.try_send(packet).expect("TODO: ConnectionError");
                },
                Some(packet) = outgoing.recv() => {
                    trace!(?packet, parsed = %String::from_utf8_lossy(&packet.data), "send");
                    connection.send(packet).await?;
                },
            }
        }
    }

    async fn heartbeat_loop(server: Arc<Self>, mut incoming: Receiver<Packet>, outgoing: Sender<Packet>) -> Result<(), ConnectionError> {
        loop {
            // This should maybe be an atomic for performance,
            // with fine-grained locking on the server instead
            // of a coarse, struct-level lock.
            let interval = Duration::from_millis(server.heartbeat_millis.load(Ordering::Relaxed));
            tokio::select! {
                received = incoming.recv() => match received {
                    None => return Ok(()),
                    Some(packet) => {
                        if packet.data == b"HEARTBEAT" {
                            let reply = Packet {
                                data: "ACK HEARTBEAT".into(),
                                peer: packet.peer,
                            };
                            outgoing.send(reply).await; // TODO
                        }
                    },
                },
                _ = sleep(interval) => {
                    let peers = server.peers.read().await;
                    for peer in peers.iter() {
                        let peer_request = Packet {
                            data: "HEARTBEAT".into(),
                            peer: peer.to_owned(),
                        };
                        outgoing.send(peer_request).await; // TODO
                    }
                }
            }
        } 
    }

    fn start(connection: impl Connection, peers: Vec<ServerAddress>, heartbeat_interval: Duration) -> Result<Arc<Self>, ConnectionError>
    {
        let server = Arc::new(Self {
            peers: RwLock::new(peers),
            heartbeat_millis: AtomicU64::new(heartbeat_interval.as_millis() as u64),
            tasks: Mutex::new(JoinSet::default()),
        });

        let (packets_receive_tx, packets_receive_rx) = tokio::sync::mpsc::channel(32);
        let (packets_send_tx, packets_send_rx) = tokio::sync::mpsc::channel(32);
        let receive_task = Self::receive_loop(connection, packets_receive_tx, packets_send_rx);

        let heartbeat_task = Self::heartbeat_loop(Arc::clone(&server), packets_receive_rx, packets_send_tx.clone());

        {
            let mut tasks = server.tasks.lock().expect("should be exclusive at this point");
            tasks.spawn(receive_task);
            tasks.spawn(heartbeat_task);
        }

        Ok(server)
    }

    async fn run(&self) -> Result<(), ConnectionError> {
        let mut tasks = self.tasks.lock().expect("should be exclusive");
        while let Some(res) = tasks.join_next().await {
            let task_res = res.expect("Task Panicked");
            task_res?;
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
    let server = Server::start(connection, opts.peers, opts.heartbeat_interval).expect("could not start server");
    server.run().await.expect("server.run exited with error");
}
