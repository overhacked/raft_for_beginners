mod config;
mod connection;

use std::collections::HashMap;
use std::iter;
use std::sync::{atomic::{AtomicU64, Ordering}, Arc};
use std::time::Duration;

use clap::Parser;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, RwLock};
use tokio::sync::mpsc::{Sender, Receiver};
use tokio::task::JoinSet;
use tokio::time::{sleep, sleep_until, Instant};
// use tracing::span::EnteredSpan;
use tracing::{trace, debug, info, warn};
use k9::{assert_greater_than, assert_lesser_than};
use rand::Rng;

use crate::config::Config;
use crate::connection::{Connection, ConnectionError, udp::UdpConnection, ServerAddress, Packet, PacketType::*};

#[derive(Clone, Debug, PartialEq)]
enum ServerState {
    Follower,
    Candidate,
    Leader,
}

#[derive(Debug)]
struct Server {
    address: ServerAddress,
    peers: RwLock<Vec<ServerAddress>>,
    state: RwLock<ServerState>,
    // state_span: RwLock<Option<EnteredSpan>>,
    election_results: Mutex<HashMap<ServerAddress, bool>>,
    last_vote: Mutex<Option<ServerAddress>>,
    term: AtomicU64,
    term_timeout: RwLock<Instant>,
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

    async fn update_term(&self, packet: &Packet) -> u64 {
        let current_term = self.term.load(Ordering::Acquire);
        if packet.term > current_term {
            self.term.store(packet.term, Ordering::Release);
            *(self.state.write().await) = ServerState::Follower;
            self.reset_vote().await;
        }
        packet.term
    }

    fn generate_random_timeout(min: u64, max: u64) -> Instant {
        let new_timeout_millis = rand::thread_rng().gen_range(min..max);
        Instant::now() + Duration::from_millis(new_timeout_millis)
    }

    /// get the timeout for a term; either when an election times out,
    /// or timeout for followers not hearing from the leader
    async fn reset_term_timeout(&self) {
        let min = self.election_timeout_min.load(Ordering::Relaxed);
        let max = self.election_timeout_max.load(Ordering::Relaxed);
        // https://stackoverflow.com/questions/19671845/how-can-i-generate-a-random-number-within-a-range-in-rust
        let mut timeout_mut = self.term_timeout.write().await;
        *timeout_mut = Self::generate_random_timeout(min, max);
    }

    async fn reset_vote(&self) {
        let mut vote = self.last_vote.lock().await;
        *vote = None;
    }

    async fn increment_term(&self) -> u64 {
        self.term.fetch_add(1, Ordering::Acquire);
        self.reset_vote().await;
        self.term.load(Ordering::Acquire)
    }

    async fn start_election(&self) {
        // Increment the term
        let current_term = self.increment_term().await;
        info!(term = ?current_term, "beginning election");

        // (Re)set the state to Candidate and establish a new timeout
        let mut state_mut = self.state.write().await;
        *state_mut = ServerState::Candidate;

        // Reset the election results
        let mut election_results = self.election_results.lock().await;
        let peers = self.peers.read().await;
        *election_results = peers.iter().cloned().zip(iter::repeat(false)).collect();
        let mut voted_for = self.last_vote.lock().await;
        *voted_for = Some(self.address.clone());

        // Reset the timeout
        self.reset_term_timeout().await;
    }

    async fn handle_appendentries(&self, packet: &Packet) -> Option<Packet> {
        // TODO: actually handle the append
        let current_term = self.term.load(Ordering::Acquire);
        let ack = packet.term == current_term;

        if ack {
            self.reset_term_timeout().await;
        }

        let state = (*self.state.read().await).clone();
        match state {
            ServerState::Leader => {
                return None; // Drop any AppendEntries if leader
            },
            ServerState::Candidate if ack => {
                info!(peer = ?packet.peer, term = ?packet.term, "got valid leader packet; becoming follower");
                let mut state_mut = self.state.write().await;
                *state_mut = ServerState::Follower;
                // TODO: append to log or wait for next AppendEntries?
            },
            ServerState::Follower if ack => {
                // TODO: append to log
            },
            _ => {
                // ack == false, continue to reply
            },
        }
        //else {
        //    warn!(peer = ?packet.peer, term = ?packet.term, "got invalid leader packet; ignoring");
        //}

        let reply = Packet {
            message_type: AppendEntriesAck { did_append: ack },
            term: current_term,
            peer: packet.peer.clone(),
        };
        Some(reply)
    }

