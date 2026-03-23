# AGENT HELPERS

## Design
* Everything should be clean and minimal. Every line of code counts against you.
    * Read as many files as you need to understand the codebase.
    * If you don't understand something I ask, look it up.
    * No hacks. This is for an enterprise-quality game; everything needs to be scalable. Write once, use forever.
    * DO NOT rewrite my comments; if code is commented in lines with lowercase first letters, it's handwritten; be hesitant about changing it.
    * Avoid pulling in new dependencies unless they're both absolutely needed and recently updated
    * Please DO NOT create new structs, enums, components, etc if not absolutely necessary.
    * Decouple unrelated systems.
    * Don't use bevy's events/messages.
    * Simplicity is everything. When in doubt, choose the lowest-additional-code implementation.
    * Use Update sparingly. Use SlowUpdate for things that don't have to happen each FixedUpdate.
    * Do NOT fundamentally change how things work without asking me first.
    * When you add or change things, leave comments justifying why.
    * I prefer simple-looking imperative code over "elegant" functional programming or clever one-liners.
    * We need both single-player and multiplayer to work without fuss.
    * Try to keep logic out of client/gameserver main.rs; use #[cfg(feature = client)] and GameState to gate functionality.
    * DO NOT git push --force, git reset --hard, rm -rf, etc.

Use make, not cargo to build and test the project.

Also see: README.md for project description

## random other info
* We use postcard for encoding.

Build in the background in between tasks.

All movement and physics should be relative. When firing a projectile, it should inherit the velocity of its owner.
