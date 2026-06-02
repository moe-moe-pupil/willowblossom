# Smooth SDF Voxel Planet Plan

Willowblossom keeps the existing `bevy_voxel_world` block map for TRPG map editing and adds a separate smooth SDF planet renderer for generated terrain.

The planet stores terrain as a sampled signed distance field, builds mesh chunks around the scene preview camera, and leaves octree/LOD extension points for later stitched adaptive terrain. V1 uses same-LOD chunks only, avoiding visible cracks until stitched boundary rings are implemented.

## Public API

- `PlanetTerrainPlugin`
- `PlanetTerrainSettings`
- `PlanetSdf`
- `SignedDistanceField`
- `PlanetTerrainRoot`
- `PlanetChunk`

## V1 Behavior

- Base SDF: `length(position - center) - radius`.
- 3D deterministic noise displaces the surface outward and inward.
- Negative noise has half displacement strength for softer inward cuts.
- Far outside the planet, distance returns early without sampling noise.
- Surface-nets chunk meshing samples each chunk on an 18x18x18 node grid and emits one smooth vertex per sign-changing cell.
- Mesh work runs on Bevy async compute tasks and completed meshes are applied on the main thread.
- Runtime chunk generation is capped per frame.

## Later Work

- Adaptive octree refinement using dirty/enqueued node state.
- Stitched high-to-low LOD boundary rings.
- Local SDF overrides for sphere add/subtract edits.
- Terrain material colors from elevation, slope, and procedural masks.
