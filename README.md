# Raft for Beginngers

## Usage
To start a test "cluster" of 3 nodes, run:

```bash
eval "$(ruby run.rb)"
```

You can then send SIGQUIT to kill the leader `<ctrl>+\`, and or SIGINT to kill all `<ctrl>+c`.

## TODO

- [x] add election timeout min / max range, and randomly wait before calling elections
- [x] validate timeout cli inputs
- [ ] add term
- [ ] send election packets
- [ ] recv election packets, and decide on new leader
