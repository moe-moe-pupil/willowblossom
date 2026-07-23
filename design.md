# 3D Voxel Scene Preview And Editor Plan

## Goal

Willowblossom should provide a GM-facing 3D TRPG scene preview for tactical play. The first milestone is a visible voxel scene in the existing app. Later milestones add runtime editing, persistence, and party-aware visibility controls.

## Current Milestone

- Add `bevy_voxel_world` as the terrain/voxel backend.
- Render a deterministic starter scene at app launch.
- Keep the current chat/summary UI usable as egui overlays.
- Avoid story generation or hidden-information leakage; the scene is controlled by the GM and persisted as explicit data.

## Architecture

- `src/scene.rs` owns the voxel preview world.
- `TrpgVoxelWorld` is the `VoxelWorldConfig` type and world identifier for `bevy_voxel_world`.
- The procedural lookup creates only starter terrain. Runtime edits should be stored in the crate's modified voxel layer through `VoxelWorld<TrpgVoxelWorld>::set_voxel`.
- The Bevy camera is tagged with `VoxelWorldCamera<TrpgVoxelWorld>` so chunk streaming follows the preview camera.
- Existing egui windows remain overlays; the central panel should not paint an opaque background over the 3D scene.

## Runtime Editing Plan

1. Add an egui toolbar for edit mode:
   - selection/inspect
   - add block
   - erase block
   - material brush
   - token placement
2. Raycast from cursor using `VoxelWorld<TrpgVoxelWorld>::raycast`.
3. For add/erase:
   - erase selected solid voxel with `WorldVoxel::Air`
   - add adjacent voxel using hit normal and selected material
4. Track edits in an app-owned `Persistent<VoxelSceneStore>` resource, not only inside the voxel crate internals.
5. Apply persisted edits after startup by replaying `set_voxel` calls.

## Persistence Model

Use `.data/willowblossom/scenes.toml`.

```toml
active_scene = "default"

[[scenes]]
id = "default"
name = "Default Preview"

[[scenes.voxels]]
position = [0, 0, 0]
voxel = { Solid = 1 }
visibility = "public"
```

Suggested Rust data:

```rust
struct VoxelSceneStore {
    active_scene: String,
    scenes: Vec<VoxelScene>,
}

struct VoxelScene {
    id: String,
    name: String,
    voxels: Vec<PersistedVoxelEdit>,
}

struct PersistedVoxelEdit {
    position: [i32; 3],
    voxel: PersistedVoxel,
    visibility: SceneVisibility,
}
```

## Visibility Rules

- Scene objects should carry visibility metadata before summaries or player-facing previews use them.
- `public` is visible to everyone.
- `party:<party_id>` is visible only to that party and the GM.
- `player:<player_id>` is visible only to that player and the GM.
- `gm` is visible only in the GM UI.

## Next Milestones

1. Camera controls: orbit, pan, zoom, reset view.
2. Basic edit tools: add, erase, material selection.
3. Persistent scene save/load.
4. Token entities for PCs/NPCs with visibility and labels.
5. Scene list and duplicate/import/export controls.
6. Optional party/player filtered preview mode.
