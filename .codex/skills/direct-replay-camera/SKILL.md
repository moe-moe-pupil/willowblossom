---
name: direct-replay-camera
description: Implement and review deterministic spectator and replay cameras that keep subjects framed, preserve the 180-degree line, use stable shot families, and move with restrained tracking or dolly motion. Use when changing Willowblossom replay camera planning, AI director shot execution, camera interpolation, speaker focus, automatic spectator behavior, shot continuity, or tests for camera framing and movement.
---

# Direct Replay Camera

Keep semantic direction separate from camera execution. Let AI select the subject, shot size, and restrained motion intent; enforce framing, continuity, and movement geometrically in Rust.

## Workflow

1. Inspect the replay events, subject positions, existing camera track, and tests.
2. Establish one world-space scene axis from the first two distinct subjects. Fall back to the initial camera's horizontal right vector when only one subject exists.
3. Select the permitted side from the initial camera position and keep every generated pose on that side. Never change the side because of a random seed or dialogue index.
4. Resolve AI output to a small deterministic shot family. Treat unsupported environment shots as a safe speaker medium shot.
5. Aim at the subject's visual center. Derive rotation from the final constrained position after applying all movement.
6. Use cuts for large subject changes. Use dolly only along the optical axis; do not orbit or fly laterally between speakers.
7. Interpolate movement with eased position and shortest-path quaternion interpolation. Keep exact cuts represented by adjacent keyframes.
8. Add deterministic tests for focus, side preservation, motion direction, and eased interpolation.

## Camera Rules

- Prefer static shots. Allow only subtle `dolly_in` and `dolly_out` during dialogue.
- Interpret `drift_left` and `drift_right` as static until a motivated tracking subject exists. Do not synthesize orbiting.
- Keep yaw composition stable within a sequence. Vary shot size through distance and modest height, not arbitrary angles.
- Preserve screen direction by using one scene-side sign for the complete camera track.
- Keep the target centered unless a tested composition system explicitly supports safe lead room or rule-of-thirds offsets.
- Recompute `looking_at` after travel limits or collision corrections so focus cannot lag behind the final camera position.
- Use visibility and collision checks when scene-query support is available. Search for a valid pose on the permitted side instead of crossing the line.
- Avoid continuous micro-corrections. Hold a shot until the speaker or important action changes.

## Focus Checks

For each generated keyframe:

- Compute `forward = rotation * Vec3::NEG_Z`.
- Compute the normalized direction from camera position to the subject's visual center.
- Require a high positive dot product between the two directions.
- Check the signed side of the scene axis and require it to remain unchanged.
- For dolly-in, require the settled distance to be less than the arrival distance; reverse this for dolly-out.

## Voxel Standee Orientation

- Treat a voxel player standee's local `Vec3::NEG_Z` as its portrait front. Local `Vec3::Z` is the back-label side marked `背`.
- To face a standee toward the replay camera, yaw it so `rotation * Vec3::NEG_Z` matches the horizontal standee-to-camera direction. Do not aim local `Vec3::Z` at the camera.
- Test both sides: require the portrait front to have a high positive dot product with the camera direction and the labeled back to have a high negative dot product.

## AI Boundary

Do not ask an LLM for world-space positions, rotations, random camera angles, or collision decisions. Validate every enum returned by the LLM and use safe deterministic defaults. A prompt is guidance, not a geometry constraint.

## Repository Validation

Run the narrow replay-camera unit tests first, then `cargo test` when practical. Review generated tracks numerically even when visual replay testing is unavailable.
