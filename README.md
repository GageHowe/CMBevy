# Critical Mass
### Rewritten with bevy

Critical Mass is a multiplayer and singleplayer physics-based space combat game.

## Social
* Discord: https://discord.gg/ZcKdnnbfXF
* Instagram: https://www.instagram.com/criticalmassdev/

## Testing

```bash
cargo run --bin gameserver
cargo run -p network_emulator -- --loss 0.05 --mindelay 40 --maxdelay 80
cargo run --bin client -- --server 127.0.0.1:42069
```

Or, just run scripts/build_release_windows.ps1 and the Windows build will appear in dist/

## Implementation inspiration
* https://github.com/Henauxg/bevy_quinnet/
* https://github.com/floco2025/cuboid-wars/

## Dev Links
* https://vercel.com/gagehowetamus-projects/off-by-three-website
* https://off-by-three.itch.io/critical-mass
* storefront: https://store.steampowered.com/app/3526510
* package name and settings: https://partner.steamgames.com/store/packagelanding/1244771
* publish changes: https://partner.steamgames.com/apps/publishing/3526510
