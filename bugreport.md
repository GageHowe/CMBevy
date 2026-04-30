## Bevy version and features

- Bevy `0.18.1`
- Non-default Bevy cargo features in this project: `jpeg`, `std`, `multi_threaded`, `tga`

## Relevant system information

It appears to be an upstream shader bug, but...
OS: EndeavourOS x86_64
Host: 82WS (Legion Pro 7 16ARX8H)
CPU: AMD Ryzen 9 7945HX (32) @ 2.50 GHz
GPU 1: NVIDIA GeForce RTX 4080 Max-Q / Mobile [Discrete]
GPU 2: AMD Radeon 610M [Integrated]
Memory: 20.27 GiB / 30.03 GiB (67%)
Swap: 511.88 MiB / 512.00 MiB (100%)
Disk (/): 419.44 GiB / 935.85 GiB (45%) - ext4

## What you did

I added a render-scale option to a `Camera3d` using Bevy's `MainPassResolutionOverride`.

The setup is roughly:

- main 3D camera uses Bevy `Skybox`
- render scale below `1.0` inserts `MainPassResolutionOverride(UVec2)`
- custom fullscreen postprocess passes were updated to use `View::main_pass_viewport`, which fixed the expected top-left sampling issues in those passes

The debugging path was:

1. Initially, changing render scale appeared to do nothing.
2. That turned out to be because `MainPassResolutionOverride` was not being extracted into the render world in this app, so the renderer never saw the component.
3. After fixing extraction, render scale became active, but the scene was visibly pinned to the top-left corner of the screen.
4. That top-left behavior was caused by my own custom fullscreen postprocess passes (`outline` and `color compression`) sampling with full-screen UVs instead of remapping through `View::main_pass_viewport`.
5. After fixing those custom passes, the main scene behaved correctly under render scaling, but the skybox still rendered incorrectly.

At that point, the remaining bad behavior was isolated to Bevy's built-in skybox path rather than app-specific rendering code.

## What went wrong

Expected:

- When `MainPassResolutionOverride` is active, the skybox should render correctly at the reduced main-pass resolution and upscale consistently with the rest of the scene.

Actual:

- The main scene mostly behaves correctly after compensating custom postprocess passes for `main_pass_viewport`.
- The skybox still renders incorrectly / behaves strangely under render scaling.
- The remaining issue persists even after local fullscreen postprocess shaders were updated to use `main_pass_viewport`, which suggests the bug is specifically in Bevy's own skybox rendering path.

Suspected root cause:

- In Bevy `0.18.1`, the built-in skybox shader reconstructs its ray direction using `view.viewport` instead of `view.main_pass_viewport`.
- File: `bevy_core_pipeline/src/skybox/skybox.wgsl`
- Current line:

```wgsl
let ray_direction = coords_to_ray_direction(in.position.xy, view.viewport);
```

- This seems inconsistent with `MainPassResolutionOverride` support, since Bevy's camera docs explicitly say shaders should use `View::main_pass_viewport` when the override is active.

Likely fix:

```wgsl
let ray_direction = coords_to_ray_direction(in.position.xy, view.main_pass_viewport);
```

## Additional information

- This was found while integrating Bevy render scaling into a game project.
- A local workaround appears possible by overriding/forking the skybox shader, but this looks like an engine-side issue in Bevy's built-in skybox path.
- Related observation: custom fullscreen postprocess shaders also need to sample using `View::main_pass_viewport` when `MainPassResolutionOverride` is active, otherwise the reduced image appears pinned to the top-left of the screen. After fixing those local shaders, only skybox rendering remained broken.
- The reason I think this belongs upstream is that the remaining failure is in Bevy's built-in `Skybox` shader, not in custom game logic. The shader appears to use the full output viewport for ray reconstruction even when the main pass is intentionally rendered at a smaller resolution.
