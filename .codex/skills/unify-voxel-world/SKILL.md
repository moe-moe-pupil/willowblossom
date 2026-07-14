---
name: unify-voxel-world
description: Enforce one canonical voxel scale and shared voxel behaviors throughout Willowblossom. Use when adding or changing voxel terrain, planets, ships, stations, doors, props, fluids, physics bodies, editing tools, raycasts, collisions, explosions, procedural generation, LOD, persistence, or rendering in the voxel scene.
---

# Unify Voxel World

Treat every editable world object as part of one voxel system. Do not create a special scale or reduced behavior set for a planet, vehicle, prop, or other subsystem.

## Invariants

- Use `VOXEL_SIZE` as the canonical gameplay cell edge. In the current scene it is `0.25` world units.
- Express gameplay coordinates as integer voxel cells and convert with `cell.as_vec3() * VOXEL_SIZE`.
- Keep rendering, ray selection, collision, editing, physics, and procedural generation aligned to the same cell centers and bounds.
- Use the same mouse/tool semantics for every voxel source: left click removes; right click uses the equipped item or tool.
- Route add, remove, paint, push, pull, physicalize, and explode through shared behavior or an explicitly equivalent implementation.
- Never substitute a sphere, static decorative model, or unrelated mesh collider for editable voxel geometry.
- Keep fluids opaque or transparent according to their material contract, never because they use a different world representation.

## Large Worlds And LOD

Use sparse storage, chunking, greedy meshing, and LOD to control cost. LOD may merge canonical cells for distant rendering or broad-phase collision, but it must not change gameplay scale.

Before an edit, ray hit, dig, or explosion mutates an LOD region:

1. Refine the affected region into canonical `VOXEL_SIZE` cells.
2. Apply the operation to those canonical cells.
3. Rebuild visible geometry and collision from the same resulting occupancy.
4. Record removed/generated cells so procedural streaming cannot refill holes.

Do not allocate an entire solid planet merely to satisfy the scale invariant. Materialize surface and buried cells on demand around interaction regions.

## Workflow

1. Search for scale constants, coordinate conversions, colliders, raycasts, and all tool dispatch paths with `rg`.
2. Identify every voxel-backed entity type affected by the change.
3. Build a behavior matrix covering render, collide, select, add/remove, paint, physics, explode, save/restore, and procedural continuation.
4. Reuse the canonical path where possible; otherwise add a parity implementation and explain why sharing is unsafe.
5. When adding Bevy queries, prove mutable accesses disjoint with `Without` filters or `ParamSet`.
6. Add regression tests for exact scale and for each newly unified behavior.
7. Run `cargo check` and the focused voxel tests. Attempt the relevant runtime path when native linking is available.

## Review Checklist

- No new gameplay voxel-size constant conflicts with `VOXEL_SIZE`.
- No hard-coded `1.0`, scale multiplier, or parent transform silently changes cell size.
- Mesh origin offsets and collider centers agree.
- Nearest-hit comparison includes every editable voxel source.
- Explosions and digging update both occupancy and collider state.
- Generated underground neighbors use canonical cells and never restore removed cells.
- LOD is an optimization only; interacted results are canonical voxels.
- Tests would fail if an object regressed to a special scale or skipped a tool behavior.
