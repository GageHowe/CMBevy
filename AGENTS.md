# AGENTS.md

This file is for getting a fresh Codex/Claude/OpenCode instance productive quickly and keeping it aligned with the intended architecture.

* Read as many files as you need to understand the codebase.
* If you don't understand something I ask, look it up or clarify.
* If code is commented in lines with lowercase first letters, it's handwritten; be hesitant about changing it.
* Use minimal words/tokens. Use `cp` instead of regenerating files
* Drop pleasantries, filler, hedging, niceties, and repetition.
* State assumptions briefly.
* If detail is necessary for correctness, include it.
* When fixing bugs:
  * Find root cause, exact fix, minimal patch.
* When implementing new features:
  * Make MVP, no extra features.
  * No hacks.
  * No new structs when old struct do fine.
* Everything should be clean and minimal. Every line of code counts against you.
* No "special-case" systems.
* Decouple unrelated systems.
* Don't use bevy's events/messages.
* Simplicity: Simplicity and decoupling is everything. I prefer simple-looking imperative code over functional programming or clever one-liners.
* Schedules: Use Update sparingly to keep framerate fast. Use SlowUpdate for things that don't have to happen each FixedUpdate.
* Before implementing anything or making large changes, assess your proposed solution for scalability, simplicity, and flexibility.
* Never use Local unless for data we'll definitely want to keep in between games.

Linear damping is BANNED.

* When finished with a task, use make to build, addressing warnings (and test if necessary)
* All movement and physics should be relative. When attaching, detaching, or spawning anything, it should inherit the velocity of its owner.
* We need both single-player and multiplayer to work without fuss.
* Don't put functions and logic in client/gameserver main.rs; use #[cfg(feature = client)] and GameState to gate functionality.
* If a cleanup makes a generic subsystem smaller and pushes object-specific behavior back to the object implementation, that is usually a good cleanup.
* when asked to reduce code size, 

## style and summary
- Keep code small.
- Keep boundaries obvious.
- Keep responsibilities local.
- Avoid cleverness.
- Avoid new types unless they earn their cost.
