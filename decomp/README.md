# decomp

`cargo run --bin decomp -- .\assets\models\planet1.glb --output .\assets\collision\planet1.obj`

## arguments

--max-hulls — how many convex pieces the mesh is split into. Higher = more accurate coverage of complex shapes, but
more colliders at runtime. 16 is fine for most props.

--resolution — voxel grid size used internally by VHACD. Higher = finer detail when detecting concavities, but slower.
64 is fast, 128–256 for complex meshes.

--concavity — how much a piece is allowed to deviate from perfectly convex before VHACD decides to split it further.
Lower = more splits, more hulls, more accurate. 0.01 is a good default; push toward 0.001 if you're seeing gaps.

--downsampling — after decomposition, each voxel-based part gets its convex hull recomputed. This controls how
aggressively vertices are simplified during that step. Higher = fewer verts per hull, faster at runtime, slightly less
accurate shape. 4 is a safe default.
