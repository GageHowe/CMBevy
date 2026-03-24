
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
* Simplicity: Simplicity is everything. When in doubt, choose the lowest-additional-code implementation. I prefer simple-looking imperative code over "elegant" functional programming or clever one-liners.
* Update schedule: Use Update sparingly to keep framerate fast. Use SlowUpdate for things that don't have to happen each FixedUpdate.
* Do NOT fundamentally change how things work without asking me first. When you add or change things, leave comments justifying why.
* DO NOT git push --force, git reset --hard, rm -rf, etc.
* Before implementing anything or making large changes, assess your proposed solution for scalability, simplicity, and flexibility.

## Iteration
* Build in the background in between tasks.
* Use make, not cargo to build and test the project.

### Game Design
* All movement and physics should be relative. When firing a projectile, it should inherit the velocity of its owner.
* We need both single-player and multiplayer to work without fuss.

## random other info
* We use postcard for encoding.

## Cleanliness
* Don't put functions and logic in client/gameserver main.rs; use #[cfg(feature = client)] and GameState to gate functionality.

Also see: README.md for project description

To claude only: Use ripgrep instead of grep/find.
