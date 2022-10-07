mod config;
mod connection;

use std::collections::HashMap;
use std::iter;
use std::sync::{atomic::{AtomicU64, Ordering}, Arc};
use std::time::Duration;

use clap::Parser;
use tokio::sync::{Mutex, RwLock};
use tokio::sync::mpsc::{Sender, Receiver};
use tokio::task::JoinSet;
use tokio::time::{sleep, sleep_until, Instant};
// use tracing::span::EnteredSpan;
use tracing::{trace, info, warn, error, info_span, instrument, Instrument};
use k9::{assert_greater_than, assert_lesser_than};
use rand::Rng;

use crate::config::Config;
use crate::connection::{Connection, ConnectionError, udp::UdpConnection, ServerAddress, Packet, PacketType::*};

#[derive(Clone, Debug, PartialEq)]
enum ServerState {
    Follower,
    Candidate {
        election_ends_at: Instant,
    },
    Leader,
}

#[derive(Debug)]
struct Server {
    peers: RwLock<Vec<ServerAddress>>,
    state: RwLock<ServerState>,
    // state_span: RwLock<Option<EnteredSpan>>,
    election_results: Mutex<HashMap<ServerAddress, bool>>,
    last_vote: Mutex<Option<ServerAddress>>,
    term: AtomicU64,
    heartbeat_millis: AtomicU64,
    election_timeout_min: AtomicU64,
    election_timeout_max: AtomicU64,
    tasks: Mutex<JoinSet<Result<(), ConnectionError>>>,
}

impl Server {
    /*
    async fn get_current_span(&self, desc: &'static str) -> Option<RwLockReadGuard<Span>> {
        let state_span = self.state_span.read().await;
        if state_span.is_none() {
            return None;
        }

        Some(RwLockReadGuard::map(state_span, |o| o.as_ref().unwrap()))
    }
    */

    fn get_current_duration(&self) -> Duration {
        Duration::from_millis(self.heartbeat_millis.load(Ordering::Relaxed))
    }

    /// get the timeout for a term; either when an election times out,
    /// or timeout for followers not hearing from the leader
    fn get_term_timeout(&self) -> Duration {
        let min = self.election_timeout_min.load(Ordering::Relaxed);
        let max = self.election_timeout_max.load(Ordering::Relaxed);
        // https://stackoverflow.com/questions/19671845/how-can-i-generate-a-random-number-within-a-range-in-rust
        let timeout = rand::thread_rng().gen_range(min..max);
        Duration::from_millis(timeout)
    }

    async fn start_election(&self) {
        // Increment the term
        self.term.fetch_add(1, Ordering::Acquire);
        let current_term = self.term.load(Ordering::Acquire);
        info!(term = ?current_term, "beginning election");

        // (Re)set the state to Candidate and establish a new timeout
        let mut state_mut = self.state.write().await;
        *state_mut = ServerState::Candidate {
            election_ends_at: Instant::now() + self.get_term_timeout(),
        };

        // Reset the election results
        let mut election_results = self.election_results.lock().await;
        let peers = self.peers.read().await;
        *election_results = peers.iter().cloned().zip(iter::repeat(false)).collect();
    }

    async fn handle_append(&self, packet: &Packet) -> Packet {
        // TODO: actually handle the append
        let current_term = self.term.load(Ordering::Acquire);
        let ack = packet.term >= current_term;
        if packet.term > current_term {
            self.term.compare_exchange(current_term, packet.term, Ordering::Acquire, Ordering::Acquire)
                .expect("handle concurrency error");
        }
        Packet {
            message_type: AppendEntriesAck { did_append: ack },
            term: self.term.load(Ordering::Acquire),
            peer: packet.peer.clone(),
        }
    }

