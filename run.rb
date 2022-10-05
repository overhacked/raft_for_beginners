puts "cargo build --release --quiet"

log_level = "trace"
peers = %w{8000 8001 8002}

leader = false

(0..(peers.size - 1)).each do
    unless leader
        puts "RUST_LOG=\"#{log_level}\" target/release/raft_for_beginners --leader -l 127.0.0.1:#{peers[0]} --peer 127.0.0.1:#{peers[1]} --peer 127.0.0.1:#{peers[2]} &"
        leader = true
    else 
        puts "RUST_LOG=\"#{log_level}\" target/release/raft_for_beginners -l 127.0.0.1:#{peers[0]} --peer 127.0.0.1:#{peers[1]} --peer 127.0.0.1:#{peers[2]} &"
    end
    peers.rotate!
end

puts "trap 'pkill raft_for_beginners; trap - INT' INT"
puts "wait"
