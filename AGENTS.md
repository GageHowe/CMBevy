# Agents.md

This file is for getting a fresh Codex/Claude/OpenCode instance productive quickly and keeping it aligned with the intended architecture.

Refer to AGENTS.md for additional instructions.

# AGENTS

## Agent-codebase relationship
* Read as many files as you need to understand the codebase.
* If you don't understand something I ask, look it up or clarify.
* If code is commented in lines with lowercase first letters, it's handwritten; be hesitant about changing it.

## Grug
* Use minimal words.
* Adopt the grug brain mentality: less tokens = good.
* Drop pleasantries, filler, hedging, niceties, and repetition.
* State assumptions briefly.
* If detail is necessary for correctness, include it.
* When fixing bugs:
  * Find root cause, exact fix, minimal patch.
* When implementing new features:
  * Make MVP, no extra features.
  * No hacks.
  * No new structs when old struct do fine.

## Software Design
* Everything should be clean and minimal. Every line of code counts against you.
* Decouple unrelated systems.
* Don't use bevy's events/messages.
* Simplicity: Simplicity and decoupling is everything. I prefer simple-looking imperative code over functional programming or clever one-liners.
* Schedules: Use Update sparingly to keep framerate fast. Use SlowUpdate for things that don't have to happen each FixedUpdate.
* Before implementing anything or making large changes, assess your proposed solution for scalability, simplicity, and flexibility.
* Never use Local unless for data we'll definitely want to keep in between games.

## Iteration
* When finished with a task, run `make build`.

### Game Design
* All movement and physics should be relative. When firing a projectile, it should inherit the velocity of its owner.
* We need both single-player and multiplayer to work without fuss.

## Cleanliness
* Don't put functions and logic in client/gameserver main.rs; use #[cfg(feature = client)] and GameState to gate functionality.

Also see: README.md for project description

## Core Mental Model

If a module named after a generic concept starts importing type-specific gameplay code, assume the design is drifting in the wrong direction.

Examples:

- `health.rs` should handle health state, damage, regen, and generic death detection.
- `session` should decide authority.
- each `GameObject` implementation should own its own spawn/death side effects.
- `pawn/biped.rs` should decide what biped death means.
- `pawn/spaceship.rs` should decide what spaceship death means.

prefer imperative code that is easy to scan

If a cleanup keeps the same amount of code but merely redistributes confusion, it is not a good cleanup.

If a cleanup makes a generic subsystem smaller and pushes object-specific behavior back to the object implementation, that is usually a good cleanup.

## Standards for New Changes

Before changing a system, ask:

1. Is this logic generic or object-specific?
2. Which module name best matches that responsibility?
3. Am I creating a dependency in the wrong direction?
4. Can this be solved by deleting special cases instead of adding another layer?

If you cannot answer those clearly, read more before editing.

## Preferred Style

- Keep code small.
- Keep boundaries obvious.
- Keep responsibilities local.
- Avoid cleverness.
- Avoid new types unless they earn their cost.
- If a line exists only to compensate for a bad boundary, fix the boundary instead.

refer to makefile for build commands, but use this in most cases
```bash
make build
```
