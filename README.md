# Critical Mass
### Rewritten with Bevy

To run:
* Server: `cargo run --bin server`
* Client: `cargo run --bin client`

## Implementation inspiration

Codebases to reference as a sanity check
* https://github.com/Henauxg/bevy_quinnet/
* https://github.com/floco2025/cuboid-wars/

## Notes
* Server sends `SpawnCommand` messages to clients to instruct them what objects to create. The client no longer spawns its own dynamic objects directly.
* Hitscan weapons: 