    /// Get and send packets!
    async fn connection_loop(connection: impl Connection, incoming: Sender<Packet>, mut outgoing: Receiver<Packet>) -> Result<(), ConnectionError> {
        loop {
            tokio::select! {
                packet = connection.receive() => {
                    let packet = packet?;
                    trace!(?packet, "receive");
                    incoming.try_send(packet).expect("TODO: ConnectionError");
                },
                Some(packet) = outgoing.recv() => {
                    trace!(?packet, "send");
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
            // TODO: make this less bad
            let instant = interval.tick().await;

            let state = (*self.state.read().await).clone();
            match state {
                // 1. we are leader
                ServerState::Leader => {
                    // send send heart beat to peers
                    let peers = self.peers.read().await;
                    for peer in peers.iter() {
                        let peer_request = Packet {
                            message_type: AppendEntries,
                            term: self.term.load(Ordering::Acquire),
                            peer: peer.to_owned(),
                        };
                        outgoing.send(peer_request).await.unwrap();

                        // see if we should be updating the interval, incase the config change somehow
                        let new_interval= self.get_current_duration();
                        if new_interval != current_interval {
                            interval = tokio::time::interval_at(instant + new_interval, new_interval);
                        }
                    }
                },
                // 2. we are candidate  
                ServerState::Candidate { election_ends_at } => {
                        // increment term
                        // send out request for votes
                        // timeout if we don't get enough votes
                    let peers = self.peers.read().await;
                    for peer in peers.iter() {
                        let current_term = self.term.load(Ordering::Acquire);
                        let peer_request = Packet {
                            message_type: VoteRequest { last_log_index: 0, last_log_term: current_term - 1 }, // TODO: THIS IS THE WRONG TERM, it should come from the log and doesn't need the -1
                            term: current_term,
                            peer: peer.to_owned(),
                        };
                        outgoing.send(peer_request).await.unwrap(); // TODO
                    }

                    sleep_until(election_ends_at).await;
                },
                ServerState::Follower => {
                    // 3. we are follower
                        // do nothing for now
                }
            }
        }
    }

    /// Get heartbeats and work out who is dead
    async fn incoming_packet_loop(self: Arc<Self>, mut incoming: Receiver<Packet>, outgoing: Sender<Packet>) -> Result<(), ConnectionError> {
        loop {

            let server_state = (*self.state.read().await).clone();
            match server_state {
                ServerState::Leader => {
                    // 1. we are leader
                        // nothing for now
                    // TODO: read and discard packets off the wire
                    let _discard = incoming.recv().await.ok_or(ConnectionError)?;
                },
                ServerState::Candidate { election_ends_at } => {
                    // 2. we are candidate
                        // if we find a valid leader, become follower
                        // become leader if we get majority of votes
                    tokio::select! {
                        received = incoming.recv() => match received {
                            None => return Ok(()),
                            Some(packet) => {
                                // TODO: make this less naive
                                match packet.message_type {
                                    AppendEntries => {
                                        let reply = self.handle_append(&packet).await;
                                        if let Packet { message_type: AppendEntriesAck { did_append: true }, .. } = reply {
                                            info!(peer = ?packet.peer, "got valid leader packet; becoming follower");
                                            let mut state_mut = self.state.write().await;
                                            *state_mut = ServerState::Follower;
                                        } else {
                                            warn!(peer = ?packet.peer, "got invalid leader packet; ignoring");
                                        }
                                        outgoing.send(reply).await.unwrap(); // TODO
                                    },
                                    VoteResponse { is_granted } => {
                                        let current_term = self.term.load(Ordering::Acquire);
                                        info!(peer = ?packet.peer, term = ?packet.term, is_granted, "got a vote response");

                                        let mut election_results = self.election_results.lock().await;

                                        // store the result from the vote
                                        election_results.insert(packet.peer, is_granted);

                                        // count votes and nodes
                                        // add 1 to each so we count ourselves
                                        let vote_cnt = election_results.values().filter(|v| **v).count() + 1;
                                        let node_cnt = self.peers.read().await.len() + 1;
                                        info!(?vote_cnt, ?node_cnt, "vote count, node count");

                                        // did we get more than half the votes, including our own?
                                        if vote_cnt > node_cnt / 2 {
                                            info!(votes = %vote_cnt, term = ?current_term, "won election; becoming Leader");
                                            let mut state_mut = self.state.write().await;
                                            *state_mut = ServerState::Leader;
                                        }
                                    },
                                    _ => {
                                        // do nothing with other packets, as we wait for timeout of the election
                                    },
                                }
                            }
                        },
                    
                        // timer that goes off when election timed out
                        // TODO: doesn't work
                        _ = sleep_until(election_ends_at) => {
                            info!("Election timeout; restarting election");
                            self.start_election().await
                        }

                    }
                },
                ServerState::Follower => {
                    // 3. we are follower
                        // if we get a heartbeat, reset our timeout
                        // if we timeout, become candidate
                        // send vote if valid vote request

                    let interval = self.get_term_timeout();
                    tokio::select! {
                        // process heartbeats from leader
                        received = incoming.recv() => match received {
                            None => return Ok(()),
                            Some(packet) => {
                                // TODO: make this less naive
                                match packet.message_type {
                                    AppendEntries => {
                                        let reply = self.handle_append(&packet).await;
                                        outgoing.send(reply).await.unwrap(); // TODO
                                    },
                                    VoteRequest { .. } => {
                                        let mut current_term = self.term.load(Ordering::Acquire);
                                        let vote_granted = if packet.term >= current_term {
                                            let mut last_vote = self.last_vote.lock().await;

                                            if packet.term > current_term {
                                                // the term in the packet is newer, so update, and also clear any existing vote...
                                                self.term.compare_exchange(current_term, packet.term, Ordering::Acquire, Ordering::Acquire).expect("Handle concurrency failure");
                                                current_term = packet.term;
                                                *last_vote = None;
                                            }

                                            match &*last_vote {
                                                None => { 
                                                    // We didn't vote... --> true vote
                                                    true
                                                },
                                                Some(p) if p == &packet.peer => {
                                                    // or we voted for this peer already --> true vote
                                                    // TODO: Is packet last_log_index last_log_term as up to date as our log?
                                                    true
                                                },
                                                Some(_) => {
                                                    // We already voted, and it wasn't for this peer --> False vote
                                                    false
                                                }
                                            }
                                        } else {
                                            // Packet term is too old
                                            false
                                        };
                                        
                                        let reply = Packet {
                                            message_type: VoteResponse { is_granted: vote_granted },
                                            term: current_term,
                                            peer: packet.peer,
                                        };
                                        info!(candidate = ?reply.peer, term = ?reply.term, ?vote_granted, "casting vote");
                                        outgoing.send(reply).await.unwrap();
                                    },
                                    _ => { error!(state = ?server_state, ?packet, "unexpected packet"); },
                                }
                            },
                        },

                        // timer that goes off when leader timed out
                        _ = sleep(interval) => {
                            info!("Leader timeout after {:?}; becoming candidate", interval);
                            self.start_election().await;
                        }
                    }
                }
            }
        } 
    }

    pub fn start(connection: impl Connection, peers: Vec<ServerAddress>, heartbeat_interval: Duration, election_timeout_min: Duration, election_timeout_max: Duration) -> Result<Arc<Self>, ConnectionError>
    {
        let server = Arc::new(Self {
            peers: RwLock::new(peers),
            state: RwLock::new(ServerState::Follower),
            // state_span: RwLock::new(None),
            term: AtomicU64::new(0),
            last_vote: Mutex::new(None),
            election_results: Mutex::new(HashMap::new()),
            heartbeat_millis: AtomicU64::new(heartbeat_interval.as_millis() as u64),
            election_timeout_min: AtomicU64::new(election_timeout_min.as_millis() as u64),
            election_timeout_max: AtomicU64::new(election_timeout_max.as_millis() as u64),
            tasks: Mutex::new(JoinSet::default()),
        });

        let (packets_receive_tx, packets_receive_rx) = tokio::sync::mpsc::channel(32);
        let (packets_send_tx, packets_send_rx) = tokio::sync::mpsc::channel(32);

        {
            let run_span = info_span!("server", address = ?connection.address());
            let mut tasks = server.tasks.try_lock().expect("should be exclusive at this point");
            tasks.spawn(Self::connection_loop(connection, packets_receive_tx, packets_send_rx).instrument(run_span));
            
            tasks.spawn(Arc::clone(&server).send_heartbeat_loop(packets_send_tx.clone()));
            tasks.spawn(Arc::clone(&server).incoming_packet_loop(packets_receive_rx, packets_send_tx));
        }

        info!("Starting Server");
        Ok(server)
    }

    #[instrument]
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
        .with(tracing_forest::ForestLayer::default())
        .init();

    let opts = Config::parse();

    // make sure the opts are ok
    assert_greater_than!(opts.heartbeat_interval, Duration::new(0, 0));
    assert_lesser_than!(opts.heartbeat_interval, opts.election_timeout_min);
    assert_lesser_than!(opts.election_timeout_min, opts.election_timeout_max);

    let connection = UdpConnection::bind(opts.listen_socket).await.unwrap();
    let server = Server::start(connection, opts.peers, opts.heartbeat_interval, opts.election_timeout_min, opts.election_timeout_max).expect("could not start server");
    server.run().await.expect("server.run exited with error");
}
