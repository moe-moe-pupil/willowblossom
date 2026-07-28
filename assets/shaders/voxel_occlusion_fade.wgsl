#import bevy_pbr::{
    pbr_types,
    pbr_functions::alpha_discard,
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
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
}
#endif

#ifdef VISIBILITY_RANGE_DITHER
#import bevy_pbr::pbr_functions::visibility_range_dither;
#endif

#ifdef MESHLET_MESH_MATERIAL_PASS
#import bevy_pbr::meshlet_visibility_buffer_resolve::resolve_vertex_output
#endif

#ifdef OIT_ENABLED
#import bevy_core_pipeline::oit::oit_draw
#endif

#ifdef FORWARD_DECAL
#import bevy_pbr::decal::forward::get_forward_decal_info
#endif

struct VoxelOcclusionFadeSettings {
    camera_and_opacity: vec4<f32>,
    focus_and_radius: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> fade_settings: VoxelOcclusionFadeSettings;

fn hash_pixel(pixel: vec2<f32>) -> f32 {
    return fract(sin(dot(pixel, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn apply_camera_blocker_opacity(in: VertexOutput) {
    let requested_opacity = clamp(fade_settings.camera_and_opacity.w, 0.0, 1.0);
    if requested_opacity >= 0.999 {
        return;
    }

    // Voxel chunks contain walls and floors in the same mesh. Only dissolve
    // near-vertical faces so a blocking wall can never take its floor with it.
    if abs(normalize(in.world_normal).y) >= 0.7 {
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

    // Screen-door transparency keeps opaque depth/sorting semantics for the
    // rest of a mixed wall-and-floor chunk.
    if hash_pixel(floor(in.position.xy)) >= fragment_opacity {
        discard;
    }
}

@fragment
fn fragment(
#ifdef MESHLET_MESH_MATERIAL_PASS
    @builtin(position) frag_coord: vec4<f32>,
#else
    vertex_output: VertexOutput,
    @builtin(front_facing) is_front: bool,
#endif
) -> FragmentOutput {
#ifdef MESHLET_MESH_MATERIAL_PASS
    let vertex_output = resolve_vertex_output(frag_coord);
    let is_front = true;
#endif

    var in = vertex_output;

#ifdef VISIBILITY_RANGE_DITHER
    visibility_range_dither(in.position, in.visibility_range_dither);
#endif

#ifdef FORWARD_DECAL
    let forward_decal_info = get_forward_decal_info(in);
    in.world_position = forward_decal_info.world_position;
    in.uv = forward_decal_info.uv;
#endif

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    apply_camera_blocker_opacity(in);
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

#ifdef OIT_ENABLED
    let alpha_mode =
        pbr_input.material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS;
    if alpha_mode != pbr_types::STANDARD_MATERIAL_FLAGS_ALPHA_MODE_OPAQUE {
        oit_draw(in.position, out.color);
        discard;
    }
#endif

#ifdef FORWARD_DECAL
    out.color.a = min(forward_decal_info.alpha, out.color.a);
#endif

    return out;
}
