# CLAUDE

## Design
* Everything should be clean and minimal. Every line of code counts against you.
* Read as many files as you need to understand the codebase.
* If you don't understand something I ask, look it up.
* No hacks. This is for an enterprise-quality game; everything needs to be scalable. Write once, use forever.
* Duplication is ok if it means we keep game code flexible and modular.
* DO NOT rewrite my comments, or add comments to code that's already commented
* Avoid pulling in new dependencies unless they're both absolutely needed and recently updated
* Please DO NOT create new structs, enums, components, etc if not absolutely necessary.

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
* We use wincode for encoding, since bincode is dead.
* I'm done trying to minimize gameserver binary size - simplicity is more important.
* Also, it's probably ok to put modules in `client` if they will absolutely not ever be used or referenced from gameserver.
* 
