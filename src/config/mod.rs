use std::{time::Duration, num::ParseIntError};

use clap::Parser;

use super::ServerAddress;

#[derive(Parser, Debug)]
pub struct Config {
    /// The address to listen on
    #[clap(short, long, default_value = "[::]:8000")]
    pub listen_socket: ServerAddress,

    /// A peer to connect to (use multiple times for multiple peers)
    #[clap(short, long = "peer")]
    pub peers: Vec<ServerAddress>,

    /// Heartbeat interval in milliseconds
    #[clap(long = "heartbeat", parse(try_from_str = parse_millis), default_value = "500")]
    pub heartbeat_interval: Duration,
}

fn parse_millis(s: &str) -> Result<Duration, ParseIntError> {
    let millis: u64 = s.parse()?;
    Ok(Duration::from_millis(millis))
}