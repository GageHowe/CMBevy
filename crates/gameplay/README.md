# gameplay

## schedules

ForceApplication SystemSet happens in FixedPreUpdate in SimulationSystems.

SimulationSystems are paused when singleplayer is paused, or the server is empty.

On the server, Update is capped to the same run rate as FixedUpdate.

## style

This codebase avoids large match statements; packet dispatch and spawning are handled via enum_dispatch.
