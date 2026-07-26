#import bevy_pbr::{
    pbr_types,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_fragment::pbr_input_from_standard_material,
    decal::clustered::apply_decals,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
}
#endif

struct VoxelOcclusionFadeSettings {
    camera_and_active: vec4<f32>,
    focus_and_radius: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> fade_settings: VoxelOcclusionFadeSettings;

fn hash_pixel(pixel: vec2<f32>) -> f32 {
    return fract(sin(dot(pixel, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn dissolve_camera_blocker(in: VertexOutput) {
    let active = fade_settings.camera_and_active.w;
    if active < 0.5 {
        return;
    }

    let camera = fade_settings.camera_and_active.xyz;
    let focus = fade_settings.focus_and_radius.xyz;
    let corridor = focus - camera;
    let corridor_length_squared = dot(corridor, corridor);
    if corridor_length_squared < 0.0001 {
        return;
    }

    let projection = dot(in.world_position.xyz - camera, corridor) / corridor_length_squared;
    if projection <= 0.0 || projection >= 1.0 {
        return;
    }

    let closest = camera + corridor * projection;
    let distance_from_view = distance(in.world_position.xyz, closest);

    // This is a perspective cone around the focused player's screen silhouette,
    // not a fixed-width tunnel through the scene. A fragment halfway to the
    // player must be within half the player's world-space radius to be a blocker.
    let focus_radius = fade_settings.focus_and_radius.w;
    let blocker_radius = focus_radius * projection;
    let silhouette_overlap =
        1.0 - smoothstep(blocker_radius * 0.72, blocker_radius, distance_from_view);
    let between_camera_and_player = smoothstep(0.015, 0.04, projection)
        * (1.0 - smoothstep(0.985, 0.998, projection));
    let dissolve = silhouette_overlap * between_camera_and_player * 0.94;

    if hash_pixel(floor(in.position.xy)) < dissolve {
        discard;
    }
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    dissolve_camera_blocker(in);

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color =
        alpha_discard(pbr_input.material, pbr_input.material.base_color);
    apply_decals(&pbr_input);

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif

    return out;
}
