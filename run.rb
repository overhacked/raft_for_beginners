puts "cargo build --release --quiet"
log_level = ENV.fetch('RUST_LOG', "info")

number_of_peers = 4
peers = (0..number_of_peers-1).map {|i| 8000 + i}

puts "tmux new-session -s raft_for_beginners -d"
puts "tmux set remain-on-exit on"

(0..(peers.size - 1)).each do
    this_node = peers[0]
    peers_for_this_node = peers[1..-1]
    puts "tmux new-window -a -t raft_for_beginners:\\$ -n 127.0.0.1:#{this_node} -e 'RUST_LOG=#{log_level}' target/release/raft_for_beginners -l 127.0.0.1:#{this_node} " + peers_for_this_node.map {|peer_port| "--peer 127.0.0.1:#{peer_port}" }.join(" ")
    peers.rotate!
end

puts "trap 'pkill raft_for_beginners; trap - INT' INT"
#puts "tmux attach -t raft_for_beginners"
#puts "wait"
