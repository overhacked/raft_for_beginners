peers = %w{8000 8001 8002}

(0..(peers.size - 1)).each do
    puts "target/release/raft_for_beginners -l 127.0.0.1:#{peers[0]} --peer 127.0.0.1:#{peers[1]} --peer 127.0.0.1:#{peers[2]} &"
    peers.rotate!
end

puts "wait"