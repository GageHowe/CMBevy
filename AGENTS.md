# AGENTS.md

This file is for getting a fresh LLM instance productive quickly and keeping it aligned with the intended architecture.

## hard requirements
* Never use Local unless for data we'll definitely want to keep in between games.
* If you don't understand something I ask, look it up or clarify instead of guessing.
* No special-case systems. We're trying to minimize Update overhead.
* Linear damping is banned.
* Use #[cfg(feature = client)] and GameState to gate functionality. 
* Refactoring changes should reduce the total LOC size of the codebase, not increase it, with very few exceptions.
* Never touch types or functions starting with `cm_`; they're especially high quality APIs and you can use, but not modify them.
* Do not create wrappers if they don't justify the indirection or LOC

## rules
* Prefer glob imports over verbose manual imports.
* Inline wrapper functions if they have many inputs/outputs, unless when it would to duplication.
* Do not delete comments with lowercase first letters, or TODOs.
* Use minimal words/tokens.
* When fixing bugs:
  * Find root cause, exact fix, minimal patch.
* No new structs unless absolutely necessary.
* Everything should be clean and minimal. Every line of code counts against you.
* Never use bevy's events/messages, since most work is done in FixedUpdate
* Simplicity: Simplicity and decoupling is everything. I prefer simple-looking imperative code over functional programming or clever one-liners.
* Schedules: Use Update sparingly to keep framerate fast. Use SlowUpdate for things that don't have to happen each FixedUpdate.
* No CCD, since it breaks relative motion.
* for imports used for one package and not another, prefer `#[allow(unused_imports)]` over `#[cfg(feature = "<package>")]` 
* keep struct impls right next to their struct.
* all entity "types" like vehicles and weapons should be completely self-contained inside their plugins; no central registries, match statements, etc.
* functions/systems only used on the client should include `client`, `visuals`, or `local` in their name, unless already in a client-only file.
* When finished with a task, use make to build, addressing warnings (and test if necessary)
* All movement and physics should be relative. When attaching, detaching, or spawning anything, it should inherit the velocity of its owner.
* If a cleanup makes a generic subsystem smaller and pushes object-specific behavior back to the object implementation, that is usually a good cleanup.
* comment your code if it's not obvious why you wrote it that way

## efficiency
* Drop pleasantries, filler, hedging, niceties, repetition, and "if you want,..."

## style
- Keep code small.
- Keep boundaries obvious.
- Keep responsibilities local.
- Avoid cleverness.
- Avoid new types unless they earn their cost.
