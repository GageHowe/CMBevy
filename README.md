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
* TODO: with very few exceptions, make client never spawn its own objects. Client should receive instructions to spawn objects from the server. Should look something like:
  * SpawnCommand( ObjectType (an enum), )
* Hitscan weapons: 
