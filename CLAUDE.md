# CLAUDE

## Design
* Everything should be clean and minimal. Every line of code counts against you.
* Read as many files as you need to understand the codebase.
* If you don't understand something I ask, look it up.
* No hacks. This is for an enterprise-quality game; everything needs to be scalable. Write once, use forever.
* DO NOT rewrite my comments.
* Avoid pulling in new dependencies unless they're both absolutely needed and recently updated
* Please DO NOT create new structs, enums, components, etc if not absolutely necessary.
* Decouple unrelated systems.
* Don't use bevy's events/messages.
* Simplicity is everything. When in doubt, choose the lowest-additional-code implementation.
* Use Update sparingly. Use SlowUpdate for things that don't have to happen each FixedUpdate.

Also see: README.md for project description

## Permissions
* DO NOT git push --force, git reset --hard, rm -rf, etc.
* Complete modules: these are considered complete, you aren't allowed to touch them, but ask me if you believe it's necessary:
  * common/lib.rs
  * common::types
  * common::tick
  * common::slow_update
  * common::macros
  * common::ring_buffer
  * any Cargo.toml or config.toml

## random other info
* We use postcard for encoding, since bincode is dead.
* It's probably ok to put modules in `client` if they will absolutely not ever be used or referenced from gameserver. But as a default, put things in common
* 

Build client in the background in between tasks that touch client or common.

All movement and physics should be relative. When firing a projectile, it should inherit the velocity of its owner.
