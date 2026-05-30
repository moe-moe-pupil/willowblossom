# Bevy 0.18.1 Local Source Index

Bevy `release-0.18.1` is checked out under `references/bevy-0.18.1/` as a sparse, blob-filtered local reference. Do not paste or load the whole tree into context; search and read only targeted files.

The checkout is intentionally ignored by git to avoid vendoring Bevy. If it is missing, recreate it from the repo root:

```powershell
git clone --filter=blob:none --depth 1 --branch release-0.18.1 --sparse https://github.com/bevyengine/bevy.git .\.codex\skills\willowblossom-trpg-access\references\bevy-0.18.1
git -C .\.codex\skills\willowblossom-trpg-access\references\bevy-0.18.1 sparse-checkout set --skip-checks Cargo.toml crates examples docs release-content migration-guides
```

## Required Rule

For Bevy work in Willowblossom, Bevy 0.18.1 is the target. Verify app, plugin, schedule, ECS, input, UI, window, asset, rendering, and state APIs from this local checkout before implementing. Do this even when the API seems obvious; older Bevy 0.14/0.15 patterns are often wrong for 0.18.1.

## Search Patterns

Use examples first for public API patterns:

```powershell
rg -n "add_systems|Startup|Update|Plugin for|insert_state|init_state" .\.codex\skills\willowblossom-trpg-access\references\bevy-0.18.1\examples .\.codex\skills\willowblossom-trpg-access\references\bevy-0.18.1\crates
rg -n "ButtonInput|MouseButton|KeyCode|WindowPlugin|WindowResolution" .\.codex\skills\willowblossom-trpg-access\references\bevy-0.18.1\examples .\.codex\skills\willowblossom-trpg-access\references\bevy-0.18.1\crates
rg -n "States|NextState|OnEnter|OnExit|run_if" .\.codex\skills\willowblossom-trpg-access\references\bevy-0.18.1\examples .\.codex\skills\willowblossom-trpg-access\references\bevy-0.18.1\crates
rg -n "0\.18|migration|breaking" .\.codex\skills\willowblossom-trpg-access\references\bevy-0.18.1\release-content .\.codex\skills\willowblossom-trpg-access\references\bevy-0.18.1\migration-guides
```

## Version Check

The local checkout should report:

```powershell
git -C .\.codex\skills\willowblossom-trpg-access\references\bevy-0.18.1 rev-parse --abbrev-ref HEAD
rg -n '^version = "0\.18\.1"' .\.codex\skills\willowblossom-trpg-access\references\bevy-0.18.1\Cargo.toml
```

Expected branch: `release-0.18.1`.
