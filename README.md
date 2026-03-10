# Critical Mass
### Rewritten with Bevy

TODO: https://claude.ai/chat/3c747893-7084-41b4-9b23-757215f1fc80

To run:
* Server: `cargo run --bin gameserver`
* Client: `cargo run --bin client`

To build release:
* `cargo build --release --bin gameserver`
* `cargo build --release --bin client`

## Implementation inspiration

Codebases to reference as a sanity check
* https://github.com/Henauxg/bevy_quinnet/
* https://github.com/floco2025/cuboid-wars/

## Notes
* TODO: with very few exceptions, make client never spawn its own objects. Client should receive instructions to spawn objects from the server. Should look something like:
  * SpawnCommand( ObjectType (an enum), )
* Hitscan weapons: 
