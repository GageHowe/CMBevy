# gametypes

Folder for gametype scripts defined with Lua in `crates/scripting`.

`koth.lua` expects a map-authored entity with:
- `ScriptZone`
- `ScriptTags { tags: ["hill"] }`

`koth_poc.lua` is the minimal proof-of-concept mode used with `assets/maps/koth_poc.ron`.

`ffa.lua` is the baseline kill-scoring mode using the `on_player_killed(victim, killer)` script hook.
