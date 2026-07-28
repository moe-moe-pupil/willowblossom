#import bevy_pbr::{
    pbr_functions::visibility_range_dither,
    pbr_prepass_functions,
    prepass_io,
}

struct VoxelOcclusionFadeSettings {
    camera_and_opacity: vec4<f32>,
    focus_and_radius: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> fade_settings: VoxelOcclusionFadeSettings;

fn hash_pixel(pixel: vec2<f32>) -> f32 {
    return fract(sin(dot(pixel, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn apply_camera_blocker_opacity(in: prepass_io::VertexOutput) {
    let requested_opacity = clamp(fade_settings.camera_and_opacity.w, 0.0, 1.0);
    if requested_opacity >= 0.999 {
        return;
    }

    // Match the color pass exactly: horizontal floor and ceiling faces always
    // keep their depth, while the same wall pixels are discarded in both passes.
    let face_normal = normalize(cross(
        dpdx(in.world_position.xyz),
        dpdy(in.world_position.xyz),
    ));
    if abs(face_normal.y) >= 0.7 {
        return;
    }

    let camera = fade_settings.camera_and_opacity.xyz;
    let focus = fade_settings.focus_and_radius.xyz;
    let camera_to_focus = focus - camera;
    let camera_to_focus_length_squared = dot(camera_to_focus, camera_to_focus);
    if camera_to_focus_length_squared < 0.0001 {
        return;
    }

    let projection =
        dot(in.world_position.xyz - camera, camera_to_focus) / camera_to_focus_length_squared;
    if projection <= 0.0 || projection >= 1.0 {
        return;
    }

    let closest = camera + camera_to_focus * projection;
    let distance_from_view = distance(in.world_position.xyz, closest);
    let blocker_radius = fade_settings.focus_and_radius.w * projection;
    let silhouette_overlap =
        1.0 - smoothstep(blocker_radius * 0.72, blocker_radius, distance_from_view);
    let between_camera_and_player = smoothstep(0.015, 0.04, projection)
        * (1.0 - smoothstep(0.985, 0.998, projection));
    let fade_strength = silhouette_overlap * between_camera_and_player;
    let fragment_opacity = mix(1.0, requested_opacity, fade_strength);

    if hash_pixel(floor(in.position.xy)) >= fragment_opacity {
        discard;
    }
}

#ifdef PREPASS_FRAGMENT
@fragment
fn fragment(in: prepass_io::VertexOutput) -> prepass_io::FragmentOutput {
#ifdef VISIBILITY_RANGE_DITHER
    visibility_range_dither(in.position, in.visibility_range_dither);
#endif

    apply_camera_blocker_opacity(in);
    pbr_prepass_functions::prepass_alpha_discard(in);

    var out: prepass_io::FragmentOutput;
#ifdef NORMAL_PREPASS
    out.normal = vec4(normalize(in.world_normal) * 0.5 + vec3(0.5), 1.0);
#endif
#ifdef MOTION_VECTOR_PREPASS
    out.motion_vector = pbr_prepass_functions::calculate_motion_vector(
        in.world_position,
        in.previous_world_position,
    );
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
    return out;
}
#else
@fragment
fn fragment(in: prepass_io::VertexOutput) {
#ifdef VISIBILITY_RANGE_DITHER
    visibility_range_dither(in.position, in.visibility_range_dither);
#endif

    apply_camera_blocker_opacity(in);
    pbr_prepass_functions::prepass_alpha_discard(in);
}
#endif
