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
    camera_and_target_count: vec4<f32>,
    opacity_and_voxel_size: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> fade_settings: VoxelOcclusionFadeSettings;

@group(#{MATERIAL_BIND_GROUP}) @binding(101)
var<storage, read> fade_targets: array<vec4<f32>>;

fn inside_player_cube_cast(voxel_center: vec3<f32>, target: vec4<f32>) -> bool {
    let camera = fade_settings.camera_and_target_count.xyz;
    let sightline = target.xyz - camera;
    let sightline_length_squared = dot(sightline, sightline);
    if sightline_length_squared <= 0.000001 {
        return false;
    }

    let progress = dot(voxel_center - camera, sightline) / sightline_length_squared;
    if progress <= 0.0 || progress >= 1.0 {
        return false;
    }
    let closest_point = camera + sightline * progress;
    let touched_voxel_half_extent =
        target.w + fade_settings.opacity_and_voxel_size.y * 0.5;
    return all(
        abs(voxel_center - closest_point) <= vec3<f32>(touched_voxel_half_extent),
    );
}

fn voxel_center_from_surface(
    world_position: vec3<f32>,
    world_normal: vec3<f32>,
) -> vec3<f32> {
    let voxel_size = fade_settings.opacity_and_voxel_size.y;
    let inside_position = world_position - world_normal * voxel_size * 0.01;
    return round(inside_position / voxel_size) * voxel_size;
}

fn opacity_dither_threshold(fragment_position: vec2<f32>) -> f32 {
    let pixel = floor(fragment_position);
    return fract(
        52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))),
    );
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
    let target_count = u32(fade_settings.camera_and_target_count.w);
    let occluder_opacity = clamp(fade_settings.opacity_and_voxel_size.x, 0.0, 1.0);
    if occluder_opacity < 0.999 {
        let voxel_center =
            voxel_center_from_surface(in.world_position.xyz, pbr_input.N);
        for (var target_index = 0u; target_index < target_count; target_index += 1u) {
            if inside_player_cube_cast(voxel_center, fade_targets[target_index]) {
                if opacity_dither_threshold(in.position.xy) >= occluder_opacity {
                    discard;
                }
                break;
            }
        }
    }
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
