# Pawns

Pawns are defined by their marker components (`BipedPawnComponent`, `SpaceshipPawnComponent`, etc.).

## Adding a new pawn type

1. Create a marker component in your pawn's module:
   ```rust
   #[derive(Component)] pub struct MyPawnComponent;
   ```

2. Implement the following functions (by convention — not a trait, yet):
   - `spawn(transform, commands, meshes, materials, world, camera?) -> Entity` — client, with mesh
   - `spawn_server(transform, commands, world) -> Entity` — server, physics only
   - `spawn_ghost(transform, commands, meshes, materials, world) -> Entity` — other players on client # looking for a way to avoid having to do this
   - `apply_movement(world: &mut PhysicsWorld, handle: &PhysicsBodyHandle, input: PawnInput)` — called every fixed tick

3. Register the movement system in `PawnPlugin::build`:
   ```rust
   move_pawns::<MyPawnComponent>(my_module::apply_movement)
   ```
   `move_pawns` handles the `Possessed` buffer consumption generically — no boilerplate needed.

## How movement works

`move_pawns::<T>(apply_fn)` is a generic system factory. It queries all entities with `Possessed + PhysicsBodyHandle + T`, consumes one input per tick from the ring buffer, and calls `apply_fn`. Adding a new pawn type requires only a marker component and an apply function.

## Camera

The local player's pawn gets a `YawPivot → PitchPivot → Camera3d` hierarchy as children.
Mouse look updates the pivots every frame in `PostUpdate` (before transform propagation) for zero-latency camera response.
`look_yaw` (pawn-local) is included in `PawnInput` so the server can reconstruct world-space facing as `body_rotation * Quat::from_rotation_y(look_yaw)`.
