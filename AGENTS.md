# Agents.md

This file is for getting a fresh Codex/Claude/OpenCode instance productive quickly and keeping it aligned with the intended architecture.

Refer to AGENTS.md for additional instructions.

# AGENTS

## Agent-codebase relationship
* Read as many files as you need to understand the codebase.
* If you don't understand something I ask, look it up.
* If code is commented in lines with lowercase first letters, it's handwritten; be hesitant about changing it.

## Codebase Structure
* Shared crates go in `crates/`.
* gameserver/src/main.rs and client/src/main.rs are for adding plugins, systems, and resources only.

## Software Design
* Everything should be clean and minimal. Every line of code counts against you.
* No hacks. This is for an enterprise-quality game; everything needs to be scalable. Write once, use forever.
* Avoid pulling in new dependencies unless they're both absolutely needed and recently updated
* Please DO NOT create new structs, enums, components, etc if not absolutely necessary.
* Decouple unrelated systems.
* Don't use bevy's events/messages.
* Simplicity: Simplicity and decoupling is everything. I prefer simple-looking imperative code over functional programming or clever one-liners.
* Update schedule: Use Update sparingly to keep framerate fast. Use SlowUpdate for things that don't have to happen each FixedUpdate.
* Do NOT fundamentally change how things work without asking me first. When you add or change things, leave comments justifying why.
* DO NOT git push --force, git reset --hard, rm -rf, etc.
* Before implementing anything or making large changes, assess your proposed solution for scalability, simplicity, and flexibility.
* don't do `use net::message::{etc, etc, etc}`, use wildcard to quickly pull everything.
* Never use Local unless for data we'll definitely want to keep between games.

## Iteration
* When finished with a task, run `make build`.

### Game Design
* All movement and physics should be relative. When firing a projectile, it should inherit the velocity of its owner.
* We need both single-player and multiplayer to work without fuss.

## random other info
* We use postcard for encoding.

## Cleanliness
* Don't put functions and logic in client/gameserver main.rs; use #[cfg(feature = client)] and GameState to gate functionality.

Also see: README.md for project description


## Core Mental Model

This codebase prefers small, blunt, scalable boundaries.

Generic-sounding modules must stay generic.

If a module named after a generic concept starts importing type-specific gameplay code, assume the design is drifting in the wrong direction.

Examples:

- `health.rs` should handle health state, damage, regen, and generic death detection.
- `session` should decide authority.
- each `GameObject` implementation should own its own spawn/death side effects.
- `pawn/biped.rs` should decide what biped death means.
- `pawn/spaceship.rs` should decide what spaceship death means.

## Dependency Direction

Prefer this direction:

- `session` depends on `game_objects`
- `game_objects` depends on lower-level crates like `common`, `net`, `physics`
- generic gameplay modules expose hooks or system sets
- higher-level crates decide policy

Avoid this direction:

- `game_objects` depending on `session`
- generic modules depending on specific gameplay types just to finish their work
- lower-level crates making authority or game-mode decisions

## Responsibility Map

Use these ownership boundaries unless there is a strong reason not to:

- `crates/game_objects/src/health.rs`
  Generic `Health`, `HealthRegen`, damage application, attribution aging, dead-entity detection, calling `GameObject::on_death`.

- `crates/game_objects/src/spawn.rs`
  Dispatch from `GameObjectKind` to the concrete type implementation.

- `crates/game_objects/src/pawn/biped.rs`
  Biped-specific movement, inventory, camera behavior, and biped death consequences.

- `crates/game_objects/src/pawn/spaceship.rs`
  Spaceship-specific movement, occupant handling, and spaceship death consequences.

- `crates/session/src/runtime.rs`
  Session-level policy: authority, respawns, multiplayer flow, match lifecycle.

- `crates/master_plugin/src/lib.rs`
  Shared plugin composition and cross-plugin ordering.

## Simplification Standard

When simplifying:

- prefer deleting code over moving it sideways
- prefer one obvious responsibility per module
- prefer imperative code that is easy to scan
- prefer fewer structs/components unless they clearly buy reuse or boundary clarity

If a cleanup keeps the same amount of code but merely redistributes confusion, it is not a good cleanup.

If a cleanup makes a generic subsystem smaller and pushes object-specific behavior back to the object implementation, that is usually a good cleanup.

## Standards for New Changes

Before changing a system, ask:

1. Is this logic generic or object-specific?
2. Which module name best matches that responsibility?
3. Am I creating a dependency in the wrong direction?
4. Can this be solved by deleting special cases instead of adding another layer?

If you cannot answer those clearly, read more before editing.

## When to Stop and Ask

Stop and ask before:

- changing a core gameplay model rather than its implementation
- changing authority ownership
- changing replication semantics
- introducing a new abstraction that spans multiple crates
- adding a dependency to avoid understanding an existing system

Do not stop and ask for:

- small boundary cleanups that clearly reduce coupling
- moving type-specific logic out of generic modules
- deleting dead or duplicate paths
- adding small helper functions inside the module that owns the behavior

## Review Standard

When reviewing your own work, check for these smells:

- generic module imports specific gameplay types
- one system both detects something and decides game-specific consequences
- authority logic duplicated in multiple crates
- type checks where dynamic dispatch or module ownership should suffice
- “temporary” code that creates a second path instead of removing the old one

## Preferred Style

- Keep code small.
- Keep boundaries obvious.
- Keep responsibilities local.
- Avoid cleverness.
- Avoid new types unless they earn their cost.
- If a line exists only to compensate for a bad boundary, fix the boundary instead.

## Iteration Rule

After finishing meaningful code changes, run:

```bash
make build
```
