use clap::Parser;

use super::ServerAddress;

#[derive(Parser, Debug)]
pub struct Config {
    #[clap(short, long, default_value = "[::]:8000")]
     pub listen_socket: ServerAddress,
}
