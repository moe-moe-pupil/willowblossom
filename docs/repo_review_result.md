# Willowblossom Repo Review Result

Date: 2026-06-03

## Verdict

The repo is good enough to build on. The existing test suite passes, and the scene stack already has the right playable foundation: `bevy_voxel_world`, persistent voxel edits, raycast editing, minimap/capture camera support, and TRPG scene position hooks.

It was not good enough to keep the SDF planet as the playable planet path. The SDF renderer produced a separate visual terrain layer, while the editor, persistence, capture flow, and battle area logic all operate through the block voxel world.

## Findings

1. High: SDF planet rendering was wired into the scene, but it did not share the voxel edit/persist/capture path.
   - Impact: a player-visible planet mesh could exist without being editable as voxel terrain or represented in saved voxel maps.
   - Result: disconnected the SDF terrain runtime from `ScenePreviewPlugin` and generated the planet through `TrpgVoxelWorld::voxel_lookup_delegate` instead.

2. Medium: `src/scene.rs` is carrying too many responsibilities.
   - It currently owns procedural map generation, voxel editor UI, map persistence, minimap, waypoints, capture cameras, NapCat image capture, and character standees.
   - This is workable for the current pass, but future work should split terrain generation, editor UI, capture, and persistence into smaller modules.

3. Medium: map bootstrap/migration behavior is still aggressive.
   - The existing `ensure_voxel_maps` flow may reset the built-in Space HiFi map under some conditions.
   - I removed the deprecated planet-marker cleanup from startup so new planet-surface edits using planet materials are not wiped, but future migrations should be versioned instead of inferred from edit counts.

4. Low: the SDF planet module still exists and its tests still pass.
   - It is no longer the playable path.
   - Keep it only if it remains useful as reference/prototype code; otherwise remove it later to reduce architecture confusion.

## Integration Done

- Added a large procedural voxel planet to the existing voxel lookup.
- Kept the planet block-based: radius 9600, deterministic elevation noise, ocean/land materials, and solid interior chunks.
- Added a coarse blocky preview mesh for distant space views, hidden automatically near the surface so real streamed voxel chunks take over.
- Added a procedural planet edit-target fallback, so surface edits still work when the streamed chunk raycast misses.
- Started the scene camera at the planet surface so the app is immediately playable on the voxel planet.
- Bound Avian physics to generated voxel chunks by producing static trimesh colliders from the same chunk meshes used for rendering.
- Added radial planet gravity and a small dynamic probe spawned above the landing area, so streamed planet chunks have a live collision target.
- Added `F` grab/drop for physics voxels: press `F` to pull the targeted solid voxel into a held physics cube, press `F` again to drop it as a dynamic body under planet gravity.
- Added delayed cleanup for voxel chunk colliders whose render mesh disappears after edits/remeshes.
- Preserved the voxel editor, saved edits, player capture cameras, and scene capture flow on the voxel world path.
- Added focused tests proving the landing-direction planet lookup has solid terrain below the surface and empty space above it.

## Verification

`cargo test` passes:

- 61 passed
- 1 ignored live DeepSeek API test
- 0 failed

Remaining compiler warnings are pre-existing unused/deprecated UI and ECS warnings, not blockers for the voxel planet integration.
