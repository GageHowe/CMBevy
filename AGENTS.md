# AGENTS.md

This file is for getting a fresh LLM instance productive quickly and keeping it aligned with the intended architecture.

## hard requirements
* Never use Local unless for data we'll definitely want to keep in between games.
* If you don't understand something I ask, look it up or clarify.
* No special-case systems. We're trying to minimize Update overhead.
* Linear damping is banned.
* Use #[cfg(feature = client)] and GameState to gate functionality. Keep all rendering out of the headless gameserver.
* Refactoring changes should reduce the total LOC size of the codebase, not increase it, with very few exceptions.
* Do not create wrappers if they don't justify the indirection.

## rules
* Read as many files as you need to understand the codebase.
* If code is commented in lines with lowercase first letters, it's handwritten; be hesitant about changing/removing it. DO NOT delete todos.
* Use minimal words/tokens.
* State assumptions briefly.
* If detail is necessary for correctness, include it.
* When fixing bugs:
  * Find root cause, exact fix, minimal patch.
* When implementing new features:
  * Make MVP, no extra features.
  * No hacks.
  * No new structs when old struct do fine.
* Everything should be clean and minimal. Every line of code counts against you.
* Decouple unrelated systems.
* Don't use bevy's events/messages.
* Simplicity: Simplicity and decoupling is everything. I prefer simple-looking imperative code over functional programming or clever one-liners.
* Schedules: Use Update sparingly to keep framerate fast. Use SlowUpdate for things that don't have to happen each FixedUpdate.

* When finished with a task, use make to build, addressing warnings (and test if necessary)
* All movement and physics should be relative. When attaching, detaching, or spawning anything, it should inherit the velocity of its owner.
* We need both single-player and multiplayer to work without fuss.
* If a cleanup makes a generic subsystem smaller and pushes object-specific behavior back to the object implementation, that is usually a good cleanup.

## efficiency
* Drop pleasantries, filler, hedging, niceties, repetition, and "if you want,..."

## style
- Keep code small.
- Keep boundaries obvious.
- Keep responsibilities local.
- Avoid cleverness.
- Avoid new types unless they earn their cost.
