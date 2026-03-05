# Pawns

Pawns are defined by their marker structs, BipedComponent, SpaceshipComponent, etc.

Pawns must:
* Have a "constructor" spawn function
* Have a move_* function that consumes input

To create a new pawn: 
* Create a new component in biped.rs (e.g., ```#[derive(Component)] pub struct EpicPawnComponent;```)
* Create and register a move_* system, similarly to move_bipeds

TODO: make a single function to spawn pawns more flexibly, like spawn_pawn(type, possessed?, etc)
