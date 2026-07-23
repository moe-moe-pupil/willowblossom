---
name: bevy-query-conflicts
description: Prevent and fix Bevy ECS system parameter conflicts, especially error B0001 from overlapping Query access to the same component, such as reading Transform in one query while mutating Transform in another. Use when editing Bevy systems, adding or changing Query parameters, touching Transform or GlobalTransform or shared component access, using With or Without filters, or diagnosing Bevy panics that mention conflicting component access, disjoint queries, Without filters, or ParamSet.
---

# Bevy Query Conflicts

## Overview

Before adding or changing Bevy `Query` system parameters, prove that every mutable component access is disjoint from all other accesses to the same component type. Bevy validates this at runtime and panics with `B0001` if two query params might touch the same entity.

## Workflow

1. List every `Query`, `Single`, `ResMut`, and other system param that accesses the same component type.
2. For each component type, identify read/write pairs and write/write pairs that could overlap on one entity.
3. If queries are intended to target different entity classes, encode that in the filters with reciprocal `Without<T>` markers.
4. If overlap is legitimate and the code needs both views, merge the access into one query or use `ParamSet`.
5. Run `cargo check`. For runtime-only schedules or dynamic paths, also run the app path that previously panicked.

## Disjoint Query Pattern

Prefer explicit filters when entities are structurally separate:

```rust
fn system(
    readers: Query<&Transform, (With<CaptureCamera>, Without<CharacterStandee>)>,
    mut writers: Query<&mut Transform, (With<CharacterStandee>, Without<CaptureCamera>)>,
) {
    // ...
}
```

Use both sides when it improves local reasoning. One `Without` can be enough for Bevy, but reciprocal filters make the invariant obvious during later edits.

## ParamSet Pattern

Use `ParamSet` when the same entity class may be queried in incompatible ways, but access can be sequenced:

```rust
fn system(
    mut queries: ParamSet<(
        Query<&Transform, With<Foo>>,
        Query<&mut Transform, With<Bar>>,
    )>,
) {
    for transform in queries.p0().iter() {
        // read first
    }

    for mut transform in queries.p1().iter_mut() {
        // then write
    }
}
```

Do not use `ParamSet` to hide a real aliasing bug. If the entities are meant to be disjoint, encode that with filters instead.

## Common Bevy Trap

This panics even if the app's current data never overlaps, because Bevy validates the declared access pattern:

```rust
fn bad(
    cameras: Query<&Transform, With<PlayerCaptureCamera>>,
    mut standees: Query<&mut Transform, With<CharacterStandee>>,
) {}
```

Fix it by proving disjointness:

```rust
fn good(
    cameras: Query<&Transform, (With<PlayerCaptureCamera>, Without<CharacterStandee>)>,
    mut standees: Query<&mut Transform, (With<CharacterStandee>, Without<PlayerCaptureCamera>)>,
) {}
```

## Review Checklist

- Search the edited system for every repeated component type across params, especially `Transform`, `GlobalTransform`, `Visibility`, `Camera`, and marker-bearing gameplay components.
- Check tuple filters after adding a new marker component; old filters may no longer prove disjointness.
- Do not assume `With<A>` and `With<B>` are disjoint unless each side excludes the other or the codebase has an enforced invariant.
- Prefer a single query when the code is updating the same entities it reads.
- Keep `Without<T>` filters close to the query that needs them, not buried in helper logic.
