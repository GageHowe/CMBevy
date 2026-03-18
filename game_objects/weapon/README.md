# Weapons

Weapons are entities that implement the Weapon trait. All weapon kinds have their own marker component.

Weapon systems don't currently run server-side. This keeps complexity low but will need some form of anticheat later.

We want to trust the client with hits for smooth gameplay feel. Raycasts and projectiles are replicated visually, but are run on the owning client.

Weapon logic runs on fixed tick.

TODO: move stuff from weapon.rs to mod.rs
