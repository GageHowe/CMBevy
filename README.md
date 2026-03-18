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

## Timing
* Physics runs on FixedUpdate
* Planet/Atmosphere forces are on FixedUpdate before physics, and also run during reconciliation
* 

## Links
* https://vercel.com/gagehowetamus-projects/off-by-three-website
* https://off-by-three.itch.io/critical-mass
* storefront: https://store.steampowered.com/app/3526510
* package name and settings: https://partner.steamgames.com/store/packagelanding/1244771
* publish changes: https://partner.steamgames.com/apps/publishing/3526510

