# game_common

This is for shared types and code needed by both client and gameserver, as well as other modules.

When modules get too big, or stop being in danger of circular dependencies, they should get their own crate.

Common should not use its sibling crates as dependencies.
