mod config;
mod connection;

use std::sync::{atomic::{AtomicU64, Ordering}, Arc};
use std::time::Duration;

use clap::Parser;
use tokio::sync::{Mutex, RwLock};
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
    fn get_current_duration(&self) -> Duration {
        Duration::from_millis(self.heartbeat_millis.load(Ordering::Relaxed))
    }

    /// Get and send packets!
    async fn connection_loop(mut connection: impl Connection, incoming: Sender<Packet>, mut outgoing: Receiver<Packet>) -> Result<(), ConnectionError> {
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

    /// Send our heartbeat out to our peers!
    async fn send_heartbeat_loop(self: Arc<Self>, outgoing: Sender<Packet>) -> Result<(), ConnectionError> {
        let current_interval = self.get_current_duration();
        let mut interval = tokio::time::interval(current_interval);

        loop {
            let instant = interval.tick().await;
            let peers = self.peers.read().await;
            for peer in peers.iter() {
                let peer_request = Packet {
                    data: "HEARTBEAT".into(),
                    peer: peer.to_owned(),
                };
                outgoing.send(peer_request).await.unwrap(); // TODO
                let new_interval= self.get_current_duration();
                if new_interval != current_interval {
                    interval = tokio::time::interval_at(instant + new_interval, new_interval);
                }
            }
        }
    }

    /// TODO: Leader packet processing
    async fn process_follower_messages_loop(self: Arc<Self>, mut incoming: Receiver<Packet>, _outgoing: Sender<Packet>) -> Result<(), ConnectionError> {
        while let Some(_packet) = incoming.recv().await {
            // DO ABSOLUTELY NOTHING
        }
        Ok(())
    }

    /// Get heartbeats and work out who is dead
    async fn process_leader_heartbeat_loop(self: Arc<Self>, mut incoming: Receiver<Packet>, outgoing: Sender<Packet>) -> Result<(), ConnectionError> {
        loop {
            // get the next peer we expect to timeout
            let interval = self.get_current_duration() * 2;

            tokio::select! {
                // process heartbeats from leader
                received = incoming.recv() => match received {
                    None => return Ok(()),
                    Some(packet) => {
                        if packet.data == b"HEARTBEAT" {
                            let reply = Packet {
                                data: "ACK HEARTBEAT".into(),
                                peer: packet.peer,
                            };
                            outgoing.send(reply).await.unwrap(); // TODO
                        }
                    },
                },

                // timer that goes off when leader timed out
                _ = sleep(interval) => {
                    tracing::warn!("Leader timeout, would start an election here");
                }
            }
        } 
    }

    pub fn start(connection: impl Connection, is_leader: bool, peers: Vec<ServerAddress>, heartbeat_interval: Duration) -> Result<Arc<Self>, ConnectionError>
    {
        let server = Arc::new(Self {
            peers: RwLock::new(peers),
            heartbeat_millis: AtomicU64::new(heartbeat_interval.as_millis() as u64),
            tasks: Mutex::new(JoinSet::default()),
        });

        let (packets_receive_tx, packets_receive_rx) = tokio::sync::mpsc::channel(32);
        let (packets_send_tx, packets_send_rx) = tokio::sync::mpsc::channel(32);

        {
            let mut tasks = server.tasks.try_lock().expect("should be exclusive at this point");
            tasks.spawn(Self::connection_loop(connection, packets_receive_tx, packets_send_rx));
            if is_leader {
                tasks.spawn(Arc::clone(&server).send_heartbeat_loop(packets_send_tx.clone()));
                tasks.spawn(Arc::clone(&server).process_follower_messages_loop(packets_receive_rx, packets_send_tx));
            } else {
                //tasks.spawn(Self::process_leader_heartbeat_loop(Arc::clone(&server), packets_receive_rx, packets_send_tx.clone()));
                tasks.spawn(Arc::clone(&server).process_leader_heartbeat_loop(packets_receive_rx, packets_send_tx));
            }
        }

        tracing::info!("Starting Server");
        Ok(server)
    }

    async fn run(&self) -> Result<(), ConnectionError> {
        let mut tasks = self.tasks.try_lock().expect("should be exclusive");
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
        EnvFilter,
        prelude::*,
        filter::LevelFilter,
    };

    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();
    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_tree::HierarchicalLayer::new(2))
        .init();

    let opts = Config::parse();

    let connection = UdpConnection::bind(opts.listen_socket).await.unwrap();
    let server = Server::start(connection, opts.leader, opts.peers, opts.heartbeat_interval).expect("could not start server");
    server.run().await.expect("server.run exited with error");
}