    /// Handle VoteRequest packets
    async fn handle_voterequest(&self, packet: &Packet) -> Packet {
        let current_term = self.term.load(Ordering::Acquire);
        let vote_granted = if packet.term == current_term {
            self.reset_term_timeout().await;
            let mut last_vote = self.last_vote.lock().await;
            match &*last_vote {
                None => { 
                    // We didn't vote... --> true vote
                    *last_vote = Some(packet.peer.clone());
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
            peer: packet.peer.clone(),
        };
        info!(candidate = ?reply.peer, term = ?reply.term, ?vote_granted, "casting vote");
        reply
    }

    /// Handle VoteResponse packets
    async fn handle_voteresponse(&self, packet: &Packet) {
        let current_term = self.term.load(Ordering::Acquire);
        if packet.term != current_term {
            warn!(peer = ?packet.peer, term = ?packet.term, "got a vote response for the wrong term");
            return;
        }

        let is_granted = if let Packet { message_type: VoteResponse { is_granted }, .. } = packet { *is_granted } else { false };
        info!(peer = ?packet.peer, term = ?packet.term, is_granted, "got a vote response");

        let mut election_results = self.election_results.lock().await;

        // store the result from the vote
        election_results.insert(packet.peer.clone(), is_granted);

        // count votes and nodes
        // add 1 to each so we count ourselves
        let vote_cnt = election_results.values().filter(|v| **v).count() + 1;
        let node_cnt = self.peers.read().await.len() + 1;
        info!(?vote_cnt, ?node_cnt, "vote count, node count");

        // did we get more than half the votes, including our own?
        if vote_cnt > node_cnt / 2 {
            let mut state_mut = self.state.write().await;
            if let ServerState::Candidate = *state_mut {
                info!(votes = %vote_cnt, term = ?self.term.load(Ordering::Acquire), "won election; becoming Leader");
                *state_mut = ServerState::Leader;
            } else {
                debug!("got vote in already-won election");
            }
        }
    }

    /// Handle signals
    async fn signal_handler(self: Arc<Self>) -> Result<(), ConnectionError> {
        use tokio::signal::unix::{signal, SignalKind};

        let mut usr1_stream = signal(SignalKind::user_defined1()).expect("signal handling failed");
        let mut stdout = tokio::io::stdout();

        loop {
            tokio::select! {
                _ = usr1_stream.recv() => {
                    let state = (*self.state.read().await).clone();
                    info!(?state, "SIGUSR1");
                },
                _ = sleep(Duration::from_secs(1)) => {
                    let server_state = self.state.read().await;
                    let state_string = format!("\x1Bk{:?}\x1B", *server_state);
                    // println!("\x1Bk{:?}\x1B", *server_state);
                    let _yeet = stdout.write_all(state_string.as_bytes()).await;
                    let _yeet = stdout.flush().await;
                }
            }
        }
    }
    /// Get and send packets!
    async fn connection_loop(connection: impl Connection, incoming: Sender<Packet>, mut outgoing: Receiver<Packet>) -> Result<(), ConnectionError> {
        loop {
            tokio::select! {
                packet = connection.receive() => {
                    let packet = packet?;
                    trace!(?packet, "receive");
                    //incoming.try_send(packet).expect("TODO: ConnectionError");
                    let _ = incoming.send(packet).await; // DEBUG: try ignoring send errors
                },
                Some(packet) = outgoing.recv() => {
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
                ServerState::Candidate => {
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

                    let election_timeout = (*self.term_timeout.read().await).clone();
                    loop {
                        let now = interval.tick().await;
                        match *self.state.read().await {
                            ServerState::Candidate => {
                                if now >= election_timeout {
                                    info!("Election timeout; restarting election");
                                    self.start_election().await;
                                    break;
                                }
                            },
                            _ => { break; },
                        }

                    }
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
            let next_timeout = *self.term_timeout.read().await;
            tokio::select! {
                Some(packet) = incoming.recv() => {
                    self.update_term(&packet).await;
                    match packet.message_type {
                        VoteRequest { .. } => {
                            let reply = self.handle_voterequest(&packet).await;
                            outgoing.send(reply).await.unwrap(); // TODO
                        },
                        VoteResponse { .. } => {
                            self.handle_voteresponse(&packet).await;
                        },
                        AppendEntries => {
                            if let Some(reply) = self.handle_appendentries(&packet).await {
                                outgoing.send(reply).await.unwrap(); // TODO
                            }
                        },
                        AppendEntriesAck { .. } => {
                            // TODO: commit in log
                        },
                    }
                },
                _ = sleep_until(next_timeout), if !matches!(server_state, ServerState::Leader) => {
                    info!("Election timeout; restarting election");
                    self.start_election().await
                },
                else => return Ok(()),  // No more packets, shutting down
            }
        } 
    }

    pub fn start(connection: impl Connection, peers: Vec<ServerAddress>, heartbeat_interval: Duration, election_timeout_min: Duration, election_timeout_max: Duration) -> Result<Arc<Self>, ConnectionError>
    {
        let election_timeout_min = election_timeout_min.as_millis() as u64;
        let election_timeout_max = election_timeout_max.as_millis() as u64;
        let term_timeout = Self::generate_random_timeout(election_timeout_min, election_timeout_max);
        let server = Arc::new(Self {
            address: connection.address(),
            peers: RwLock::new(peers),
            state: RwLock::new(ServerState::Follower),
            // state_span: RwLock::new(None),
            term: AtomicU64::new(0),
            term_timeout: RwLock::new(term_timeout),
            last_vote: Mutex::new(None),
            election_results: Mutex::new(HashMap::new()),
            heartbeat_millis: AtomicU64::new(heartbeat_interval.as_millis() as u64),
            election_timeout_min: AtomicU64::new(election_timeout_min),
            election_timeout_max: AtomicU64::new(election_timeout_max),
            tasks: Mutex::new(JoinSet::default()),
        });

        let (packets_receive_tx, packets_receive_rx) = tokio::sync::mpsc::channel(32);
        let (packets_send_tx, packets_send_rx) = tokio::sync::mpsc::channel(32);

        {
            //let run_span = info_span!("server", address = ?connection.address());
            let mut tasks = server.tasks.try_lock().expect("should be exclusive at this point");
            tasks.spawn(Arc::clone(&server).signal_handler());
            tasks.spawn(Self::connection_loop(connection, packets_receive_tx, packets_send_rx));
            
            tasks.spawn(Arc::clone(&server).send_heartbeat_loop(packets_send_tx.clone()));
            tasks.spawn(Arc::clone(&server).incoming_packet_loop(packets_receive_rx, packets_send_tx));
        }

        info!("Starting Server");
        Ok(server)
    }

    async fn run(&self) -> Result<(), ConnectionError> {
        let mut tasks = self.tasks.try_lock().expect("should be exclusive");
        while let Some(res) = tasks.join_next().await {
            let task_res = res.expect("Task Panicked");
            task_res?;
            debug!("Task Exited");
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
